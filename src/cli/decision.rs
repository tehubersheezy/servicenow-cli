//! `sn decision` — decision tables (`sys_decision`): list them, show one, and
//! run one against inputs.
//!
//! Three surfaces, all measured on a live Australia PDI:
//!
//! - **list** is a Table API read of `sys_decision` (not `sys_decision_table`,
//!   which does not exist).
//! - **show** is one GraphQL document over the Decision Builder's scripted
//!   schemas — `snDtableDesigner` (`decisionInput`, `decisionCondition`,
//!   `decision`) and `snDecisionTable` (`answerElement`) — plus a Table API read
//!   of the table's own row, which is also the existence check: every resolver
//!   answers a nonexistent sys_id with `[]`, indistinguishable from an empty
//!   table.
//! - **run** posts to the Generic API's
//!   `POST /api/sn_decision_table/generic/evaluate_decision_table?decisionTableId=`,
//!   the route Decision Builder's own test panel calls. Its script hands the
//!   inputs to `sn_dt.DecisionTableBuilderAPI.evaluateDecisionTable` and returns
//!   a map of matched decision sys_id → answer.
//!
//! Every trap below failed *silently* when measured, which is what shapes the
//! code:
//!
//! 1. `getDecisionsByDecisionTable` without `first_row`/`last_row` returns `[]`
//!    (the resolver calls `chooseWindow(null, null)`), so the window is always
//!    sent — sized one past [`MAX_DECISIONS`] so truncation is detectable.
//! 2. The evaluator ignores an input name it does not know, a choice *label*
//!    where the value belongs, and a record number in a reference input; each
//!    comes back as the default decision (or `{}`), exit 0, looking like a real
//!    answer. `run` therefore validates every `--input` against the table's
//!    own inputs before evaluating, and resolves a reference input's number to
//!    its sys_id (the same limit-2 canary lookup `record_ref.rs` uses).
//! 3. The resolvers answer a caller without decision-table access with `null`,
//!    not an error (`UserUtil.hasGraphQLAccess()`: `decision_table_admin`,
//!    `decision_table_reader`, `change_manager`, or delegated development on a
//!    scope), so a null list is reported as a refusal rather than read as
//!    "no inputs".
//! 4. Without the Decision Builder app the namespaces are simply absent
//!    (`FieldUndefined` on `snDtableDesigner`), and the REST route answers
//!    **400** "Requested URI does not represent any resource" — both are mapped
//!    to one message naming the missing plugin.

use crate::cli::graphql::{errors_to_api_error, execute, graphql_errors};
use crate::cli::journal::undefined_field;
use crate::cli::kernel::{connect, emit, write_response};
use crate::cli::record_ref::{is_sys_id, parse_ref};
use crate::cli::{DisplayValueArg, DisplayValueOpt, GlobalFlags, OutputMode, Paging};
use crate::client::Client;
use crate::error::{Error, NO_HTTP_STATUS, Result};
use clap::Subcommand;
use serde_json::{Map, Value, json};

const TABLE: &str = "sys_decision";
/// `answer_table` of a table whose answers are several named result elements
/// rather than one reference.
const MULTI_RESULT_TABLE: &str = "sys_decision_multi_result";
const EVALUATE_PATH: &str = "/api/sn_decision_table/generic/evaluate_decision_table";
const LIST_FIELDS: &str =
    "sys_id,name,description,answer_table,active,access,sys_scope,sys_class_name,sys_updated_on";
const HEADER_FIELDS: &str = "sys_id,name,description,answer_table,active,sys_scope.scope";
/// Decisions read per table. The window asks for one more, so a table this
/// large is reported as truncated instead of silently cut.
const MAX_DECISIONS: usize = 10_000;
const ACCESS_ROLES: &str = "decision_table_admin, decision_table_reader or change_manager \
                            (or delegated development rights on a scope)";
const PLUGIN_ABSENT: &str = "the Decision Builder API (plugin sn_decision_table) is not \
                             available on this instance";

#[derive(Subcommand, Debug)]
pub enum DecisionSub {
    /// List decision tables (GET /api/now/table/sys_decision), ordered by name.
    List(DecisionListArgs),
    /// One decision table: its inputs, answer elements, and decisions in evaluation order.
    Show(DecisionShowArgs),
    /// Evaluate a decision table against inputs and report the matching decision(s).
    Run(DecisionRunArgs),
}

#[derive(clap::Args, Debug)]
pub struct DecisionListArgs {
    /// Encoded query over sys_decision (e.g. `active=true^answer_table=chg_approval_def`).
    #[arg(long, short = 'q', alias = "sysparm-query")]
    pub query: Option<String>,
    #[command(flatten)]
    pub paging: Paging<100>,
    #[command(flatten)]
    pub display_value: DisplayValueOpt,
}

