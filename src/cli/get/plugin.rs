//! `sn get plugin <ID>…` — whether plugins are installed and active.
//!
//! The only trustworthy route (see `doctor.rs`): GraphQL
//! `snWorkflowStudio.workflowStudio.isPluginInstalled`. `sys_plugins` and
//! `sys_store_app` are 403 even to admin (re-measured on a Brazil PDI), so
//! there is no version to report. A canary asks about `com.glide.graphql`,
//! the plugin the request itself runs on: a getter that denies it is not
//! believed. Non-admins get `workflowStudio: null`, which is an error, not a
//! "not installed".

use super::common::identifier;
use crate::cli::GlobalFlags;
use crate::cli::graphql::{errors_to_api_error, execute, graphql_errors};
use crate::cli::kernel::{connect, write_response};
use crate::error::{Error, Result};
use serde_json::{Map, Value, json};

#[derive(clap::Args, Debug)]
pub struct PluginArgs {
    /// Plugin id (e.g. `com.snc.incident`, `sn_access_analyzer`); several may
    /// be given.
    #[arg(value_name = "PLUGIN_ID", required = true)]
    pub ids: Vec<String>,
}

const CANARY: &str = "com.glide.graphql";

pub fn run(global: &GlobalFlags, args: PluginArgs) -> Result<()> {
    let mut ids: Vec<String> = Vec::new();
    for raw in &args.ids {
        let id = identifier(raw, "plugin id")?.to_string();
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    let client = connect(global)?;

    let mut decls = Vec::new();
    let mut vars = Map::new();
    let mut fields = format!("canary: isPluginInstalled(plugin_name: \"{CANARY}\")");
    for (i, id) in ids.iter().enumerate() {
        decls.push(format!("$p{i}: String!"));
        vars.insert(format!("p{i}"), Value::from(id.clone()));
        fields.push_str(&format!(" p{i}: isPluginInstalled(plugin_name: $p{i})"));
    }
    let doc = format!(
        "query ({}) {{ snWorkflowStudio {{ workflowStudio {{ {fields} }} }} }}",
        decls.join(", ")
    );
    let resp = execute(&client, &doc, Some(Value::Object(vars)), None)?;
    let errors = graphql_errors(&resp);
    if !errors.is_empty() {
        return Err(errors_to_api_error(errors));
    }
    write_response(global, &answers(&resp, &ids)?)
}

fn answers(resp: &Value, ids: &[String]) -> Result<Value> {
    let Some(ws) = resp
        .pointer("/data/snWorkflowStudio/workflowStudio")
        .filter(|v| v.is_object())
    else {
        return Err(Error::Instance {
            message: "the instance will not say which plugins are installed to this account".into(),
            detail: Some("the plugin getter answers admins only".into()),
        });
    };
    if ws.get("canary").and_then(Value::as_bool) != Some(true) {
        return Err(Error::Instance {
            message: format!(
                "the plugin getter denied {CANARY}, which this request runs on, so its answers \
                 are not trusted"
            ),
            detail: None,
        });
    }
    let mut out = Vec::new();
    for (i, id) in ids.iter().enumerate() {
        let installed = ws
            .get(format!("p{i}"))
            .and_then(Value::as_bool)
            .ok_or_else(|| Error::Instance {
                message: format!("the plugin getter gave no answer for {id}"),
                detail: None,
            })?;
        out.push(json!({ "id": id, "installed": installed }));
    }
    Ok(Value::Array(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(v: Value) -> Value {
        json!({"data": {"snWorkflowStudio": {"workflowStudio": v}}})
    }

    #[test]
    fn answers_in_argv_order() {
        let ids = vec!["a".to_string(), "b".to_string()];
        let out = answers(&ws(json!({"canary": true, "p0": true, "p1": false})), &ids).unwrap();
        assert_eq!(
            out,
            json!([{"id": "a", "installed": true}, {"id": "b", "installed": false}])
        );
    }

    #[test]
    fn null_namespace_and_failed_canary_are_errors() {
        let ids = vec!["a".to_string()];
        assert!(answers(&ws(Value::Null), &ids).is_err());
        assert!(answers(&ws(json!({"canary": false, "p0": true})), &ids).is_err());
    }
}
