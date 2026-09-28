//! `sn flow` — Flow Designer flows and subflows.
//!
//! Two sources, chosen per verb:
//!
//! - **`list`** reads `sys_hub_flow` through the Table API. That table holds
//!   both flows and subflows (`type` = `flow`/`subflow`; two legacy rows on the
//!   reference instance have it empty) and excludes the snapshot rows that share
//!   the `sys_hub_flow_base` parent.
//! - **`get`/`versions`** read the Flow Designer canvas's own scripted REST API,
//!   `/api/now/processflow/` — undocumented and absent from `/api/now/doc`.
//!   Everything below was measured on a live instance (issue #48):
//!   - `GET processflow/flow/{sys_id}` answers `{"result": {"data": <model>,
//!     "errorCode", "errorMessage", "integrationsPluginActive"}}`, the model being
//!     ~70 keys (`triggerInstances`, `actionInstances`, `flowLogicInstances`,
//!     `subFlowInstances`, `inputs`/`outputs`, …). It is **slow**: 9–52 s per
//!     read on a PDI, so these verbs default to [`PROCESSFLOW_TIMEOUT_SECS`]
//!     rather than the global 30 s, which cut the first measured read off.
//!   - A missing flow is HTTP 404 whose body is *not* the platform's `error`
//!     envelope but `{"result": {"errorMessage": "Flow <id> not found.",
//!     "errorCode": 0}}` — [`promote`] lifts that message into the error. The
//!     item URL accepts only a sys_id; an internal name 404s the same way, so
//!     names are resolved against `sys_hub_flow` first ([`resolve_flow`]).
//!   - `GET processflow/versioning/{sys_id}` answers `result.data` as an array —
//!     and an **empty array under HTTP 200 for a sys_id that names nothing**, so
//!     an empty history is only reported after the flow's existence is checked.
//!   - A caller without Flow Designer rights gets 403 "User Not Authorized"
//!     (exit 4) from `processflow` while `sys_hub_flow` stays Table-API readable.
//!
//! The execution-debugging verbs (`runs`, `debug`, `steps`, `logs`, `why-not`,
//! `tail`) live in [`debug`], which reads the documented flow-engine tables and
//! is flattened into [`FlowSub`] so its verbs sit directly under `sn flow`.
//!
//! Writes (`POST`/`PUT processflow/flow`, the `snFlowDesigner.flowPatch` GraphQL
//! mutation) are deliberately not wired: they could not be measured end to end
//! on a throwaway flow (the reference instance refused creation mid-upgrade:
//! 500 "Unable to create new flows during system upgrade"), and the model is a
//! denormalized snapshot whose round-trip must be proven before a CLI writes it.

use crate::cli::kernel::{build_client, build_profile, connect, emit, write_response};
use crate::cli::record_ref::is_sys_id;
use crate::cli::{GlobalFlags, OutputMode, Paging};
use crate::client::Client;
use crate::error::{Error, NO_HTTP_STATUS, Result};
use clap::{Subcommand, ValueEnum};
use serde_json::{Map, Value, json};

pub mod debug;
use std::collections::HashMap;

/// Default request timeout for the `processflow` reads when `--timeout` is not
/// given. Measured reads took 9–52 s; 180 s leaves headroom for larger models
/// (~500 KB for an 11-action flow) without hanging forever on a dead socket.
pub(crate) const PROCESSFLOW_TIMEOUT_SECS: u64 = 180;

const PROCESSFLOW: &str = "/api/now/processflow";

/// Fields `sn flow list` returns unless `--fields` overrides them.
const LIST_FIELDS: &str =
    "sys_id,name,internal_name,type,status,active,sys_scope.scope,sys_updated_on,sys_updated_by";

#[derive(Subcommand, Debug)]
pub enum FlowSub {
    /// List flows and subflows (Table API over sys_hub_flow).
    List(FlowListArgs),
    /// The full Flow Designer model of one flow or subflow, or a compact outline of it.
    ///
    /// Reads the designer's own undocumented `/api/now/processflow` API, which is slow
    /// (tens of seconds per flow), so the request timeout here defaults to 180s unless
    /// --timeout is given. Needs Flow Designer rights: a caller without them gets 403 (exit 4).
    Get(FlowGetArgs),
    /// A flow's version history (saves and publishes).
    ///
    /// Same `/api/now/processflow` API and 180s default timeout as `get`.
    Versions(FlowVersionsArgs),
    #[command(flatten)]
    Debug(debug::FlowDebugSub),
}

