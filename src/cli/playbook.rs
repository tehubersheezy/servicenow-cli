//! `sn playbook` — playbook executions on a record, over the Playbook
//! Experience GraphQL API (`snPlaybookExp`, scope `sn_playbook_exp`).
//!
//! The schema is the backend of the workspace "Playbook" panel and ships with
//! the Playbook Experience application; it is a scripted GraphQL schema on the
//! shared `POST /api/now/graphql` endpoint, so this module rides
//! `graphql.rs`'s transport and in-band error mapping. Everything below was
//! read off a live instance (dev421992, Australia) — the SDL from
//! `sys_graphql_schema`, the behavior from its resolvers and from probing:
//!
//! - **Union member names are namespaced.** The SDL says `PlaybookContext |
//!   TriggerPlaybookError`; the wire names are
//!   `snPlaybookExp_playbook_PlaybookContext` / `…_TriggerPlaybookError`, and
//!   an inline fragment on the bare name is a validation error.
//! - **Typed failures arrive as data, not `errors`.** `triggerPlaybook` and
//!   `launchPlaybook` answer every expected failure (bad table, missing record,
//!   bad definition, missing permission, a playbook-engine exception) as a
//!   `TriggerPlaybookError` member with an `errorType` enum, under a clean 200
//!   with no `errors` array. They exit 2 here with the typed object in
//!   `sn_error`, so a caller branches on `sn_error.errorType` rather than
//!   parsing prose.
//! - **`getPlaybooksForParentRecord` answers `null` for a record it cannot
//!   read** — it looks the parent up through `GlideRecordSecure` — so a missing
//!   record and an ACL-hidden one are the same bytes. An existing record with
//!   no executions is `[]`. `null` therefore exits 2 hedged "(or not readable
//!   by this profile)"; an invalid *table* is a resolver exception instead.
//! - **`onlyIfNoProcessRunning` is broader than its name.** The resolver asks
//!   `parentRecordContainsPlaybook(record)`, which takes no playbook name and
//!   (measured) counts a *cancelled* execution too: a record that has ever had
//!   any playbook is skipped. A skip is not an error — the mutation answers a
//!   `PlaybookContext` whose `sys_id` is `null` — so it is emitted as
//!   `triggered: false`, exit 0. The CLI flag is named for what it does,
//!   `--only-if-none`.
//! - **A trigger does not check the playbook's table.** A playbook defined for
//!   one table started against a record of another (measured), so the scoped
//!   name is the caller's responsibility.
//! - **`launchPlaybook`'s `input` is not JSON.** The resolver URI-decodes the
//!   whole string, splits it on `&` and `=`, and URI-decodes each half — a
//!   query string. [`encode_inputs`] double-encodes each key and value so an
//!   `&` or `=` inside a value survives that first whole-string decode.
//! - **`launchPlaybook` derives the parent table from the definition's
//!   `parent_record` input.** On the reference instance no definition declares
//!   one (`sys_pd_snapshot_input` is empty), and the resolver then throws —
//!   a bare "Error occurred while executing the resolver" — for every
//!   definition and any record. That error gets a detail naming this cause.
//!
//! An instance without the application has no `snPlaybookExp` field on the
//! root types; that validation error is mapped to a message naming the
//! application rather than a missing GraphQL field.

use crate::cli::GlobalFlags;
use crate::cli::OutputMode;
use crate::cli::graphql::{errors_to_api_error, execute, graphql_errors};
use crate::cli::journal::{undefined_field, validate_sys_id};
use crate::cli::kernel::{connect, write_response};
use crate::cli::record_ref;
use crate::error::{Error, NO_HTTP_STATUS, Result};
use clap::Subcommand;
use serde_json::{Map, Value, json};

/// The GraphQL root field every document here hangs off.
const NAMESPACE: &str = "snPlaybookExp";
/// Union member type names as the merged schema spells them.
const CONTEXT_TYPE: &str = "snPlaybookExp_playbook_PlaybookContext";
const ERROR_TYPE: &str = "snPlaybookExp_playbook_TriggerPlaybookError";

const LIST_DOC: &str = "query($table: String!, $record: ID!) { snPlaybookExp { playbook { \
     getPlaybooksForParentRecord(parentTable: $table, parentRecord: $record) { \
     sys_id title scoped_name playbook_id state { value displayValue } \
     cancellation_reason can_read } } } }";

