//! `sn flow runs/debug/steps/logs/why-not/tail` — Flow Designer execution
//! debugging over the flow engine's own tables.
//!
//! Everything here reads documented tables through the Table and Aggregate
//! APIs (and `tail` rides the `sn watch` machinery); nothing touches the
//! designer's undocumented `processflow` API. What each table holds, as
//! measured on a Zurich/Australia PDI:
//!
//! - `sys_flow_context` — one row per execution. `flow` is a plain **string**
//!   column holding the `sys_hub_flow` sys_id (not a reference), `name` is the
//!   flow's display name, `source_table`/`source_record` name the triggering
//!   record for record triggers (`calling_source = CRUD_TRIGGER`), and
//!   `reporting` is the reporting level the run was recorded at.
//! - `sys_flow_report` — one row per step (subclassed into
//!   `sys_flow_flow_report`/`sys_flow_action_report`/`sys_flow_step_report`).
//!   **Only written when the run's reporting level is not `OFF`**, and `OFF` is
//!   the shipped default of `com.snc.process_flow.reporting.level`. So for
//!   most production runs there are no steps at all, and the verbs say so
//!   rather than returning an empty list that reads as "nothing ran". On the
//!   Australia PDI even a run at `FULL` wrote no rows here: its values went to
//!   `sys_flow_report_value` (row-ACL'd and 403 on a filtered query, admin
//!   included), so `steps` is only as good as the release's use of this table.
//! - `sys_flow_log` — engine log lines per run; `level` is a numeric string
//!   (`-1` debug, `0` info, `1` warning, `2` error). Error lines are written
//!   for runs that still end `COMPLETE` (a script step that catches and logs),
//!   so `debug` shows them regardless of the run's state.
//! - `sys_hub_flow.remote_trigger_id` → `sys_flow_trigger`: the *runtime*
//!   trigger registration of the published flow. Its subclass
//!   `sys_flow_record_trigger` carries `table`, `condition`, `on_insert`/
//!   `on_update`/`on_delete`, `active` and `run_on_extended` as plain columns,
//!   which is everything `why-not` needs.
//!
//! Row ACLs gate all of it: an `itil` caller gets the aggregate count of
//! `sys_flow_context` but zero rows, and a 404 for a context read by sys_id.

use crate::cli::journal::{validate_identifier, validate_sys_id};
use crate::cli::kernel::{connect, take_field, write_response};
use crate::cli::record_ref::{self, RecordRef, is_sys_id};
use crate::cli::watch::{self, WatchArgs, WatchLimits};
use crate::cli::{GlobalFlags, SetLimit};
use crate::client::Client;
use crate::error::{Error, NO_HTTP_STATUS, Result};
use clap::{Subcommand, ValueEnum};
use serde_json::{Map, Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Subcommand, Debug)]
pub enum FlowDebugSub {
    /// Executions of a flow, newest first (sys_flow_context). `--record`
    /// answers "what ran when this record changed?".
    Runs(FlowRunsArgs),
    /// One paste-able document for one execution: the context, its first
    /// failed step, a step summary, and the engine log tail.
    Debug(FlowContextDebugArgs),
    /// Per-step timeline of one execution, ordered as the engine ran it.
    Steps(FlowStepsArgs),
    /// Flow engine log lines for one execution (sys_flow_log).
    Logs(FlowLogsArgs),
    /// Trigger post-mortem: why did this flow not run for this record?
    #[command(name = "why-not")]
    WhyNot(FlowWhyNotArgs),
    /// Stream a flow's executions live as JSONL (an `sn watch` preset on
    /// sys_flow_context).
    Tail(FlowTailArgs),
}

/// `sys_flow_context.state`, as the dictionary's choice list spells it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum RunState {
    Waiting,
    Complete,
    Error,
    Cancelled,
    InProgress,
    Queued,
    ContinueSync,
    PresumedInterrupted,
    Paused,
    PausedInDebug,
}

impl RunState {
    fn value(self) -> &'static str {
        match self {
            RunState::Waiting => "WAITING",
            RunState::Complete => "COMPLETE",
            RunState::Error => "ERROR",
            RunState::Cancelled => "CANCELLED",
            RunState::InProgress => "IN_PROGRESS",
            RunState::Queued => "QUEUED",
            RunState::ContinueSync => "CONTINUE_SYNC",
            RunState::PresumedInterrupted => "PRESUMED_INTERRUPTED",
            RunState::Paused => "PAUSED",
            RunState::PausedInDebug => "PAUSED_IN_DEBUG",
        }
    }
}

#[derive(clap::Args, Debug)]
pub struct FlowRunsArgs {
    /// The flow: its sys_id, display name, or internal name. Optional when
    /// --record is given (then every flow that ran for the record is listed).
    #[arg(required_unless_present = "record")]
    pub flow: Option<String>,
    /// Only runs triggered by this record: a `table:sys_id` or `table:number`
    /// reference (e.g. `incident:INC0010001`), or a bare sys_id.
    #[arg(long, value_name = "REF")]
    pub record: Option<String>,
    /// Only runs that ended in ERROR or recorded a caught/skipped error.
    #[arg(long, conflicts_with = "state")]
    pub errors: bool,
    /// Only runs in these states. Repeatable or comma-separated.
    #[arg(long, value_enum, value_delimiter = ',', value_name = "STATE")]
    pub state: Vec<RunState>,
    /// Only runs started within this window: a number plus s, m, h or d
    /// (e.g. `90m`, `24h`, `7d`).
    #[arg(long, value_name = "DURATION")]
    pub since: Option<String>,
    #[command(flatten)]
    pub limit: SetLimit<20>,
}