/// Dispatch one `sn flow` verb.
pub fn run(global: &GlobalFlags, sub: FlowSub) -> Result<()> {
    match sub {
        FlowSub::List(args) => list(global, args),
        FlowSub::Get(args) => get(global, args),
        FlowSub::Versions(args) => versions(global, args),
        FlowSub::Debug(sub) => debug::run(global, sub),
    }
}

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
#[value(rename_all = "lowercase")]
pub enum FlowType {
    Flow,
    Subflow,
}

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
#[value(rename_all = "lowercase")]
pub enum FlowStatus {
    Draft,
    Published,
}

#[derive(clap::Args, Debug)]
pub struct FlowListArgs {
    /// Only flows in this application scope, by its namespace (e.g. `global`, `x_acme_app`, `sn_itsm`).
    #[arg(long)]
    pub scope: Option<String>,
    /// Only flows or only subflows.
    #[arg(long = "type", value_enum)]
    pub flow_type: Option<FlowType>,
    /// Only draft or only published flows.
    #[arg(long, value_enum)]
    pub status: Option<FlowStatus>,
    /// Only active flows.
    #[arg(long)]
    pub active: bool,
    /// Extra encoded query ANDed onto the filters above, e.g. `nameLIKEincident`. Values come back raw (display values off).
    #[arg(long, short = 'q', alias = "sysparm-query")]
    pub query: Option<String>,
    /// Comma-separated fields to return instead of the default summary columns.
    #[arg(long, short = 'f', alias = "sysparm-fields")]
    pub fields: Option<String>,
    #[command(flatten)]
    pub paging: Paging<100>,
}

#[derive(clap::Args, Debug)]
pub struct FlowGetArgs {
    /// The flow: its sys_id, its internal name (e.g. `send_email`), or a scope-qualified internal name (`global.send_email`).
    pub flow: String,
    /// Emit a compact outline — header, inputs/outputs, triggers, and the ordered, nested steps — instead of the full model.
    #[arg(long)]
    pub outline: bool,
}

#[derive(clap::Args, Debug)]
pub struct FlowVersionsArgs {
    /// The flow: its sys_id, its internal name, or a scope-qualified internal name (`scope.internal_name`).
    pub flow: String,
}

/// How the caller named a flow. Parsed from argv alone (exit 1 on a bad
/// token), so nothing unvalidated is ever spliced into an encoded query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FlowRef {
    SysId(String),
    Name {
        scope: Option<String>,
        internal_name: String,
    },
}

/// Parse a FLOW positional: a 32-hex sys_id, an internal name, or
/// `scope.internal_name`. Internal names and scope namespaces are restricted to
/// `[A-Za-z0-9_-]` — the charset every one on the reference instance uses — so
/// the token can carry no `^`, `=` or `.` into the lookup query.
pub(crate) fn parse_flow_ref(token: &str) -> Result<FlowRef> {
    if is_sys_id(token) {
        return Ok(FlowRef::SysId(token.to_ascii_lowercase()));
    }
    let (scope, name) = match token.split_once('.') {
        Some((scope, name)) => (Some(scope), name),
        None => (None, token),
    };
    let valid = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    };
    if !valid(name) || scope.is_some_and(|s| !valid(s)) {
        return Err(Error::Usage(format!(
            "'{token}' is not a flow reference: pass a 32-character sys_id, an internal \
             name (e.g. send_email), or scope.internal_name (e.g. global.send_email)"
        )));
    }
    Ok(FlowRef::Name {
        scope: scope.map(str::to_string),
        internal_name: name.to_string(),
    })
}

/// A flow's `sys_hub_flow` sys_id. Free for a sys_id; one Table API lookup for
/// a name. `internal_name` is **not** unique — it repeats across scopes
/// (`send_email` exists in both `global` and `sn_creatorstudio` on the
/// reference instance) — so several matches are an ambiguity for the caller to
/// settle with a scope qualifier, not a pick. Every returned row is checked
/// against the requested name and scope: a row that does not match proves the
/// instance dropped a query term, and the rows are then arbitrary.
pub(crate) fn resolve_flow(client: &Client, flow: &FlowRef) -> Result<String> {
    let (scope, name) = match flow {
        FlowRef::SysId(id) => return Ok(id.clone()),
        FlowRef::Name {
            scope,
            internal_name,
        } => (scope.as_deref(), internal_name.as_str()),
    };
    let mut query = format!("internal_name={name}");
    if let Some(scope) = scope {
        query.push_str(&format!("^sys_scope.scope={scope}"));
    }
    let pairs = vec![
        ("sysparm_query".to_string(), query),
        (
            "sysparm_fields".to_string(),
            "sys_id,name,internal_name,sys_scope.scope".to_string(),
        ),
        ("sysparm_limit".to_string(), "10".to_string()),
        ("sysparm_display_value".to_string(), "false".to_string()),
        (
            "sysparm_exclude_reference_link".to_string(),
            "true".to_string(),
        ),
    ];
    let resp = client.get("/api/now/table/sys_hub_flow", &pairs)?;
    let rows = resp
        .get("result")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    pick_flow(&rows, scope, name)
}

