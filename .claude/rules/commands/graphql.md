---
paths:
  - "src/cli/graphql.rs"
  - "src/cli/gr.rs"
  - "tests/graphql.rs"
  - "tests/gr.rs"
  - "docs/graphql.md"
---

# `sn graphql` and `sn gr`

## Modules

- `src/cli/graphql.rs` → sn graphql <query> (GraphQL passthrough: POST /api/now/graphql; in-band errors → exit 2 with the array in sn_error)
- `src/cli/gr.rs` → sn gr <table> (reads compiled to GlideRecord_Query: -f dot-walks references — `_reference` insertion decided by path position alone, no dictionary lookup — results flattened back to the dotted keys; -q rides as a GraphQL variable, never interpolated, and segments are identifier-validated before connect(); --count → _rowCount. An unqueryable table is detected by the error *path* ending at the table level (`@[GlideRecord_Query/<table>])`) because the live spelling is UnknownArgument 'pagination', not the FieldUndefined sn get documented — both are pinned)

**Endpoints** (`graphql`): `POST /api/now/graphql`

one endpoint, the whole merged schema: scripted namespaces + generated `GlideRecord_Query`/`GlideRecord_Mutation`/`GlideAggregateRecord_Query` (a field per table). Failure is in-band — HTTP 200 with an `errors` array, sometimes beside partial `data` — so the command maps non-empty `errors` to exit 2 (full array in `sn_error`) and still writes partial `data` to stdout; success unwraps `data`
