---
paths:
  - "src/schema_cache.rs"
  - "src/cli/cache.rs"
  - "src/cli/completion.rs"
  - "tests/cache.rs"
---

# Offline schema cache and dynamic completion

## Modules

- `src/schema_cache.rs` → the offline schema index (tables, parents, own columns; inherited columns resolved at lookup): per-instance path under config_dir, atomic 0600 load/save, and the sharded Aggregate-API build behind `sn cache refresh`
- `src/cli/cache.rs` → sn cache refresh/status/tables/columns (refresh is the only networked verb; the rest read schema_cache's file) + profile_instance (config-only instance lookup shared with the completers)
- `src/cli/completion.rs` → sn completion <shell> [--dynamic] (clap_complete static scripts, or the runtime engine behind SN_COMPLETE with cache-backed table/column completers)

`sn cache refresh` builds one index per instance at `<config_dir>/cache/<host>/schema.json` (under the config dir, not the platform cache dir, so `SN_CONFIG_DIR` isolates it; keyed by instance, not profile). It stores each table's parent and *own* columns; `SchemaIndex::columns` walks the parent chain, cycle-guarded. Written through `config::write_atomic` (0600) with no `.sn.lock`: it is a whole-file replace, not a read-modify-write.

**The source is the Aggregate API, measured against the alternatives** (dev421992, 7,891 tables, ~140k own columns): `GET /api/now/stats/sys_dictionary` grouped by `name,element` runs ~0.6 ms/row (the `c*` tables, 70,658 rows, in 44s); the Table API over the same rows is ~12 ms/row and a 5,000-row page was cut off at the instance's 60s transaction quota; GraphQL introspection 504'd at 300s on the table list alone, and its bad-faith guard forbids selecting `__Type.fields` twice, so columns would be one request per table. Hierarchy is one `sys_db_object` aggregate grouped by `name,super_class.name` (the dot-walked group is checked for — an instance ignoring it would silently strip every inherited column). A full build was 17 requests, ~56s, 2.4 MB. `sys_dictionary` is admin-only (itil gets 403 from Table and Aggregate APIs alike), so a 403 there degrades to a tables-only index (`columns_indexed: false`) rather than failing.

The dictionary is fetched in ~10k-row shards of table-name ranges (`name>=lo^name<hi`), 4 in flight. Boundaries come from a `GROUP BY name` pass **ordered by the database**, because its collation sorts `_` after letters and a byte sort would misplace them. Coverage does not depend on that order (first range unbounded below, last above — every name lands in at least one range; misordering only overlaps, and the per-table dedup absorbs it). A shard returning more foreign rows than it planned is a dropped range term → `Error::Instance`. A shard that times out (transport error or 502–504) is halved and retried, up to 4 rounds. `var__m_*` pseudo-tables (~32k rows) are excluded server-side and, should that term be dropped, client-side (only `sys_db_object` names are kept).

`sn completion <shell> --dynamic` emits clap_complete's runtime-engine registration (`unstable-dynamic`, hence the `~4.6` pin), keyed on **`SN_COMPLETE`**, not clap's bare `COMPLETE` — a stray generic variable would turn every invocation into a completion request. `main` calls `completion::complete_if_requested()` before anything else. Completers are attached in `completion_command()` (a wrapper over `cli::command()`, so parsing and introspect never see them) by arg id: positional `table`/`class` (CMDB classes: `cmdb_ci` descendants)/`staging_table`, and `fields`/`field`/`query`/`group_by`-style flags (not `variables set --field`, which names catalog variables). A completer learns the profile and table by re-parsing the whole line through `cli::command().ignore_errors(true)`. They read only the cache file and return nothing on any failure — a TAB must never touch the network or print an error. The implied-verb shorthand (`sn table incident -f …`) gets no column completion: the lenient parse doesn't see a `table` there.