#[derive(clap::Args, Debug)]
pub struct FlowContextDebugArgs {
    /// The execution's sys_flow_context sys_id (from `sn flow runs`).
    pub context: String,
    /// How many of the newest engine log lines to include.
    #[arg(long, value_name = "N", default_value_t = 20)]
    pub log_lines: u32,
}

#[derive(clap::Args, Debug)]
pub struct FlowStepsArgs {
    /// The execution's sys_flow_context sys_id (from `sn flow runs`).
    pub context: String,
    /// Only steps whose state is ERROR.
    #[arg(long)]
    pub failed: bool,
    /// Include each step's runtime input and output values. These can carry
    /// record data; they are recorded only at reporting level FULL.
    #[arg(long)]
    pub values: bool,
}

/// `sys_flow_log.level`, lowest to highest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
#[value(rename_all = "lowercase")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(clap::Args, Debug)]
pub struct FlowLogsArgs {
    /// The execution's sys_flow_context sys_id (from `sn flow runs`).
    pub context: String,
    /// Minimum level to include (`warn` includes errors). Default: all.
    #[arg(long, value_enum)]
    pub level: Option<LogLevel>,
    #[command(flatten)]
    pub limit: SetLimit<1000>,
}

#[derive(clap::Args, Debug)]
pub struct FlowWhyNotArgs {
    /// The flow: its sys_id, display name, or internal name.
    pub flow: String,
    /// The record the flow did not run for: `table:sys_id` or `table:number`
    /// (e.g. `incident:INC0010001`).
    #[arg(long, value_name = "REF")]
    pub record: String,
}

#[derive(clap::Args, Debug)]
pub struct FlowTailArgs {
    /// The flow: its sys_id, display name, or internal name.
    pub flow: String,
    /// Only emit runs as they enter the ERROR state.
    #[arg(long)]
    pub errors: bool,
    #[command(flatten)]
    pub limits: WatchLimits,
}

pub fn run(global: &GlobalFlags, sub: FlowDebugSub) -> Result<()> {
    match sub {
        FlowDebugSub::Runs(a) => runs(global, a),
        FlowDebugSub::Debug(a) => debug(global, a),
        FlowDebugSub::Steps(a) => steps(global, a),
        FlowDebugSub::Logs(a) => logs(global, a),
        FlowDebugSub::WhyNot(a) => why_not(global, a),
        FlowDebugSub::Tail(a) => tail(global, a),
    }
}

// ---------------------------------------------------------------------------
// Shared reads
// ---------------------------------------------------------------------------

/// The context columns every verb reports. `attributes`/`plan` are left out on
/// purpose: multi-kilobyte engine JSON that answers none of these questions.
const CONTEXT_FIELDS: &str = "sys_id,name,flow,state,error_state,error_message,run_time,\
calling_source,source_table,source_record,is_test_run,reporting,sys_created_on,sys_updated_on";

/// Raw values (`sysparm_display_value=false`) and flat references: output that
/// round-trips into the next command's arguments and encoded queries, with
/// timestamps in UTC rather than the caller's locale.
fn table_rows(
    client: &Client,
    table: &str,
    query: &str,
    fields: &str,
    limit: u32,
) -> Result<Vec<Value>> {
    let pairs = vec![
        ("sysparm_query".to_string(), query.to_string()),
        ("sysparm_fields".to_string(), fields.to_string()),
        ("sysparm_limit".to_string(), limit.to_string()),
        ("sysparm_display_value".to_string(), "false".to_string()),
        (
            "sysparm_exclude_reference_link".to_string(),
            "true".to_string(),
        ),
    ];
    let resp = client.get(&format!("/api/now/table/{table}"), &pairs)?;
    Ok(match take_field(resp, "result") {
        Some(Value::Array(rows)) => rows,
        _ => Vec::new(),
    })
}

/// Row count through the Aggregate API.
fn count(client: &Client, table: &str, query: &str) -> Result<u64> {
    let mut pairs = vec![("sysparm_count".to_string(), "true".to_string())];
    if !query.is_empty() {
        pairs.push(("sysparm_query".to_string(), query.to_string()));
    }
    let resp = client.get(&format!("/api/now/stats/{table}"), &pairs)?;
    resp.pointer("/result/stats/count")
        .and_then(|c| match c {
            Value::String(s) => s.parse().ok(),
            Value::Number(n) => n.as_u64(),
            _ => None,
        })
        .ok_or_else(|| Error::Instance {
            message: format!("the {table} count came back without a number"),
            detail: None,
        })
}

/// One execution, by sys_id. A missing row and an unreadable one are the same
/// bytes under row ACLs, so the message names both.
fn fetch_context(client: &Client, context: &str) -> Result<Value> {
    let rows = table_rows(
        client,
        "sys_flow_context",
        &format!("sys_id={context}"),
        CONTEXT_FIELDS,
        1,
    )?;
    match rows.into_iter().next() {
        Some(row) if str_field(&row, "sys_id") == context => Ok(row),
        Some(_) => Err(Error::Instance {
            message: format!(
                "the sys_id={context} term was dropped by the instance; the row returned is \
                 not that execution"
            ),
            detail: None,
        }),
        None => Err(Error::Api {
            status: NO_HTTP_STATUS,
            message: format!(
                "no flow execution {context} (or not readable by this profile — \
                 sys_flow_context rows need flow_operator or admin)"
            ),
            detail: None,
            transaction_id: None,
            sn_error: None,
        }),
    }
}

