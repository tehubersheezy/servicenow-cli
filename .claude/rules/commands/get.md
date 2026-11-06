---
paths:
  - "src/cli/get/**"
  - "tests/get_nouns.rs"
---

# `sn get <noun>` and `sn set`

## Modules

- `src/cli/get/mod.rs` → `GetArgs` (`args_conflicts_with_subcommands`: a noun subcommand *or* the `get_record.rs` REF positional), `GetNoun`, dispatch, `choices` (= `schema choices`), `noun_hint` for a digit-free, colon-free REF token
- `src/cli/get/{scope,property,script,plugin,me,instance,mine,last,logs}.rs` → one noun each; `common.rs` → shared helpers (`caller`, `groups_of`, row canary `dropped`)
- `sn set scope|updateset` is `context::set_scope|set_updateset` under another verb; `ContextSub` is reused

A noun can never collide with a reference: a reference has a `:` or a counter's digits, no noun has either. Every noun that filters checks **each returned row** against every term it sent (`common::dropped` → `Error::Instance`, exit 2). Ambiguous names (`property` across scopes, `script`, `updateset <name>`) are exit 1 listing the candidates, never picked.

## Measured (Brazil PDI dev457896, 2026-11-04)

- **`glide.buildname`/`glide.buildtag` have no `sys_properties` row**, but `getSysPropertyValue` (GraphQL, admin-only, `null` for others) answers them. `instance` reads everything through it; `property` falls back to it on a miss (`source: "runtime"`). `sn ping`'s `build_name`/`build_tag` read the table and so are `null` on this release.
- **`sys_script_include.api_name` is calculated**: `api_name=global.JSUtil` matches nothing (GraphQL and Table API). `script` splits a dotted name into `name=<tail>^sys_scope.scope=<head>` as a second `^NQ` segment, each segment carrying the `--scope` term.
- One GraphQL document over the 7 script tables answers in ~0.6s; `script`/`active` exist on all of them (`active` is `null` on `sys_script_fix`).
- `sys_store_app` and `sys_plugins` are 403 to admin, so `plugin` has no version to report.
- `sys_user_has_role` / `sys_user_grmember` are itil-readable for the caller's rows; `current_user` carries no roles.
- `syslog`: `sys_created_onRELATIVEGT@minute@ago@N` evaluates server-side (~0.2s); levels -2 trace … 3 fatal. itil with a query is 403 (unfiltered it reads as empty).
- `stats/sys_update_xml?sysparm_group_by=update_set` returns `[{groupby_fields:[{value}], stats:{count:"N"}}]`; a foreign group is the dropped-term canary.
- **The update-set picker PUT 400s ("Cannot set update set as current") when the target is already current**, so `set updateset` short-circuits that case as a success without a PUT.
- `sn get INC…` as itil fails 403 on the `question_answer` variables read (pre-existing; `get last` inherits it).