/// Selection for the `TriggerPlaybookResult` union both mutations return.
/// Only `sys_id`/`parent_table`/`parent_record` of the context member are
/// populated by either resolver (measured: the rest come back null), so only
/// those are asked for.
fn result_selection() -> String {
    format!(
        "__typename \
         ... on {CONTEXT_TYPE} {{ sys_id parent_table parent_record }} \
         ... on {ERROR_TYPE} {{ errorType message parent_table parent_record \
         process_definition_id input }}"
    )
}

fn trigger_doc() -> String {
    format!(
        "mutation($scopedName: String!, $table: String!, $record: ID!, $onlyIfNone: Boolean) {{ \
         snPlaybookExp {{ playbook {{ triggerPlaybook(scopedName: $scopedName, \
         parentTable: $table, parentRecord: $record, onlyIfNoProcessRunning: $onlyIfNone) \
         {{ {} }} }} }} }}",
        result_selection()
    )
}

fn launch_doc() -> String {
    format!(
        "mutation($definition: String!, $record: ID, $input: String) {{ \
         snPlaybookExp {{ playbook {{ launchPlaybook(processDefinitionId: $definition, \
         parentRecord: $record, input: $input) {{ {} }} }} }} }}",
        result_selection()
    )
}

#[derive(Subcommand, Debug)]
pub enum PlaybookSub {
    /// List the playbook executions on a record, with their state
    /// (`sn playbook list incident:INC0010001`).
    List(PlaybookListArgs),
    /// Start a playbook, by scoped name, against a record
    /// (`sn playbook trigger incident:INC0010001 --scoped-name sn_app.my_playbook`).
    Trigger(PlaybookTriggerArgs),
    /// Start a playbook by its process definition sys_id, with optional inputs
    /// (`sn playbook launch <DEFINITION> --record <SYS_ID> --input k=v`).
    Launch(PlaybookLaunchArgs),
}

