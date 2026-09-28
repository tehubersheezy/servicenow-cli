//! The offline schema index behind `sn cache` and dynamic shell completion.
//!
//! One file per instance — every table, the table it extends, and the columns
//! it *defines* — built by `sn cache refresh` and afterwards read with no
//! network at all. Inherited columns are not stored per table; they are
//! resolved at lookup by walking the parent chain, which keeps the file at
//! ~one entry per `sys_dictionary` row instead of one per (table × ancestor).
//!
//! # Where the data comes from, and why
//!
//! Measured on a Zurich PDI (dev421992, 7,891 tables, ~140k own columns):
//!
//! * **Aggregate API over `sys_dictionary`** (`GROUP BY name, element`) — the
//!   source used here. ~0.6 ms/row; the `c*` tables alone (70,658 rows) came
//!   back in 44 s as one request.
//! * **Table API over `sys_dictionary`** — ~12 ms/row, twenty times slower
//!   (per-record ACL and field evaluation), and a 5,000-row page was cut off at
//!   the instance's 60 s transaction quota. A full pass is half an hour.
//! * **GraphQL introspection** — not viable. Even the table list alone
//!   (`__type(name: "GlideRecord_Query") { fields { name } }`) ran into a 504
//!   gateway timeout at 300 s, and graphql-java's "bad faith introspection"
//!   guard rejects any document that selects `__Type.fields` twice, so columns
//!   would cost one request per table.
//!
//! The hierarchy comes from one aggregate over `sys_db_object`
//! (`GROUP BY name, super_class.name`), which an `itil` profile can also read —
//! `sys_dictionary` is admin-only, so such a profile gets a tables-only index
//! rather than a failure.
//!
//! # Sharding
//!
//! The dictionary is fetched in shards of ~[`SHARD_ROWS`] rows, each a range
//! of table names `name>=lo^name<hi`. The boundaries come from a first
//! `GROUP BY name` pass ordered by the database itself — its collation sorts
//! `_` after the letters, so a client-side byte sort would draw the ranges in
//! the wrong places. Coverage does not depend on that ordering, though: the
//! first range is unbounded below and the last unbounded above, so every name
//! falls in at least one range whatever the order, and a misordering could only
//! produce overlap, which the per-table dedup absorbs. A shard that times out
//! is split in half and retried.

use crate::client::Client;
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::ops::Bound;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// On-disk format version. A file written under another version is refused
/// with a pointer at `sn cache refresh` rather than half-read.
pub const FORMAT: u32 = 1;

/// Target rows per dictionary shard. ~6 s on a loaded PDI, comfortably inside
/// both the client timeout and the instance's transaction quota.
pub const SHARD_ROWS: u64 = 10_000;

/// Shards fetched concurrently. Enough to hide per-request latency without
/// turning a cache refresh into a load test of the instance.
pub const WORKERS: usize = 4;

/// Times a timed-out shard may be halved before the refresh gives up.
const MAX_SPLIT_ROUNDS: usize = 4;

/// Encoded-query terms every dictionary request carries: real columns only
/// (a table's own definition row has an empty `element`), and not the
/// `var__m_*` variable-editor pseudo-tables — ~9,400 names and ~32k rows on a
/// PDI that are not tables in `sys_db_object` at all. Should an instance drop
/// the `NOT LIKE` term, those rows are discarded client-side anyway: only names
/// present in `sys_db_object` are kept.
const DICTIONARY_TERMS: &str = "elementISNOTEMPTY^nameNOT LIKEvar__m_";

/// The whole index, as persisted.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SchemaIndex {
    pub format: u32,
    /// The profile's instance host the index was built from.
    pub instance: String,
    /// Unix seconds.
    pub built_at: u64,
    /// `false` when the building profile could not read `sys_dictionary`, so
    /// the index names tables but carries no columns.
    pub columns_indexed: bool,
    pub tables: BTreeMap<String, TableEntry>,
}

/// One table: what it extends, and the columns defined on it directly.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct TableEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<String>,
}

impl SchemaIndex {
    /// Every column on `table`: its own plus every ancestor's, sorted. `None`
    /// when the table is not in the index.
    pub fn columns(&self, table: &str) -> Option<Vec<String>> {
        self.tables.get(table)?;
        let mut out = BTreeSet::new();
        for t in self.lineage(table) {
            if let Some(entry) = self.tables.get(t) {
                out.extend(entry.columns.iter().cloned());
            }
        }
        Some(out.into_iter().collect())
    }