#[derive(clap::Args, Debug)]
pub struct DecisionShowArgs {
    /// Decision table: its sys_id, or its exact name (case-insensitive; must be unique).
    #[arg(value_name = "TABLE")]
    pub table: String,
}

#[derive(clap::Args, Debug)]
pub struct DecisionRunArgs {
    /// Decision table: its sys_id, or its exact name (case-insensitive; must be unique).
    #[arg(value_name = "TABLE")]
    pub table: String,
    /// Repeatable input as name=value, using the input names `sn decision show`
    /// reports. Choice inputs take the choice value (not its label); reference
    /// inputs take a sys_id or the referenced record's number. Every mandatory
    /// input must be given (`name=` sends it empty).
    #[arg(long = "input", short = 'i', value_name = "NAME=VALUE")]
    pub input: Vec<String>,
    /// Report every matching decision instead of only the first.
    #[arg(long)]
    pub all_matches: bool,
}

pub fn list(global: &GlobalFlags, args: DecisionListArgs) -> Result<()> {
    let query = match args.query.as_deref().map(str::trim) {
        Some(q) if !q.is_empty() => format!("{q}^ORDERBYname"),
        _ => "ORDERBYname".to_string(),
    };
    let dv = match args
        .display_value
        .display_value
        .unwrap_or(DisplayValueArg::True)
    {
        DisplayValueArg::True => "true",
        DisplayValueArg::False => "false",
        DisplayValueArg::All => "all",
    };
    let mut pairs = pairs(&[
        ("sysparm_query", &query),
        ("sysparm_fields", LIST_FIELDS),
        ("sysparm_limit", &args.paging.setlimit().to_string()),
        ("sysparm_display_value", dv),
        ("sysparm_exclude_reference_link", "true"),
    ]);
    if let Some(offset) = args.paging.offset {
        pairs.push(("sysparm_offset".into(), offset.to_string()));
    }
    let client = connect(global)?;
    let resp = client.get(&format!("/api/now/table/{TABLE}"), &pairs)?;
    emit(global, resp)
}

pub fn show(global: &GlobalFlags, args: DecisionShowArgs) -> Result<()> {
    reject_raw(global, "show")?;
    validate_table_arg(&args.table)?;

    let client = connect(global)?;
    let header = resolve_table(&client, &args.table)?;
    let def = fetch_definition(&client, &header.sys_id)?;

    let mut out = header.to_json();
    out.insert("inputs".into(), Value::Array(def.inputs_json()));
    out.insert(
        "answer_elements".into(),
        Value::Array(def.answer_elements.clone()),
    );
    out.insert("conditions".into(), Value::Array(def.conditions.clone()));
    out.insert(
        "decisions".into(),
        Value::Array(
            def.decisions
                .iter()
                .map(|d| d.to_json(header.multi_result))
                .collect(),
        ),
    );
    write_response(global, &Value::Object(out))
}

pub fn run(global: &GlobalFlags, args: DecisionRunArgs) -> Result<()> {
    reject_raw(global, "run")?;
    validate_table_arg(&args.table)?;
    let given = parse_inputs(&args.input)?;

    let client = connect(global)?;
    let header = resolve_table(&client, &args.table)?;
    let def = fetch_definition(&client, &header.sys_id)?;
    let checked = check_inputs(&def.inputs, &given)?;

    // Reference inputs given as a record number resolve to the sys_id the
    // evaluator needs; it would otherwise compare the number against sys_ids
    // and quietly match nothing.
    let mut test_inputs = Map::new();
    let mut resolved_from = Map::new();
    for c in checked {
        let value = match c.lookup {
            Some(r) => {
                let sys_id = r.resolve(&client)?;
                resolved_from.insert(c.name.clone(), Value::String(c.value.clone()));
                sys_id
            }
            None => c.value,
        };
        test_inputs.insert(c.name, Value::String(value));
    }

    let mut body = Map::new();
    body.insert("testInputs".into(), Value::Object(test_inputs.clone()));
    body.insert("isFirstMatch".into(), Value::Bool(!args.all_matches));
    body.insert("includeDelta".into(), Value::Bool(false));
    body.insert(
        "isMultiResultTable".into(),
        Value::Bool(header.multi_result),
    );
    if header.multi_result {
        let names: Vec<Value> = def
            .answer_elements
            .iter()
            .filter_map(|e| e.get("name").cloned())
            .collect();
        body.insert("answerElementNames".into(), Value::Array(names));
    }
    let resp = client
        .post(
            EVALUATE_PATH,
            &pairs(&[("decisionTableId", &header.sys_id)]),
            &Value::Object(body),
        )
        .map_err(plugin_absent_on_400)?;
    let result = match resp.get("result") {
        Some(Value::Object(m)) => m.clone(),
        _ => {
            return Err(Error::Instance {
                message: "the decision table evaluation returned no result map".into(),
                detail: Some(resp.to_string()),
            });
        }
    };

    let mut out = Map::new();
    out.insert("sys_id".into(), Value::String(header.sys_id.clone()));
    out.insert("name".into(), Value::String(header.name.clone()));
    out.insert("inputs".into(), Value::Object(test_inputs));
    if !resolved_from.is_empty() {
        out.insert("resolved_from".into(), Value::Object(resolved_from));
    }
    out.insert(
        "matches".into(),
        Value::Array(matches(&result, &def.decisions, header.multi_result)),
    );
    write_response(global, &Value::Object(out))
}

