//! `sn codesearch` — instance-wide code search over the Code Search API
//! (`GET /api/sn_codesearch/code_search/search`), the endpoint Studio and the
//! VS Code extension use to find a string across every script-bearing table in
//! one call.
//!
//! Everything below was measured against a live instance, and most of it is in
//! the `CodeSearch` script include rather than in any documentation:
//!
//! - **A search runs over a "search group"** (`sn_codesearch_search_group`),
//!   which names the tables and fields searched. The stock
//!   `sn_codesearch.Default Search Group` lists 31 tables. The CLI always names
//!   it explicitly, so the table set `--table` is validated against and the one
//!   searched are the same group by construction.
//! - **An unknown `table` is silently ignored**: the script falls back to
//!   searching the whole group, which is slower and answers a different
//!   question under HTTP 200. `--table` is therefore checked against the
//!   group's `tables` endpoint first, and the response shape (one object for a
//!   table search, an array for a group search) is checked after.
//! - **The API defaults to the session's current application scope**, which
//!   for a REST caller is normally `global` — a search for code that lives in a
//!   scoped app then finds nothing. The CLI defaults to every scope instead,
//!   which is what "where on this instance is X" means; `--scope` narrows it.
//!   An unknown `current_app` also matches nothing under HTTP 200, so an empty
//!   scoped result is followed by one `sys_scope` lookup that tells a typo from
//!   a genuinely empty scope.
//! - **The term is spliced unescaped into an encoded query** (`fieldLIKE<term>`
//!   joined with `^OR`), so a term containing `^` is cut into pieces the
//!   instance reads as separate query terms — `^OR` finds nothing although
//!   scripts plainly contain it. Such terms are refused before the network.
//! - **The limit counts records examined, not hits.** Records the caller cannot
//!   read, and matches in fields it cannot see (protected scripts), are dropped
//!   after the limit was spent on them — 4–23% of examined records on the
//!   reference instance — and across tables the budget is whatever the earlier
//!   tables left. The instance also clamps it to the
//!   `sn_codesearch.search.results.max` property (500 stock). A cut-off result
//!   therefore does not announce itself; [`truncation_warning`] reconstructs
//!   each table's budget from the response and names the tables that may have
//!   been cut short.
//! - **An instance without the plugin** answers the route with HTTP 400
//!   "Requested URI does not represent any resource" (the generic answer for an
//!   absent scripted REST namespace), which is rewritten to say so.
//! - **Results are filtered by the caller's read access** (`canRead` per record
//!   and per `script` field). An `itil` caller gets HTTP 200 with almost
//!   nothing; that is the instance's ACL answer, not a failure.
//!
//! Output flattens the instance's `[{recordType, hits: [{matches: [...]}]}]`
//! nesting into one record per matching field, with the table, sys_id and
//! display name a caller needs to feed a hit to `sn table get` or `sn open`.

use crate::cli::GlobalFlags;
use crate::cli::OutputMode;
use crate::cli::kernel::{connect, unwrap_or_raw, write_response};
use crate::client::Client;
use crate::error::{Error, Result};
use serde_json::{Value, json};

const SEARCH_PATH: &str = "/api/sn_codesearch/code_search/search";
const TABLES_PATH: &str = "/api/sn_codesearch/code_search/tables";
/// The search group the CLI always searches, named explicitly so the `tables`
/// check and the search itself can never disagree about which group is meant.
const SEARCH_GROUP: &str = "sn_codesearch.Default Search Group";
/// The instance's stock ceiling on examined records
/// (`sn_codesearch.search.results.max`), and the CLI's default `--limit`.
const STOCK_MAX_RESULTS: u32 = 500;
/// A whole-group search walks 31 tables one query at a time: 5–24s measured on
/// a small PDI, so the global 30s default would cut off exactly the searches
/// this command exists for. An explicit `--timeout` still wins.
const DEFAULT_TIMEOUT_SECS: u64 = 120;
/// A table whose hits reach this share of the budget it was given may have
/// spent the rest of the budget on records it then dropped as unreadable. The
/// highest drop rate measured was 23%, so 0.8 keeps every measured cut-off
/// result inside the warning.
const TRUNCATION_RATIO: f64 = 0.8;
/// ServiceNow's reply for a scripted REST namespace that does not exist, which
/// is what the Code Search route is on an instance without the plugin.
const NO_SUCH_RESOURCE: &str = "Requested URI does not represent any resource";