fn str_field<'a>(row: &'a Value, key: &str) -> &'a str {
    row.get(key).and_then(Value::as_str).unwrap_or("")
}

/// Validate a positional context id before it is spliced into a query.
fn context_arg(context: &str) -> Result<String> {
    validate_sys_id(context)?;
    Ok(context.to_string())
}

// ---------------------------------------------------------------------------
// Flow resolution
// ---------------------------------------------------------------------------

const FLOW_FIELDS: &str =
    "sys_id,name,internal_name,type,active,status,remote_trigger_id,sys_scope,sys_class_name";

/// Refuse what would reshape the encoded query the token is spliced into.
fn validate_flow_token(token: &str) -> Result<()> {
    if token.trim().is_empty() || token.contains('^') || token.contains('\n') {
        return Err(Error::Usage(format!(
            "invalid flow '{token}': expected a sys_id, display name or internal name \
             (no '^' or newlines)"
        )));
    }
    Ok(())
}

/// A `sys_hub_flow` row from a sys_id, display name, or internal name.
///
/// Names are not unique (internal names repeat across scopes), so more than one
/// match is a usage error listing the candidates. Every returned row must
/// actually carry the name asked for — ServiceNow drops a query term it cannot
/// parse and returns unfiltered rows, and a stranger's flow must never be
/// debugged in place of the one named. `=` compares case-insensitively on the
/// instance, so the check does too.
fn resolve_flow(client: &Client, token: &str) -> Result<Value> {
    let query = if is_sys_id(token) {
        format!("sys_id={token}")
    } else {
        format!("name={token}^ORinternal_name={token}")
    };
    let rows = table_rows(client, "sys_hub_flow", &query, FLOW_FIELDS, 5)?;
    let wanted = token.to_lowercase();
    let matches = |r: &Value| {
        if is_sys_id(token) {
            str_field(r, "sys_id").eq_ignore_ascii_case(token)
        } else {
            str_field(r, "name").to_lowercase() == wanted
                || str_field(r, "internal_name").to_lowercase() == wanted
        }
    };
    if rows.iter().any(|r| !matches(r)) {
        return Err(Error::Instance {
            message: format!(
                "cannot resolve flow '{token}': the instance dropped the lookup term, so the \
                 rows returned are arbitrary"
            ),
            detail: None,
        });
    }
    match rows.len() {
        0 => Err(Error::Api {
            status: NO_HTTP_STATUS,
            message: format!("no flow named or with sys_id '{token}' (or not readable by this profile)"),
            detail: Some("flows live in sys_hub_flow; `sn table list sys_hub_flow -q nameLIKE<text> --fields sys_id,name,internal_name` finds one".into()),
            transaction_id: None,
            sn_error: None,
        }),
        1 => Ok(rows.into_iter().next().expect("one row")),
        _ => {
            let candidates: Vec<String> = rows
                .iter()
                .map(|r| {
                    format!(
                        "{} ({}, scope {})",
                        str_field(r, "sys_id"),
                        str_field(r, "internal_name"),
                        str_field(r, "sys_scope")
                    )
                })
                .collect();
            Err(Error::Usage(format!(
                "flow '{token}' is ambiguous; pass one of these sys_ids instead: {}",
                candidates.join(", ")
            )))
        }
    }
}

// ---------------------------------------------------------------------------
// runs
// ---------------------------------------------------------------------------

/// `--record`'s shapes: a `table:id` reference, or a bare sys_id (a context's
/// `source_record` is a document id, so the table is not needed to filter).
enum RecordArg {
    Ref(RecordRef),
    SysId(String),
}

fn parse_record_arg(raw: &str) -> Result<RecordArg> {
    if raw.contains(':') {
        return Ok(RecordArg::Ref(record_ref::parse_ref(raw, "table")?));
    }
    if is_sys_id(raw) {
        return Ok(RecordArg::SysId(raw.to_string()));
    }
    Err(Error::Usage(format!(
        "--record '{raw}' is neither a `table:sys_id`/`table:number` reference nor a sys_id"
    )))
}

/// `90m` → 5400 seconds. Deliberately small: the window is computed here and
/// sent as an absolute UTC timestamp, because the encoded query's relative
/// operators were measured unreliable (`RELATIVEGE@day@ago@1` matched nothing
/// while `@hour@ago@24` matched 592 rows).
pub(crate) fn parse_since(s: &str) -> Result<u64> {
    let bad = || {
        Error::Usage(format!(
            "invalid --since '{s}': expected a number followed by s, m, h or d (e.g. 90m, 24h)"
        ))
    };
    let s = s.trim();
    let unit = s.chars().last().ok_or_else(bad)?;
    let n: u64 = s[..s.len() - unit.len_utf8()].parse().map_err(|_| bad())?;
    let mult = match unit {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        'd' => 86_400,
        _ => return Err(bad()),
    };
    n.checked_mul(mult).filter(|&v| v > 0).ok_or_else(bad)
}

