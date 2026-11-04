//! `sn get script <NAME>` — a script's source by name, across the tables
//! scripts live in, in one GraphQL document (one `GlideRecord_Query` field per
//! table, `limit: 3` each). Script includes also match on `api_name`
//! (`global.JSUtil`).
//!
//! Each returned row is checked against the name asked: a dropped `name=`
//! term returns a table's first rows. Names are not unique — business rules
//! especially — so more than one match is the caller's to narrow (`--type`,
//! `--scope`), never picked for them.

use super::common::{dropped, not_found, query_value};
use crate::cli::graphql::{errors_to_api_error, execute, graphql_errors};
use crate::cli::kernel::{connect, write_response};
use crate::cli::{GlobalFlags, OutputMode};
use crate::error::{Error, Result};
use serde_json::{Value, json};
use std::io::Write;

#[derive(clap::Args, Debug)]
pub struct ScriptArgs {
    /// Script name (a script include also matches on its API name, e.g.
    /// `global.JSUtil`).
    pub name: String,
    /// Print only the script's source, as text, instead of the JSON record.
    #[arg(long)]
    pub source: bool,
    /// Only this kind of script.
    #[arg(long = "type", short = 't', value_enum)]
    pub kind: Option<ScriptType>,
    /// Only scripts in this application scope (scope name, e.g. `global`).
    #[arg(long)]
    pub scope: Option<String>,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptType {
    /// Script include (sys_script_include).
    Include,
    /// Business rule (sys_script).
    #[value(alias = "br")]
    BusinessRule,
    /// Client script (sys_script_client).
    Client,
    /// UI action (sys_ui_action).
    #[value(alias = "action")]
    UiAction,
    /// UI script (sys_ui_script).
    UiScript,
    /// Scheduled script job (sysauto_script).
    Job,
    /// Fix script (sys_script_fix).
    Fix,
}

impl ScriptType {
    const ALL: [ScriptType; 7] = [
        ScriptType::Include,
        ScriptType::BusinessRule,
        ScriptType::Client,
        ScriptType::UiAction,
        ScriptType::UiScript,
        ScriptType::Job,
        ScriptType::Fix,
    ];

    fn table(self) -> &'static str {
        match self {
            ScriptType::Include => "sys_script_include",
            ScriptType::BusinessRule => "sys_script",
            ScriptType::Client => "sys_script_client",
            ScriptType::UiAction => "sys_ui_action",
            ScriptType::UiScript => "sys_ui_script",
            ScriptType::Job => "sysauto_script",
            ScriptType::Fix => "sys_script_fix",
        }
    }

    fn label(self) -> &'static str {
        match self {
            ScriptType::Include => "include",
            ScriptType::BusinessRule => "business-rule",
            ScriptType::Client => "client",
            ScriptType::UiAction => "ui-action",
            ScriptType::UiScript => "ui-script",
            ScriptType::Job => "job",
            ScriptType::Fix => "fix",
        }
    }

    /// The table a rule or action runs on, where the kind has one.
    fn target_field(self) -> Option<&'static str> {
        match self {
            ScriptType::BusinessRule => Some("collection"),
            ScriptType::Client | ScriptType::UiAction => Some("table"),
            _ => None,
        }
    }
}

const PER_TABLE: u32 = 3;