/// The decision half of [`resolve_flow`], separated so it is testable without
/// a server.
fn pick_flow(rows: &[Value], scope: Option<&str>, name: &str) -> Result<String> {
    let text = |row: &Value, key: &str| {
        row.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let qualified = match scope {
        Some(s) => format!("{s}.{name}"),
        None => name.to_string(),
    };
    let stray = rows.iter().any(|row| {
        text(row, "internal_name") != name
            || scope.is_some_and(|s| text(row, "sys_scope.scope") != s)
    });
    if stray {
        return Err(Error::Instance {
            message: format!(
                "cannot resolve flow '{qualified}': the instance returned flows that do not \
                 match the lookup, so the query was not applied"
            ),
            detail: Some(Value::Array(rows.to_vec()).to_string()),
        });
    }
    match rows {
        [] => Err(Error::Api {
            // The lookup succeeded and matched nothing — no HTTP verdict to report.
            status: NO_HTTP_STATUS,
            message: format!(
                "no flow with internal name '{qualified}' (or not readable by this profile)"
            ),
            detail: None,
            transaction_id: None,
            sn_error: None,
        }),
        [row] => {
            let id = text(row, "sys_id");
            if id.is_empty() {
                return Err(Error::Instance {
                    message: format!("the flow matching '{qualified}' came back without a sys_id"),
                    detail: Some(row.to_string()),
                });
            }
            Ok(id)
        }
        many => {
            let list = many
                .iter()
                .map(|r| {
                    format!(
                        "{}.{name} ({})",
                        text(r, "sys_scope.scope"),
                        text(r, "sys_id")
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            Err(Error::Usage(format!(
                "flow internal name '{qualified}' is ambiguous — it matches {list}; \
                 qualify it as scope.internal_name or pass the sys_id"
            )))
        }
    }
}

pub fn list(global: &GlobalFlags, args: FlowListArgs) -> Result<()> {
    let query = list_query(&args)?;
    let client = connect(global)?;
    let mut pairs = vec![
        ("sysparm_query".to_string(), query),
        (
            "sysparm_fields".to_string(),
            args.fields.unwrap_or_else(|| LIST_FIELDS.to_string()),
        ),
        (
            "sysparm_limit".to_string(),
            args.paging.setlimit().to_string(),
        ),
        ("sysparm_display_value".to_string(), "false".to_string()),
        (
            "sysparm_exclude_reference_link".to_string(),
            "true".to_string(),
        ),
    ];
    if let Some(offset) = args.paging.offset {
        pairs.push(("sysparm_offset".to_string(), offset.to_string()));
    }
    let resp = client.get("/api/now/table/sys_hub_flow", &pairs)?;
    emit(global, resp)
}

/// The encoded query for `sn flow list`: the filter flags, then `--query`,
/// then a name ordering. A `--query` holding `^NQ` is refused when a filter
/// flag is also set, because ServiceNow applies AND terms to one `^NQ`
/// segment only — the flags would silently filter half the union.
fn list_query(args: &FlowListArgs) -> Result<String> {
    let mut terms: Vec<String> = Vec::new();
    if let Some(scope) = &args.scope {
        if scope.is_empty()
            || !scope
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(Error::Usage(format!(
                "--scope '{scope}' is not a scope namespace (e.g. global, x_acme_app)"
            )));
        }
        terms.push(format!("sys_scope.scope={scope}"));
    }
    if let Some(t) = args.flow_type {
        terms.push(
            match t {
                FlowType::Flow => "type=flow",
                FlowType::Subflow => "type=subflow",
            }
            .to_string(),
        );
    }
    if let Some(s) = args.status {
        terms.push(
            match s {
                FlowStatus::Draft => "status=draft",
                FlowStatus::Published => "status=published",
            }
            .to_string(),
        );
    }
    if args.active {
        terms.push("active=true".to_string());
    }
    if let Some(q) = args.query.as_deref().filter(|q| !q.is_empty()) {
        if !terms.is_empty() && q.contains("^NQ") {
            return Err(Error::Usage(
                "--query with ^NQ cannot be combined with --scope/--type/--status/--active: \
                 the filters would apply to only one NQ segment. Put every term in --query, \
                 or use `sn table list sys_hub_flow -q ...`"
                    .into(),
            ));
        }
        terms.push(q.to_string());
    }
    terms.push("ORDERBYname".to_string());
    Ok(terms.join("^"))
}

/// A client for the `processflow` reads: the global `--timeout` when given,
/// [`PROCESSFLOW_TIMEOUT_SECS`] otherwise.
fn processflow_client(global: &GlobalFlags) -> Result<Client> {
    let profile = build_profile(global)?;
    build_client(
        &profile,
        Some(global.timeout.unwrap_or(PROCESSFLOW_TIMEOUT_SECS)),
    )
}

pub fn get(global: &GlobalFlags, args: FlowGetArgs) -> Result<()> {
    let flow = parse_flow_ref(&args.flow)?;
    if args.outline && global.output == OutputMode::Raw {
        return Err(Error::Usage(
            "--outline is a derived view with no envelope to keep; drop --output raw \
             (or drop --outline for the raw model)"
                .into(),
        ));
    }
    let client = processflow_client(global)?;
    let sys_id = resolve_flow(&client, &flow)?;
    let resp = client
        .get(&format!("{PROCESSFLOW}/flow/{sys_id}"), &[])
        .map_err(promote)?;
    let data = checked_data(&resp)?;
    if args.outline {
        return write_response(global, &outline(data));
    }
    emit_data(global, resp)
}

pub fn versions(global: &GlobalFlags, args: FlowVersionsArgs) -> Result<()> {
    let flow = parse_flow_ref(&args.flow)?;
    let client = processflow_client(global)?;
    let sys_id = resolve_flow(&client, &flow)?;
    let resp = client
        .get(&format!("{PROCESSFLOW}/versioning/{sys_id}"), &[])
        .map_err(promote)?;
    let empty = checked_data(&resp)?
        .as_array()
        .is_some_and(|a| a.is_empty());
    // The endpoint answers an unknown sys_id with `[]` under HTTP 200, the same
    // bytes as a real flow with no history. A name was already proven to exist
    // by resolution; a bare sys_id has to be checked before `[]` is believable.
    if empty && matches!(flow, FlowRef::SysId(_)) {
        ensure_flow_exists(&client, &sys_id)?;
    }
    emit_data(global, resp)
}

/// One Table API read of the flow's row. `sys_hub_flow_base` rather than
/// `sys_hub_flow` so a snapshot's sys_id (which `processflow` also serves) is
/// not reported missing.
fn ensure_flow_exists(client: &Client, sys_id: &str) -> Result<()> {
    let pairs = vec![("sysparm_fields".to_string(), "sys_id".to_string())];
    match client.get(
        &format!("/api/now/table/sys_hub_flow_base/{sys_id}"),
        &pairs,
    ) {
        Ok(_) => Ok(()),
        Err(Error::Api {
            status: 404,
            transaction_id,
            sn_error,
            ..
        }) => Err(Error::Api {
            status: 404,
            message: format!("no flow with sys_id {sys_id} (or not readable by this profile)"),
            detail: None,
            transaction_id,
            sn_error,
        }),
        Err(e) => Err(e),
    }
}

/// `result.data` of a `processflow` response, after the in-band error check:
/// a non-empty `result.errorMessage` is a failure even under HTTP 200, and it
/// is reported with that real status (see CLAUDE.md, "Exit codes").
fn checked_data(resp: &Value) -> Result<&Value> {
    let result = resp.get("result").ok_or_else(|| Error::Instance {
        message: "processflow response has no `result` envelope".into(),
        detail: Some(truncate(&resp.to_string())),
    })?;
    if let Some(msg) = result
        .get("errorMessage")
        .and_then(Value::as_str)
        .filter(|m| !m.is_empty())
    {
        return Err(Error::Api {
            status: 200,
            message: msg.to_string(),
            detail: None,
            transaction_id: None,
            sn_error: Some(without_data(result)),
        });
    }
    result.get("data").ok_or_else(|| Error::Instance {
        message: "processflow response has no `result.data`".into(),
        detail: Some(truncate(&result.to_string())),
    })
}

/// `result.data` by default; the whole envelope under `--output raw`.
fn emit_data(global: &GlobalFlags, resp: Value) -> Result<()> {
    match global.output {
        OutputMode::Raw => emit(global, resp),
        OutputMode::Default | OutputMode::Table => {
            let data = crate::cli::kernel::take_field(
                crate::cli::kernel::take_field(resp, "result").unwrap_or(Value::Null),
                "data",
            )
            .unwrap_or(Value::Null);
            write_response(global, &data)
        }
    }
}

/// A `processflow` HTTP error carries its reason as `result.errorMessage`
/// inside a body the client could not read as the platform's `error`
/// envelope, so it arrives as the generic "HTTP 404 Not Found" with the body
/// in `detail`. Lift the message (and the parsed body into `sn_error`).
fn promote(err: Error) -> Error {
    let Error::Api {
        status,
        message,
        detail,
        transaction_id,
        sn_error,
    } = err
    else {
        return err;
    };
    let result = detail
        .as_deref()
        .and_then(|d| serde_json::from_str::<Value>(d).ok())
        .and_then(|v| v.get("result").cloned());
    let lifted = result.as_ref().and_then(|r| {
        r.get("errorMessage")
            .and_then(Value::as_str)
            .filter(|m| !m.is_empty())
            .map(str::to_string)
    });
    match lifted {
        Some(msg) => Error::Api {
            status,
            message: msg,
            detail: None,
            transaction_id,
            sn_error: result,
        },
        None => Error::Api {
            status,
            message,
            detail,
            transaction_id,
            sn_error,
        },
    }
}

fn without_data(result: &Value) -> Value {
    match result {
        Value::Object(m) => Value::Object(
            m.iter()
                .filter(|(k, _)| k.as_str() != "data")
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn truncate(s: &str) -> String {
    const MAX: usize = 500;
    if s.chars().count() <= MAX {
        s.to_string()
    } else {
        s.chars().take(MAX).collect::<String>() + "…"
    }
}

/// The compact view of a flow model: what it is, what it takes and returns,
/// what starts it, and its steps in execution order with their nesting.
///
/// Steps merge `actionInstances`, `flowLogicInstances` and `subFlowInstances`
/// (each carries a numeric-string `order` that is global across the three),
/// and `depth`/`parent` come from the `parent` → `uiUniqueIdentifier` wiring:
/// an action inside an If block names the block's `uiUniqueIdentifier` as its
/// `parent`. `parent` in the outline is the enclosing step's `order`.
pub(crate) fn outline(data: &Value) -> Value {
    let s = |key: &str| data.get(key).cloned().unwrap_or(Value::Null);
    let io = |key: &str| -> Value {
        Value::Array(
            array(data, key)
                .iter()
                .map(|v| {
                    json!({
                        "name": v.get("name").cloned().unwrap_or(Value::Null),
                        "label": v.get("label").cloned().unwrap_or(Value::Null),
                        "type": v.get("type").cloned().unwrap_or(Value::Null),
                    })
                })
                .collect(),
        )
    };
    let triggers: Vec<Value> = array(data, "triggerInstances")
        .iter()
        .filter(|t| !is_deleted(t))
        .map(|t| {
            let inputs: Map<String, Value> = array(t, "inputs")
                .iter()
                .filter_map(|i| {
                    let name = i.get("name")?.as_str()?;
                    let value = i.get("value")?;
                    let blank = value.is_null() || value.as_str().is_some_and(|v| v.is_empty());
                    (!blank).then(|| (name.to_string(), value.clone()))
                })
                .collect();
            json!({
                "name": t.get("name").cloned().unwrap_or(Value::Null),
                "type": t.get("type").cloned().unwrap_or(Value::Null),
                "trigger_type": t.get("triggerType").cloned().unwrap_or(Value::Null),
                "inputs": inputs,
            })
        })
        .collect();

    json!({
        "id": s("id"),
        "name": s("name"),
        "internal_name": s("internalName"),
        "type": s("type"),
        "status": s("status"),
        "active": s("active"),
        "scope": s("scopeName"),
        "version": s("version"),
        "run_as": s("runAs"),
        "description": s("description"),
        "updated": s("updated"),
        "updated_by": s("updatedBy"),
        "inputs": io("inputs"),
        "outputs": io("outputs"),
        "triggers": triggers,
        "steps": steps(data),
    })
}

fn array<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn is_deleted(v: &Value) -> bool {
    v.get("deleted").and_then(Value::as_bool).unwrap_or(false)
}

fn steps<'d>(data: &'d Value) -> Vec<Value> {
    struct Step<'a> {
        order: Option<u64>,
        kind: &'static str,
        ui: &'a str,
        parent: &'a str,
        v: &'a Value,
    }
    let mut all: Vec<Step<'d>> = Vec::new();
    for (key, kind) in [
        ("actionInstances", "action"),
        ("flowLogicInstances", "flowlogic"),
        ("subFlowInstances", "subflow"),
    ] {
        for v in array(data, key).iter().filter(|v| !is_deleted(v)) {
            all.push(Step {
                order: order_of(v),
                kind,
                ui: str_of(v, "uiUniqueIdentifier"),
                parent: str_of(v, "parent"),
                v,
            });
        }
    }
    all.sort_by_key(|s| s.order.unwrap_or(u64::MAX));

    let parents: HashMap<&str, &str> = all
        .iter()
        .filter(|s| !s.ui.is_empty())
        .map(|s| (s.ui, s.parent))
        .collect();
    let orders: HashMap<&str, Option<u64>> = all
        .iter()
        .filter(|s| !s.ui.is_empty())
        .map(|s| (s.ui, s.order))
        .collect();
    // Bounded walk: a malformed model with a parent cycle must not hang.
    fn depth_of<'p>(parents: &HashMap<&'p str, &'p str>, mut parent: &'p str) -> u64 {
        let mut d = 0;
        while !parent.is_empty() && d < 64 {
            d += 1;
            parent = parents.get(parent).copied().unwrap_or("");
        }
        d
    }

    all.iter()
        .map(|s| {
            let mut step = Map::new();
            step.insert("order".into(), s.order.map_or(Value::Null, Value::from));
            step.insert("depth".into(), Value::from(depth_of(&parents, s.parent)));
            step.insert("kind".into(), Value::from(s.kind));
            step.insert(
                "name".into(),
                s.v.get("name").cloned().unwrap_or(Value::Null),
            );
            let internal = str_of(s.v, "internalName");
            if !internal.is_empty() {
                step.insert("internal_name".into(), Value::from(internal));
            }
            step.insert(
                "parent".into(),
                orders
                    .get(s.parent)
                    .copied()
                    .flatten()
                    .map_or(Value::Null, Value::from),
            );
            step.insert("id".into(), s.v.get("id").cloned().unwrap_or(Value::Null));
            Value::Object(step)
        })
        .collect()
}

fn str_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn order_of(v: &Value) -> Option<u64> {
    match v.get("order")? {
        Value::String(s) => s.trim().parse().ok(),
        Value::Number(n) => n.as_u64(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEX: &str = "7d124cc7b7e8f2107df5c0cd2e11a9f6";

    #[test]
    fn a_flow_ref_is_a_sys_id_a_name_or_a_scoped_name() {
        assert_eq!(parse_flow_ref(HEX).unwrap(), FlowRef::SysId(HEX.into()));
        assert_eq!(
            parse_flow_ref("send_email").unwrap(),
            FlowRef::Name {
                scope: None,
                internal_name: "send_email".into()
            }
        );
        assert_eq!(
            parse_flow_ref("global.send_email").unwrap(),
            FlowRef::Name {
                scope: Some("global".into()),
                internal_name: "send_email".into()
            }
        );
        // Hyphens occur in real internal names.
        assert!(parse_flow_ref("create_ai_asset_off-boarding_task").is_ok());
    }

    #[test]
    fn a_flow_ref_cannot_smuggle_query_syntax() {
        for bad in [
            "",
            "send_email^active=true",
            "a=b",
            "x.y.z",
            ".send_email",
            "global.",
            "has space",
        ] {
            assert!(
                matches!(parse_flow_ref(bad), Err(Error::Usage(_))),
                "{bad:?} should be refused"
            );
        }
    }

    fn row(id: &str, name: &str, scope: &str) -> Value {
        json!({"sys_id": id, "internal_name": name, "sys_scope.scope": scope})
    }

    #[test]
    fn one_matching_row_resolves() {
        let rows = [row("abc", "send_email", "global")];
        assert_eq!(pick_flow(&rows, None, "send_email").unwrap(), "abc");
        assert_eq!(
            pick_flow(&rows, Some("global"), "send_email").unwrap(),
            "abc"
        );
    }

    #[test]
    fn no_row_is_a_statusless_not_found() {
        match pick_flow(&[], None, "nope") {
            Err(Error::Api {
                status, message, ..
            }) => {
                assert_eq!(status, NO_HTTP_STATUS);
                assert!(message.contains("'nope'"), "{message}");
            }
            other => panic!("expected Api, got {other:?}"),
        }
    }

    #[test]
    fn a_name_in_two_scopes_is_ambiguous_and_names_both() {
        let rows = [
            row("a1", "send_email", "sn_creatorstudio"),
            row("b2", "send_email", "global"),
        ];
        match pick_flow(&rows, None, "send_email") {
            Err(Error::Usage(msg)) => {
                assert!(msg.contains("sn_creatorstudio.send_email (a1)"), "{msg}");
                assert!(msg.contains("global.send_email (b2)"), "{msg}");
            }
            other => panic!("expected Usage, got {other:?}"),
        }
    }

    #[test]
    fn a_row_that_does_not_match_proves_the_term_was_dropped() {
        let rows = [row("zz", "some_other_flow", "global")];
        assert!(matches!(
            pick_flow(&rows, None, "send_email"),
            Err(Error::Instance { .. })
        ));
        let rows = [row("zz", "send_email", "sn_creatorstudio")];
        assert!(matches!(
            pick_flow(&rows, Some("global"), "send_email"),
            Err(Error::Instance { .. })
        ));
    }

    fn list_args() -> FlowListArgs {
        FlowListArgs {
            scope: None,
            flow_type: None,
            status: None,
            active: false,
            query: None,
            fields: None,
            paging: Paging {
                limit: crate::cli::SetLimit { setlimit: 100 },
                offset: None,
            },
        }
    }

    #[test]
    fn list_query_orders_filters_then_query_then_sort() {
        let mut a = list_args();
        assert_eq!(list_query(&a).unwrap(), "ORDERBYname");
        a.scope = Some("global".into());
        a.flow_type = Some(FlowType::Subflow);
        a.status = Some(FlowStatus::Published);
        a.active = true;
        a.query = Some("nameLIKEemail".into());
        assert_eq!(
            list_query(&a).unwrap(),
            "sys_scope.scope=global^type=subflow^status=published^active=true^nameLIKEemail^ORDERBYname"
        );
    }

    #[test]
    fn list_query_refuses_nq_beside_filters_and_bad_scopes() {
        let mut a = list_args();
        a.query = Some("type=flow^NQtype=subflow".into());
        assert!(list_query(&a).is_ok(), "NQ alone is the caller's own query");
        a.active = true;
        assert!(matches!(list_query(&a), Err(Error::Usage(_))));
        let mut b = list_args();
        b.scope = Some("global^active=false".into());
        assert!(matches!(list_query(&b), Err(Error::Usage(_))));
    }

    #[test]
    fn in_band_error_message_fails_even_under_200() {
        let resp = json!({"result": {"data": null, "errorCode": 0, "errorMessage": "boom"}});
        match checked_data(&resp) {
            Err(Error::Api {
                status,
                message,
                sn_error,
                ..
            }) => {
                assert_eq!(status, 200);
                assert_eq!(message, "boom");
                assert!(sn_error.unwrap().get("data").is_none());
            }
            other => panic!("expected Api, got {other:?}"),
        }
        let ok = json!({"result": {"data": {"id": "x"}, "errorCode": 0, "errorMessage": ""}});
        assert_eq!(checked_data(&ok).unwrap()["id"], "x");
    }

    #[test]
    fn promote_lifts_the_processflow_error_message() {
        let err = Error::Api {
            status: 404,
            message: "HTTP 404 Not Found".into(),
            detail: Some(
                r#"{"result":{"errorMessage":"Flow abc not found.","errorCode":0,"integrationsPluginActive":false}}"#
                    .into(),
            ),
            transaction_id: Some("tx".into()),
            sn_error: None,
        };
        match promote(err) {
            Error::Api {
                status,
                message,
                detail,
                transaction_id,
                sn_error,
            } => {
                assert_eq!(status, 404);
                assert_eq!(message, "Flow abc not found.");
                assert!(detail.is_none());
                assert_eq!(transaction_id.as_deref(), Some("tx"));
                assert_eq!(sn_error.unwrap()["errorCode"], 0);
            }
            other => panic!("expected Api, got {other:?}"),
        }
        // Anything else passes through untouched.
        let other = Error::Api {
            status: 500,
            message: "m".into(),
            detail: Some("not json".into()),
            transaction_id: None,
            sn_error: None,
        };
        assert!(matches!(promote(other), Error::Api { message, .. } if message == "m"));
    }

    /// Shape taken from the live "Policy Review" flow: a subflow, then an If
    /// holding a nested If/End and an Else, each wired by `parent` →
    /// `uiUniqueIdentifier`.
    fn model() -> Value {
        json!({
            "id": "f1", "name": "Policy Review", "internalName": "policy_review",
            "type": "flow", "active": true, "scopeName": "sn_grc", "version": "3",
            "inputs": [], "outputs": [{"name": "o", "label": "O", "type": "string", "extra": 1}],
            "triggerInstances": [{
                "name": "Created or Updated", "type": "record_create_or_update",
                "triggerType": "Record", "deleted": false,
                "inputs": [
                    {"name": "table", "value": "sn_grc_policy", "displayValue": "Policy"},
                    {"name": "condition", "value": "state=published"},
                    {"name": "run_when_user_list", "value": ""}
                ]
            }],
            "subFlowInstances": [
                {"order": "1", "name": "Add a Pause", "internalName": "add_a_pause", "parent": "", "uiUniqueIdentifier": "u1", "id": "s1"}
            ],
            "flowLogicInstances": [
                {"order": "2", "name": "If: published", "parent": "", "uiUniqueIdentifier": "u2", "id": "l2", "internalName": ""},
                {"order": "3", "name": "If: reviewers", "parent": "u2", "uiUniqueIdentifier": "u3", "id": "l3"},
                {"order": "5", "name": "End: ", "parent": "u3", "uiUniqueIdentifier": "u5", "id": "l5"},
                {"order": "6", "name": "Else: ", "parent": "u2", "uiUniqueIdentifier": "u6", "id": "l6"},
                {"order": "9", "name": "gone", "parent": "", "uiUniqueIdentifier": "u9", "id": "l9", "deleted": true}
            ],
            "actionInstances": [
                {"order": "4", "name": "Update Record", "internalName": "update_record", "parent": "u3", "uiUniqueIdentifier": "u4", "id": "a4"},
                {"order": "7", "name": "Update Record", "internalName": "update_record", "parent": "u6", "uiUniqueIdentifier": "u7", "id": "a7"}
            ]
        })
    }

    #[test]
    fn outline_orders_and_nests_steps() {
        let o = outline(&model());
        let steps = o["steps"].as_array().unwrap();
        let orders: Vec<u64> = steps.iter().map(|s| s["order"].as_u64().unwrap()).collect();
        assert_eq!(orders, [1, 2, 3, 4, 5, 6, 7], "deleted steps are dropped");
        let depths: Vec<u64> = steps.iter().map(|s| s["depth"].as_u64().unwrap()).collect();
        assert_eq!(depths, [0, 0, 1, 2, 2, 1, 2]);
        assert_eq!(steps[0]["kind"], "subflow");
        assert_eq!(steps[0]["internal_name"], "add_a_pause");
        assert_eq!(steps[1]["kind"], "flowlogic");
        assert!(steps[1].get("internal_name").is_none(), "empty is omitted");
        assert_eq!(steps[3]["kind"], "action");
        assert_eq!(steps[3]["parent"], 3);
        assert_eq!(steps[0]["parent"], Value::Null);
    }

    #[test]
    fn outline_header_io_and_trigger_inputs() {
        let o = outline(&model());
        assert_eq!(o["name"], "Policy Review");
        assert_eq!(o["internal_name"], "policy_review");
        assert_eq!(o["scope"], "sn_grc");
        // A key the model omits (status, on this flow) is null, not invented.
        assert_eq!(o["status"], Value::Null);
        assert_eq!(
            o["outputs"],
            json!([{"name": "o", "label": "O", "type": "string"}])
        );
        let t = &o["triggers"][0];
        assert_eq!(t["trigger_type"], "Record");
        assert_eq!(
            t["inputs"],
            json!({"table": "sn_grc_policy", "condition": "state=published"})
        );
    }

    #[test]
    fn outline_survives_a_parent_cycle_and_missing_keys() {
        let v = json!({"actionInstances": [
            {"order": "1", "parent": "b", "uiUniqueIdentifier": "a"},
            {"order": "x", "parent": "a", "uiUniqueIdentifier": "b"}
        ]});
        let o = outline(&v);
        assert_eq!(o["steps"].as_array().unwrap().len(), 2);
        let empty = outline(&json!({}));
        assert_eq!(empty["steps"], json!([]));
        assert_eq!(empty["triggers"], json!([]));
    }
}