#[derive(clap::Args, Debug)]
pub struct CodesearchArgs {
    /// Text to find in script and code fields: a case-insensitive substring,
    /// matched literally (no wildcards or regex). Must not contain `^`, which
    /// the instance would split into separate query terms.
    #[arg(value_name = "TERM")]
    pub term: String,
    /// Search only this table (e.g. `sys_script_include`). Must be one of the
    /// tables Code Search is configured to search; an unknown one is refused
    /// with the list of those that are.
    #[arg(long, value_name = "TABLE")]
    pub table: Option<String>,
    /// Search only one application scope: its name (`global`, `sn_codesearch`)
    /// or sys_id. Default: every scope.
    #[arg(long, value_name = "SCOPE")]
    pub scope: Option<String>,
    /// Maximum records the instance examines (not hits: unreadable records
    /// count against it too). The instance clamps it to its
    /// `sn_codesearch.search.results.max` property, 500 unless changed. A
    /// result that may have been cut short is reported on stderr.
    #[arg(long, value_name = "N", default_value_t = STOCK_MAX_RESULTS,
          value_parser = clap::value_parser!(u32).range(1..))]
    pub limit: u32,
}

pub fn run(global: &GlobalFlags, args: CodesearchArgs) -> Result<()> {
    validate_term(&args.term)?;
    if let Some(scope) = &args.scope {
        validate_scope(scope)?;
    }

    let mut scoped = global.clone();
    scoped.timeout = global.timeout.or(Some(DEFAULT_TIMEOUT_SECS));
    let client = connect(&scoped)?;

    if let Some(table) = &args.table {
        check_table(&client, table)?;
    }

    let resp = client
        .get(SEARCH_PATH, &search_query(&args))
        .map_err(plugin_missing)?;
    let groups = table_groups(&resp, args.table.as_deref())?;

    if let Some(warning) = truncation_warning(&groups, args.limit) {
        eprintln!("sn: warning: {warning}");
    }
    let rows = flatten(&groups, &args.term);
    if rows.is_empty()
        && let Some(scope) = &args.scope
    {
        check_scope(&client, scope)?;
    }

    match global.output {
        OutputMode::Raw => write_response(global, &unwrap_or_raw(resp, global.output)),
        OutputMode::Default | OutputMode::Table => write_response(global, &Value::Array(rows)),
    }
}

/// Refuse terms the instance cannot search for faithfully, before any network
/// round trip.
fn validate_term(term: &str) -> Result<()> {
    if term.trim().is_empty() {
        // The API answers an empty term with `[]` and HTTP 200 — "no matches"
        // for a search that never ran.
        return Err(Error::Usage("the search term must not be empty".into()));
    }
    if term.contains('^') {
        return Err(Error::Usage(format!(
            "the search term {term:?} contains `^`, which the Code Search API splices \
             unescaped into an encoded query, where it separates query terms — the \
             search would silently match the wrong records or none; search for a \
             fragment without `^` instead"
        )));
    }
    Ok(())
}

/// A scope is a name like `sn_codesearch` or a 32-hex sys_id; either way only
/// identifier characters, which also keeps the `sys_scope` lookup's encoded
/// query well-formed.
fn validate_scope(scope: &str) -> Result<()> {
    if scope.is_empty() || !scope.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(Error::Usage(format!(
            "--scope {scope:?} is not a scope name or sys_id (expected letters, digits and `_`, \
             e.g. `global` or `sn_codesearch`)"
        )));
    }
    Ok(())
}

fn search_query(args: &CodesearchArgs) -> Vec<(String, String)> {
    let mut q = vec![
        ("term".to_string(), args.term.clone()),
        ("search_group".to_string(), SEARCH_GROUP.to_string()),
        ("limit".to_string(), args.limit.to_string()),
    ];
    match &args.scope {
        Some(scope) => {
            q.push(("search_all_scopes".into(), "false".into()));
            q.push(("current_app".into(), scope.clone()));
        }
        None => q.push(("search_all_scopes".into(), "true".into())),
    }
    if let Some(table) = &args.table {
        q.push(("table".into(), table.clone()));
    }
    q
}