/// A composite built from several requests has no single envelope to keep.
fn reject_raw(global: &GlobalFlags, verb: &str) -> Result<()> {
    if global.output == OutputMode::Raw {
        return Err(Error::Usage(format!(
            "--output raw cannot render sn decision {verb}'s composite result (several \
             requests, no single envelope to keep); use the default output or --output table"
        )));
    }
    Ok(())
}

/// The name form is spliced into an encoded query, where `^` separates terms
/// and cannot be escaped; refuse it before connecting.
fn validate_table_arg(table: &str) -> Result<()> {
    let t = table.trim();
    if t.is_empty() {
        return Err(Error::Usage(
            "decision table name or sys_id is empty".into(),
        ));
    }
    if t.contains('^') || t.contains('\n') || t.contains('\r') {
        return Err(Error::Usage(format!(
            "decision table name '{t}' contains '^' or a line break, which an encoded \
             query cannot carry; pass the table's sys_id instead"
        )));
    }
    Ok(())
}

fn pairs(p: &[(&str, &str)]) -> Vec<(String, String)> {
    p.iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// The decision table's own row.
#[derive(Debug)]
struct Header {
    sys_id: String,
    name: String,
    description: String,
    active: bool,
    answer_table: String,
    scope: String,
    multi_result: bool,
}

impl Header {
    fn from_row(row: &Value) -> Result<Header> {
        let s = |k: &str| {
            row.get(k)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        let sys_id = s("sys_id");
        if sys_id.is_empty() {
            return Err(Error::Instance {
                message: "the decision table row came back without a sys_id".into(),
                detail: None,
            });
        }
        let answer_table = s("answer_table");
        Ok(Header {
            sys_id,
            name: s("name"),
            description: s("description"),
            active: s("active") == "true",
            multi_result: answer_table == MULTI_RESULT_TABLE,
            answer_table,
            scope: s("sys_scope.scope"),
        })
    }

    fn to_json(&self) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("sys_id".into(), json!(self.sys_id));
        m.insert("name".into(), json!(self.name));
        m.insert("description".into(), json!(self.description));
        m.insert("active".into(), json!(self.active));
        m.insert("scope".into(), json!(self.scope));
        m.insert("answer_table".into(), json!(self.answer_table));
        m.insert("multi_result".into(), json!(self.multi_result));
        m
    }
}

/// Resolve a sys_id or exact name to the table's row. A sys_id is one GET
/// (whose 404 propagates); a name is one list read filtered client-side too,
/// because a dropped `name=` term would return arbitrary tables.
fn resolve_table(client: &Client, token: &str) -> Result<Header> {
    let token = token.trim();
    let fields = [
        ("sysparm_fields", HEADER_FIELDS),
        ("sysparm_display_value", "false"),
        ("sysparm_exclude_reference_link", "true"),
    ];
    if is_sys_id(token) {
        let resp = client.get(&format!("/api/now/table/{TABLE}/{token}"), &pairs(&fields))?;
        return Header::from_row(resp.get("result").unwrap_or(&Value::Null));
    }

    let query = format!("name={token}");
    let mut p = pairs(&fields);
    p.push(("sysparm_query".into(), query));
    p.push(("sysparm_limit".into(), "20".into()));
    let resp = client.get(&format!("/api/now/table/{TABLE}"), &p)?;
    let rows = resp
        .get("result")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let named: Vec<&Value> = rows
        .iter()
        .filter(|r| {
            r.get("name")
                .and_then(Value::as_str)
                .is_some_and(|n| n.eq_ignore_ascii_case(token))
        })
        .collect();
    match named.len() {
        0 if rows.is_empty() => Err(Error::Api {
            // The lookup succeeded and found nothing: no HTTP verdict to report.
            status: NO_HTTP_STATUS,
            message: format!("no decision table named '{token}'"),
            detail: Some("`sn decision list` shows the tables this profile can read".into()),
            transaction_id: None,
            sn_error: None,
        }),
        0 => Err(Error::Instance {
            message: format!(
                "cannot resolve decision table '{token}': the name= query term was dropped \
                 by the instance, so the rows returned are arbitrary"
            ),
            detail: Some("pass the table's sys_id instead".into()),
        }),
        1 => Header::from_row(named[0]),
        _ => {
            let candidates: Vec<String> = named
                .iter()
                .map(|r| {
                    format!(
                        "{} (scope {})",
                        r.get("sys_id").and_then(Value::as_str).unwrap_or(""),
                        r.get("sys_scope.scope")
                            .and_then(Value::as_str)
                            .unwrap_or("?")
                    )
                })
                .collect();
            Err(Error::Usage(format!(
                "{} decision tables are named '{token}'; pass a sys_id: {}",
                named.len(),
                candidates.join(", ")
            )))
        }
    }
}

/// One decision table input, as the evaluator knows it.
#[derive(Debug, Clone)]
struct Input {
    sys_id: String,
    name: String,
    label: String,
    kind: String,
    mandatory: bool,
    active: bool,
    order: Value,
    reference: String,
    /// `(value, label)` pairs; empty for anything but a choice input.
    choices: Vec<(String, String)>,
}

impl Input {
    fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("name".into(), json!(self.name));
        m.insert("label".into(), json!(self.label));
        m.insert("type".into(), json!(self.kind));
        m.insert("mandatory".into(), json!(self.mandatory));
        m.insert("active".into(), json!(self.active));
        m.insert("order".into(), self.order.clone());
        if !self.reference.is_empty() {
            m.insert("reference".into(), json!(self.reference));
        }
        if !self.choices.is_empty() {
            let choices: Vec<Value> = self
                .choices
                .iter()
                .map(|(v, l)| json!({ "value": v, "label": l }))
                .collect();
            m.insert("choices".into(), Value::Array(choices));
        }
        m.insert("sys_id".into(), json!(self.sys_id));
        Value::Object(m)
    }
}