/// Unix seconds → `YYYY-MM-DD HH:MM:SS` in UTC, the form an encoded query
/// compares `glide_date_time` columns against (measured: REST query values are
/// read as UTC, not in the caller's timezone).
pub(crate) fn utc_timestamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// The encoded query for `runs`. `^OR` binds only to the term before it, so
/// the error alternative stays inside its own AND group.
pub(crate) fn runs_query(
    flow: Option<&str>,
    record: Option<&str>,
    errors: bool,
    states: &[RunState],
    since: Option<&str>,
) -> String {
    let mut terms: Vec<String> = Vec::new();
    if let Some(f) = flow {
        terms.push(format!("flow={f}"));
    }
    if let Some(r) = record {
        terms.push(format!("source_record={r}"));
    }
    if errors {
        terms.push("state=ERROR^ORerror_stateISNOTEMPTY".into());
    }
    if !states.is_empty() {
        let v: Vec<&str> = states.iter().map(|s| s.value()).collect();
        terms.push(format!("stateIN{}", v.join(",")));
    }
    if let Some(t) = since {
        terms.push(format!("sys_created_on>={t}"));
    }
    terms.push("ORDERBYDESCsys_created_on".into());
    terms.join("^")
}

fn runs(global: &GlobalFlags, a: FlowRunsArgs) -> Result<()> {
    // argv-only checks first: none of these should cost a network call.
    if let Some(f) = &a.flow {
        validate_flow_token(f)?;
    }
    let record = a.record.as_deref().map(parse_record_arg).transpose()?;
    let since = a
        .since
        .as_deref()
        .map(|s| {
            let secs = parse_since(s)?;
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            Ok::<_, Error>(utc_timestamp(now.saturating_sub(secs)))
        })
        .transpose()?;

    let client = connect(global)?;
    let flow_id = match &a.flow {
        Some(f) => Some(str_field(&resolve_flow(&client, f)?, "sys_id").to_string()),
        None => None,
    };
    let record_id = match record {
        Some(RecordArg::Ref(r)) => Some(r.resolve(&client)?),
        Some(RecordArg::SysId(s)) => Some(s),
        None => None,
    };
    let query = runs_query(
        flow_id.as_deref(),
        record_id.as_deref(),
        a.errors,
        &a.state,
        since.as_deref(),
    );
    let rows = table_rows(
        &client,
        "sys_flow_context",
        &query,
        CONTEXT_FIELDS,
        a.limit.setlimit,
    )?;
    write_response(global, &Value::Array(rows))
}

// ---------------------------------------------------------------------------
// steps
// ---------------------------------------------------------------------------

const STEP_FIELDS: &str = "sys_id,sys_class_name,order,state,error,start_time,end_time,\
run_time,mid_time,dependent_on,dependents";

/// A context's step rows, in execution order.
fn fetch_steps(client: &Client, context: &str, failed: bool, values: bool) -> Result<Vec<Value>> {
    let mut query = format!("context={context}");
    if failed {
        query.push_str("^state=ERROR");
    }
    query.push_str("^ORDERBYorder");
    let fields = if values {
        format!("{STEP_FIELDS},input,output")
    } else {
        STEP_FIELDS.to_string()
    };
    let rows = table_rows(client, "sys_flow_report", &query, &fields, 10_000)?;
    Ok(rows.into_iter().map(|r| shape_step(r, values)).collect())
}

/// `input`/`output` are JSON columns delivered as strings; parse them so the
/// caller gets objects rather than escaped text (left as strings when they are
/// not JSON — the engine truncates oversized values).
fn shape_step(row: Value, values: bool) -> Value {
    let Value::Object(mut m) = row else {
        return row;
    };
    if values {
        for key in ["input", "output"] {
            if let Some(Value::String(s)) = m.get(key)
                && let Ok(parsed) = serde_json::from_str::<Value>(s)
            {
                m.insert(key.to_string(), parsed);
            }
        }
    }
    Value::Object(m)
}

/// Why a context has no step rows. An empty timeline is never the answer by
/// itself: it would read as "nothing ran".
///
/// Two measured causes. Reporting `OFF` (the shipped default) writes no step
/// rows at all. And on an Australia PDI a run recorded at `FULL` *also* left
/// `sys_flow_report` empty — the engine wrote its runtime values to
/// `sys_flow_report_value` instead, which answers even an admin's filtered
/// query with 403 — so a reported run with no rows says where to look rather
/// than blaming the reporting level.
fn no_steps_note(ctx: &Value) -> String {
    let context = str_field(ctx, "sys_id");
    match str_field(ctx, "reporting") {
        "OFF" | "" => "no step data: this run was recorded at reporting level OFF (the default). \
                       Set the system property com.snc.process_flow.reporting.level to BASIC \
                       (states and timings) or FULL (adds input/output values) and reproduce \
                       the run"
            .into(),
        level => format!(
            "no step rows: the run was recorded at reporting level {level}, but this instance \
             wrote no sys_flow_report rows for it (newer releases keep execution details in \
             tables REST cannot read); open it in the UI with `sn open sys_flow_context \
             {context}`"
        ),
    }
}

fn steps(global: &GlobalFlags, a: FlowStepsArgs) -> Result<()> {
    let context = context_arg(&a.context)?;
    let client = connect(global)?;
    let ctx = fetch_context(&client, &context)?;
    let rows = fetch_steps(&client, &context, a.failed, a.values)?;
    if rows.is_empty() && !a.failed {
        return Err(Error::Api {
            status: NO_HTTP_STATUS,
            message: no_steps_note(&ctx),
            detail: Some(format!(
                "execution {context} ({}) state {}",
                str_field(&ctx, "name"),
                str_field(&ctx, "state")
            )),
            transaction_id: None,
            sn_error: None,
        });
    }
    write_response(global, &Value::Array(rows))
}