/// Name the plugin when the route itself is missing. Every other error passes
/// through untouched.
fn plugin_missing(err: Error) -> Error {
    match err {
        Error::Api {
            status: 400,
            message,
            detail,
            transaction_id,
            sn_error,
        } if message.trim() == NO_SUCH_RESOURCE => Error::Api {
            status: 400,
            message: format!(
                "the Code Search API is not available on this instance ({message}): the \
                 Code Search application (sn_codesearch) is not installed, or its REST API \
                 is inactive"
            ),
            detail,
            transaction_id,
            sn_error,
        },
        other => other,
    }
}

/// Refuse a `--table` the search group does not cover, naming the ones it does.
/// Without this the instance ignores the table and searches the whole group.
fn check_table(client: &Client, table: &str) -> Result<()> {
    let resp = client
        .get(
            TABLES_PATH,
            &[("search_group".to_string(), SEARCH_GROUP.to_string())],
        )
        .map_err(plugin_missing)?;
    let names: Vec<&str> = resp
        .get("result")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|t| t.get("name").and_then(Value::as_str))
        .collect();
    if names.contains(&table) {
        return Ok(());
    }
    let mut sorted = names;
    sorted.sort_unstable();
    Err(Error::Usage(format!(
        "Code Search does not search table '{table}' on this instance; it searches: {}",
        sorted.join(", ")
    )))
}

/// The per-table result objects, in the order the instance searched them. A
/// table search answers one object, a group search an array of them; a
/// `--table` search that comes back as an array means the table was ignored
/// and the whole group searched instead.
fn table_groups(resp: &Value, table: Option<&str>) -> Result<Vec<Value>> {
    match resp.get("result") {
        Some(Value::Array(groups)) => {
            if let Some(table) = table {
                return Err(Error::Instance {
                    message: format!(
                        "the instance ignored --table {table} and searched every table in \
                         the search group instead"
                    ),
                    detail: None,
                });
            }
            Ok(groups.clone())
        }
        Some(obj @ Value::Object(_)) => Ok(vec![obj.clone()]),
        _ => Err(Error::Instance {
            message: "the Code Search API returned a response without a `result`".into(),
            detail: Some(truncate(&resp.to_string(), 300)),
        }),
    }
}