/// One row of the table, in evaluation order.
#[derive(Debug, Clone)]
struct Decision {
    sys_id: String,
    label: String,
    order: Value,
    active: Value,
    default: bool,
    condition: String,
    answer_value: String,
    answer_display: String,
    /// `(name, value, display_value)` for a multi-result table's answer.
    elements: Vec<(String, String, String)>,
}

impl Decision {
    fn to_json(&self, multi_result: bool) -> Value {
        let mut answer = Map::new();
        answer.insert("value".into(), json!(self.answer_value));
        answer.insert("display_value".into(), json!(self.answer_display));
        if multi_result {
            answer.insert("elements".into(), elements_json(&self.elements));
        }
        json!({
            "sys_id": self.sys_id,
            "order": self.order,
            "label": self.label,
            "active": self.active,
            "default": self.default,
            "condition": self.condition,
            "answer": Value::Object(answer),
        })
    }
}

fn elements_json(elements: &[(String, String, String)]) -> Value {
    let mut m = Map::new();
    for (name, value, display) in elements {
        m.insert(
            name.clone(),
            json!({ "value": value, "display_value": display }),
        );
    }
    Value::Object(m)
}

#[derive(Debug)]
struct Definition {
    inputs: Vec<Input>,
    answer_elements: Vec<Value>,
    conditions: Vec<Value>,
    decisions: Vec<Decision>,
}

impl Definition {
    fn inputs_json(&self) -> Vec<Value> {
        self.inputs.iter().map(Input::to_json).collect()
    }
}

/// The whole definition in one GraphQL round trip. The window literal is
/// [`MAX_DECISIONS`] + 1 (see the module doc, trap 1).
fn document() -> String {
    let row = "sys_id label order active condition \
               answer { value displayValue answerElementValues { name value displayValue } }";
    let window = MAX_DECISIONS + 1;
    format!(
        "query ($id: String!) {{ \
         snDtableDesigner {{ \
         decisionInput {{ getDecisionInputsByDecisionTable(sys_id: $id) {{ \
         sys_id {{ value }} element {{ value }} label {{ value }} internal_type {{ value }} \
         mandatory {{ value }} active {{ value }} order {{ value }} reference {{ value }} \
         choices {{ value {{ value }} label {{ value }} }} }} }} \
         decisionCondition {{ getDecisionConditionsByDecisionTable(sys_id: $id) {{ \
         sys_id label field field_label order default_operator \
         type {{ value }} decision_input {{ value }} reference {{ value }} }} }} \
         decision {{ \
         rows: getDecisionsByDecisionTable(sys_id: $id, first_row: 0, last_row: {window}, default_answer: \"false\") {{ {row} }} \
         fallback: getDecisionsByDecisionTable(sys_id: $id, first_row: 0, last_row: {window}, default_answer: \"true\") {{ {row} }} \
         }} }} \
         snDecisionTable {{ answerElement {{ getAnswerElementsOfDecisionTable(sys_id: $id) {{ \
         sys_id {{ value }} element {{ value }} label {{ value }} internal_type {{ value }} order {{ value }} \
         }} }} }} }}"
    )
}

