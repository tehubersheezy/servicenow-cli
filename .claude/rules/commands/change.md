---
paths:
  - "src/cli/change.rs"
  - "tests/new_apis.rs"
---

# `sn change`

## Modules

- `src/cli/change.rs` → sn change list/get/create/update/delete + task/ci/conflict/nextstates/approvals/risk/schedule/models/templates

**Endpoints** (`change`): `/api/sn_chg_rest/change` + `/normal`, `/emergency`, `/standard`

`--type` routes to the sub-path; standard requires `--template <id>`; nested `/task`, `/ci`, `/conflict` + state ops (nextstates, approvals, risk, schedule). List responses end in a `__meta` element (`list`, `models`, `templates`; **not** `task list` — measured): `emit_list` strips it so an array holds records only (`--output raw` keeps it) and promotes `fields.ignored` to a stderr warning naming dropped query terms — after filtering `""` and `sysparm_*` entries, which are parameter echo, not terms (with no `sysparm_query` the API tries every request parameter as a column filter and reports the CLI's own switches as ignored). The API also silently discards `ORDERBY` clauses (a query of only a sort arrives empty), so `change list -q` refuses them before the network and points at `sn table list change_request`, which honors them