    /// `table`, then its parent, then its parent's parent. Stops at a parent
    /// the index does not know and at a cycle (a corrupt or hand-edited file
    /// must not hang a completion).
    pub fn lineage<'a>(&'a self, table: &'a str) -> Vec<&'a str> {
        let mut chain = Vec::new();
        let mut current = Some(table);
        while let Some(t) = current {
            if chain.contains(&t) || !self.tables.contains_key(t) {
                break;
            }
            chain.push(t);
            current = self.tables.get(t).and_then(|e| e.parent.as_deref());
        }
        chain
    }

    /// Whether `table` is `root` or extends it, directly or not.
    pub fn extends(&self, table: &str, root: &str) -> bool {
        self.lineage(table).contains(&root)
    }

    /// Table names starting with `prefix`, in name order.
    pub fn tables_with_prefix<'a>(&'a self, prefix: &'a str) -> impl Iterator<Item = &'a str> {
        self.tables
            .range::<str, _>((Bound::Included(prefix), Bound::Unbounded))
            .map(|(name, _)| name.as_str())
            .take_while(move |name| name.starts_with(prefix))
    }

    /// Number of own-column entries across every table.
    pub fn column_count(&self) -> usize {
        self.tables.values().map(|t| t.columns.len()).sum()
    }
}

/// The per-instance cache directory name: the host, lowercased, with anything
/// outside `[a-z0-9.-]` (a port's `:`, a path) flattened to `_`. Keyed by
/// instance rather than profile, so two profiles on one instance share an
/// index — the schema is the instance's, not the caller's.
pub fn cache_key(instance: &str) -> String {
    let host = instance
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');
    host.chars()
        .map(|c| {
            let c = c.to_ascii_lowercase();
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// `<config dir>/cache/<instance>/schema.json`. Under the config directory, not
/// the platform cache directory, so `SN_CONFIG_DIR` isolates it along with
/// everything else.
pub fn cache_path_in(config_dir: &Path, instance: &str) -> PathBuf {
    config_dir
        .join("cache")
        .join(cache_key(instance))
        .join("schema.json")
}

/// [`cache_path_in`] under the resolved config directory.
pub fn cache_path(instance: &str) -> Result<PathBuf> {
    Ok(cache_path_in(&crate::config::config_dir()?, instance))
}

/// Read an index. `Ok(None)` when there is no file; a file that does not parse,
/// or was written under another [`FORMAT`], is a config error naming the fix.
pub fn load(path: &Path) -> Result<Option<SchemaIndex>> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Error::Config(format!("read {}: {e}", path.display()))),
    };
    let stale = || {
        Error::Config(format!(
            "schema cache {} is unreadable or from another sn version; run `sn cache refresh`",
            path.display()
        ))
    };
    let index: SchemaIndex = serde_json::from_slice(&bytes).map_err(|_| stale())?;
    if index.format != FORMAT {
        return Err(stale());
    }
    Ok(Some(index))
}

/// Persist an index through the config module's atomic 0600 write. No
/// directory lock: this is a whole-file replacement, not a read-modify-write,
/// so two concurrent refreshes each publish a complete index and the last
/// rename wins.
pub fn save(path: &Path, index: &SchemaIndex) -> Result<()> {
    let json = serde_json::to_string(index)
        .map_err(|e| Error::Config(format!("serialize schema cache: {e}")))?;
    crate::config::write_atomic(path, &json)
}

/// What a build did, for the caller's report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildStats {
    pub requests: usize,
}

