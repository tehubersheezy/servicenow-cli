---
paths:
  - "src/cli/flow.rs"
  - "src/cli/flow/**"
  - "tests/flow*.rs"
---

# `sn flow`

## Modules

- `src/cli/flow.rs` → sn flow list/get/versions (Flow Designer: list over the Table API on sys_hub_flow; get/versions over the undocumented processflow API with a 180s default timeout; FLOW = sys_id | internal_name | scope.internal_name, resolved with a mismatch canary — internal_name is NOT unique across scopes, so several matches are exit 1. `FlowSub` + `flow::run` own dispatch so later verbs only touch this file)
- `src/cli/flow/debug.rs` → sn flow runs/debug/steps/logs/why-not/tail (flattened into flow.rs's FlowSub): execution debugging over the documented flow-engine tables only — sys_flow_context (`flow` is a *string* holding the sys_hub_flow sys_id), sys_flow_report, sys_flow_log (`level` "-1".."2") — plus the runtime trigger for why-not (sys_hub_flow.remote_trigger_id → sys_flow_record_trigger's plain table/condition/on_* columns; no processflow call). why-not evaluates the condition per ^NQ segment and per AND-clause (`<clause>^sys_id=<id>` via the Stats API), skips change operators (CHANGESTO/VALCHANGES/CHANGESFROM — the Stats API 400s on them), and flags a clause matching every row as possibly silently dropped. `--since` is sent as an absolute UTC timestamp because `RELATIVEGE@day@ago@N` measured matching nothing. tail is an `sn watch` preset on sys_flow_context (measured: a `flow=<id>` channel carries the insert at QUEUED plus each state update, and `--errors`' `flow=<id>^state=ERROR` relies on AMB delivering a row's *entry* into the query as `action: entry, operation: update` — verified with `state=COMPLETE`). Steps come only from sys_flow_report, which reporting level OFF (the default) never writes — and which an Australia run at FULL left empty too (values went to ACL-locked sys_flow_report_value) — so an empty timeline is an error naming the cause, never `[]`

**Endpoints** (`flow` (undocumented)): `GET /api/now/processflow/flow/{sys_id}` (model); `GET /api/now/processflow/versioning/{sys_id}` (history); `GET /api/now/table/sys_hub_flow` (list + name resolution)

the Flow Designer canvas's own scripted REST API, absent from `/api/now/doc`. Answers `{"result":{"data","errorCode","errorMessage","integrationsPluginActive"}}`; `data` is the default output, and a non-empty `errorMessage` is a failure even under HTTP 200. **Slow**: 9–52s per model read on a PDI (the first measured read died at the global 30s), hence the 180s default when `--timeout` is absent. A missing flow is a 404 whose body is that same envelope rather than the platform's `error` object, so `flow::promote` lifts `errorMessage` into the message. The item URL takes only a sys_id, so names resolve through `sys_hub_flow` first. **`versioning` answers an unknown sys_id with `[]` under 200**, so an empty history for a bare sys_id is believed only after a `sys_hub_flow_base/{id}` read. A caller without Flow Designer rights gets 403 from processflow but can still read `sys_hub_flow`. Writes (`POST`/`PUT processflow/flow`, GraphQL `snFlowDesigner.flowPatch`) are unwired: they could not be measured on a throwaway flow, because the reference instance refused creation mid-upgrade (500 "Unable to create new flows during system upgrade")
