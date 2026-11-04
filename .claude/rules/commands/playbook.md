---
paths:
  - "src/cli/playbook.rs"
  - "tests/playbook.rs"
---

# `sn playbook`

## Modules

- `src/cli/playbook.rs` → sn playbook list/trigger/launch (Playbook Experience GraphQL API, `snPlaybookExp`: union results selected by their namespaced member names, typed `TriggerPlaybookError` members → exit 2 with the object in sn_error, `list`'s `null` → exit 2 hedged "or not readable", `--only-if-none` skip → `triggered: false` exit 0, `--input` double URI-encoded for the resolver's query-string parse)

**Endpoints** (`playbook`): `POST /api/now/graphql` (`snPlaybookExp.playbook`: `getPlaybooksForParentRecord`, `triggerPlaybook`, `launchPlaybook`)

the workspace Playbook panel's scripted schema (Playbook Experience app, scope `sn_playbook_exp`; SDL readable in `sys_graphql_schema`, resolvers in `sys_graphql_resolver`). Measured on dev421992: union members are spelled `snPlaybookExp_playbook_PlaybookContext`/`…_TriggerPlaybookError` on the wire (the bare SDL names fail validation); expected failures are *data* (the error member, clean 200, no `errors`), while an unexpected one is a `DataFetchingException` naming the field. `getPlaybooksForParentRecord` reads the parent through `GlideRecordSecure` and answers `null` for missing *or* unreadable, `[]` for a record with none. `onlyIfNoProcessRunning` takes no playbook name and a **cancelled** execution still blocks — it means "the record has never had one"; a skip is the context member with `sys_id: null`. A trigger does not check the playbook's table (a `sn_vsc` playbook started on an incident). `launchPlaybook`'s `input` is a query string, not JSON (decoded whole, then split on `&`/`=`, then decoded per part — hence the double encoding), and it reads the parent table from the definition's `parent_record` snapshot input; no definition on the reference instance has one, so launch there always throws in the resolver (success path unmeasured). Executions (`sys_pd_context`) are 403 to Table API PATCH even as admin; cancel through the form's "Cancel Process" UI action (`POST /api/now/ui/ui_action/{sys_id}`), then "Force Cancel" out of `PENDING_CANCEL`, then Table API DELETE works (cascades activity/lane contexts)
