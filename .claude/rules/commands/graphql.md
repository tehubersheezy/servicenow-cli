---
paths:
  - "src/cli/graphql.rs"
  - "src/cli/gr.rs"
  - "tests/graphql.rs"
  - "tests/gr.rs"
---

# `sn graphql` and `sn gr`

## Modules

- `src/cli/graphql.rs` → sn graphql <query> (GraphQL passthrough: POST /api/now/graphql; in-band errors → exit 2 with the array in sn_error)
- `src/cli/gr.rs` → sn gr <table> (reads compiled to GlideRecord_Query: -f dot-walks references — `_reference` insertion decided by path position alone, no dictionary lookup — results flattened back to the dotted keys; -q rides as a GraphQL variable, never interpolated, and segments are identifier-validated before connect(); --count → _rowCount. An unqueryable table is detected by the error *path* ending at the table level (`@[GlideRecord_Query/<table>])`) because the live spelling is UnknownArgument 'pagination', not the FieldUndefined sn get documented — both are pinned)

**Endpoints** (`graphql`): `POST /api/now/graphql`

one endpoint, the whole merged schema: scripted namespaces + generated `GlideRecord_Query`/`GlideRecord_Mutation`/`GlideAggregateRecord_Query` (a field per table). Failure is in-band — HTTP 200 with an `errors` array, sometimes beside partial `data` — so the command maps non-empty `errors` to exit 2 (full array in `sn_error`) and still writes partial `data` to stdout; success unwraps `data`

Everything below was measured live against a Zurich PDI (dev421992, 2026-08-11), as both
admin and a freshly-minted `itil` user.

## The endpoint (measured)

One endpoint serves everything: `POST /api/now/graphql` (basic auth works —
same credentials as the Table API). The merged schema contains two kinds of
surface:

- **Scripted namespaces** (`now`, `global`, `sn*` — ~28 of them): hand-written
  APIs for chat, Playbook, Flow Designer tooling. Nothing generally useful to
  this CLI.
- **Generated namespaces**: `GlideRecord_Query`, `GlideRecord_Mutation`,
  `GlideAggregateRecord_Query` — auto-derived from the data dictionary.
  Every table (~6,200 on a stock PDI) gets a query field, insert/update/delete
  mutations, and aggregates. This is the surface worth wrapping.

Full introspection of the merged schema is ~94 MB and takes ~2 minutes —
never introspect at runtime. Also note graphql-java's "good faith
introspection" guard: a query that mentions `__Type.fields` (or similar meta
fields) more than once is rejected with `BadFaithIntrospection`.

## What GraphQL does that the Table API can't


1. **Many tables / many queries in one request** — aliases allow the same
   table under different conditions; reads and aggregates mix in one document.
2. **Total count with the page** — `_rowCount` returns the full match count
   alongside `pagination: {limit: N}` results. The Table API can report totals in headers, but `sn table list` does not
   include those headers in its JSON output; `sn aggregate --count` is another option.
3. **Field metadata inline, ACL-evaluated for the caller** — every column is
   a typed wrapper carrying `label`, `internalType`, `isMandatory`, and live
   `canRead`/`canWrite`/`canCreate` verdicts, per field, per record, per user.
4. **Choice lists in context** — choice columns expose
   `_choices { value displayValue }`, evaluated against the record (dependent
   choices resolve correctly). `sys_choice` gives you the static dump.
5. **Table-level capability discovery** — `_table_metadata { label plural
   canRead canWrite canCreate canDelete auditWanted }`, again evaluated for
   the calling user. "Can I create incidents?" without trying and catching 403.
6. **Structured dot-walking** — reference columns expose `_reference`, which
   opens the full results type of the target table: any fields, their
   metadata, further references. REST's `sysparm_fields=caller_id.name` gives
   flat strings with no nesting control.
7. **Per-field display values** — select `value`, `displayValue`, or both per
   column. `sysparm_display_value` is all-or-nothing per request.
8. **Journal access that survives itil ACLs** — see the next section.

## Journal fields: the ACL asymmetry (measured)

Comments and work notes live in `sys_journal_field` (one row per entry). The
access model is inverted from what you'd expect:

| Route | admin | itil |
|---|---|---|
| `sys_journal_field` via GraphQL or Table API | full rows | **blocked** — `_rowCount` leaks the count but `_results` comes back empty (row ACL filtering) |
| `GlideAggregateRecord_Query` on `sys_journal_field` | works | **denied** ("Insufficient rights to query records") |
| `comments` / `work_notes` / `comments_and_work_notes` columns on the record | full stream | **full stream** |