fn truncate(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

fn hits(group: &Value) -> &[Value] {
    group
        .get("hits")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

/// The instance serializes every number as a float (`"line":1.0`).
fn int(v: Option<&Value>) -> Option<u64> {
    v.and_then(Value::as_f64).map(|f| f as u64)
}

/// One output record per matching field of each hit record. Context lines the
/// instance includes around a match are kept, with `match` telling them apart
/// (the same case-insensitive containment the instance tests with).
fn flatten(groups: &[Value], term: &str) -> Vec<Value> {
    let needle = term.to_lowercase();
    let mut rows = Vec::new();
    for group in groups {
        let record_type = group.get("recordType").and_then(Value::as_str);
        for hit in hits(group) {
            let table = hit
                .get("className")
                .and_then(Value::as_str)
                .or(record_type)
                .unwrap_or_default();
            let matches = hit.get("matches").and_then(Value::as_array);
            for m in matches.into_iter().flatten() {
                let lines: Vec<Value> = m
                    .get("lineMatches")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|l| {
                        let text = l.get("context").and_then(Value::as_str).unwrap_or_default();
                        json!({
                            "line": int(l.get("line")),
                            "text": text,
                            "match": text.to_lowercase().contains(&needle),
                        })
                    })
                    .collect();
                rows.push(json!({
                    "table": table,
                    "sys_id": hit.get("sysId").cloned().unwrap_or(Value::Null),
                    "name": hit.get("name").cloned().unwrap_or(Value::Null),
                    "field": m.get("field").cloned().unwrap_or(Value::Null),
                    "count": int(m.get("count")),
                    "lines": lines,
                }));
            }
        }
    }
    rows
}

/// Name the tables whose results may have been cut short, reconstructing the
/// budget the instance gave each one: the effective limit minus the hits of
/// every table searched before it (the script's own `getThisLimit`). A table
/// that filled most of its budget may have examined as many records as it was
/// allowed; tables left with no budget at all were never searched.
fn truncation_warning(groups: &[Value], limit: u32) -> Option<String> {
    let effective = u64::from(limit.min(STOCK_MAX_RESULTS));
    let mut found = 0u64;
    let mut near_full = Vec::new();
    let mut skipped = Vec::new();
    for group in groups {
        let table = group
            .get("recordType")
            .and_then(Value::as_str)
            .unwrap_or("?");
        let budget = effective.saturating_sub(found);
        let n = hits(group).len() as u64;
        if budget == 0 {
            skipped.push(table);
        } else if n as f64 >= budget as f64 * TRUNCATION_RATIO {
            near_full.push(format!("{table} ({n} of {budget})"));
        }
        found += n;
    }
    if near_full.is_empty() && skipped.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    if !near_full.is_empty() {
        parts.push(format!(
            "hits close to the record limit in {}",
            near_full.join(", ")
        ));
    }
    if !skipped.is_empty() {
        parts.push(format!(
            "{} table(s) not searched because the limit was used up ({})",
            skipped.len(),
            skipped.join(", ")
        ));
    }
    Some(format!(
        "results may be incomplete: {}. The limit counts records examined, including \
         unreadable ones, and the instance caps it at sn_codesearch.search.results.max \
         (500 unless changed); narrow with --table, --scope or a more specific term",
        parts.join("; ")
    ))
}

/// An empty `--scope` search: tell a scope that does not exist (a typo matches
/// nothing under HTTP 200) from one with no matching code. A lookup that
/// cannot be made — `sys_scope` is admin-readable only — leaves the empty
/// result standing with a warning rather than refusing a search that ran.
fn check_scope(client: &Client, scope: &str) -> Result<()> {
    let query = [
        (
            "sysparm_query".to_string(),
            format!("scope={scope}^ORsys_id={scope}"),
        ),
        ("sysparm_fields".to_string(), "sys_id".to_string()),
        ("sysparm_limit".to_string(), "1".to_string()),
    ];
    match client.get("/api/now/table/sys_scope", &query) {
        Ok(resp) => {
            let found = resp
                .get("result")
                .and_then(Value::as_array)
                .is_some_and(|rows| !rows.is_empty());
            if found {
                Ok(())
            } else {
                Err(Error::Usage(format!(
                    "no application scope '{scope}' on this instance; pass a scope name \
                     (e.g. `global`, `sn_codesearch`) or a scope sys_id"
                )))
            }
        }
        Err(err) => {
            eprintln!(
                "sn: warning: no matches, and --scope '{scope}' could not be verified ({err}); \
                 a scope that does not exist also matches nothing"
            );
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(table: &str, n: usize) -> Value {
        let hits: Vec<Value> = (0..n)
            .map(|i| json!({"className": table, "sysId": format!("id{i}"), "name": "n", "matches": []}))
            .collect();
        json!({"recordType": table, "tableLabel": table, "hits": hits})
    }

    #[test]
    fn a_caret_or_an_empty_term_is_refused() {
        assert!(matches!(validate_term("^OR"), Err(Error::Usage(_))));
        assert!(matches!(validate_term("a^b"), Err(Error::Usage(_))));
        assert!(matches!(validate_term("  "), Err(Error::Usage(_))));
        assert!(validate_term("new GlideRecord('incident')").is_ok());
        assert!(validate_term("sys_scope=global").is_ok());
    }

    #[test]
    fn a_scope_must_be_an_identifier() {
        assert!(validate_scope("global").is_ok());
        assert!(validate_scope("f9752f20d7120200b6bddb0c8252032e").is_ok());
        assert!(matches!(
            validate_scope("x^ORsys_id!=y"),
            Err(Error::Usage(_))
        ));
        assert!(matches!(validate_scope(""), Err(Error::Usage(_))));
    }

    #[test]
    fn the_query_searches_every_scope_unless_one_is_named() {
        let args = |scope: Option<&str>| CodesearchArgs {
            term: "x".into(),
            table: None,
            scope: scope.map(str::to_string),
            limit: 500,
        };
        let all = search_query(&args(None));
        assert!(all.contains(&("search_all_scopes".into(), "true".into())));
        assert!(!all.iter().any(|(k, _)| k == "current_app"));
        let one = search_query(&args(Some("global")));
        assert!(one.contains(&("search_all_scopes".into(), "false".into())));
        assert!(one.contains(&("current_app".into(), "global".into())));
        assert!(one.contains(&("search_group".into(), SEARCH_GROUP.into())));
    }

    #[test]
    fn flatten_emits_one_row_per_field_with_integer_positions_and_match_flags() {
        let groups = vec![json!({
            "recordType": "sys_script_include",
            "hits": [{
                "className": "sys_script_include",
                "sysId": "abc",
                "name": "CodeSearch",
                "modified": 1446757340000u64,
                "tableLabel": "sys_script_include",
                "matches": [
                    {"field": "name", "fieldLabel": "Name", "count": 1.0,
                     "lineMatches": [{"line": 1.0, "context": "CodeSearch", "escaped": "CodeSearch"}]},
                    {"field": "script", "fieldLabel": "Script", "count": 1.0,
                     "lineMatches": [
                        {"line": 4.0, "context": "var a;", "escaped": "var a;"},
                        {"line": 5.0, "context": "new codesearch()", "escaped": "new codesearch()"},
                        {"line": 6.0, "context": "}", "escaped": "}"}
                     ]}
                ]
            }]
        })];
        let rows = flatten(&groups, "CodeSearch");
        assert_eq!(
            rows,
            vec![
                json!({"table": "sys_script_include", "sys_id": "abc", "name": "CodeSearch",
                       "field": "name", "count": 1,
                       "lines": [{"line": 1, "text": "CodeSearch", "match": true}]}),
                json!({"table": "sys_script_include", "sys_id": "abc", "name": "CodeSearch",
                "field": "script", "count": 1,
                "lines": [
                    {"line": 4, "text": "var a;", "match": false},
                    {"line": 5, "text": "new codesearch()", "match": true},
                    {"line": 6, "text": "}", "match": false}
                ]}),
            ]
        );
    }

    #[test]
    fn flatten_prefers_the_record_class_over_the_searched_table() {
        let groups = vec![json!({
            "recordType": "sys_metadata",
            "hits": [{"className": "sys_ui_page", "sysId": "x", "name": "p",
                      "matches": [{"field": "html", "count": 1.0, "lineMatches": []}]}]
        })];
        assert_eq!(flatten(&groups, "t")[0]["table"], "sys_ui_page");
    }

    #[test]
    fn a_table_search_that_came_back_as_a_group_search_is_an_instance_error() {
        let resp = json!({"result": [group("sys_script", 0)]});
        assert!(matches!(
            table_groups(&resp, Some("sys_script_include")),
            Err(Error::Instance { .. })
        ));
        assert_eq!(table_groups(&resp, None).unwrap().len(), 1);
        let single = json!({"result": group("sys_script_include", 2)});
        assert_eq!(
            table_groups(&single, Some("sys_script_include"))
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn no_warning_when_every_table_stayed_well_inside_its_budget() {
        let groups = vec![group("a", 10), group("b", 0), group("c", 200)];
        assert_eq!(truncation_warning(&groups, 500), None);
    }

    #[test]
    fn a_table_that_nearly_filled_its_remaining_budget_is_named() {
        // 100 hits leave c a budget of 400; 330 of 400 is past the ratio.
        let groups = vec![group("a", 100), group("c", 330)];
        let w = truncation_warning(&groups, 500).unwrap();
        assert!(w.contains("c (330 of 400)"), "{w}");
        assert!(!w.contains("a ("), "{w}");
    }

    #[test]
    fn tables_left_without_budget_are_reported_as_not_searched() {
        let groups = vec![group("a", 5), group("b", 0), group("c", 0)];
        let w = truncation_warning(&groups, 5).unwrap();
        assert!(w.contains("a (5 of 5)"), "{w}");
        assert!(w.contains("2 table(s) not searched"), "{w}");
        assert!(w.contains("(b, c)"), "{w}");
    }

    #[test]
    fn the_budget_never_exceeds_the_stock_ceiling() {
        // --limit 2000 is clamped to 500 by a stock instance, so 480 hits in
        // one table is near-full even though it is far below 2000.
        let groups = vec![group("a", 480)];
        assert!(truncation_warning(&groups, 2000).is_some());
    }

    #[test]
    fn only_the_missing_route_is_rewritten_to_name_the_plugin() {
        let api = |status, message: &str| Error::Api {
            status,
            message: message.into(),
            detail: None,
            transaction_id: None,
            sn_error: None,
        };
        let Error::Api {
            status, message, ..
        } = plugin_missing(api(400, NO_SUCH_RESOURCE))
        else {
            panic!("not an API error")
        };
        assert_eq!(status, 400);
        assert!(message.contains("sn_codesearch"), "{message}");
        let Error::Api { message, .. } = plugin_missing(api(400, "Invalid table")) else {
            panic!("not an API error")
        };
        assert_eq!(message, "Invalid table");
    }
}