/// Build a fresh index from the instance behind `client`.
pub fn build(client: &Client, instance: &str, built_at: u64) -> Result<(SchemaIndex, BuildStats)> {
    let mut requests = 0;

    let resp = client.get(
        "/api/now/stats/sys_db_object",
        &pairs(&[
            ("sysparm_group_by", "name,super_class.name"),
            ("sysparm_count", "true"),
            ("sysparm_display_value", "false"),
        ]),
    )?;
    requests += 1;
    let mut tables = parse_tables(&resp)?;

    let plan_resp = client.get(
        "/api/now/stats/sys_dictionary",
        &pairs(&[
            ("sysparm_query", DICTIONARY_TERMS),
            ("sysparm_group_by", "name"),
            ("sysparm_order_by", "name"),
            ("sysparm_count", "true"),
            ("sysparm_display_value", "false"),
        ]),
    );
    requests += 1;
    let plan = match plan_resp {
        Ok(v) => parse_plan(&v)?,
        // sys_dictionary is admin-only (an itil profile gets 403 from the
        // Table and Aggregate APIs alike); the table list is still worth
        // having, so this degrades to a tables-only index instead of failing.
        Err(Error::Auth { status: 403, .. }) => {
            let index = SchemaIndex {
                format: FORMAT,
                instance: instance.to_string(),
                built_at,
                columns_indexed: false,
                tables,
            };
            return Ok((index, BuildStats { requests }));
        }
        Err(e) => return Err(e),
    };

    let (rows, shard_requests) = fetch_shards(client, &plan)?;
    requests += shard_requests;
    for (table, column) in rows {
        if let Some(entry) = tables.get_mut(&table) {
            entry.columns.push(column);
        }
    }
    for entry in tables.values_mut() {
        entry.columns.sort();
        entry.columns.dedup();
    }

    let index = SchemaIndex {
        format: FORMAT,
        instance: instance.to_string(),
        built_at,
        columns_indexed: true,
        tables,
    };
    Ok((index, BuildStats { requests }))
}

fn pairs(p: &[(&str, &str)]) -> Vec<(String, String)> {
    p.iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// The `groupby_fields` of one aggregate row as `field → value`.
fn group_fields(row: &Value) -> BTreeMap<&str, &str> {
    row.get("groupby_fields")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|g| Some((g.get("field")?.as_str()?, g.get("value")?.as_str()?)))
        .collect()
}

fn result_rows<'a>(resp: &'a Value, what: &str) -> Result<&'a Vec<Value>> {
    resp.get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Instance {
            message: format!("aggregate over {what} returned no result array"),
            detail: None,
        })
}

/// Tables and their parents from the `sys_db_object` aggregate.
///
/// The dot-walked `super_class.name` group is checked for, not assumed: an
/// instance that ignored it would still return one group per table, and the
/// index would silently lose every inheritance edge — and with them every
/// inherited column (`incident` would complete none of `task`'s).
pub fn parse_tables(resp: &Value) -> Result<BTreeMap<String, TableEntry>> {
    let rows = result_rows(resp, "sys_db_object")?;
    let mut tables = BTreeMap::new();
    let mut saw_parent_group = false;
    for row in rows {
        let g = group_fields(row);
        let Some(name) = g.get("name").filter(|n| !n.is_empty()) else {
            continue;
        };
        let parent = g.get("super_class.name").copied();
        saw_parent_group |= parent.is_some();
        tables.insert(
            name.to_string(),
            TableEntry {
                parent: parent.filter(|p| !p.is_empty()).map(str::to_string),
                columns: Vec::new(),
            },
        );
    }
    if tables.is_empty() {
        return Err(Error::Instance {
            message: "aggregate over sys_db_object returned no tables".into(),
            detail: None,
        });
    }
    if !saw_parent_group {
        return Err(Error::Instance {
            message: "aggregate over sys_db_object ignored the super_class.name grouping; \
                      the table hierarchy is unavailable"
                .into(),
            detail: None,
        });
    }
    Ok(tables)
}

/// `(table, column count)` in the order the instance returned them — its own
/// collation order, which the shard boundaries must follow.
pub fn parse_plan(resp: &Value) -> Result<Vec<(String, u64)>> {
    let rows = result_rows(resp, "sys_dictionary")?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            let name = group_fields(row).get("name")?.to_string();
            let count = row
                .get("stats")
                .and_then(|s| s.get("count"))
                .and_then(|c| c.as_str().and_then(|s| s.parse().ok()).or(c.as_u64()))
                .unwrap_or(0);
            (!name.is_empty()).then_some((name, count))
        })
        .collect())
}

/// `(table, column)` pairs from one shard's `GROUP BY name, element`.
pub fn parse_pairs(resp: &Value) -> Result<Vec<(String, String)>> {
    let rows = result_rows(resp, "sys_dictionary")?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            let g = group_fields(row);
            let name = g.get("name").filter(|n| !n.is_empty())?;
            let element = g.get("element").filter(|e| !e.is_empty())?;
            Some((name.to_string(), element.to_string()))
        })
        .collect())
}

/// A contiguous run `plan[start..end]`. Its query bounds come from the plan
/// entries at the run's edges; the first run has no lower bound and the last
/// no upper one, which is what guarantees coverage (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shard {
    pub start: usize,
    pub end: usize,
}