The record columns return the *rendered* stream — all entries concatenated,
each prefixed `YYYY-MM-DD HH:MM:SS - Author Name (Comments|Work notes)`,
entries separated by blank lines. Two caveats: timestamps are rendered in the
calling user's timezone (the raw table stores UTC), and it's a display string,
not rows — parse it (header regex:
`^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2} - (.+) \((Comments|Work notes)\)$`).

## Unshipped ideas

None of these flags exist. They are proposals, kept for when one is picked up.

### `sn table list --with-total`

Route the read through GraphQL to return `_rowCount` (total matches) alongside
the page. Output shape must remain an array by default, so propose the total
goes to a wrapper only under the flag: `{"total": N, "results": [...]}` —
flag-gated, so no existing consumer breaks.

### Multi-get batching

`sn table get incident <id> <id> <id>` (today `get` takes exactly one sys_id)
compiled into one GraphQL document via aliases:

```graphql
query { GlideRecord_Query {
  a: incident(sys_id: "…") { _results { … } }
  b: incident(sys_id: "…") { _results { … } }
} }
```

N round trips → 1. Table and column names are `[a-z0-9_]+`; validate before
splicing into query text — they cross a syntax boundary.

### ACL-aware schema commands

- `sn schema table-meta <table>` → `_table_metadata` verdicts for the calling
  profile. Agent pre-flight: know `canCreate` before attempting the insert.
- `sn schema columns <table> --live` / `sn schema choices <table> <col> --live`
  → field-wrapper metadata (`label`, `internalType`, `isMandatory`, `canWrite`,
  `_choices`) read off one sampled record (`pagination: {limit: 1}`).
  Per-user, record-context answers instead of the static `sys_dictionary` /
  `sys_choice` dump. **Caveat**: field-level metadata hangs off `_results`
  rows — an empty or fully ACL-filtered table yields nothing; fall back to the
  REST path and say so on stderr.

### `--expand` on `table get`/`list`

`--expand caller_id=name,email,manager.name` → nested `_reference` selection,
emitting real nested JSON objects instead of REST's flattened dot-walk strings.

## Non-goals

- **Writes over GraphQL.** Verified gotcha: a *successful* `update_incident`
  can return `_rowCount: null, _results: null` — you cannot confirm a write
  from the mutation response. REST PATCH echoes the updated record reliably.
  Principle: reads go GraphQL where it wins; writes stay REST.
- **GraphQL subscriptions.** Every generated table type has a `_subscription`
  field, but `sn watch` already rides the same AMB channel natively with
  reconnection and gap reporting. The GraphQL wrapper adds nothing.
- **CICD / Performance Analytics.** No GraphQL surface exists — they are
  procedural APIs, not tables. (Their *state* tables — `sys_update_set`,
  `sys_atf_test_result`, `pa_scores` — are readable like any other table.)

## Reference: request/response shapes

Get one record:

```json
{
  "query": "query ($id: String!) { GlideRecord_Query { incident(sys_id: $id) { _results { number { value } short_description { value } state { value displayValue } assigned_to { value displayValue _reference { email { value } } } } } } }",
  "variables": { "id": "47a91e3c2f8acf107efd1d707fa4e387" }
}
```

List with conditions + total:

```graphql
query { GlideRecord_Query {
  incident(queryConditions: "active=true^ORDERBYDESCsys_updated_on",
           pagination: { limit: 5 }) {
    _rowCount
    _results { number { value } state { displayValue } }
} } }
```

`queryConditions` is the same encoded-query syntax as `sysparm_query`,
`ORDERBY`/`ORDERBYDESC` included. Aggregate:

```graphql
query { GlideAggregateRecord_Query(
    tableName: "incident", queryConditions: "active=true",
    groupBy: ["state"]) {
  totalCount
  aggregates { groupBy { field value displayValue } count }
} }
```

(Also available per group: `avg` / `min` / `max` / `sum` / `countDistinct`;
root args include `having`, `orderBy`, `groupPagination`.)

Schema discovery on a live record:

```graphql
query { GlideRecord_Query { incident(pagination: { limit: 1 }) {
  _table_metadata { label plural canCreate canWrite canDelete }
  _results {
    state { label internalType isMandatory canWrite
            _choices { value displayValue } }
    assigned_to { label referenceTableName }
} } } }
```