// ---------------------------------------------------------------------------
// logs
// ---------------------------------------------------------------------------

const LOG_FIELDS: &str = "order,level,action,operation,message,sys_created_on";

/// `sys_flow_log.level`'s stored values (a numeric string), per its choice list.
fn level_name(raw: &str) -> Option<&'static str> {
    match raw {
        "-1" => Some("debug"),
        "0" => Some("info"),
        "1" => Some("warn"),
        "2" => Some("error"),
        _ => None,
    }
}

/// The level filter as an encoded-query term: this level and every one above.
pub(crate) fn level_term(min: LogLevel) -> Option<String> {
    let codes: &[&str] = match min {
        LogLevel::Debug => return None,
        LogLevel::Info => &["0", "1", "2"],
        LogLevel::Warn => &["1", "2"],
        LogLevel::Error => &["2"],
    };
    Some(format!("levelIN{}", codes.join(",")))
}

/// Rename `level` to its word; an unrecognized code passes through untouched.
fn shape_log(row: Value) -> Value {
    let Value::Object(mut m) = row else {
        return row;
    };
    if let Some(Value::String(code)) = m.get("level")
        && let Some(name) = level_name(code)
    {
        m.insert("level".into(), Value::String(name.into()));
    }
    Value::Object(m)
}

fn fetch_logs(
    client: &Client,
    context: &str,
    min: Option<LogLevel>,
    newest: bool,
    limit: u32,
) -> Result<Vec<Value>> {
    let mut query = format!("context={context}");
    if let Some(term) = min.and_then(level_term) {
        query.push('^');
        query.push_str(&term);
    }
    query.push_str(if newest {
        "^ORDERBYDESCorder"
    } else {
        "^ORDERBYorder"
    });
    let mut rows = table_rows(client, "sys_flow_log", &query, LOG_FIELDS, limit)?;
    if newest {
        rows.reverse();
    }
    Ok(rows.into_iter().map(shape_log).collect())
}

fn logs(global: &GlobalFlags, a: FlowLogsArgs) -> Result<()> {
    let context = context_arg(&a.context)?;
    let client = connect(global)?;
    let rows = fetch_logs(&client, &context, a.level, false, a.limit.setlimit)?;
    write_response(global, &Value::Array(rows))
}

// ---------------------------------------------------------------------------
// debug
// ---------------------------------------------------------------------------

fn debug(global: &GlobalFlags, a: FlowContextDebugArgs) -> Result<()> {
    let context = context_arg(&a.context)?;
    let client = connect(global)?;
    let ctx = fetch_context(&client, &context)?;
    let steps = fetch_steps(&client, &context, false, false)?;
    let failed_id = steps
        .iter()
        .find(|s| str_field(s, "state") == "ERROR")
        .map(|s| str_field(s, "sys_id").to_string());
    // The failed step is re-read with its values: that is the one step whose
    // input/output a debugger wants, and fetching every step's would drag
    // record data for the whole run into the document.
    let failed_step = match failed_id {
        Some(id) => table_rows(
            &client,
            "sys_flow_report",
            &format!("sys_id={id}"),
            &format!("{STEP_FIELDS},input,output"),
            1,
        )?
        .into_iter()
        .next()
        .map(|r| shape_step(r, true)),
        None => None,
    };
    let logs = fetch_logs(&client, &context, None, true, a.log_lines)?;
    let doc = debug_document(ctx, &steps, failed_step, logs);
    write_response(global, &doc)
}

/// Assemble `debug`'s document. Pure, so the notes' rules are testable.
pub(crate) fn debug_document(
    ctx: Value,
    steps: &[Value],
    failed_step: Option<Value>,
    logs: Vec<Value>,
) -> Value {
    let mut by_state: Map<String, Value> = Map::new();
    for s in steps {
        let state = str_field(s, "state").to_string();
        let n = by_state.get(&state).and_then(Value::as_u64).unwrap_or(0);
        by_state.insert(state, json!(n + 1));
    }
    let mut notes: Vec<String> = Vec::new();
    if steps.is_empty() {
        notes.push(no_steps_note(&ctx));
    }
    let state = str_field(&ctx, "state");
    if state == "ERROR" && failed_step.is_none() && !steps.is_empty() {
        notes.push(
            "the run is in ERROR but no step is; the cause is in context.error_message".into(),
        );
    }
    let error_logs = logs
        .iter()
        .filter(|l| str_field(l, "level") == "error")
        .count();
    if error_logs > 0 && state != "ERROR" {
        notes.push(format!(
            "the run ended {state} but its log tail holds {error_logs} error line(s): a step \
             logged a failure without failing the run"
        ));
    }
    json!({
        "context": ctx,
        "failed_step": failed_step,
        "steps": {"total": steps.len(), "by_state": by_state},
        "logs": logs,
        "notes": notes,
    })
}

// ---------------------------------------------------------------------------
// why-not
// ---------------------------------------------------------------------------

const RECORD_TRIGGER_FIELDS: &str = "sys_id,sys_class_name,active,table,condition,on_insert,\
on_update,on_delete,run_on_extended,run_when_setting,run_when_user_setting";

/// One AND-clause of a condition: a term plus the `^OR` alternatives that bind
/// to it.
#[derive(Debug, PartialEq)]
pub(crate) struct Clause(pub String);