impl Shard {
    /// The encoded query for this shard.
    pub fn query(&self, plan: &[(String, u64)]) -> String {
        let mut q = String::from(DICTIONARY_TERMS);
        if self.start > 0 {
            q.push_str("^name>=");
            q.push_str(&plan[self.start].0);
        }
        if self.end < plan.len() {
            q.push_str("^name<");
            q.push_str(&plan[self.end].0);
        }
        q
    }

    fn planned_rows(&self, plan: &[(String, u64)]) -> u64 {
        plan[self.start..self.end].iter().map(|(_, c)| c).sum()
    }

    fn split(&self) -> Option<(Shard, Shard)> {
        (self.end - self.start >= 2).then(|| {
            let mid = self.start + (self.end - self.start) / 2;
            (
                Shard {
                    start: self.start,
                    end: mid,
                },
                Shard {
                    start: mid,
                    end: self.end,
                },
            )
        })
    }
}

/// Cut the plan into runs of about `target` rows. A single table larger than
/// `target` gets a shard of its own. An empty plan is one unbounded shard.
pub fn plan_shards(plan: &[(String, u64)], target: u64) -> Vec<Shard> {
    let mut shards = Vec::new();
    let mut start = 0;
    let mut rows = 0;
    for (i, (_, count)) in plan.iter().enumerate() {
        if rows > 0 && rows + count > target {
            shards.push(Shard { start, end: i });
            start = i;
            rows = 0;
        }
        rows += count;
    }
    shards.push(Shard {
        start,
        end: plan.len(),
    });
    shards
}

/// Whether a failed shard is worth halving: the request ran out of time (on
/// the client, at a gateway, or at the instance's transaction quota, which
/// surfaces as a truncated body the client cannot parse) rather than being
/// refused.
fn is_timeout_like(e: &Error) -> bool {
    match e {
        Error::Transport(_) => true,
        Error::Api { status, .. } => matches!(status, 502..=504),
        _ => false,
    }
}

type ShardOutcome = Result<Vec<(String, String)>>;

fn fetch_shards(client: &Client, plan: &[(String, u64)]) -> Result<(Vec<(String, String)>, usize)> {
    let mut pending = plan_shards(plan, SHARD_ROWS);
    let mut rows = Vec::new();
    let mut requests = 0;
    for _round in 0..=MAX_SPLIT_ROUNDS {
        if pending.is_empty() {
            break;
        }
        let outcomes = fetch_round(client, plan, &pending);
        requests += pending.len();
        let mut retry = Vec::new();
        for (shard, outcome) in pending.iter().zip(outcomes) {
            match outcome {
                Ok(mut r) => rows.append(&mut r),
                Err(e) if is_timeout_like(&e) => match shard.split() {
                    Some((a, b)) => retry.extend([a, b]),
                    None => return Err(e),
                },
                Err(e) => return Err(e),
            }
        }
        pending = retry;
    }
    if !pending.is_empty() {
        return Err(Error::Transport(
            "schema cache: sys_dictionary shards kept timing out after repeated splitting; \
             retry with a larger --timeout"
                .into(),
        ));
    }
    Ok((rows, requests))
}

/// Fetch `shards` with up to [`WORKERS`] requests in flight, outcomes in
/// shard order.
fn fetch_round(client: &Client, plan: &[(String, u64)], shards: &[Shard]) -> Vec<ShardOutcome> {
    let next = AtomicUsize::new(0);
    let slots: Mutex<Vec<Option<ShardOutcome>>> =
        Mutex::new((0..shards.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..WORKERS.min(shards.len()) {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(shard) = shards.get(i) else { break };
                    let outcome = fetch_shard(client, plan, shard);
                    slots.lock().unwrap_or_else(|p| p.into_inner())[i] = Some(outcome);
                }
            });
        }
    });
    slots
        .into_inner()
        .unwrap_or_else(|p| p.into_inner())
        .into_iter()
        .map(|o| {
            o.unwrap_or_else(|| Err(Error::Transport("schema cache: shard not fetched".into())))
        })
        .collect()
}

fn fetch_shard(client: &Client, plan: &[(String, u64)], shard: &Shard) -> ShardOutcome {
    let resp = client.get(
        "/api/now/stats/sys_dictionary",
        &pairs(&[
            ("sysparm_query", &shard.query(plan)),
            ("sysparm_group_by", "name,element"),
            ("sysparm_count", "true"),
            ("sysparm_display_value", "false"),
        ]),
    )?;
    let rows = parse_pairs(&resp)?;
    check_shard(plan, shard, &rows)?;
    Ok(rows)
}

