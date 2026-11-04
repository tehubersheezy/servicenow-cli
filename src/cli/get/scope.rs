//! `sn get scope` and `sn get updateset`: the session context `sn context`
//! reports, filled out with the records it names.
//!
//! The *which* comes from the platform's own resolution (`context::read_context`,
//! one `GET …/concoursepicker/current`); the detail is one read of the named
//! record. `updateset` also counts the set's customer updates and lists the
//! latest. The count is a stats call grouped by `update_set`, so a dropped
//! `update_set=` term shows up as a second group (or a foreign one) instead of
//! the whole table's count; each listed change is checked against the set too.

use super::common::{
    PAGE, display, dropped, not_found, opt, pairs, query_value, rows, truthy, value,
};
use crate::cli::GlobalFlags;
use crate::cli::context::{STATE_IN_PROGRESS, read_context};
use crate::cli::kernel::{connect, write_response};
use crate::client::Client;
use crate::error::{Error, Result};
use serde_json::{Value, json};

#[derive(clap::Args, Debug)]
pub struct UpdatesetArgs {
    /// The update set to read, by name or sys_id; the current one when omitted.
    #[arg(value_name = "SET")]
    pub set: Option<String>,
    /// How many of the latest changes to list (0 lists none).
    #[arg(long, short = 'l', default_value_t = 10, conflicts_with = "all")]
    pub limit: u32,
    /// List every change in the set instead of the latest `--limit`.
    #[arg(long)]
    pub all: bool,
}

pub fn scope(global: &GlobalFlags) -> Result<()> {
    let client = connect(global)?;
    let current = read_context(&client)?.scope;
    let resp = client.get(
        &format!("/api/now/table/sys_scope/{}", current.sys_id),
        &pairs(&[
            (
                "sysparm_fields",
                "sys_id,name,scope,version,vendor,vendor_prefix,short_description,active",
            ),
            ("sysparm_display_value", "false"),
        ]),
    )?;
    let row = crate::cli::kernel::take_field(resp, "result").unwrap_or(Value::Null);
    write_response(global, &scope_json(&row, &current.sys_id))
}

fn scope_json(row: &Value, sys_id: &str) -> Value {
    json!({
        "sys_id": sys_id,
        "name": opt(value(row, "name")),
        "scope": opt(value(row, "scope")),
        "version": opt(value(row, "version")),
        "vendor": opt(value(row, "vendor")),
        "vendor_prefix": opt(value(row, "vendor_prefix")),
        "description": opt(value(row, "short_description")),
        "active": truthy(row, "active"),
        "global": sys_id == "global",
    })
}

const SET_FIELDS: &str = "sys_id,name,state,application,description,is_default,parent,\
                          sys_created_by,sys_created_on,sys_updated_on";

pub fn updateset(global: &GlobalFlags, args: UpdatesetArgs) -> Result<()> {
    let target = args
        .set
        .as_deref()
        .map(|s| query_value(s, "update set"))
        .transpose()?;
    let client = connect(global)?;

    let (row, current) = match target {
        Some(t) => (find_set(&client, t)?, false),
        None => {
            let id = read_context(&client)?.update_set.sys_id;
            let resp = client.get(
                &format!("/api/now/table/sys_update_set/{id}"),
                &pairs(&[
                    ("sysparm_fields", SET_FIELDS),
                    ("sysparm_display_value", "all"),
                ]),
            )?;
            let row = crate::cli::kernel::take_field(resp, "result").unwrap_or(Value::Null);
            (row, true)
        }
    };
    let sys_id = value(&row, "sys_id").ok_or_else(|| Error::Instance {
        message: "the update set record came back without a sys_id".into(),
        detail: None,
    })?;

    let count = change_count(&client, &sys_id)?;
    let limit = if args.all { None } else { Some(args.limit) };
    let changes = if limit == Some(0) {
        Vec::new()
    } else {
        list_changes(&client, &sys_id, limit)?
    };

    let mut out = set_json(&row, &sys_id);
    out["current"] = Value::Bool(current);
    out["change_count"] = Value::from(count);
    out["changes"] = Value::Array(changes);
    write_response(global, &out)
}

