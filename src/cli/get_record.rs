//! `sn get` — one record by reference, with its variables and journal.
//!
//! The composite read: `sn get incident:INC0010001` (or `sn get INC0010001`,
//! whose prefix names the table) answers with the record, its catalog
//! variables, and its parsed journal entries in one command. Three round
//! trips, each doing the only job it can:
//!
//! 1. **One GraphQL document** resolves the reference and fetches the journal
//!    streams together: `{table}(queryConditions: "number=…"|"sys_id=…")`
//!    selecting `sys_id` plus the `comments`/`work_notes` displayValues (the
//!    itil-safe journal route — see `journal.rs`). `pagination: {limit: 2}`
//!    with `_rowCount` is the silent-drop canary: ServiceNow drops a query
//!    term it cannot parse and returns unfiltered rows, so on a table with no
//!    `number` field a second row (measured live: `_rowCount: 67` on
//!    sys_user_group) is proof the term is gone, and that is an error rather
//!    than a stranger's record.
//! 2. A Table API GET for the record body — GraphQL cannot select "all
//!    fields" (no wildcard, and runtime introspection is ~94 MB), so the REST
//!    read is what carries the row. `--fields`/`--display-value` shape this
//!    request only.
//! 3. The variable pool via `variables.rs`'s joins (probe-free: step 1
//!    already proved the record exists), including the sc_task → sc_req_item
//!    hop.
//!
//! A bare number costs a prefix → table step before 1 (free for a cached or
//! built-in prefix, one `sys_number` read otherwise — see `record_ref.rs`),
//! and step 1 then also selects `sys_class_name`: a prefix names the table
//! its counter is defined on, which for an inheriting table is an ancestor,
//! so the row's own class is the table steps 2 and 3 read.
//!
//! Current instances return `null` for a column a table does not have (also
//! measured live), so tables without journal fields simply yield an empty
//! `journal` — but older releases fail the whole document with a
//! FieldUndefined error naming the column, so the fetch retries without any
//! column named that way.

use crate::cli::graphql::{errors_to_api_error, execute, graphql_errors};
use crate::cli::journal::{self, undefined_field, validate_identifier};
use crate::cli::kernel::{bool_opt, connect, unwrap_or_raw, write_response};
use crate::cli::record_ref::{
    GetRef, PrefixSource, RecordRef, RefId, parse_get_ref, recheck_prefix, resolve_prefix,
};
use crate::cli::variables::{fetch_vars_unchecked, resolve_target};
use crate::cli::{ADVANCED, DisplayValueOpt, GlobalFlags, OutputMode};
use crate::client::Client;
use crate::error::{Error, NO_HTTP_STATUS, Result};
use crate::query::GetQuery;
use serde_json::{Value, json};

#[derive(clap::Args, Debug)]
pub struct GetRecordArgs {
    /// Record reference: `table:sys_id`, `table:number`
    /// (e.g. `incident:INC0010001`), or a bare number whose prefix names the
    /// table — the standard ones (INC, CHG, CTASK, PRB, REQ, RITM, SCTASK, KB,
    /// SIR) are built in, any other is looked up in the instance's sys_number
    /// table (admin-readable by default) and cached.
    #[arg(value_name = "REF")]
    pub reference: String,
    /// Comma-separated fields to return on the record (variables and journal
    /// are unaffected).
    #[arg(long, short = 'f', alias = "sysparm-fields")]
    pub fields: Option<String>,
    #[command(flatten)]
    pub display_value: DisplayValueOpt,
    /// Strip reference-link URLs from reference fields.
    #[arg(
        long,
        alias = "sysparm-exclude-reference-link",
        help_heading = ADVANCED
    )]
    pub exclude_reference_link: bool,
    /// Apply a named form/list view.
    #[arg(long, alias = "sysparm-view", help_heading = ADVANCED)]
    pub view: Option<String>,
    /// Cross-domain access if authorized.
    #[arg(long, alias = "sysparm-query-no-domain", help_heading = ADVANCED)]
    pub query_no_domain: bool,
}