/// Split a trigger condition into `^NQ` segments of AND-clauses.
///
/// `^NQ` separates whole alternatives; inside a segment `^` is AND and a term
/// starting `OR` joins the clause before it. `EQ` (the end-of-query marker the
/// condition builder writes) and `ORDERBY` terms are dropped, and `^^` — an
/// escaped literal caret — never splits.
pub(crate) fn split_condition(cond: &str) -> Vec<Vec<Clause>> {
    const CARET: &str = "\u{0}";
    let escaped = cond.replace("^^", CARET);
    let mut segments = Vec::new();
    for seg in escaped.split("^NQ") {
        let mut clauses: Vec<Clause> = Vec::new();
        for term in seg.split('^') {
            let term = term.replace(CARET, "^^");
            if term.is_empty() || term == "EQ" || term.starts_with("ORDERBY") {
                continue;
            }
            match (term.strip_prefix("OR"), clauses.last_mut()) {
                (Some(_), Some(last)) => {
                    last.0.push('^');
                    last.0.push_str(&term);
                }
                _ => clauses.push(Clause(term)),
            }
        }
        if !clauses.is_empty() {
            segments.push(clauses);
        }
    }
    segments
}

/// The change operators, which test the triggering *write* rather than stored
/// values — a Table API query cannot evaluate them (measured: the Aggregate
/// API answers `stateCHANGESTO-5` with HTTP 400, and `VALCHANGES` matches
/// nothing).
pub(crate) fn change_operator(clause: &str) -> Option<&'static str> {
    ["VALCHANGES", "CHANGESFROM", "CHANGESTO"]
        .into_iter()
        .find(|op| clause.contains(op))
}

fn truthy(row: &Value, key: &str) -> bool {
    str_field(row, key) == "true"
}

fn why_not(global: &GlobalFlags, a: FlowWhyNotArgs) -> Result<()> {
    validate_flow_token(&a.flow)?;
    let record = record_ref::parse_ref(&a.record, "table")?;
    let client = connect(global)?;
    let flow = resolve_flow(&client, &a.flow)?;
    let flow_id = str_field(&flow, "sys_id").to_string();
    let sys_id = record.resolve(&client)?;

    let mut blockers: Vec<String> = Vec::new();
    let mut notes: Vec<String> = Vec::new();

    if !truthy(&flow, "active") {
        blockers.push("the flow is not active".into());
    }
    let status = str_field(&flow, "status");
    if status != "published" {
        blockers.push(format!(
            "the flow is not published (status '{status}'); only a published flow's trigger runs"
        ));
    }

    // Did it in fact run? Answering this first keeps a post-mortem honest: a
    // run that exists and failed is a different question (`sn flow debug`).
    let runs = table_rows(
        &client,
        "sys_flow_context",
        &format!("flow={flow_id}^source_record={sys_id}^ORDERBYDESCsys_created_on"),
        "sys_id,state,error_message,sys_created_on",
        5,
    )?;
    if !runs.is_empty() {
        notes.push(format!(
            "the flow HAS run for this record ({} recent run(s) listed under `runs`); inspect \
             one with `sn flow debug <sys_id>`",
            runs.len()
        ));
    }

    let trigger_id = str_field(&flow, "remote_trigger_id").to_string();
    let mut trigger = Value::Null;
    let mut condition = Value::Null;
    let mut record_doc = json!({"table": record.table, "sys_id": sys_id});

    if str_field(&flow, "type") == "subflow" {
        notes.push(
            "this is a subflow: it has no trigger and runs only when a flow or script calls it"
                .into(),
        );
    } else if trigger_id.is_empty() {
        blockers.push(
            "the flow has no registered runtime trigger (sys_hub_flow.remote_trigger_id is empty); \
             it was never activated with a trigger"
                .into(),
        );
    } else {
        validate_sys_id(&trigger_id)?;
        let base = table_rows(
            &client,
            "sys_flow_trigger",
            &format!("sys_id={trigger_id}"),
            "sys_id,sys_class_name,active",
            1,
        )?;
        let Some(base) = base.into_iter().next() else {
            return Err(Error::Api {
                status: NO_HTTP_STATUS,
                message: format!(
                    "the flow's runtime trigger {trigger_id} is missing (or not readable by this profile)"
                ),
                detail: None,
                transaction_id: None,
                sn_error: None,
            });
        };
        let class = str_field(&base, "sys_class_name").to_string();
        if class != "sys_flow_record_trigger" {
            trigger = base;
            if !truthy(&trigger, "active") {
                blockers.push("the flow's trigger is inactive".into());
            }
            notes.push(format!(
                "the trigger is a {class}, not a record trigger, so it cannot be evaluated \
                 against a record"
            ));
        } else {
            let t = table_rows(
                &client,
                "sys_flow_record_trigger",
                &format!("sys_id={trigger_id}"),
                RECORD_TRIGGER_FIELDS,
                1,
            )?
            .into_iter()
            .next()
            .unwrap_or(base);
            evaluate_record_trigger(
                &client,
                &t,
                &sys_id,
                &mut record_doc,
                &mut condition,
                &mut blockers,
                &mut notes,
            )?;
            trigger = t;
        }
    }

    let doc = json!({
        "flow": flow,
        "trigger": trigger,
        "record": record_doc,
        "condition": condition,
        "runs": runs,
        "blockers": blockers,
        "notes": notes,
    });
    write_response(global, &doc)
}