pub fn run(global: &GlobalFlags, args: ScriptArgs) -> Result<()> {
    let name = query_value(&args.name, "script name")?;
    let scope = args
        .scope
        .as_deref()
        .map(|s| super::common::identifier(s, "scope name"))
        .transpose()?;
    if args.source && global.output != OutputMode::Default {
        return Err(Error::Usage(
            "--source prints the script as text; it cannot be combined with --output".into(),
        ));
    }
    let kinds: Vec<ScriptType> = match args.kind {
        Some(k) => vec![k],
        None => ScriptType::ALL.to_vec(),
    };
    let client = connect(global)?;

    let scope_term = scope.map_or(String::new(), |s| format!("^sys_scope.scope={s}"));
    let mut fields = String::new();
    // Only variables the document uses are declared: an unused declaration
    // is itself a GraphQL validation error.
    let mut vars = serde_json::Map::new();
    for k in &kinds {
        let (var, condition) = if *k == ScriptType::Include {
            ("byApiName", include_condition(name, &scope_term))
        } else {
            ("byName", format!("name={name}{scope_term}"))
        };
        vars.insert(var.into(), Value::from(condition));
        let extra = match (k, k.target_field()) {
            (ScriptType::Include, _) => " api_name { value }".to_string(),
            (_, Some(f)) => format!(" {f} {{ value }}"),
            _ => String::new(),
        };
        fields.push_str(&format!(
            " {t}(queryConditions: ${var}, pagination: {{ limit: {PER_TABLE} }}) {{ _results {{ \
             sys_id {{ value }} name {{ value }} script {{ value }} active {{ value }} \
             sys_scope {{ value displayValue }} sys_updated_on {{ value }} \
             sys_updated_by {{ value }}{extra} }} }}",
            t = k.table()
        ));
    }
    let decls: Vec<String> = vars.keys().map(|v| format!("${v}: String!")).collect();
    let doc = format!(
        "query ({}) {{ GlideRecord_Query {{{fields} }} }}",
        decls.join(", ")
    );
    let resp = execute(&client, &doc, Some(Value::Object(vars)), None)?;
    let errors = graphql_errors(&resp);
    if !errors.is_empty() {
        return Err(errors_to_api_error(errors));
    }
    let matches = collect(&resp, &kinds, name)?;

    match matches.len() {
        0 => Err(not_found(format!("no script named {name}"))),
        1 => {
            let m = matches.into_iter().next().expect("len checked");
            if args.source {
                let src = m["script"].as_str().unwrap_or_default();
                let mut out = std::io::stdout().lock();
                out.write_all(src.as_bytes())
                    .and_then(|_| {
                        if src.ends_with('\n') {
                            Ok(())
                        } else {
                            out.write_all(b"\n")
                        }
                    })
                    .and_then(|_| out.flush())
                    .map_err(|e| Error::Transport(format!("write stdout: {e}")))
            } else {
                write_response(global, &m)
            }
        }
        _ => Err(Error::Usage(format!(
            "{} scripts are named {name}: {}; narrow with --type or --scope, or read one with \
             `sn get <table>:<sys_id>`",
            matches.len(),
            matches
                .iter()
                .map(|m| format!(
                    "{} {}:{} [{}]",
                    m["type"].as_str().unwrap_or_default(),
                    m["table"].as_str().unwrap_or_default(),
                    m["sys_id"].as_str().unwrap_or_default(),
                    m["scope"].as_str().unwrap_or_default(),
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// A script include by name, or by API name (`global.JSUtil`). `api_name` is
/// calculated, not stored — measured, a query on it matches nothing — so a
/// dotted name is also tried as `<scope>.<name>`. Each `^NQ` segment carries
/// the scope filter itself: a trailing term binds only the last segment.
fn include_condition(name: &str, scope_term: &str) -> String {
    let mut c = format!("name={name}{scope_term}");
    if let Some((scope, short)) = name.rsplit_once('.')
        && !scope.is_empty()
        && !short.is_empty()
    {
        c.push_str(&format!(
            "^NQname={short}^sys_scope.scope={scope}{scope_term}"
        ));
    }
    c
}

/// Every row the document returned, flattened, each checked against `name`.
fn collect(resp: &Value, kinds: &[ScriptType], name: &str) -> Result<Vec<Value>> {
    let mut out = Vec::new();
    for k in kinds {
        let rows = resp
            .pointer(&format!("/data/GlideRecord_Query/{}/_results", k.table()))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for r in rows {
            let field = |f: &str| r.pointer(&format!("/{f}/value")).cloned();
            let s = |f: &str| field(f).and_then(|v| v.as_str().map(str::to_string));
            let row_name = s("name").unwrap_or_default();
            let api = s("api_name").unwrap_or_default();
            if !row_name.eq_ignore_ascii_case(name) && !api.eq_ignore_ascii_case(name) {
                return Err(dropped("name=", k.table()));
            }
            let mut m = json!({
                "type": k.label(),
                "table": k.table(),
                "sys_id": s("sys_id"),
                "name": row_name,
                "scope": r.pointer("/sys_scope/displayValue").cloned().unwrap_or(Value::Null),
                "active": field("active").unwrap_or(Value::Null),
                "updated_on": s("sys_updated_on"),
                "updated_by": s("sys_updated_by"),
            });
            if *k == ScriptType::Include {
                m["api_name"] = Value::from(api);
            }
            if let Some(f) = k.target_field() {
                m["runs_on"] = field(f).unwrap_or(Value::Null);
            }
            m["script"] = Value::from(s("script").unwrap_or_default());
            out.push(m);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(table: &str, rows: Value) -> Value {
        json!({"data": {"GlideRecord_Query": {table: {"_results": rows}}}})
    }

    #[test]
    fn include_matches_on_api_name() {
        let resp = doc(
            "sys_script_include",
            json!([{"sys_id": {"value": "a"}, "name": {"value": "JSUtil"},
                    "api_name": {"value": "global.JSUtil"}, "script": {"value": "x"},
                    "sys_scope": {"displayValue": "Global"}}]),
        );
        let got = collect(&resp, &[ScriptType::Include], "global.JSUtil").unwrap();
        assert_eq!(got[0]["api_name"], "global.JSUtil");
        assert_eq!(got[0]["scope"], "Global");
    }

    #[test]
    fn dotted_include_names_try_the_scope_split() {
        assert_eq!(include_condition("JSUtil", ""), "name=JSUtil");
        assert_eq!(
            include_condition("global.JSUtil", "^sys_scope.scope=global"),
            "name=global.JSUtil^sys_scope.scope=global\
             ^NQname=JSUtil^sys_scope.scope=global^sys_scope.scope=global"
        );
    }

    #[test]
    fn a_foreign_row_is_a_dropped_term() {
        let resp = doc(
            "sys_script",
            json!([{"sys_id": {"value": "a"}, "name": {"value": "Other"}}]),
        );
        assert!(matches!(
            collect(&resp, &[ScriptType::BusinessRule], "Mine"),
            Err(Error::Instance { .. })
        ));
    }
}
