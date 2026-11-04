//! `sn get property <NAME>` — one system property, or a prefix's worth.
//!
//! The `sys_properties` row is the answer when there is one. Some properties
//! the platform answers have no row at all — measured on a Brazil PDI,
//! `glide.buildname` and `glide.buildtag` are not in the table yet
//! `gs.getProperty` returns them — so a miss asks the runtime once, through
//! the GraphQL `getSysPropertyValue` getter (`gs.getProperty` with a
//! default). That getter is admin-only (a non-admin gets `null` for
//! everything) and folds unset and empty into the default, so only a value
//! that differs from the sentinel counts as found.
//!
//! Every returned row is checked against the name asked: a dropped `name=`
//! term returns arbitrary properties.

use super::common::{display, dropped, identifier, not_found, opt, pairs, rows, value};
use crate::cli::GlobalFlags;
use crate::cli::graphql::{errors_to_api_error, execute, graphql_errors};
use crate::cli::kernel::{connect, write_response};
use crate::client::Client;
use crate::error::{Error, Result};
use serde_json::{Value, json};

#[derive(clap::Args, Debug)]
pub struct PropertyArgs {
    /// Property name (e.g. `glide.ui.session_timeout`). End it with `*` to
    /// list every property with that prefix.
    pub name: String,
    /// Only the property defined in this application scope (scope name, e.g.
    /// `x_acme_app`), when several scopes define the same name.
    #[arg(long)]
    pub scope: Option<String>,
}

const FIELDS: &str =
    "sys_id,name,value,type,description,sys_scope,is_private,read_roles,write_roles";

/// Upper bound on a prefix listing.
const PREFIX_LIMIT: &str = "200";

/// The default handed to `getSysPropertyValue`: a property can't plausibly
/// hold it, so getting it back means "unset".
const UNSET: &str = "\u{1}sn-unset\u{1}";

pub fn run(global: &GlobalFlags, args: PropertyArgs) -> Result<()> {
    let (name, prefix) = match args.name.trim().strip_suffix('*') {
        Some(p) => (identifier(p, "property name prefix")?, true),
        None => (identifier(&args.name, "property name")?, false),
    };
    let scope = args
        .scope
        .as_deref()
        .map(|s| identifier(s, "scope name"))
        .transpose()?;
    let client = connect(global)?;

    let mut query = if prefix {
        format!("nameSTARTSWITH{name}")
    } else {
        format!("name={name}")
    };
    if let Some(s) = scope {
        query.push_str(&format!("^sys_scope.scope={s}"));
    }
    query.push_str("^ORDERBYname");
    let resp = client.get(
        "/api/now/table/sys_properties",
        &pairs(&[
            ("sysparm_query", &query),
            ("sysparm_fields", FIELDS),
            ("sysparm_display_value", "all"),
            ("sysparm_limit", if prefix { PREFIX_LIMIT } else { "5" }),
        ]),
    )?;
    let found = rows(resp);
    for r in &found {
        let n = value(r, "name").unwrap_or_default().to_ascii_lowercase();
        let hit = if prefix {
            n.starts_with(&name.to_ascii_lowercase())
        } else {
            n == name.to_ascii_lowercase()
        };
        if !hit {
            return Err(dropped("name", "sys_properties"));
        }
    }

    if prefix {
        let out: Vec<Value> = found.iter().map(property_json).collect();
        return write_response(global, &Value::Array(out));
    }
    match found.len() {
        1 => write_response(global, &property_json(&found[0])),
        0 if scope.is_none() => match runtime_value(&client, name)? {
            Some(v) => write_response(
                global,
                &json!({ "name": name, "value": v, "source": "runtime" }),
            ),
            None => Err(not_found(format!("no system property named {name}"))),
        },
        0 => Err(not_found(format!(
            "no system property named {name} in scope {}",
            scope.unwrap_or_default()
        ))),
        _ => Err(Error::Usage(format!(
            "{name} is defined in more than one scope: {}; pick one with --scope",
            found
                .iter()
                .map(|r| display(r, "sys_scope").unwrap_or_default())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

fn property_json(row: &Value) -> Value {
    json!({
        "name": opt(value(row, "name")),
        "value": value(row, "value").unwrap_or_default(),
        "type": opt(value(row, "type")),
        "description": opt(value(row, "description")),
        "scope": opt(display(row, "sys_scope")),
        "private": super::common::truthy(row, "is_private"),
        "read_roles": opt(value(row, "read_roles")),
        "write_roles": opt(value(row, "write_roles")),
        "sys_id": opt(value(row, "sys_id")),
        "source": "sys_properties",
    })
}

/// `gs.getProperty(name)` through GraphQL. `None` for unset, empty, or an
/// account the getter answers `null` for.
fn runtime_value(client: &Client, name: &str) -> Result<Option<String>> {
    let resp = execute(
        client,
        "query ($n: String!, $d: String) { snDecisionTable { sysProperties { \
         v: getSysPropertyValue(sys_property_name: $n, default_value: $d) } } }",
        Some(json!({ "n": name, "d": UNSET })),
        None,
    )?;
    let errors = graphql_errors(&resp);
    if !errors.is_empty() {
        return Err(errors_to_api_error(errors));
    }
    Ok(resp
        .pointer("/data/snDecisionTable/sysProperties/v")
        .and_then(Value::as_str)
        .filter(|v| *v != UNSET)
        .map(str::to_string))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn property_json_flattens_display_all() {
        let row = json!({
            "name": {"value": "a.b", "display_value": "a.b"},
            "value": {"value": "", "display_value": ""},
            "sys_scope": {"value": "global", "display_value": "Global"},
            "is_private": {"value": "false", "display_value": "false"},
        });
        let out = property_json(&row);
        assert_eq!(out["value"], "");
        assert_eq!(out["scope"], "Global");
        assert_eq!(out["private"], false);
        assert_eq!(out["source"], "sys_properties");
    }
}
