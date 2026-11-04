---
paths:
  - "src/cli/api.rs"
  - "tests/api.rs"
---

# `sn api`

## Modules

- `src/cli/api.rs` → sn api list/search/spec (instance API discovery over the REST API Explorer's undocumented doc endpoints)

**Endpoints** (`api` (undocumented)): `GET /api/now/doc` (whole catalogue); `/api/now/doc/services?namespace=` (one namespace, same shape); `/api/now/doc/oas_3` (OpenAPI 3 export); `/api/now/doc/namespaces` (flat name array)

the REST API Explorer's own endpoints, unversioned. **`oas_3` answers `Accept: application/json` with 406** ("Supported response media types for this service are: [application/octet-stream]") — measured live, and it serves octet-stream whatever `?format=` says — so the group builds its client through `build_client_with_headers` with `Accept: */*`; the doc/services endpoints return JSON under `*/*` too, so one client serves all three verbs. `namespaces` is the family's cheap probe (1,494 bytes against 472,842 for the catalogue on the reference instance): only *its* 404 licenses "absent on older releases", and everything else promotes the endpoint's own `detail`, which names the bad argument precisely. It also spell-checks `--namespace`, because `services` answers an unknown namespace with `{"result":{}}` under HTTP 200 — a typo and a real scope publishing no REST API are the same bytes. `GET /api/now/doc/{httpMethod}/{route}` is deliberately unwired: every spelling probed answered 400. `spec --format yaml` is the one response written straight to stdout (the JSON emitter cannot carry it), with a trailing newline added when the instance omits one, and it **buffers rather than streams** — the opposite of `sn attachment download`, on purpose: a spec is a bounded document (9,556 bytes for the Table API; 460 KB for the whole catalogue) and the JSON arm already holds one parsed in memory, so buffering keeps the newline fixup a question about the last byte rather than about writer state