pub fn run(global: &GlobalFlags, args: GetRecordArgs) -> Result<()> {
    // Pure argv checks first, per the guard-ordering rule: neither may cost a
    // network call (on an OAuth profile `connect` mints a token).
    if global.output == OutputMode::Raw {
        return Err(Error::Usage(
            "--output raw cannot render sn get's composite result (three requests, \
             no single envelope to keep); use the default output or --output table, \
             or `sn table get` for the bare record"
                .into(),
        ));
    }
    let parsed = parse_get_ref(&args.reference)?;

    let client = connect(global)?;
    let (table, found) = match parsed {
        GetRef::Ref(r) => match resolve_and_journal(&client, &r, false)? {
            Some(found) => (r.table, found),
            None => return Err(not_found(&r)),
        },
        GetRef::Bare { number, prefix } => resolve_bare(&client, number, &prefix)?,
    };
    let Found {
        sys_id, entries, ..
    } = found;

    let q = GetQuery {
        fields: args.fields,
        display_value: args.display_value.display_value.map(Into::into),
        exclude_reference_link: bool_opt(args.exclude_reference_link),
        view: args.view,
        query_no_domain: bool_opt(args.query_no_domain),
    };
    let resp = client.get(&format!("/api/now/table/{table}/{sys_id}"), &q.to_pairs())?;
    // Always unwrapped: `--output raw` was rejected above, because a composite
    // built from three requests has no single envelope to keep.
    let record = unwrap_or_raw(resp, OutputMode::Default);

    // An sc_task's variable pool lives on its request item; every other table
    // holds its own. The hop is the one extra request, and only for sc_task.
    let (var_table, var_sys_id, _) = resolve_target(&client, &table, &sys_id)?;
    let variables = fetch_vars_unchecked(&client, &var_table, &var_sys_id)?;

    let out = json!({
        "table": table,
        "sys_id": sys_id,
        "record": record,
        "variables": variables,
        "journal": entries,
    });
    write_response(global, &out)
}

/// A bare number: its prefix names the table (`record_ref::resolve_prefix`),
/// and the row's own `sys_class_name` names the table the record is then read
/// from. Returns that table and the row.
///
/// A cached or built-in answer that finds nothing is re-checked against
/// `sys_number` once: if the prefix now belongs to another table (a
/// renumbered stock table, a stale cache entry) the lookup is retried there;
/// otherwise the original not-found stands.
fn resolve_bare(client: &Client, number: String, prefix: &str) -> Result<(String, Found)> {
    let (table, source) = resolve_prefix(client, prefix)?;
    let mut r = RecordRef {
        table,
        id: RefId::Number(number),
    };
    let mut found = resolve_and_journal(client, &r, true)?;
    if found.is_none()
        && source != PrefixSource::Instance
        && let Some(table) = recheck_prefix(client, prefix, &r.table)
    {
        r.table = table;
        found = resolve_and_journal(client, &r, true)?;
    }
    let Some(mut found) = found else {
        return Err(not_found(&r));
    };
    let table = found.class.take().unwrap_or(r.table);
    Ok((table, found))
}

/// The row step 1 matched.
struct Found {
    sys_id: String,
    /// The row's `sys_class_name`, when it was selected, is a valid table
    /// name, and differs from the table queried.
    class: Option<String>,
    entries: Vec<journal::Entry>,
}

fn not_found(r: &RecordRef) -> Error {
    let named = match &r.id {
        RefId::SysId(id) => format!("sys_id {id}"),
        RefId::Number(n) => format!("number {n}"),
    };
    Error::Api {
        // The HTTP call succeeded; the operation found nothing — and GraphQL
        // row ACLs make "absent" and "not readable" the same bytes, so the
        // message hedges rather than overclaiming.
        status: NO_HTTP_STATUS,
        message: format!(
            "no {} record with {named} (or not readable by this profile)",
            r.table
        ),
        detail: None,
        transaction_id: None,
        sn_error: None,
    }
}

