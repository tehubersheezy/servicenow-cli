//! `sn get mine` — your open work: active tasks assigned to you, plus
//! unassigned ones in your groups.
//!
//! The caller and their groups are named first (`common::caller`,
//! `common::groups_of`), so the task queries carry literal sys_ids, not a
//! scripted `javascript:gs.getUserID()` term the instance might drop. The two
//! halves are two queries rather than one `^NQ` (a trailing term binds only
//! the last `^NQ` segment), merged newest-updated first. Every row is checked
//! against the half it came from.

use super::common::{caller, display, dropped, groups_of, identifier, opt, pairs, rows, value};
use crate::cli::GlobalFlags;
use crate::cli::kernel::{connect, write_response};
use crate::client::Client;
use crate::error::Result;
use serde_json::{Value, json};

#[derive(clap::Args, Debug)]
pub struct MineArgs {
    /// Only tasks assigned to you, not unassigned work in your groups.
    #[arg(long)]
    pub me_only: bool,
    /// Only this task table (e.g. `incident`); every task type by default.
    #[arg(long, default_value = "task")]
    pub table: String,
    /// Maximum tasks to return.
    #[arg(long, short = 'l', default_value_t = 50)]
    pub limit: u32,
}

const FIELDS: &str = "sys_id,number,sys_class_name,short_description,state,priority,active,\
                      assigned_to,assignment_group,sys_updated_on";

pub fn run(global: &GlobalFlags, args: MineArgs) -> Result<()> {
    let table = identifier(&args.table, "table")?.to_string();
    let client = connect(global)?;
    let (_, me) = caller(&client)?;

    let mut tasks = query(
        &client,
        &table,
        &format!("assigned_to={me}"),
        args.limit,
        |r| value(r, "assigned_to").as_deref() == Some(me.as_str()),
    )?;
    if !args.me_only {
        let groups: Vec<String> = groups_of(&client, &me)?
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        if !groups.is_empty() {
            let term = format!("assigned_toISEMPTY^assignment_groupIN{}", groups.join(","));
            tasks.extend(query(&client, &table, &term, args.limit, |r| {
                value(r, "assigned_to").is_none()
                    && value(r, "assignment_group").is_some_and(|g| groups.contains(&g))
            })?);
        }
    }
    // `sys_updated_on` values are UTC `YYYY-MM-DD HH:MM:SS`, so they sort as strings.
    tasks.sort_by(|a, b| b["updated_on"].as_str().cmp(&a["updated_on"].as_str()));
    tasks.truncate(args.limit as usize);
    write_response(global, &Value::Array(tasks))
}

fn query(
    client: &Client,
    table: &str,
    term: &str,
    limit: u32,
    belongs: impl Fn(&Value) -> bool,
) -> Result<Vec<Value>> {
    let resp = client.get(
        &format!("/api/now/table/{table}"),
        &pairs(&[
            (
                "sysparm_query",
                &format!("active=true^{term}^ORDERBYDESCsys_updated_on"),
            ),
            ("sysparm_fields", FIELDS),
            ("sysparm_display_value", "all"),
            ("sysparm_limit", &limit.to_string()),
        ]),
    )?;
    let mut out = Vec::new();
    for r in rows(resp) {
        if value(&r, "active").as_deref() != Some("true") || !belongs(&r) {
            return Err(dropped("assignment", table));
        }
        out.push(task_json(&r));
    }
    Ok(out)
}

fn task_json(r: &Value) -> Value {
    json!({
        "number": opt(value(r, "number")),
        "table": opt(value(r, "sys_class_name")),
        "short_description": opt(value(r, "short_description")),
        "state": opt(display(r, "state")),
        "priority": opt(display(r, "priority")),
        "assigned_to": opt(display(r, "assigned_to")),
        "assignment_group": opt(display(r, "assignment_group")),
        "updated_on": opt(value(r, "sys_updated_on")),
        "sys_id": opt(value(r, "sys_id")),
    })
}