/// Refuse a shard whose range terms the instance evidently did not apply.
///
/// ServiceNow drops query terms it cannot evaluate and answers with unfiltered
/// rows, so a shard that came back holding more rows from *outside* its planned
/// tables than it planned rows in total was not filtered by its range. A table
/// created between the plan and the shard lands in some range as a handful of
/// foreign rows, which this tolerates; a dropped range is the whole dictionary.
pub fn check_shard(plan: &[(String, u64)], shard: &Shard, rows: &[(String, String)]) -> Result<()> {
    if plan.is_empty() {
        return Ok(());
    }
    let own: HashSet<&str> = plan[shard.start..shard.end]
        .iter()
        .map(|(n, _)| n.as_str())
        .collect();
    let foreign = rows
        .iter()
        .filter(|(n, _)| !own.contains(n.as_str()))
        .count() as u64;
    let planned = shard.planned_rows(plan);
    if foreign > planned {
        return Err(Error::Instance {
            message: format!(
                "sys_dictionary shard returned {foreign} rows outside its name range \
                 (planned {planned}); the instance did not apply the range terms"
            ),
            detail: Some(shard.query(plan)),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(fields: &[(&str, &str)], count: u64) -> Value {
        json!({
            "groupby_fields": fields.iter().map(|(f, v)| json!({"field": f, "value": v})).collect::<Vec<_>>(),
            "stats": {"count": count.to_string()}
        })
    }

    fn index() -> SchemaIndex {
        let mut tables = BTreeMap::new();
        let mut t = |name: &str, parent: Option<&str>, cols: &[&str]| {
            tables.insert(
                name.to_string(),
                TableEntry {
                    parent: parent.map(str::to_string),
                    columns: cols.iter().map(|c| c.to_string()).collect(),
                },
            );
        };
        t("task", None, &["number", "short_description", "state"]);
        t("incident", Some("task"), &["caller_id", "severity"]);
        t("incident_task", Some("task"), &["incident"]);
        t("cmdb_ci", None, &["name"]);
        t("cmdb_ci_server", Some("cmdb_ci"), &["os"]);
        t("loop_a", Some("loop_b"), &["a"]);
        t("loop_b", Some("loop_a"), &["b"]);
        SchemaIndex {
            format: FORMAT,
            instance: "x.service-now.com".into(),
            built_at: 1,
            columns_indexed: true,
            tables,
        }
    }

    #[test]
    fn columns_include_inherited_and_are_sorted() {
        let idx = index();
        assert_eq!(
            idx.columns("incident").unwrap(),
            [
                "caller_id",
                "number",
                "severity",
                "short_description",
                "state"
            ]
        );
        assert_eq!(idx.columns("nope"), None);
    }

    #[test]
    fn lineage_survives_a_cycle() {
        let idx = index();
        assert_eq!(idx.lineage("loop_a"), ["loop_a", "loop_b"]);
        assert_eq!(idx.columns("loop_a").unwrap(), ["a", "b"]);
    }

    #[test]
    fn extends_walks_the_chain() {
        let idx = index();
        assert!(idx.extends("cmdb_ci_server", "cmdb_ci"));
        assert!(idx.extends("cmdb_ci", "cmdb_ci"));
        assert!(!idx.extends("incident", "cmdb_ci"));
    }

    #[test]
    fn prefix_lookup_is_a_range_not_a_scan_miss() {
        let idx = index();
        let got: Vec<_> = idx.tables_with_prefix("inc").collect();
        assert_eq!(got, ["incident", "incident_task"]);
        assert_eq!(idx.tables_with_prefix("zzz").count(), 0);
        assert_eq!(idx.tables_with_prefix("").count(), idx.tables.len());
    }

    #[test]
    fn cache_key_is_a_safe_host() {
        assert_eq!(cache_key("dev1.service-now.com"), "dev1.service-now.com");
        assert_eq!(
            cache_key("https://Dev1.Service-Now.com/"),
            "dev1.service-now.com"
        );
        assert_eq!(cache_key("http://127.0.0.1:8080"), "127.0.0.1_8080");
        assert_eq!(cache_key("../../etc"), ".._.._etc");
        let p = cache_path_in(Path::new("/cfg"), "https://a.b/");
        assert_eq!(p, Path::new("/cfg/cache/a.b/schema.json"));
    }

    #[test]
    fn parse_tables_reads_parents_by_field_name() {
        let resp = json!({"result": [
            row(&[("name", "incident"), ("super_class.name", "task")], 1),
            row(&[("super_class.name", ""), ("name", "task")], 1),
        ]});
        let t = parse_tables(&resp).unwrap();
        assert_eq!(t["incident"].parent.as_deref(), Some("task"));
        assert_eq!(t["task"].parent, None);
    }

    #[test]
    fn parse_tables_refuses_a_dropped_hierarchy_group() {
        let resp = json!({"result": [row(&[("name", "incident")], 1)]});
        let err = parse_tables(&resp).unwrap_err();
        assert!(matches!(err, Error::Instance { .. }), "{err:?}");
        assert!(parse_tables(&json!({"result": []})).is_err());
        assert!(parse_tables(&json!({})).is_err());
    }

    #[test]
    fn parse_plan_keeps_instance_order_and_counts() {
        let resp = json!({"result": [
            row(&[("name", "b")], 3),
            row(&[("name", "a_x")], 2),
            json!({"groupby_fields": [{"field": "name", "value": "c"}], "stats": {"count": 7}}),
        ]});
        assert_eq!(
            parse_plan(&resp).unwrap(),
            [("b".into(), 3), ("a_x".into(), 2), ("c".into(), 7)]
        );
    }

    fn plan(counts: &[(&str, u64)]) -> Vec<(String, u64)> {
        counts.iter().map(|(n, c)| (n.to_string(), *c)).collect()
    }

    #[test]
    fn shards_are_contiguous_and_open_ended() {
        let p = plan(&[("a", 4), ("b", 4), ("c", 4), ("d", 20), ("e", 1)]);
        let shards = plan_shards(&p, 10);
        assert_eq!(
            shards,
            [
                Shard { start: 0, end: 2 },
                Shard { start: 2, end: 3 },
                Shard { start: 3, end: 4 },
                Shard { start: 4, end: 5 },
            ]
        );
        assert_eq!(shards[0].query(&p), format!("{DICTIONARY_TERMS}^name<c"));
        assert_eq!(
            shards[1].query(&p),
            format!("{DICTIONARY_TERMS}^name>=c^name<d")
        );
        assert_eq!(shards[3].query(&p), format!("{DICTIONARY_TERMS}^name>=e"));
    }

    #[test]
    fn empty_plan_is_one_unbounded_shard() {
        let shards = plan_shards(&[], 10);
        assert_eq!(shards, [Shard { start: 0, end: 0 }]);
        assert_eq!(shards[0].query(&[]), DICTIONARY_TERMS);
    }

    #[test]
    fn split_halves_and_keeps_bounds_contiguous() {
        let p = plan(&[("a", 1), ("b", 1), ("c", 1), ("d", 1)]);
        let (l, r) = Shard { start: 0, end: 4 }.split().unwrap();
        assert_eq!(
            (l, r),
            (Shard { start: 0, end: 2 }, Shard { start: 2, end: 4 })
        );
        assert_eq!(l.query(&p), format!("{DICTIONARY_TERMS}^name<c"));
        assert_eq!(r.query(&p), format!("{DICTIONARY_TERMS}^name>=c"));
        assert!(Shard { start: 1, end: 2 }.split().is_none());
    }

    #[test]
    fn check_shard_refuses_an_unfiltered_answer_but_tolerates_a_new_table() {
        let p = plan(&[("a", 2), ("b", 2), ("c", 2)]);
        let shard = Shard { start: 1, end: 2 };
        let r = |n: &str| (n.to_string(), "x".to_string());
        assert!(check_shard(&p, &shard, &[r("b"), r("b"), r("b_new")]).is_ok());
        let err = check_shard(&p, &shard, &[r("a"), r("a"), r("b"), r("c"), r("c")]).unwrap_err();
        assert!(matches!(err, Error::Instance { .. }), "{err:?}");
    }

    #[test]
    fn save_and_load_round_trip_and_reject_other_formats() {
        let dir = tempfile::tempdir().unwrap();
        let path = cache_path_in(dir.path(), "x.service-now.com");
        assert_eq!(load(&path).unwrap(), None);
        let idx = index();
        save(&path, &idx).unwrap();
        assert_eq!(load(&path).unwrap(), Some(idx.clone()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let mut other = idx;
        other.format = FORMAT + 1;
        std::fs::write(&path, serde_json::to_string(&other).unwrap()).unwrap();
        assert!(matches!(load(&path), Err(Error::Config(_))));
        std::fs::write(&path, "not json").unwrap();
        assert!(matches!(load(&path), Err(Error::Config(_))));
    }
}
