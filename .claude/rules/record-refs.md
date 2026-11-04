---
paths:
  - "src/cli/record_ref.rs"
  - "src/cli/get_record.rs"
  - "src/cli/journal.rs"
  - "src/cli/variables.rs"
  - "src/cli/open_record.rs"
  - "tests/record_ref.rs"
  - "tests/journal.rs"
  - "tests/variables.rs"
  - "tests/open_url.rs"
  - "fuzz/**"
---

# Record references and the record-reading commands

## Modules

- `src/cli/record_ref.rs` → the `table:identifier` record-reference vocabulary (parse is pure argv/exit 1; number→sys_id resolution is one Table API lookup with the limit-2 canary/exit 2) plus bare-number prefix → table resolution for `sn get` (per-instance cache → built-in ITSM + SIR map → one `sys_number` read; see "Record references" below). Every `(table, sys_id)` positional pair routes through `parse_pair`, `attachment upload` through `parse_flag_pair`
- `src/cli/get_record.rs` → sn get <ref> (`locate` + `emit_composite` are reused by `sn get last`/`sn get xml`; nouns such as `sn get scope` live in `src/cli/get/`, see `commands/get.md`) (composite read: record + catalog variables + parsed journal; one GraphQL document resolves the ref and carries the journal streams, the record body is a Table API GET — GraphQL has no all-fields selection — and variables reuse variables.rs's joins probe-free)
- `src/cli/journal.rs` → sn journal <table> <sys_id> (comments/work notes parsed into entries; record-stream source by default, sys_journal_field rows via --source table)
- `src/cli/variables.rs` → sn variables get/set (catalog variables on a record; set pre-validates names, writes via the undocumented sn_sc variables PUT — the one path open to itil — and re-reads to verify, since the endpoint 200s on silently-skipped names)
- `src/cli/open_record.rs` → sn open <table> [<sys_id> | -q <query>] (opens a record form, or the table's list view — optionally filtered — in the browser; the query is percent-encoded twice, once as `sysparm_query`'s value and again with the whole target inside `nav_to.do?uri=`, so `&`/`%`/`+` survive both decodes)

- **Record references**: every `(table, sys_id)` positional pair (`table get/update/delete`, `journal`, `open`, `variables`, `cmdb get/update`, `cmdb relation add/delete`) and `attachment upload --record` also accept one combined `table:sys_id` / `table:number` token, through `cli/record_ref.rs`. Parse (exit 1) precedes the confirm gate and `connect`; number resolution (one Table API `number=` lookup, `sysparm_limit=2` canary) runs after `connect` and exits 2. A ref *plus* an explicit second positional is refused, never arbitrated. A destructive confirm names what the caller typed (`RecordRef::Display`: `table/sys_id`, `table:number`) — the gate must not connect to learn a sys_id. `sn get` is the composite read on top (`get_record.rs`): reference or bare number → `{table, sys_id, record, variables, journal}`; its resolution rides the journal GraphQL document instead of the Table API lookup, deliberately a second (equally canaried, equally pinned) path because the batching is the command's point.
  - **A bare number's prefix → table is the one step between parse and resolution.** Parse splits the prefix off pre-network (prefix = everything before the trailing digits, so stock `K8S_DEPLOY`/`MID_` survive; a token with no prefix or no counter is exit 1 without connecting). After `connect`: a fresh entry in `number_prefixes.toml` (config dir, keyed by **base URL** — the map is the instance's, not the profile's; 7-day TTL; written atomically 0600 under `.sn.lock` with a 2s lock budget, and every cache failure is a `-d` note, never an error), else the built-in map, else one `sys_number` read (`prefix=`, `category` is the table). Measured on dev421992 (216 rows): **prefixes are not unique** (six shared by two tables, e.g. `SR`), so the canary is per row — a returned row whose prefix is not the asked one, even case-insensitively (the instance's `=` ignores case), proves the term dropped (`Error::Instance`); a shared prefix is refused naming every table. Unknown/shared prefix is **exit 1** even though it took a request: the fix is the caller's (`table:number`). **`sys_number` is admin-only by default** (itil and the `svc-cc` OAuth client both get 403), so a 403 degrades to "only the built-ins are known" (exit 1), never exit 4; a 401 stays an auth error. Only unique answers are cached. A cached or built-in answer that finds no row earns one `sys_number` re-check (renumbered stock table, stale entry) and a retry on the new table; that re-check is best-effort and its failure leaves the original not-found standing.
  - **A prefix names the table its counter is defined on, which for an inheriting table is an ancestor**: `cert_follow_on_task` has no `sys_number` row and draws `TASK` numbers from `task` (1,801 rows on dev421992). The number is found on the ancestor, so bare-number resolution also selects `sys_class_name` and `sn get` reads the record (and its variables — a `TASK…` sc_task still gets the RITM hop) from the row's own class. An explicit `table:number` keeps the caller's table.
