//! `sn get last` — the task you most recently updated, read the way
//! `sn get <REF>` reads one (record, variables, journal).
//!
//! One Table API read finds it (`sys_updated_by=<you>`, newest first, two
//! rows: each must name you, or the term was dropped and the row is someone
//! else's), and the row's own `sys_class_name` is the table the composite
//! read then uses.

use super::common::{caller, dropped, identifier, not_found, pairs, rows, value};
use crate::cli::get_record::{emit_composite, resolve_and_journal};
use crate::cli::journal::validate_identifier;
use crate::cli::kernel::connect;
use crate::cli::record_ref::{RecordRef, RefId};
use crate::cli::{GlobalFlags, OutputMode};
use crate::error::{Error, Result};
use crate::query::GetQuery;

#[derive(clap::Args, Debug)]
pub struct LastArgs {
    /// Look in this table instead of every task (e.g. `incident`, `sys_script`).
    #[arg(long, default_value = "task")]
    pub table: String,
    /// Comma-separated fields to return on the record (variables and journal
    /// are unaffected).
    #[arg(long, short = 'f')]
    pub fields: Option<String>,
}

pub fn run(global: &GlobalFlags, args: LastArgs) -> Result<()> {
    if global.output == OutputMode::Raw {
        return Err(Error::Usage(
            "--output raw cannot render sn get's composite result; use the default output or \
             --output table"
                .into(),
        ));
    }
    let table = identifier(&args.table, "table")?.to_string();
    let client = connect(global)?;
    let (user_name, _) = caller(&client)?;

    let resp = client.get(
        &format!("/api/now/table/{table}"),
        &pairs(&[
            (
                "sysparm_query",
                &format!("sys_updated_by={user_name}^ORDERBYDESCsys_updated_on"),
            ),
            ("sysparm_fields", "sys_id,sys_class_name,sys_updated_by"),
            ("sysparm_display_value", "false"),
            ("sysparm_limit", "2"),
        ]),
    )?;
    let found = rows(resp);
    for r in &found {
        if !value(r, "sys_updated_by").is_some_and(|u| u.eq_ignore_ascii_case(&user_name)) {
            return Err(dropped("sys_updated_by=", &table));
        }
    }
    let Some(row) = found.first() else {
        return Err(not_found(format!(
            "no {table} record was last updated by {user_name}"
        )));
    };
    let sys_id = value(row, "sys_id").ok_or_else(|| Error::Instance {
        message: format!("the latest {table} row came back without a sys_id"),
        detail: None,
    })?;
    let class = value(row, "sys_class_name")
        .filter(|c| validate_identifier(c, "table").is_ok())
        .unwrap_or(table);

    let r = RecordRef {
        table: class,
        id: RefId::SysId(sys_id),
    };
    let found = resolve_and_journal(&client, &r, false, true)?
        .ok_or_else(|| not_found(format!("{r} is no longer readable")))?;
    let q = GetQuery {
        fields: args.fields,
        display_value: Some(crate::query::DisplayValue::True),
        exclude_reference_link: None,
        view: None,
        query_no_domain: None,
    };
    emit_composite(global, &client, &r.table, found, &q)
}