fn fetch_definition(client: &Client, sys_id: &str) -> Result<Definition> {
    let resp = execute(client, &document(), Some(json!({ "id": sys_id })), None)?;
    let errors = graphql_errors(&resp);
    if !errors.is_empty() {
        if undefined_field(&errors, "snDtableDesigner")
            || undefined_field(&errors, "snDecisionTable")
        {
            let mut err = errors_to_api_error(errors);
            if let Error::Api { message, .. } = &mut err {
                *message = PLUGIN_ABSENT.to_string();
            }
            return Err(err);
        }
        return Err(errors_to_api_error(errors));
    }
    parse_definition(&resp)
}

fn parse_definition(resp: &Value) -> Result<Definition> {
    let list = |pointer: &str| -> Result<Vec<Value>> {
        match resp.pointer(pointer) {
            Some(Value::Array(a)) => Ok(a.clone()),
            // The resolvers return null, not an error, when the caller lacks
            // decision-table access (module doc, trap 3).
            _ => Err(Error::Api {
                status: 200,
                message: format!(
                    "the Decision Builder API returned no data for this table: the \
                     profile's user needs {ACCESS_ROLES}"
                ),
                detail: Some(format!("null at {pointer}")),
                transaction_id: None,
                sn_error: None,
            }),
        }
    };
    let inputs = list("/data/snDtableDesigner/decisionInput/getDecisionInputsByDecisionTable")?;
    let conditions =
        list("/data/snDtableDesigner/decisionCondition/getDecisionConditionsByDecisionTable")?;
    let rows = list("/data/snDtableDesigner/decision/rows")?;
    let fallback = list("/data/snDtableDesigner/decision/fallback")?;
    let elements = list("/data/snDecisionTable/answerElement/getAnswerElementsOfDecisionTable")?;

    if rows.len() > MAX_DECISIONS {
        eprintln!(
            "sn: warning: decision table has more than {MAX_DECISIONS} decisions; only the \
             first {MAX_DECISIONS} are shown"
        );
    }
    let mut decisions: Vec<Decision> = rows
        .iter()
        .take(MAX_DECISIONS)
        .map(|r| parse_decision(r, false))
        .collect();
    decisions.extend(fallback.iter().map(|r| parse_decision(r, true)));

    Ok(Definition {
        inputs: inputs.iter().map(parse_input).collect(),
        answer_elements: elements
            .iter()
            .map(|e| {
                json!({
                    "name": leaf(e, "element"),
                    "label": leaf(e, "label"),
                    "type": leaf(e, "internal_type"),
                    "order": leaf(e, "order"),
                    "sys_id": leaf(e, "sys_id"),
                })
            })
            .collect(),
        conditions: conditions
            .iter()
            .map(|c| {
                json!({
                    "label": leaf(c, "label"),
                    "field": leaf(c, "field"),
                    "field_label": leaf(c, "field_label"),
                    "operator": leaf(c, "default_operator"),
                    "type": leaf(c, "type"),
                    "input": leaf(c, "decision_input"),
                    "reference": leaf(c, "reference"),
                    "order": leaf(c, "order"),
                    "sys_id": leaf(c, "sys_id"),
                })
            })
            .collect(),
        decisions,
    })
}

/// A field that is either a scalar or a `{value, …}` wrapper, as its scalar.
fn leaf(v: &Value, key: &str) -> Value {
    match v.get(key) {
        Some(Value::Object(m)) => m.get("value").cloned().unwrap_or(Value::Null),
        Some(other) => other.clone(),
        None => Value::Null,
    }
}