/// The record-trigger half of `why-not`: table membership, operations, the
/// session/user gates, and the condition clause by clause.
fn evaluate_record_trigger(
    client: &Client,
    t: &Value,
    sys_id: &str,
    record_doc: &mut Value,
    condition: &mut Value,
    blockers: &mut Vec<String>,
    notes: &mut Vec<String>,
) -> Result<()> {
    if !truthy(t, "active") {
        blockers.push("the flow's trigger is inactive".into());
    }
    let ops: Vec<&str> = [
        ("on_insert", "insert"),
        ("on_update", "update"),
        ("on_delete", "delete"),
    ]
    .into_iter()
    .filter(|(k, _)| truthy(t, k))
    .map(|(_, v)| v)
    .collect();
    notes.push(format!(
        "the trigger fires on {} only; a write of any other kind never starts it",
        if ops.is_empty() {
            "no operation".to_string()
        } else {
            ops.join("/")
        }
    ));
    match str_field(t, "run_when_setting") {
        "" | "both" => {}
        s => notes.push(format!(
            "run_when_setting is '{s}': the session kind of the triggering write gates it (not checked)"
        )),
    }
    match str_field(t, "run_when_user_setting") {
        "" | "any" => {}
        s => notes.push(format!(
            "run_when_user_setting is '{s}': the triggering user gates it (not checked)"
        )),
    }

    let table = str_field(t, "table").to_string();
    validate_identifier(&table, "trigger table")?;
    // Querying the trigger's table by sys_id also finds a record stored in a
    // child table, and its sys_class_name says which.
    let hit = table_rows(
        client,
        &table,
        &format!("sys_id={sys_id}"),
        "sys_id,sys_class_name",
        2,
    )?;
    let class = hit
        .first()
        .filter(|r| str_field(r, "sys_id") == sys_id)
        .map(|r| str_field(r, "sys_class_name").to_string());
    record_doc["trigger_table"] = json!(table);
    record_doc["sys_class_name"] = json!(class);
    let Some(class) = class else {
        blockers.push(format!(
            "the record is not in the trigger's table {table} or any table extending it (or \
             not readable by this profile)"
        ));
        return Ok(());
    };
    if !class.is_empty() && class != table && str_field(t, "run_on_extended") != "true" {
        blockers.push(format!(
            "the record's class is {class}, which extends {table}, but the trigger runs only for \
             {table} itself (run_on_extended is false)"
        ));
    }

    let cond = str_field(t, "condition");
    let segments = split_condition(cond);
    if segments.is_empty() {
        *condition = json!({"condition": cond, "matches": true, "segments": []});
        return Ok(());
    }
    let total = count(client, &table, "")?;
    let mut seg_docs = Vec::new();
    let mut any_match = false;
    let mut any_unknown = false;
    for seg in &segments {
        let mut clause_docs = Vec::new();
        let mut seg_match: Option<bool> = Some(true);
        for Clause(q) in seg {
            if let Some(op) = change_operator(q) {
                clause_docs.push(json!({
                    "clause": q,
                    "matches": Value::Null,
                    "note": format!("{op} tests the triggering write, not stored values; cannot be evaluated here"),
                }));
                if seg_match == Some(true) {
                    seg_match = None;
                }
                continue;
            }
            let hit = count(client, &table, &format!("{q}^sys_id={sys_id}"))? > 0;
            let mut doc = json!({"clause": q, "matches": hit});
            if hit && total > 1 && count(client, &table, q)? == total {
                doc["note"] = json!(format!(
                    "matches every row of {table}: either always true, or silently dropped by \
                     the instance as unparseable"
                ));
            }
            clause_docs.push(doc);
            if !hit {
                seg_match = Some(false);
            }
        }
        match seg_match {
            Some(true) => any_match = true,
            None => any_unknown = true,
            Some(false) => {}
        }
        seg_docs.push(json!({"matches": seg_match, "clauses": clause_docs}));
    }
    let overall = if any_match {
        Some(true)
    } else if any_unknown {
        None
    } else {
        Some(false)
    };
    if overall == Some(false) {
        blockers.push(
            "the trigger condition does not match the record's current values (see \
             condition.segments for the failing clause)"
                .into(),
        );
    }
    notes.push(
        "the condition is evaluated against the record as stored now; the trigger saw the \
         record as it was at the write"
            .into(),
    );
    *condition = json!({"condition": cond, "matches": overall, "segments": seg_docs});
    Ok(())
}

// ---------------------------------------------------------------------------
// tail
// ---------------------------------------------------------------------------

/// The watch channel's query for `tail`.
pub(crate) fn tail_query(flow_id: &str, errors: bool) -> String {
    if errors {
        format!("flow={flow_id}^state=ERROR")
    } else {
        format!("flow={flow_id}")
    }
}