#[derive(clap::Args, Debug)]
pub struct PlaybookListArgs {
    /// Table the record lives in (e.g. `incident`), or a combined
    /// `table:sys_id` / `table:number` reference (e.g. `incident:INC0010001`).
    pub table: String,
    /// sys_id of the record. Omit when TABLE is a `table:id` reference.
    pub sys_id: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct PlaybookTriggerArgs {
    /// Table the record lives in (e.g. `incident`), or a combined
    /// `table:sys_id` / `table:number` reference (e.g. `incident:INC0010001`).
    pub table: String,
    /// sys_id of the record. Omit when TABLE is a `table:id` reference.
    pub sys_id: Option<String>,
    /// The playbook's scoped name, `<scope>.<name>` (e.g. `sn_vsc.task_steps_5`;
    /// `sn playbook list` shows it as `scoped_name`). The instance does not
    /// check that the playbook was built for this record's table.
    #[arg(long = "scoped-name", value_name = "SCOPE.NAME")]
    pub scoped_name: String,
    /// Start only if the record has no playbook execution at all — of any
    /// playbook, in any state (a cancelled one counts). Otherwise nothing
    /// starts and the output says `triggered: false` (exit 0), so a retry never
    /// stacks a second run. The API calls this onlyIfNoProcessRunning; it is
    /// broader than that name.
    #[arg(long = "only-if-none")]
    pub only_if_none: bool,
}

#[derive(clap::Args, Debug)]
pub struct PlaybookLaunchArgs {
    /// sys_id of the playbook's process definition (sys_pd_process_definition;
    /// `sn playbook list` shows it as `playbook_id`).
    #[arg(value_name = "DEFINITION")]
    pub definition: String,
    /// sys_id of the parent record. Its table is taken from the definition's
    /// own `parent_record` input, so only the sys_id is given. Required unless
    /// the playbook is on-demand.
    #[arg(long, value_name = "SYS_ID")]
    pub record: Option<String>,
    /// Repeatable playbook input as key=value (split at the first `=`). Values
    /// are sent as strings; encoding for the instance's query-string parser is
    /// handled here, so `&`, `=` and `%` in a value are safe.
    #[arg(long = "input", value_name = "KEY=VALUE")]
    pub input: Vec<String>,
}

pub fn list(global: &GlobalFlags, args: PlaybookListArgs) -> Result<()> {
    let r = record_ref::parse_pair(&args.table, args.sys_id.as_deref(), "table")?;

    let client = connect(global)?;
    let sys_id = r.resolve(&client)?;
    let vars = json!({ "table": r.table, "record": sys_id });
    let resp = execute(&client, LIST_DOC, Some(vars), None)?;
    check_errors(&resp, "getPlaybooksForParentRecord")?;
    if global.output == OutputMode::Raw {
        return write_response(global, &resp);
    }

    match field(&resp, "getPlaybooksForParentRecord")? {
        Value::Null => Err(Error::Api {
            // A clean 200 whose answer is "no such record": no HTTP status to
            // publish, and none is invented.
            status: NO_HTTP_STATUS,
            message: format!(
                "no {} record {sys_id} (or not readable by this profile)",
                r.table
            ),
            detail: None,
            transaction_id: None,
            sn_error: None,
        }),
        list @ Value::Array(_) => write_response(global, &list),
        other => Err(unexpected("getPlaybooksForParentRecord", &other)),
    }
}

pub fn trigger(global: &GlobalFlags, args: PlaybookTriggerArgs) -> Result<()> {
    let r = record_ref::parse_pair(&args.table, args.sys_id.as_deref(), "table")?;
    if args.scoped_name.trim().is_empty() {
        return Err(Error::Usage("--scoped-name must not be empty".into()));
    }

    let client = connect(global)?;
    let sys_id = r.resolve(&client)?;
    let vars = json!({
        "scopedName": args.scoped_name,
        "table": r.table,
        "record": sys_id,
        "onlyIfNone": args.only_if_none,
    });
    let resp = execute(&client, &trigger_doc(), Some(vars), None)?;
    check_errors(&resp, "triggerPlaybook")?;
    if global.output == OutputMode::Raw {
        return write_response(global, &resp);
    }
    let ctx = started_context(field(&resp, "triggerPlaybook")?, "triggerPlaybook")?;
    write_response(global, &started_output("triggered", ctx))
}

pub fn launch(global: &GlobalFlags, args: PlaybookLaunchArgs) -> Result<()> {
    validate_sys_id(&args.definition)?;
    if let Some(rec) = args.record.as_deref() {
        validate_sys_id(rec)?;
    }
    let input = encode_inputs(&args.input)?;

    let client = connect(global)?;
    let mut vars = Map::new();
    vars.insert("definition".into(), Value::String(args.definition));
    if let Some(rec) = args.record {
        vars.insert("record".into(), Value::String(rec));
    }
    if let Some(input) = input {
        vars.insert("input".into(), Value::String(input));
    }
    let resp = execute(&client, &launch_doc(), Some(Value::Object(vars)), None)?;
    check_errors(&resp, "launchPlaybook")?;
    if global.output == OutputMode::Raw {
        return write_response(global, &resp);
    }
    let ctx = started_context(field(&resp, "launchPlaybook")?, "launchPlaybook")?;
    if ctx.get("sys_id").is_none_or(Value::is_null) {
        // launchPlaybook has no skip path: a context without an id is a reply
        // that does not answer the question.
        return Err(Error::Instance {
            message: "launchPlaybook reported success but returned no execution sys_id".into(),
            detail: None,
        });
    }
    write_response(global, &started_output("launched", ctx))
}

/// Map a non-empty `errors` array. The two failures with a known cause get a
/// message naming it; everything else is `graphql.rs`'s generic mapping.
fn check_errors(resp: &Value, op: &str) -> Result<()> {
    let errors = graphql_errors(resp);
    if errors.is_empty() {
        return Ok(());
    }
    if undefined_field(&errors, NAMESPACE) {
        return Err(Error::Api {
            status: 200,
            message: "this instance has no Playbook Experience GraphQL API (snPlaybookExp)".into(),
            detail: Some(
                "it ships with the Playbook Experience application (scope sn_playbook_exp); \
                 the application, or its 'Playbook' GraphQL schema, is not installed or not active"
                    .into(),
            ),
            transaction_id: None,
            sn_error: Some(Value::Array(errors)),
        });
    }
    let resolver_threw = errors.iter().any(|e| {
        e.get("errorType").and_then(Value::as_str) == Some("DataFetchingException")
            && e.get("path")
                .and_then(Value::as_array)
                .and_then(|p| p.last())
                .and_then(Value::as_str)
                == Some(op)
    });
    if resolver_threw {
        let detail = match op {
            "launchPlaybook" => {
                "the launchPlaybook resolver raised an exception; it reads the parent table \
                 from the definition's `parent_record` input, so a definition that declares \
                 none fails this way whatever --record is"
            }
            "getPlaybooksForParentRecord" => {
                "the resolver raised an exception; an invalid table name fails this way"
            }
            _ => "the resolver raised an exception",
        };
        let mut err = errors_to_api_error(errors);
        if let Error::Api { detail: d, .. } = &mut err {
            *d = Some(detail.into());
        }
        return Err(err);
    }
    Err(errors_to_api_error(errors))
}

/// The value at `data.snPlaybookExp.playbook.<op>`. Absent (as opposed to
/// `null`) means the reply does not answer the document that was sent.
fn field(resp: &Value, op: &str) -> Result<Value> {
    resp.pointer(&format!("/data/{NAMESPACE}/playbook/{op}"))
        .cloned()
        .ok_or_else(|| Error::Instance {
            message: format!("GraphQL response carried no {NAMESPACE}.playbook.{op}"),
            detail: Some("the request succeeded but the reply does not answer it".into()),
        })
}

/// Resolve a `TriggerPlaybookResult` union value: the context member is
/// returned, the typed error member becomes exit 2 with the error object in
/// `sn_error` (status 200: that is what HTTP said).
fn started_context(v: Value, op: &str) -> Result<Map<String, Value>> {
    let Value::Object(mut m) = v else {
        return Err(unexpected(op, &v));
    };
    match m.remove("__typename").as_ref().and_then(Value::as_str) {
        Some(CONTEXT_TYPE) => Ok(m),
        Some(ERROR_TYPE) => {
            let message = m
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("playbook request failed")
                .to_string();
            let error_type = m
                .get("errorType")
                .and_then(Value::as_str)
                .unwrap_or("UNKNOWN")
                .to_string();
            // Drop the fields the resolver left unset; they are noise in an
            // error an agent branches on.
            m.retain(|_, v| !v.is_null());
            Err(Error::Api {
                status: 200,
                message,
                detail: Some(format!("{op} failed with {error_type}")),
                transaction_id: None,
                sn_error: Some(Value::Object(m)),
            })
        }
        _ => Err(unexpected(op, &Value::Object(m))),
    }
}

/// `{<key>: bool, sys_id, parent_table, parent_record}` — `<key>` is false
/// exactly when the resolver started nothing (a `--only-if-none` skip).
fn started_output(key: &str, ctx: Map<String, Value>) -> Value {
    let sys_id = ctx.get("sys_id").cloned().unwrap_or(Value::Null);
    let mut out = Map::new();
    out.insert(key.into(), Value::Bool(!sys_id.is_null()));
    out.insert("sys_id".into(), sys_id);
    for k in ["parent_table", "parent_record"] {
        out.insert(k.into(), ctx.get(k).cloned().unwrap_or(Value::Null));
    }
    Value::Object(out)
}

fn unexpected(op: &str, v: &Value) -> Error {
    Error::Instance {
        message: format!("unexpected {op} result shape"),
        detail: Some(v.to_string()),
    }
}

/// Build `launchPlaybook`'s `input` string from `key=value` pairs, or `None`
/// for no pairs (the variable is then omitted).
///
/// The resolver parses it as
/// `decodeURIComponent(input).split("&").map(s => s.split("=")).map(decode)`:
/// one whole-string decode *before* splitting, then a per-part decode. A
/// single-encoded `a%26b` would be decoded to `a&b` and split there, so each
/// key and value is encoded twice — the first decode leaves the separators'
/// escapes intact, the second restores the original text.
fn encode_inputs(pairs: &[String]) -> Result<Option<String>> {
    if pairs.is_empty() {
        return Ok(None);
    }
    let mut parts = Vec::with_capacity(pairs.len());
    for spec in pairs {
        let (k, v) = spec
            .split_once('=')
            .ok_or_else(|| Error::Usage(format!("--input '{spec}' must be in key=value form")))?;
        if k.is_empty() {
            return Err(Error::Usage(format!("--input '{spec}' has an empty key")));
        }
        parts.push(format!(
            "{}={}",
            uri_encode(&uri_encode(k)),
            uri_encode(&uri_encode(v))
        ));
    }
    Ok(Some(parts.join("&")))
}

/// JavaScript's `encodeURIComponent`: UTF-8 percent-encoding of everything
/// but `A-Z a-z 0-9 - _ . ! ~ * ' ( )`, so the resolver's
/// `decodeURIComponent` is its exact inverse.
fn uri_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// JavaScript's decodeURIComponent, for round-tripping in tests.
    fn uri_decode(s: &str) -> String {
        let bytes = s.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' {
                out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap());
                i += 3;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
        String::from_utf8(out).unwrap()
    }

    /// The resolver's parse, transcribed from its script.
    fn resolver_parse(input: &str) -> Vec<(String, String)> {
        uri_decode(input)
            .trim()
            .split('&')
            .map(|s| {
                let mut it = s.split('=').map(uri_decode);
                (it.next().unwrap(), it.next().unwrap_or_default())
            })
            .collect()
    }

    #[test]
    fn inputs_round_trip_through_the_resolver_parse() {
        let pairs = vec![
            "plain=value".to_string(),
            "tricky=a&b=c%d e".to_string(),
            "unicode=café ✓".to_string(),
            "empty=".to_string(),
        ];
        let encoded = encode_inputs(&pairs).unwrap().unwrap();
        assert_eq!(
            resolver_parse(&encoded),
            vec![
                ("plain".into(), "value".into()),
                ("tricky".into(), "a&b=c%d e".into()),
                ("unicode".into(), "café ✓".into()),
                ("empty".into(), String::new()),
            ]
        );
    }

    #[test]
    fn no_inputs_omits_the_variable() {
        assert_eq!(encode_inputs(&[]).unwrap(), None);
    }

    #[test]
    fn malformed_inputs_are_usage_errors() {
        for bad in ["novalue", "=v"] {
            let err = encode_inputs(&[bad.to_string()]).unwrap_err();
            assert!(matches!(err, Error::Usage(_)), "{bad}");
        }
    }

    #[test]
    fn uri_encode_matches_encode_uri_component() {
        assert_eq!(uri_encode("a-z_A.Z!~*'()09"), "a-z_A.Z!~*'()09");
        assert_eq!(uri_encode("a b&c=d/%"), "a%20b%26c%3Dd%2F%25");
        assert_eq!(uri_encode("é"), "%C3%A9");
    }

    #[test]
    fn skipped_trigger_is_triggered_false() {
        let ctx = started_context(
            json!({"__typename": CONTEXT_TYPE, "sys_id": null,
                   "parent_table": "incident", "parent_record": "abc"}),
            "triggerPlaybook",
        )
        .unwrap();
        assert_eq!(
            started_output("triggered", ctx),
            json!({"triggered": false, "sys_id": null,
                   "parent_table": "incident", "parent_record": "abc"})
        );
    }

    #[test]
    fn typed_error_member_is_exit_2_with_the_object() {
        let err = started_context(
            json!({"__typename": ERROR_TYPE, "errorType": "PARENT_RECORD_NOT_FOUND",
                   "message": "'x' not found in incident table.",
                   "parent_table": "incident", "parent_record": "x",
                   "process_definition_id": null, "input": null}),
            "triggerPlaybook",
        )
        .unwrap_err();
        assert_eq!(err.exit_code(), 2);
        let env = err.to_stderr_json();
        assert_eq!(env["error"]["status_code"], 200);
        assert_eq!(env["error"]["message"], "'x' not found in incident table.");
        assert_eq!(
            env["error"]["sn_error"],
            json!({"errorType": "PARENT_RECORD_NOT_FOUND",
                   "message": "'x' not found in incident table.",
                   "parent_table": "incident", "parent_record": "x"})
        );
    }

    #[test]
    fn missing_namespace_names_the_application() {
        let resp = json!({"data": null, "errors": [{
            "message": "Validation error (FieldUndefined@[snPlaybookExp]) : Field 'snPlaybookExp' in type 'QueryType' is undefined",
            "errorType": "ValidationError"
        }]});
        let err = check_errors(&resp, "getPlaybooksForParentRecord").unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("Playbook Experience"), "{err}");
    }
}