fn leaf_str(v: &Value, key: &str) -> String {
    match leaf(v, key) {
        Value::String(s) => s,
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn parse_input(v: &Value) -> Input {
    let choices = v
        .get("choices")
        .and_then(Value::as_array)
        .map(|cs| {
            cs.iter()
                .map(|c| (leaf_str(c, "value"), leaf_str(c, "label")))
                .collect()
        })
        .unwrap_or_default();
    Input {
        sys_id: leaf_str(v, "sys_id"),
        name: leaf_str(v, "element"),
        label: leaf_str(v, "label"),
        kind: leaf_str(v, "internal_type"),
        mandatory: leaf(v, "mandatory") == Value::Bool(true),
        // A missing flag reads as active: the evaluator is the authority, and
        // refusing a real input would be worse than accepting a retired one.
        active: leaf(v, "active") != Value::Bool(false),
        order: leaf(v, "order"),
        reference: leaf_str(v, "reference"),
        choices,
    }
}

fn parse_decision(v: &Value, default: bool) -> Decision {
    let answer = v.get("answer").cloned().unwrap_or(Value::Null);
    let elements = answer
        .get("answerElementValues")
        .and_then(Value::as_array)
        .map(|es| {
            es.iter()
                .map(|e| {
                    (
                        leaf_str(e, "name"),
                        leaf_str(e, "value"),
                        leaf_str(e, "displayValue"),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    Decision {
        sys_id: leaf_str(v, "sys_id"),
        label: leaf_str(v, "label"),
        order: leaf(v, "order"),
        active: leaf(v, "active"),
        default,
        condition: leaf_str(v, "condition"),
        answer_value: leaf_str(&answer, "value"),
        answer_display: leaf_str(&answer, "displayValue"),
        elements,
    }
}

/// `--input name=value` pairs, in argv order. Only the first `=` splits, so a
/// value may itself contain one.
fn parse_inputs(specs: &[String]) -> Result<Vec<(String, String)>> {
    let mut out: Vec<(String, String)> = Vec::new();
    for spec in specs {
        let (k, v) = spec
            .split_once('=')
            .ok_or_else(|| Error::Usage(format!("--input '{spec}' must be in name=value form")))?;
        let k = k.trim();
        if k.is_empty() {
            return Err(Error::Usage(format!("--input '{spec}' has an empty name")));
        }
        if out.iter().any(|(name, _)| name == k) {
            return Err(Error::Usage(format!(
                "--input '{k}' is given more than once"
            )));
        }
        out.push((k.to_string(), v.to_string()));
    }
    Ok(out)
}

/// One input ready to send; `lookup` is set when the value is a record number
/// that must be resolved to a sys_id first.
#[derive(Debug)]
struct Checked {
    name: String,
    value: String,
    lookup: Option<crate::cli::record_ref::RecordRef>,
}

/// Validate the given inputs against the table's own, because the evaluator
/// accepts every mistake silently (module doc, trap 2).
fn check_inputs(inputs: &[Input], given: &[(String, String)]) -> Result<Vec<Checked>> {
    let known = |name: &str| inputs.iter().find(|i| i.name == name);

    let unknown: Vec<&str> = given
        .iter()
        .filter(|(n, _)| known(n).is_none())
        .map(|(n, _)| n.as_str())
        .collect();
    if !unknown.is_empty() {
        let available: Vec<&str> = inputs.iter().map(|i| i.name.as_str()).collect();
        return Err(Error::Usage(format!(
            "unknown input(s) {} (names are case-sensitive); this table's inputs: {}",
            unknown.join(", "),
            if available.is_empty() {
                "none".to_string()
            } else {
                available.join(", ")
            }
        )));
    }

    let missing: Vec<&str> = inputs
        .iter()
        .filter(|i| i.mandatory && i.active && !given.iter().any(|(n, _)| *n == i.name))
        .map(|i| i.name.as_str())
        .collect();
    if !missing.is_empty() {
        return Err(Error::Usage(format!(
            "missing mandatory input(s): {} (pass --input name=value; `name=` sends it empty)",
            missing.join(", ")
        )));
    }

    let mut out = Vec::with_capacity(given.len());
    for (name, value) in given {
        let input = known(name).expect("unknown names were rejected above");
        let mut lookup = None;
        if !value.is_empty() {
            match input.kind.as_str() {
                "boolean" if value != "true" && value != "false" => {
                    return Err(Error::Usage(format!(
                        "input '{name}' is a boolean; pass true or false, not '{value}'"
                    )));
                }
                "choice" if !input.choices.is_empty() => {
                    if !input.choices.iter().any(|(v, _)| v == value) {
                        let hint = input
                            .choices
                            .iter()
                            .find(|(_, l)| l.eq_ignore_ascii_case(value))
                            .map(|(v, l)| format!(" ('{l}' is the label of value '{v}')"))
                            .unwrap_or_default();
                        let values: Vec<&str> =
                            input.choices.iter().map(|(v, _)| v.as_str()).collect();
                        return Err(Error::Usage(format!(
                            "input '{name}' has no choice '{value}'{hint}; choices: {}",
                            values.join(", ")
                        )));
                    }
                }
                "reference" if !is_sys_id(value) => {
                    if input.reference.is_empty() {
                        return Err(Error::Usage(format!(
                            "input '{name}' is a reference with no target table; pass a sys_id"
                        )));
                    }
                    // parse_ref's charset guard keeps the number from carrying
                    // anything into the encoded query it is resolved through.
                    lookup = Some(parse_ref(&format!("{}:{value}", input.reference), "table")?);
                }
                _ => {}
            }
        }
        out.push(Checked {
            name: name.clone(),
            value: value.clone(),
            lookup,
        });
    }
    Ok(out)
}

/// The evaluator's `{decision_sys_id: [answer…]}` map as a list in the table's
/// own evaluation order (a JSON object carries none), labeled from the
/// definition. A decision the definition does not list (truncated past
/// [`MAX_DECISIONS`]) sorts last with what the evaluator said about it.
fn matches(result: &Map<String, Value>, decisions: &[Decision], multi_result: bool) -> Vec<Value> {
    let mut keyed: Vec<(usize, Value)> = result
        .iter()
        .map(|(id, answers)| {
            let pos = decisions.iter().position(|d| d.sys_id == *id);
            let answer = evaluated_answer(answers, multi_result);
            let entry = match pos.map(|p| &decisions[p]) {
                Some(d) => json!({
                    "sys_id": id,
                    "label": d.label,
                    "order": d.order,
                    "default": d.default,
                    "answer": answer,
                }),
                None => json!({ "sys_id": id, "answer": answer }),
            };
            (pos.unwrap_or(usize::MAX), entry)
        })
        .collect();
    keyed.sort_by_key(|(pos, _)| *pos);
    keyed.into_iter().map(|(_, v)| v).collect()
}

/// One matched decision's answer: `{value, display_value}` for a reference
/// table (the answer record), `{elements: {name: {value, display_value}}}` for
/// a multi-result table.
fn evaluated_answer(answers: &Value, multi_result: bool) -> Value {
    let items = answers.as_array().cloned().unwrap_or_default();
    if multi_result {
        let elements: Vec<(String, String, String)> = items
            .iter()
            .map(|e| {
                (
                    leaf_str(e, "name"),
                    leaf_str(e, "value"),
                    leaf_str(e, "displayValue"),
                )
            })
            .collect();
        json!({ "elements": elements_json(&elements) })
    } else {
        let first = items.first().cloned().unwrap_or(Value::Null);
        json!({
            "value": leaf_str(&first, "value"),
            "display_value": leaf_str(&first, "displayValue"),
        })
    }
}

/// The evaluate route is absent without the plugin, and the instance says so
/// with a 400 rather than a 404 (module doc, trap 4).
fn plugin_absent_on_400(err: Error) -> Error {
    match err {
        Error::Api {
            status: 400,
            message,
            detail,
            transaction_id,
            sn_error,
        } if message.contains("does not represent any resource") => Error::Api {
            status: 400,
            message: PLUGIN_ABSENT.to_string(),
            detail: Some(detail.unwrap_or(message)),
            transaction_id,
            sn_error,
        },
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(name: &str, kind: &str, mandatory: bool) -> Input {
        Input {
            sys_id: String::new(),
            name: name.into(),
            label: name.into(),
            kind: kind.into(),
            mandatory,
            active: true,
            order: json!(100),
            reference: String::new(),
            choices: Vec::new(),
        }
    }

    fn given(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn parse_inputs_splits_on_first_equals_and_refuses_duplicates() {
        let got = parse_inputs(&["q=a=b".into(), "empty=".into()]).unwrap();
        assert_eq!(got, given(&[("q", "a=b"), ("empty", "")]));
        for bad in [
            vec!["novalue".to_string()],
            vec!["=v".into()],
            vec!["a=1".into(), "a=2".into()],
        ] {
            assert!(
                matches!(parse_inputs(&bad), Err(Error::Usage(_))),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn unknown_input_names_are_refused_with_the_real_names() {
        let inputs = vec![input("releaseops_plugin_is_installed", "boolean", false)];
        let err =
            check_inputs(&inputs, &given(&[("releaseops_plugin_installed", "true")])).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("releaseops_plugin_installed"), "{msg}");
        assert!(msg.contains("releaseops_plugin_is_installed"), "{msg}");
    }

    #[test]
    fn mandatory_inputs_must_be_given_but_may_be_empty() {
        let inputs = vec![input("a", "string", true), input("b", "string", false)];
        assert!(matches!(
            check_inputs(&inputs, &given(&[("b", "x")])),
            Err(Error::Usage(m)) if m.contains("missing mandatory input(s): a")
        ));
        assert!(check_inputs(&inputs, &given(&[("a", "")])).is_ok());
    }

    #[test]
    fn inactive_mandatory_inputs_are_not_demanded() {
        let mut a = input("a", "string", true);
        a.active = false;
        assert!(check_inputs(&[a], &[]).is_ok());
    }

    #[test]
    fn booleans_take_true_or_false_only() {
        let inputs = vec![input("flag", "boolean", false)];
        assert!(check_inputs(&inputs, &given(&[("flag", "true")])).is_ok());
        assert!(check_inputs(&inputs, &given(&[("flag", "yes")])).is_err());
    }

    #[test]
    fn a_choice_label_is_refused_and_its_value_suggested() {
        let mut i = input("instance_type", "choice", false);
        i.choices = vec![
            ("test".into(), "Testing".into()),
            ("prod".into(), "Production".into()),
        ];
        let err = check_inputs(&[i.clone()], &given(&[("instance_type", "Testing")])).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("'Testing' is the label of value 'test'"),
            "{msg}"
        );
        assert!(check_inputs(&[i], &given(&[("instance_type", "test")])).is_ok());
    }

    #[test]
    fn a_reference_number_becomes_a_lookup_on_the_input_table() {
        let mut i = input("change_request", "reference", false);
        i.reference = "change_request".into();
        let hex = "46cb2f54a9fe198101cf6814a2754606";
        let by_id = check_inputs(&[i.clone()], &given(&[("change_request", hex)])).unwrap();
        assert!(by_id[0].lookup.is_none());
        let by_number =
            check_inputs(&[i.clone()], &given(&[("change_request", "CHG0000008")])).unwrap();
        let r = by_number[0].lookup.as_ref().unwrap();
        assert_eq!(r.table, "change_request");
        // The charset guard refuses anything that could extend the query.
        assert!(check_inputs(&[i], &given(&[("change_request", "CHG1^ORactive=true")])).is_err());
    }

    #[test]
    fn table_names_with_query_separators_are_refused() {
        assert!(validate_table_arg("Normal Change Policy").is_ok());
        assert!(validate_table_arg("a^ORname=b").is_err());
        assert!(validate_table_arg("  ").is_err());
    }

    #[test]
    fn document_always_sends_the_decision_window() {
        let doc = document();
        assert_eq!(doc.matches("first_row: 0, last_row: 10001").count(), 2);
        assert!(doc.contains("default_answer: \"false\""));
        assert!(doc.contains("default_answer: \"true\""));
    }

    #[test]
    fn null_resolver_lists_mean_no_access() {
        let resp = json!({"data": {
            "snDtableDesigner": {
                "decisionInput": {"getDecisionInputsByDecisionTable": null},
                "decisionCondition": {"getDecisionConditionsByDecisionTable": null},
                "decision": {"rows": null, "fallback": null}
            },
            "snDecisionTable": {"answerElement": {"getAnswerElementsOfDecisionTable": null}}
        }});
        let err = parse_definition(&resp).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("decision_table_reader"), "{err}");
    }

    #[test]
    fn matches_follow_evaluation_order_and_mark_the_default() {
        let d = |id: &str, default: bool| Decision {
            sys_id: id.into(),
            label: format!("L{id}"),
            order: json!(0),
            active: json!(true),
            default,
            condition: String::new(),
            answer_value: String::new(),
            answer_display: String::new(),
            elements: Vec::new(),
        };
        let decisions = vec![d("b", false), d("a", false), d("z", true)];
        let mut result = Map::new();
        result.insert("z".into(), json!([{"value": "v", "displayValue": "V"}]));
        result.insert("a".into(), json!([{"value": "w", "displayValue": "W"}]));
        result.insert("b".into(), json!([{"value": "x", "displayValue": "X"}]));
        let got = matches(&result, &decisions, false);
        let ids: Vec<&str> = got.iter().map(|m| m["sys_id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["b", "a", "z"]);
        assert_eq!(got[2]["default"], true);
        assert_eq!(
            got[0]["answer"],
            json!({"value": "x", "display_value": "X"})
        );
    }

    #[test]
    fn multi_result_answers_are_keyed_by_element_name() {
        let got = evaluated_answer(
            &json!([{"name": "use_releaseops", "value": "true", "displayValue": "true"}]),
            true,
        );
        assert_eq!(
            got,
            json!({"elements": {"use_releaseops": {"value": "true", "display_value": "true"}}})
        );
    }

    #[test]
    fn plugin_absence_400_is_named() {
        let err = plugin_absent_on_400(Error::Api {
            status: 400,
            message: "Requested URI does not represent any resource".into(),
            detail: None,
            transaction_id: None,
            sn_error: None,
        });
        assert!(err.to_string().contains("sn_decision_table"), "{err}");
    }
}