fn tail(global: &GlobalFlags, a: FlowTailArgs) -> Result<()> {
    validate_flow_token(&a.flow)?;
    let flow_id = if is_sys_id(&a.flow) {
        a.flow.clone()
    } else {
        let client = connect(global)?;
        str_field(&resolve_flow(&client, &a.flow)?, "sys_id").to_string()
    };
    watch::run(
        global,
        WatchArgs {
            table: "sys_flow_context".into(),
            query: tail_query(&flow_id, a.errors),
            operation: Vec::new(),
            on_change: Vec::new(),
            session_rotate: 45,
            no_hydrate: false,
            limits: a.limits,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn since_parses_units() {
        assert_eq!(parse_since("90s").unwrap(), 90);
        assert_eq!(parse_since("90m").unwrap(), 5400);
        assert_eq!(parse_since("24h").unwrap(), 86_400);
        assert_eq!(parse_since("7d").unwrap(), 604_800);
        for bad in ["", "h", "10", "10w", "-1h", "0m", "1.5h"] {
            assert!(parse_since(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn utc_timestamp_formats_civil_time() {
        assert_eq!(utc_timestamp(0), "1970-01-01 00:00:00");
        // 2026-09-28 18:11:53 UTC, a context's sys_created_on on the PDI.
        assert_eq!(utc_timestamp(1_790_619_113), "2026-09-28 18:11:53");
        assert_eq!(utc_timestamp(951_782_400), "2000-02-29 00:00:00");
    }

    #[test]
    fn runs_query_keeps_the_error_alternative_in_its_own_group() {
        let q = runs_query(
            Some("f1"),
            Some("r1"),
            true,
            &[],
            Some("2026-09-28 17:00:00"),
        );
        assert_eq!(
            q,
            "flow=f1^source_record=r1^state=ERROR^ORerror_stateISNOTEMPTY^\
             sys_created_on>=2026-09-28 17:00:00^ORDERBYDESCsys_created_on"
        );
        let q = runs_query(
            None,
            None,
            false,
            &[RunState::InProgress, RunState::Waiting],
            None,
        );
        assert_eq!(q, "stateININ_PROGRESS,WAITING^ORDERBYDESCsys_created_on");
    }

    #[test]
    fn level_terms_include_everything_above() {
        assert_eq!(level_term(LogLevel::Debug), None);
        assert_eq!(level_term(LogLevel::Warn).unwrap(), "levelIN1,2");
        assert_eq!(level_term(LogLevel::Error).unwrap(), "levelIN2");
        let shaped = shape_log(json!({"level": "2", "message": "m"}));
        assert_eq!(shaped["level"], "error");
        let unknown = shape_log(json!({"level": "7"}));
        assert_eq!(unknown["level"], "7");
    }

    fn clauses(segs: &[Vec<Clause>]) -> Vec<Vec<&str>> {
        segs.iter()
            .map(|s| s.iter().map(|c| c.0.as_str()).collect())
            .collect()
    }

    #[test]
    fn condition_splits_into_nq_segments_and_or_groups() {
        let s = split_condition(
            "short_descriptionSTARTSWITHDelegate roles to^short_descriptionENDSWITHgroup",
        );
        assert_eq!(
            clauses(&s),
            vec![vec![
                "short_descriptionSTARTSWITHDelegate roles to",
                "short_descriptionENDSWITHgroup"
            ]]
        );
        let s = split_condition("a=1^b=2^ORb=3^NQc=4^EQ");
        assert_eq!(clauses(&s), vec![vec!["a=1", "b=2^ORb=3"], vec!["c=4"]]);
        assert!(split_condition("^EQ").is_empty());
        assert!(split_condition("").is_empty());
        // An escaped literal caret never splits.
        let s = split_condition("name=a^^b^x=1");
        assert_eq!(clauses(&s), vec![vec!["name=a^^b", "x=1"]]);
        // ORDERBY is not a condition term.
        let s = split_condition("a=1^ORDERBYnumber");
        assert_eq!(clauses(&s), vec![vec!["a=1"]]);
    }

    #[test]
    fn change_operators_are_detected() {
        assert_eq!(change_operator("stateCHANGESTO6"), Some("CHANGESTO"));
        assert_eq!(change_operator("stateVALCHANGES"), Some("VALCHANGES"));
        assert_eq!(change_operator("stateCHANGESFROM3"), Some("CHANGESFROM"));
        assert_eq!(change_operator("state=6"), None);
    }

    #[test]
    fn debug_notes_name_off_reporting_and_logged_errors() {
        let ctx = json!({"sys_id": "c", "state": "COMPLETE", "reporting": "OFF"});
        let logs = vec![json!({"level": "error", "message": "boom"})];
        let doc = debug_document(ctx, &[], None, logs);
        let notes = doc["notes"].as_array().unwrap();
        assert_eq!(notes.len(), 2);
        assert!(notes[0].as_str().unwrap().contains("reporting level OFF"));
        assert!(notes[1].as_str().unwrap().contains("1 error line"));
        assert_eq!(doc["steps"]["total"], 0);
        assert!(doc["failed_step"].is_null());
    }

    #[test]
    fn a_reported_run_without_rows_points_at_the_ui_not_the_property() {
        let ctx = json!({"sys_id": "c1", "state": "COMPLETE", "reporting": "FULL"});
        let doc = debug_document(ctx, &[], None, vec![]);
        let note = doc["notes"][0].as_str().unwrap();
        assert!(note.contains("reporting level FULL"), "{note}");
        assert!(note.contains("sn open sys_flow_context c1"), "{note}");
        assert!(
            !note.contains("com.snc.process_flow.reporting.level"),
            "{note}"
        );
    }

    #[test]
    fn debug_counts_steps_by_state() {
        let ctx = json!({"sys_id": "c", "state": "ERROR", "reporting": "FULL"});
        let steps = vec![
            json!({"state": "COMPLETE"}),
            json!({"state": "COMPLETE"}),
            json!({"state": "ERROR"}),
        ];
        let failed = json!({"state": "ERROR", "error": "x"});
        let doc = debug_document(ctx, &steps, Some(failed), vec![]);
        assert_eq!(doc["steps"]["by_state"]["COMPLETE"], 2);
        assert_eq!(doc["steps"]["by_state"]["ERROR"], 1);
        assert!(doc["notes"].as_array().unwrap().is_empty());
    }
}