/// An update set by name or sys_id, in any scope or state (unlike `sn set
/// updateset`, which only offers what the picker would). Several sets may
/// share a name; that is the caller's to disambiguate.
fn find_set(client: &Client, target: &str) -> Result<Value> {
    let resp = client.get(
        "/api/now/table/sys_update_set",
        &pairs(&[
            ("sysparm_query", &format!("sys_id={target}^ORname={target}")),
            ("sysparm_fields", SET_FIELDS),
            ("sysparm_display_value", "all"),
            ("sysparm_limit", "5"),
        ]),
    )?;
    let found = rows(resp);
    for r in &found {
        let hit = value(r, "sys_id").as_deref() == Some(target)
            || value(r, "name").is_some_and(|n| n.eq_ignore_ascii_case(target));
        if !hit {
            return Err(dropped("name=", "sys_update_set"));
        }
    }
    match found.len() {
        0 => Err(not_found(format!("no update set named '{target}'"))),
        1 => Ok(found.into_iter().next().expect("len checked")),
        _ => Err(Error::Usage(format!(
            "'{target}' names more than one update set: {}; pass the sys_id",
            found
                .iter()
                .map(|r| format!(
                    "{} [{}] ({})",
                    value(r, "name").unwrap_or_default(),
                    display(r, "application").unwrap_or_default(),
                    value(r, "sys_id").unwrap_or_default()
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

fn set_json(row: &Value, sys_id: &str) -> Value {
    json!({
        "sys_id": sys_id,
        "name": opt(value(row, "name")),
        "state": opt(value(row, "state")),
        "in_progress": value(row, "state").as_deref() == Some(STATE_IN_PROGRESS),
        "scope": {
            "sys_id": opt(value(row, "application")),
            "name": opt(display(row, "application")),
        },
        "description": opt(value(row, "description")),
        "is_default": truthy(row, "is_default"),
        "parent": opt(value(row, "parent")),
        "created_by": opt(value(row, "sys_created_by")),
        "created_on": opt(value(row, "sys_created_on")),
        "updated_on": opt(value(row, "sys_updated_on")),
    })
}

/// How many customer updates the set holds: one stats call, grouped by
/// `update_set` so the answer carries its own proof of filtering.
fn change_count(client: &Client, set: &str) -> Result<u64> {
    let resp = client.get(
        "/api/now/stats/sys_update_xml",
        &pairs(&[
            ("sysparm_count", "true"),
            ("sysparm_query", &format!("update_set={set}")),
            ("sysparm_group_by", "update_set"),
        ]),
    )?;
    count_from_groups(&resp, set)
}

fn count_from_groups(resp: &Value, set: &str) -> Result<u64> {
    let groups = resp
        .get("result")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut total = 0;
    for g in &groups {
        let named = g
            .pointer("/groupby_fields/0/value")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if named != set {
            return Err(dropped("update_set=", "sys_update_xml"));
        }
        total += g
            .pointer("/stats/count")
            .and_then(|c| c.as_str().and_then(|s| s.parse().ok()).or(c.as_u64()))
            .unwrap_or(0);
    }
    Ok(total)
}

/// The set's changes, newest first: `limit` of them, or every one.
fn list_changes(client: &Client, set: &str, limit: Option<u32>) -> Result<Vec<Value>> {
    let page = limit.map_or(PAGE.to_string(), |l| l.to_string());
    let q = pairs(&[
        (
            "sysparm_query",
            &format!("update_set={set}^ORDERBYDESCsys_updated_on"),
        ),
        (
            "sysparm_fields",
            "sys_id,name,type,target_name,action,sys_updated_on,sys_updated_by,update_set",
        ),
        ("sysparm_display_value", "false"),
        ("sysparm_limit", &page),
    ]);
    let mut out = Vec::new();
    for row in client.paginate("/api/now/table/sys_update_xml", &q, limit) {
        let row = row?;
        if value(&row, "update_set").as_deref() != Some(set) {
            return Err(dropped("update_set=", "sys_update_xml"));
        }
        out.push(json!({
            "type": opt(value(&row, "type")),
            "target": opt(value(&row, "target_name")),
            "action": opt(value(&row, "action")),
            "updated_on": opt(value(&row, "sys_updated_on")),
            "updated_by": opt(value(&row, "sys_updated_by")),
            "update_name": opt(value(&row, "name")),
            "sys_id": opt(value(&row, "sys_id")),
        }));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_trusts_only_its_own_group() {
        let ok = json!({"result": [{"groupby_fields": [{"field": "update_set", "value": "s1"}],
                                     "stats": {"count": "297"}}]});
        assert_eq!(count_from_groups(&ok, "s1").unwrap(), 297);
        assert_eq!(count_from_groups(&json!({"result": []}), "s1").unwrap(), 0);
        let dropped = json!({"result": [
            {"groupby_fields": [{"field": "update_set", "value": "s1"}], "stats": {"count": "3"}},
            {"groupby_fields": [{"field": "update_set", "value": "s2"}], "stats": {"count": "9"}},
        ]});
        assert!(matches!(
            count_from_groups(&dropped, "s1"),
            Err(Error::Instance { .. })
        ));
    }

    #[test]
    fn global_scope_is_flagged() {
        let row = json!({"name": "Global", "scope": "global", "active": "true", "version": ""});
        let out = scope_json(&row, "global");
        assert_eq!(out["global"], true);
        assert_eq!(out["version"], Value::Null);
        assert_eq!(out["active"], true);
    }

    #[test]
    fn set_json_reads_display_all_pairs() {
        let row = json!({
            "name": {"value": "Default", "display_value": "Default"},
            "state": {"value": "in progress", "display_value": "In progress"},
            "application": {"value": "global", "display_value": "Global"},
            "is_default": {"value": "true", "display_value": "true"},
        });
        let out = set_json(&row, "s1");
        assert_eq!(out["in_progress"], true);
        assert_eq!(out["scope"], json!({"sys_id": "global", "name": "Global"}));
        assert_eq!(out["is_default"], true);
    }
}
