---
paths:
  - "src/cli/codesearch.rs"
  - "tests/codesearch.rs"
---

# `sn codesearch`

## Modules

- `src/cli/codesearch.rs` → sn codesearch <term> (instance-wide code search over the Code Search API: every scope by default, --table pre-validated against the search group, `^` terms refused, the per-table limit budget reconstructed to warn on possible truncation; one flattened row per matching field)

**Endpoints** (`codesearch`): `GET /api/sn_codesearch/code_search/search`; `/code_search/tables?search_group=`

the Code Search plugin's scripted API (Studio's). Measured behaviors, all from its `CodeSearch` script include: the search runs over a *search group* (the CLI always names `sn_codesearch.Default Search Group`, 31 tables stock); an **unknown `table` is silently ignored and the whole group searched** (the answer is then an array, not the single object a table search returns), so `--table` is checked against `/tables` first and the shape canaried after (`Error::Instance`); the API **defaults to the session's current scope** (`global` for a REST caller), so the CLI sends `search_all_scopes=true` unless `--scope` is given, and an unknown `current_app` matches nothing under 200, so an empty scoped result earns one `sys_scope` lookup (403 for non-admins → warn, don't refuse); the term is spliced **unescaped into `fieldLIKE<term>^OR…`**, so `^` terms are refused pre-network (`^OR` returned 0 hits against scripts containing it); `limit` counts **records examined, not hits** (unreadable/protected records drop 4–23% after being counted) and is clamped to `sn_codesearch.search.results.max` (500), each table getting `limit − hits so far` — `truncation_warning` rebuilds those budgets and names tables ≥80% full or never reached. `/tables` **500s without `search_group`** (`"empty" is not defined`). Every number is serialized as a float (`"line":1.0`). No plugin → HTTP 400 "Requested URI does not represent any resource", rewritten to name it. A whole-group search took 5–36s on the PDI, so `--timeout` defaults to 120s here