/// One GraphQL document: resolve the reference to a sys_id and fetch the
/// journal streams with it (entries newest first). `Ok(None)` when no row
/// matched. `with_class` also selects `sys_class_name`.
fn resolve_and_journal(
    client: &Client,
    r: &RecordRef,
    mut with_class: bool,
) -> Result<Option<Found>> {
    let condition = match &r.id {
        RefId::SysId(id) => format!("sys_id={id}"),
        RefId::Number(n) => format!("number={n}"),
    };
    let mut journal_cols = vec!["comments", "work_notes"];
    let table = &r.table;

    loop {
        let mut selection: String = journal_cols
            .iter()
            .map(|c| format!(" {c} {{ displayValue }}"))
            .collect();
        if with_class {
            selection.push_str(" sys_class_name { value }");
        }
        let query = format!(
            "query ($qc: String!) {{ GlideRecord_Query {{ {table}(queryConditions: $qc, \
             pagination: {{ limit: 2 }}) {{ _rowCount _results {{ sys_id {{ value }}{selection} \
             }} }} }} }}"
        );
        let resp = execute(client, &query, Some(json!({ "qc": condition })), None)?;
        let errors = graphql_errors(&resp);
        if !errors.is_empty() {
            if undefined_field(&errors, table) {
                return Err(Error::Api {
                    status: 200,
                    message: format!("table '{table}' is not queryable via GraphQL"),
                    detail: None,
                    transaction_id: None,
                    sn_error: Some(Value::Array(errors)),
                });
            }
            // An older instance failing the document over a column the table
            // does not have: retry without it. Modern instances return null
            // for such a column and never take this path.
            let before = journal_cols.len();
            journal_cols.retain(|c| !undefined_field(&errors, c));
            let class_dropped = with_class && undefined_field(&errors, "sys_class_name");
            if class_dropped {
                with_class = false;
            }
            if journal_cols.len() < before || class_dropped {
                continue;
            }
            return Err(errors_to_api_error(errors));
        }

        let node = resp
            .pointer(&format!("/data/GlideRecord_Query/{table}"))
            .cloned()
            .unwrap_or(Value::Null);
        let row_count = node.get("_rowCount").and_then(Value::as_u64).unwrap_or(0);
        let results = node
            .get("_results")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        if let RefId::Number(n) = &r.id {
            // `number` is unique where it exists, so more than one match means
            // the instance dropped the term and these rows are unfiltered.
            if results.len() > 1 || row_count > 1 {
                return Err(Error::Instance {
                    message: format!(
                        "cannot resolve {n}: the number={n} query term was dropped by \
                         the instance, so the rows returned are arbitrary"
                    ),
                    detail: Some(format!(
                        "{table} likely has no queryable `number` field; pass the \
                         record's sys_id instead ({table}:<sys_id>)"
                    )),
                });
            }
        }
        let Some(record) = results.first() else {
            return Ok(None);
        };

        let sys_id = record
            .pointer("/sys_id/value")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .ok_or_else(|| Error::Instance {
                message: format!(
                    "the {table} record matching {condition} came back without a sys_id"
                ),
                detail: None,
            })?;

        // The class is spliced into the record read's URL path, so it gets
        // the same guard a typed table name does; anything else keeps the
        // table queried.
        let class = record
            .pointer("/sys_class_name/value")
            .and_then(Value::as_str)
            .filter(|c| c != table && validate_identifier(c, "table").is_ok())
            .map(str::to_string);

        let mut entries = Vec::new();
        for col in &journal_cols {
            let stream = record
                .pointer(&format!("/{col}/displayValue"))
                .and_then(Value::as_str)
                .unwrap_or("");
            entries.extend(journal::parse_stream(stream)?);
        }
        journal::sort_newest_first(&mut entries);
        return Ok(Some(Found {
            sys_id,
            class,
            entries,
        }));
    }
}
