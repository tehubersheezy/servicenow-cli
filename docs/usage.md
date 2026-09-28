# Usage guide

Command examples and the conventions they share:

- **JSON is the default for API data.** Check the exit code before using it, and see the
  [output contract](#output-contract) for exceptions and stderr diagnostics.
- **Commands that connect to an instance need a profile** saved by `sn init` or
  `sn profile add`. Local commands such as `--help`, `completion`, and `introspect`
  don't need credentials. See the [setup guide](setup.md) to connect.

Replace placeholders such as `<sys_id>` with values from your instance. Record numbers
and payloads in these examples are illustrative.

Working with records:

- [Reading records](#reading-records)
- [Writing records](#writing-records)
- [Pagination](#pagination)
- [Watching records (live)](#watching-records-live)
- [Schema discovery](#schema-discovery)
- [Journal: comments and work notes](#journal-comments-and-work-notes)
- [Catalog variables](#catalog-variables)
- [Session context](#session-context)
- [Aggregate queries](#aggregate-queries)
- [GraphQL](#graphql)
- [Dot-walked reads](#dot-walked-reads-sn-gr)

ITSM and platform APIs:

- [Change Management](#change-management)
- [Attachments](#attachments)
- [CMDB](#cmdb)
- [Import Sets](#import-sets)
- [Service Catalog](#service-catalog)
- [Identification & Reconciliation](#identification--reconciliation)
- [CICD operations](#cicd-operations)
- [Performance Analytics scorecards](#performance-analytics-scorecards)
- [Decision tables](#decision-tables)
- [Playbooks](#playbooks)
- [Flow Designer](#flow-designer)
- [Debugging flows](#debugging-flows)

Utilities:

- [API discovery](#api-discovery)
- [Code search](#code-search)
- [Inspect and connect](#inspect-and-connect)
- [Acting as another user](#acting-as-another-user)
- [Open a record or list in the web UI](#open-a-record-or-list-in-the-web-ui)
- [Raw REST passthrough](#raw-rest-passthrough)
- [Background scripts](#background-scripts)
- [Human-readable table output](#human-readable-table-output)
- [Shell completions](#shell-completions)

The contract:

- [Output contract](#output-contract)
- [Exit codes](#exit-codes)
- [Parameters](#parameters)
- [Debugging](#debugging)

## Reading records

```bash
# List incidents (default: up to 1000 records)
sn table list incident

# Filter, select fields, limit
sn table list incident --query "active=true^priority=1" \
  --fields "number,short_description,state" --setlimit 10

# One record. Reference and choice fields come back as readable labels by default;
# --display-value false returns raw sys_ids and codes, all returns both.
sn table get incident <sys_id>
sn table get incident <sys_id> --display-value false

# Record references: one token names the record. `table:sys_id` is used directly;
# `table:number` costs one lookup that fails loudly if the table has no usable
# `number` field (rather than matching an arbitrary record).
sn table get incident:INC0010001
sn journal incident:INC0010001            # every (TABLE, SYS_ID) pair takes the form
sn open incident:INC0010001

# The composite read: the record plus its catalog variables and journal entries.
# Takes a reference, or a bare number: the standard prefixes (INC, CHG, CTASK,
# PRB, REQ, RITM, SCTASK, KB, SIR) are built in, any other is looked up once in
# the instance's sys_number table (admin-readable by default) and cached. A
# prefix several tables share is refused — name the table with table:number.
sn get INC0010001
sn get incident:<sys_id>

# The read verb is optional on table and cmdb — these mirror the REST path:
sn table incident <sys_id>          # same as: sn table get incident <sys_id>
sn table incident:INC0010001        # a reference implies get too
sn table incident                   # same as: sn table list incident
sn cmdb cmdb_ci_server <sys_id>     # same as: sn cmdb get cmdb_ci_server <sys_id>
```

Use the explicit verb for help (`sn table get --help` or `sn cmdb list --help`);
help on an implied-verb form currently reports an unrecognized subcommand.

Only `get` and `list` are ever inferred, never a write, and only when the choice is
unambiguous — a `table:id` reference is a `get`, a bare noun is a `list`, and a noun
plus sys_id is a `get`. A misspelled verb stays an error: `sn table lst incident`
still tells you it meant `list`.

Note that `--display-value true` (the default) also renders dates in the calling user's
timezone and locale format, and a display-formatted date cannot be fed back into an
encoded query. Use `--display-value false` when a value has to round-trip.

## Writing records

`create` and `update` take either `--data` (`-D`) or `--field` (`-F`), mutually exclusive:

- `--data` / `-D '<json>'` — inline JSON object (`@file.json` reads a file, `@-` reads stdin)
- `--field` / `-F key=value` — repeatable key/value pairs (`key=@file` reads the value from a file)

```bash
# Key/value pairs, or inline JSON, or piped from another tool
sn table create incident --field short_description="Disk full on prod-db-01" --field urgency=2
sn table create incident --data '{"short_description":"Server down","priority":"1"}'
echo '{"short_description":"from pipe"}' | sn table create incident --data @-

# update = PATCH, and it is a partial update: omitted fields keep their values.
# To clear one, set it explicitly (e.g. --field description="").
sn table update incident <sys_id> --field state=2
sn table update incident <sys_id> --data @record.json

# Delete
sn table delete incident <sys_id> --yes
```

`--yes` skips the confirmation, and without a terminal it is required — a non-interactive run
without it exits 1 naming the operation and its target rather than prompting into a void. Every
destructive command carries the same guard, not only the ones spelled `delete`: `table delete`,
`change delete`, `change task delete`, `change conflict remove`, `attachment delete`,
`cmdb relation delete`, `catalog cart-remove`, `catalog cart-empty`, `updateset back-out`,
`app rollback`, and `profile remove`.

## Pagination

Automatic pagination is available on **`sn table list`**. Other list commands use
their own paging flags; check the command's `--help`.

```bash
# Stream matches as JSONL (one record per line; capped at 100,000 by default)
sn table list incident --query "active=true" --all

# ...or buffer into one JSON array; cap the total with --max-records
sn table list incident --all --array --max-records 5000

# Pipe to jq
sn table list incident --all | jq -r '.number'
```

`--max-records` defaults to 100,000; use `--max-records 0` to remove that cap.
`--setlimit` controls page size, and `--offset` is ignored with `--all`.

`--all` walks by `sys_id` (keyset). Each page is "rows after the last sys_id seen", with no
per-page count, so a table that changes during the walk never makes it skip or repeat a row.
Records arrive in `sys_id` order. A query with its own `ORDERBY`, `--paginate offset`, and
tables with no plain `sys_id` (database views) page by offset instead. If a stream fails
part-way, the error's `resume_from` names the last record written. Rerun with
`--resume-from <sys_id>` and append to continue with no gap and no overlap:

```bash
sn table list syslog -q "$Q" --all > out.jsonl         # fails part-way: exit 3, resume_from "…"
sn table list syslog -q "$Q" --all --resume-from 8b03… >> out.jsonl
```

`--all` streams JSONL unless you add `--array`. Streaming refuses other output modes
with exit 1. For columns, buffer first: `--all --array --output table`.
`--output raw` has no equivalent: pagination flattens every page's `{"result": ...}` envelope into
a record stream, so there is nothing left to keep; page by hand with `--setlimit`/`--offset` if the
envelope is what you need.

## Watching records (live)

`sn watch` streams record changes as they happen, over ServiceNow's AMB websocket. Output is **JSONL on stdout**, flushed as it arrives: one event per line, plus the occasional supervisor marker described under [Gaps](#gaps-a-watch-is-best-effort-and-says-where-it-broke).

```bash
# Stream changes to matching records. Bound the stream, or it runs until you stop it.
sn watch incident --query "priority=1^active=true" --max-events 5
sn watch incident --query "sys_id=<SYS_ID>" --duration 60   # one record, stop after 60s
sn watch incident --query "active=true" --idle-timeout 30   # stop after 30s of quiet

# Narrow it down
sn watch incident --query "active=true" --operation insert          # only new records
sn watch incident --query "active=true" --on-change state,priority  # only these fields
```

**An event carries the fields that changed, with their new values.** `record` holds every field named in `changes` as a `{display_value, value}` pair, plus a few `sys_*` audit columns. This is what you get by default, with no extra API call:

```jsonc
{"table_name":"incident","sys_id":"1c74…","display_value":"INC0008001",
 "operation":"update","changes":["urgency","priority"],
 "changes_with_users":{"urgency":"abeyahmad"},
 "record":{"urgency":{"display_value":"1 - High","value":"1"},      // ← the new value
           "priority":{"display_value":"3 - Moderate","value":"3"}, // ← derived, recomputed
           "sys_updated_by":{"display_value":"abeyahmad","value":"abeyahmad"}}}
```

What an event does **not** carry is any field that *didn't* change: an event about `urgency` has no `number` and no `assigned_to`, because nobody wrote them. When you need a field nobody touched, read it explicitly — `sn table get incident <sys_id> --fields "number,assigned_to"` — which keeps the extra call visible and reads the row when *you* ask, not mid-stream. (A `--hydrate` flag used to do this per event; it was removed in 0.13.0 because a fetched row is current as of the fetch, not the event, so back-to-back writes hydrated the first event with the second's values.)

Worth knowing:

- **`changes` includes derived fields.** Writing `urgency` also reports `priority`, because ServiceNow recomputes it.
- **Inserts list every populated field** in `changes`, so an insert's `record` is the whole new row. **Deletes carry `changes: []`**, so `--on-change` never matches a delete — and no `record`, since there is nothing left to report.
- Ctrl-C exits 0. Works with both basic and OAuth/SSO profiles.
- `--insecure` and `--ca-cert` are honored. **Proxies are not supported**: a profile with a proxy configured exits 1 rather than connecting around it.

### Gaps: a watch is best-effort, and says where it broke

AMB has no replay and no cursor. A subscription starts at "now", so every change that happened
while a session was down is gone — no later message carries it and a reconnect cannot ask for it.
What the watcher *can* do is say where the hole is. After an established session drops and the
resubscribe succeeds, it writes one synthetic line:

```json
{"sn_watch":"reconnected","downtime_ms":4100,"attempt":2}
```

Everything between the preceding line and that marker is missing. Without it, a lost feed and a
quiet table look identical.

- **The marker is keyed, not shaped like an event.** `sn_watch` appears on nothing else, and it
  carries neither `operation` nor `changes` — the two fields `--operation`/`--on-change` match on,
  and the two a `jq` predicate is most likely to test. A marker shaped like an event would be
  dropped by exactly the pipelines that most need to see it. Filter with
  `jq 'select(.sn_watch == null)'` if you want events only.
- **It is not an event**: it does not count against `--max-events` and does not reset the
  `--idle-timeout` clock.
- **Rotation outruns the reaper.** ServiceNow reaps a watcher's HTTP session every minute or
  two regardless of traffic, so the watcher preemptively replaces its session every 45 seconds
  (`--session-rotate <SECS>`, `0` disables): the replacement subscribes *before* the old
  session disconnects, the old socket is drained, and the overlap is deduplicated — no gap
  opens and no marker is written. A marker therefore means something genuinely broke (a network
  drop, or a rotation that lost its race to the reaper). When completeness matters, reconcile
  against the table over the marker's window, starting shortly *before* it — detection of a
  genuine death can lag by up to one ~30s long-poll cycle.
- **One marker per gap, not per attempt.** `downtime_ms` spans the whole outage however many
  reconnects it took; `attempt` is the ordinal of the one that succeeded. A clean run emits no
  marker at all.
- Anything that has to be complete must be reconciled against the table itself over the reported
  window — the marker gives you exactly the interval to re-query.

**`--idle-timeout` measures subscribed time, and only subscribed time.** The clock starts at the
first successful subscribe, not at process start, and every interval spent off the channel is
forgiven — `downtime_ms` is exactly what was forgiven. So `--idle-timeout 3` on an instance that
takes a second to mint a session runs about four seconds, and an outage longer than the timeout
cannot make the marker the last line of the stream. Silence still accumulates *across* sessions,
so a connection flapping faster than the timeout cannot hold a silent watcher open forever.

## Schema discovery

Explore an unfamiliar instance:

```bash
sn schema tables --filter incident        # find tables by keyword
sn schema columns incident --writable     # writable columns for a table
sn schema choices incident state          # valid values for a choice field
```

### Offline schema cache

`sn schema` asks the instance every time. `sn cache refresh` pulls every table, its parent and
its columns into one local index (~1 minute and ~2.4 MB on a stock PDI); after that these answer
instantly with no network — and dynamic shell completion reads the same file:

```bash
sn cache refresh                          # build/rebuild for the profile's instance (needs admin)
sn cache status                           # where it lives, when it was built, how big it is
sn cache tables cmdb_ci_                  # table names by prefix
sn cache columns incident                 # every column, inherited ones included (task's number, state, …)
```

The index lives under the config directory at `cache/<instance>/schema.json`, one per instance
(profiles on the same instance share it). It is a snapshot: rerun `refresh` after schema changes.
Columns come from `sys_dictionary`, which only admin can read — a non-admin profile gets a
tables-only index and a warning. `refresh` defaults to a 120s per-request timeout; the work is
split into ~10,000-row requests, and one that still times out is halved and retried.

## Journal: comments and work notes

Journal entries live in `sys_journal_field`, which can have stricter read access than
the parent record. By default, `sn journal` fetches the record's rendered journal
fields over GraphQL and parses them into structured entries, newest first. Access
still depends on the record and field ACLs:

```bash
sn journal incident <sys_id>                    # all entries: [{created_on, author, element, label, text}]
sn journal incident <sys_id> --comments         # customer-visible comments only
sn journal incident <sys_id> --work-notes       # work notes only
sn journal incident <sys_id> --limit 5          # newest 5
sn journal incident <sys_id> --raw              # the unparsed rendered stream, as a JSON string
sn journal incident <sys_id> --source table     # exact sys_journal_field rows (needs table ACL access)
```

The default `--source record` uses timestamps rendered in the calling user's timezone
and date format. `--source table`
returns exact rows with UTC timestamps and usernames instead — and when rows exist but
ACLs filter them all, the error says so and points back at `--source record`. Adding an
entry needs no dedicated command: `sn table update incident <sys_id> --field
work_notes="..."` writes one.

## Catalog variables

Read a request item's variables or update them by name:

```bash
sn variables get sc_req_item:RITM0010001
sn variables set sc_req_item:RITM0010001 --field acrobat=true
```

Use the names returned by `get`; names are case-sensitive. `set` validates them before
writing and reads the values back to verify the change. An `sc_task` reference resolves
to its request item's variables. See the [agent guide](agent-guide.md#catalog-variables-variables)
for response shapes, record-producer variables, and limitations.

## Session context

View or switch the application scope and update set used for tracked configuration writes:

```bash
sn context
sn context scope x_myapp
sn context updateset "Sprint 12 fixes"
```

This requires access to scope and update-set records. Setters read back the result to
verify it; an update set must be in progress and belong to the current scope. See the
[agent guide](agent-guide.md#session-context-context) for how stale preferences are reported.

## Aggregate queries

Server-side statistics, without fetching individual records:

```bash
# Count records grouped by state, with readable labels
sn aggregate incident --count --group-by state --display-value true

# Average a field, filtered
sn aggregate incident --avg-fields reassignment_count --query "active=true"

# Several aggregations in one call
sn aggregate incident --sum-fields reassignment_count --min-fields priority --max-fields priority
```

## GraphQL

`POST /api/now/graphql` serves ServiceNow's whole GraphQL surface, including the generated `GlideRecord_Query` / `GlideRecord_Mutation` / `GlideAggregateRecord_Query` namespaces — a query field and CRUD mutations for every table, with per-field display values, inline choice lists, ACL-evaluated metadata, and server-side dot-walking through reference fields. `sn graphql` runs a document against it under the profile's auth and the standard output/error contract:

```bash
sn graphql 'query { GlideRecord_Query { incident(queryConditions: "active=true", pagination: { limit: 5 }) { _rowCount _results { number { value } state { displayValue } } } } }'
sn graphql @query.graphql --var id=a1b2c3d4e5f6           # document from a file, one string variable
sn graphql @- --variables '{"limit": 5}' < query.graphql  # document from stdin, typed variables
sn graphql @doc.graphql --operation GetIncident           # pick one operation from a multi-op document
```

On success stdout gets `data` unwrapped — the GraphQL analogue of stripping `{"result": ...}` (`--output raw` keeps the whole response). GraphQL reports failure **in-band**: HTTP 200 with an `errors` array, sometimes alongside partial `data`. A response with errors exits 2 with the first error's message in the stderr envelope and the full array under `sn_error`; any partial `data` still reaches stdout first. `--var k=v` sets a string variable (repeatable; only the first `=` splits, so encoded queries pass through). `--variables` takes a whole JSON object for non-string variables; `--var` entries overlay it.

GraphQL can put a total match count in the response body (`_rowCount`), combine many
tables or queries in one request, and provide per-field `canRead`/`canWrite`
verdicts, choice lists resolved in record context (`_choices`), and structured dot-walking
through reference fields (`_reference`). See [graphql.md](graphql.md) for the design notes.

## Dot-walked reads (`sn gr`)

`sn gr` compiles field paths into a `GlideRecord_Query` document. It follows reference
fields through GraphQL's `_reference` selections in one request, then flattens the
results for use in scripts:

```bash
sn gr incident -f number,short_description,caller_id.manager.email -q "active=true" --limit 20
sn gr incident --count -q "active=true"       # just the matching row count
sn gr incident -f caller_id,caller_id.email   # a reference itself plus a field behind it
```

Each dotted path nests through `_reference` (`caller_id.manager.email` compiles to
`caller_id { _reference { manager { _reference { email … } } } }`); paths sharing a prefix
are selected once. Results are flattened back to the dotted keys you typed, so output
looks like `sn table list` and pipes the same way:

```json
[{"number": "INC0010001", "caller_id.manager.email": "beth.anglin@example.com"}]
```

`--display-value` works as everywhere (default `true`; `all` emits
`{display_value, value}` per field, the Table API's spelling). A null reference anywhere
along a path yields `null` for the whole key — dot-walking semantics. `ORDERBY`/
`ORDERBYDESC` clauses in `-q` are honored. Dot-walking through a non-reference field
fails with a message naming the mistake, and a table with no GraphQL query field is named
too. `sn graphql` remains the passthrough for everything the compiler doesn't reach:
mutations, aggregates, scripted namespaces, multi-operation documents.

`--count` returns a JSON number in an object, such as `{"count":70}`. It cannot be
combined with `--fields`. Reads default to 100 records; use `--limit` and `--offset`
to page manually. `sn gr` has no `--all` flag. On GraphQL errors it exits 2 without
emitting partial records; use `sn graphql` when you need the partial response data.

## Change Management

Normal, emergency, and standard change requests across their lifecycle:

```bash
# List; create (standard changes require --template); update; delete
sn change list --type normal --query "state=1" --setlimit 10
sn change create --type normal --field short_description="DB migration" --field category=software
sn change create --type standard --template <template_sys_id> --field short_description="Routine patching"
sn change update <sys_id> --field state=2
sn change delete <sys_id> --yes

# Workflow helpers
sn change nextstates <sys_id>                          # valid next states
sn change approvals <sys_id> --field approval="approved"
sn change risk <sys_id> --data '{"risk_value":"moderate"}'
sn change schedule <sys_id>
sn change models                                       # change models
sn change templates                                    # standard-change templates
```

### Change tasks, CIs, and conflicts

```bash
# Tasks
sn change task list <change_sys_id>
sn change task create <change_sys_id> --field short_description="Pre-check"
sn change task update <change_sys_id> <task_sys_id> --field state=2
sn change task delete <change_sys_id> <task_sys_id> --yes

# CIs and conflicts
sn change ci add <change_sys_id> --data '{"cmdb_ci_sys_id":"<ci_id>"}'
sn change conflict get <sys_id>
sn change conflict remove <sys_id> --yes   # takes no conflict id: this clears them all
```

## Attachments

Files on any record:

```bash
sn attachment list --query "table_name=incident"
sn attachment get <sys_id>

# Upload a file (optionally override its name and content type)
sn attachment upload --table incident --record <record_sys_id> --file ./screenshot.png
sn attachment upload --table incident --record <record_sys_id> --file ./data.csv \
  --file-name "export_2026.csv" --content-type text/csv

# Download to a file, or to stdout for piping
sn attachment download <sys_id> --out ./downloaded.png   # -o also works
sn attachment download <sys_id> | gzip > backup.gz

sn attachment delete <sys_id> --yes
```

Downloads stream through a fixed 64 KiB buffer instead of buffering the whole file.
Three details matter when scripting them:

- **`--timeout` is a per-read idle timeout on a download**, not a cap on the whole transfer (it
  still is on every other command). A slow but healthy transfer runs as long as it needs; a stalled
  one dies `--timeout` seconds after the last byte. The connect and header phase stays bounded, so
  a 404 still fails immediately.
- **`--out` never leaves a truncated file behind.** Bytes are staged in a hidden `.part` file in the
  destination's own directory and renamed into place only once the transfer completes — same
  filesystem, so the rename is atomic. If the download fails, the staging file is removed and a file
  already at that path is left byte-for-byte untouched, so a retry is always safe. Ctrl-C removes the
  staging file and exits **130**.
- **stdout has no undo.** Bytes handed to a pipe cannot be recalled, so a mid-stream failure is exit
  3 with an error naming how many truncated bytes were already written. Prefer `--out` for anything
  large.

## CMDB

Read, create, and update configuration items, inspect class schemas, and manage relationships:

```bash
sn cmdb list cmdb_ci_server --query "operational_status=1" --setlimit 20
sn cmdb get cmdb_ci_server <sys_id>                                     # includes relations
sn cmdb create cmdb_ci_server --field name=web-server-01 --field ip_address=10.0.1.50
sn cmdb update cmdb_ci_server <sys_id> --field operational_status=2     # PATCH
sn cmdb update cmdb_ci_server <sys_id> --field name=web-01 --source "Other Automated"
sn cmdb meta cmdb_ci_server                                             # class schema

# Relations
sn cmdb relation add cmdb_ci_server <sys_id> --data '{"outbound_relations":[{"type":"<cmdb_rel_type_sys_id>","target":"<target_ci_sys_id>"}]}'
sn cmdb relation delete cmdb_ci_server <sys_id> <rel_sys_id> --yes
```

The CMDB Instance API takes writes in an envelope, `{"attributes": {...}, "source": "..."}`, and
`sn` builds it: give `create`/`update` flat fields exactly as on `table` and they are wrapped for
you. Values go out as strings, because the API casts each attribute to a Java `String` and answers a
JSON number or boolean with an HTTP 500 — so `--field cpu_count=8` sends `"8"`, while an object or
array is refused up front with a usage error. A body whose `attributes` is a JSON object is taken as
an envelope you wrote yourself and passed through unchanged; that is how `inbound_relations` /
`outbound_relations` ride along on a create.

`--source` is the record's provenance and lands in `discovery_source`. It defaults to
`"Manual Entry"` — the truthful value for a CLI write. Name a real discovery source only when
standing in for it: the IRE reconciles by source, so borrowing a tool's name lets that tool's next
run overwrite the record. Valid values are the choices on `cmdb_ci.discovery_source`
(`sn schema choices cmdb_ci discovery_source`). Giving `source` in both a flat body and the flag —
or in a flat body at all, where the API would drop it — is a usage error rather than a silent
preference.

## Import Sets

Insert into staging tables for transform-based imports:

```bash
sn import create u_staging_table --field u_name="Server-01" --field u_ip="10.0.1.1"
sn import bulk u_staging_table --data '[{"u_name":"Server-01"},{"u_name":"Server-02"}]'
sn import get u_staging_table <sys_id>
```

## Service Catalog

Browse catalogs and items, then order directly or through the cart:

```bash
# Browse
sn catalog list
sn catalog categories <catalog_sys_id>
sn catalog items --text "laptop" --catalog <catalog_id>
sn catalog item <item_sys_id>
sn catalog item-variables <item_sys_id>       # form fields required to order

# Order immediately (bypasses the cart)
sn catalog order <item_sys_id> --data '{"sysparm_quantity":"1"}'

# ...or work the cart
sn catalog add-to-cart <item_sys_id> --data '{"sysparm_quantity":"1"}'
sn catalog cart
sn catalog cart-update <cart_item_id> --data '{"sysparm_quantity":"2"}'
sn catalog cart-remove <cart_item_id> --yes   # drops one line
sn catalog cart-empty <cart_sys_id> --yes     # drops the whole cart; nothing restores it
sn catalog checkout
sn catalog submit-order

sn catalog wishlist
```

`cart-remove` and `cart-empty` are gated like a delete: on a terminal they prompt, and without a
terminal they need `--yes` or exit 1.

## Identification & Reconciliation

Create, update, or identify CIs through the reconciliation engine. Each call takes an `items` payload:

```bash
# Create or update
sn identify create-update --data '{"items":[{"className":"cmdb_ci_server","values":{"name":"web-01","ip_address":"10.0.1.1"}}]}'

# Identify only, without modifying anything
sn identify query --data '{"items":[{"className":"cmdb_ci_server","values":{"name":"web-01"}}]}'

# Enhanced variants add --data-source and --options (partial payload/commit)
sn identify create-update-enhanced --data @payload.json \
  --data-source "discovery" --options "partial_payload:true,partial_commits:true"
sn identify query-enhanced --data @query.json --data-source "discovery"
```

## CICD operations

`app`, `updateset`, and `atf run` are asynchronous — they return a progress object and run in the background on the instance. Add `--wait` to block until the operation finishes and emit the final result, and `--wait-timeout <SECS>` to bound that wait (on expiry `sn` exits 3 with a pointer to `sn progress`). Without `--wait`, take the id from `links.progress.id` and poll manually with `sn progress <id>`.

```bash
# App Repository lifecycle
sn app install  --scope x_myapp --version 1.2.0 --wait
sn app publish  --scope x_myapp --version 1.3.0 --dev-notes "Bug fixes" --wait
sn app rollback --scope x_myapp --version 1.1.0 --wait --yes

# Update sets
sn updateset create --name "My Changes" --description "Sprint 42 work"
sn updateset retrieve --update-set-id <id> --auto-preview
sn updateset preview <remote_update_set_id> --wait
sn updateset commit  <remote_update_set_id> --wait
sn updateset commit-multiple --ids id1,id2,id3
sn updateset back-out --update-set-id <id> --wait --yes

# ATF suites
sn atf run --suite-name "Regression Suite" --wait --wait-timeout 900
sn atf results <result_id>

# Poll an operation already in flight
sn progress <progress_id>
```

`app rollback` and `updateset back-out` require `--yes` without a terminal, the same guard the
deletes carry. Back-out reverses tracked configuration changes and can encounter
conflicts; it is not a general data restore. See the ServiceNowDocs
[back-out procedure](https://github.com/ServiceNow/ServiceNowDocs/blob/brazil/markdown/application-development/system-update-sets/t_BackOutUpdateSet.md).
App rollback restores an earlier application version. Both operations run asynchronously.

`--wait` honors `--output raw` and `--output table` — under raw it used to emit the initial,
unpolled response and never wait at all. A failed operation is exit 2 with the progress object on
stderr under `.error.sn_error`, and no `status_code`, since the instance reported the failure inside
an HTTP 200. If the initial response has no `links.progress.id`, `--wait` emits it
without polling; inspect that response before treating the operation as complete.

## Performance Analytics scorecards

```bash
# List scorecards (paged and sorted)
sn scores list --per-page 20 --sort-by VALUE --sort-dir DESC

# Historical scores for one indicator
sn scores list --uuid <indicator_id> --include-scores --from 2026-01-01 --to 2026-04-01

sn scores favorite <uuid>
sn scores unfavorite <uuid>
```

## Decision tables

`sn decision` reads decision tables (`sys_decision`) and evaluates them — "what does this
policy decide for these inputs?" — without opening Decision Builder:

```bash
sn decision list                                   # every table, ordered by name
sn decision list -q "answer_table=chg_approval_def"
sn decision show "Normal Change Policy"            # by exact name (case-insensitive) or sys_id
sn decision run "Normal Change Policy" -i change_request=CHG0000008 -i manager_approved=false
sn decision run <sys_id> -i instance_type=test --all-matches
```

`show` returns the table's row plus `inputs` (name, type, mandatory, choices, reference
table), `answer_elements` (multi-result tables only), `conditions`, and `decisions` in
evaluation order — each with its encoded-query `condition`, its `answer`, and
`default: true` on the fallback row the table answers when nothing else matches. `run`
returns `{sys_id, name, inputs, matches}`; each match names the decision (`sys_id`,
`label`, `order`, `default`) and its `answer`: `{value, display_value}` for a table whose
answer is a record, `{elements: {name: {value, display_value}}}` for a multi-result
table. `matches: []` means no decision (and no default) applies — that is an answer, exit 0.

The evaluator accepts every input mistake silently, answering with the default decision
as though the input were real, so `run` checks inputs against the table first (exit 1):
unknown names, missing mandatory inputs (`name=` sends one empty on purpose), a choice
*label* where the value belongs, and a boolean that is not `true`/`false`. A reference
input takes a sys_id or the referenced record's number, which is resolved first and
reported under `resolved_from`. Both verbs need the Decision Builder plugin
(`sn_decision_table`) and one of `decision_table_admin`, `decision_table_reader` or
`change_manager`; its API answers a caller without them with empty data rather than an
error, so `sn decision` reports that as exit 2 naming the roles. Editing tables is not
wired yet.

## Playbooks

`sn playbook` drives the workspace Playbook panel's own API (the `snPlaybookExp` GraphQL
schema, shipped with the Playbook Experience application). Records take the usual
`table sys_id` pair or a `table:sys_id` / `table:number` reference.

```bash
sn playbook list incident:INC0010001        # [{sys_id, title, scoped_name, playbook_id, state: {value, displayValue}, …}]
sn playbook trigger incident:INC0010001 --scoped-name sn_app.my_playbook
sn playbook trigger incident:INC0010001 --scoped-name sn_app.my_playbook --only-if-none
sn playbook launch <process_definition_sys_id> --record <sys_id> --input priority=high --input note="a&b"
```

`trigger` prints `{"triggered": true, "sys_id": "<execution>", …}`. With `--only-if-none`
it starts nothing — and prints `"triggered": false`, exit 0 — when the record already has
*any* playbook execution, of any playbook and in any state (a cancelled one counts), so a
retry never stacks a second run. `launch` prints `{"launched": true, …}`; its `--input`
pairs are encoded for the instance's query-string parser, so `&`, `=` and `%` in values are
safe. `list` shows each execution's `playbook_id`, the process definition sys_id `launch`
takes.

Failures the API reports as typed errors (`PARENT_TABLE_NOT_VALID`,
`PARENT_RECORD_NOT_FOUND`, `PROCESS_DEFINITION_ID_NOT_VALID`, `INSUFFICIENT_PERMISSIONS`,
`TRIGGER_PLAYBOOK_FAILED`, `LAUNCH_PLAYBOOK_FAILED`) exit 2 with the error object in
`sn_error` — branch on `sn_error.errorType`. A record the profile cannot read and one that
does not exist are the same answer to `list` (exit 2, "or not readable by this profile").
An instance without the application exits 2 naming it. The API does not check that a
playbook was built for the record's table.

## Flow Designer

Find flows and subflows, read one, and see its version history. This is read-only for now:
no command writes a flow.

```bash
sn flow list --scope global --type subflow --active    # sys_hub_flow rows, raw values, sorted by name
sn flow list -q "nameLIKEincident" --limit 20
sn flow get <sys_id>                                    # the full designer model (~70 keys)
sn flow get sn_itsm.my_flow --outline                   # header, triggers, and the ordered, nested steps
sn flow versions my_flow                                # save/publish history
```

A flow can be named by its sys_id, its internal name, or `scope.internal_name`.
Internal names repeat across scopes (`send_email` exists in more than one scope), so an
ambiguous name exits 1 and lists the qualified candidates. `get` and `versions` read the
designer's own undocumented `/api/now/processflow` API. It takes tens of seconds per flow,
so these two verbs default to a 180s timeout, and they need Flow Designer rights: an
`itil`-only caller gets 403 (exit 4), even though `sn flow list` still works for it.
`--outline` gives each step its `order`, `depth`, `kind` (`action`/`flowlogic`/`subflow`),
and `parent` (the enclosing step's `order`), which is usually all an agent needs out of
a model that can run to hundreds of KB.

## Debugging flows

The execution side of Flow Designer, read from the flow engine's own tables
(`sys_flow_context`, `sys_flow_report`, `sys_flow_log`) instead of clicking through
execution details. A flow is named by sys_id, display name or internal name; an
execution by its `sys_flow_context` sys_id, which `runs` lists.

```bash
sn flow runs "Change - Normal - Assess" --errors --since 24h    # executions, newest first
sn flow runs --record incident:INC0010001                       # what ran for this record?
sn flow debug <context_sys_id>      # context + first failed step (with values) + log tail + notes
sn flow steps <context_sys_id> --failed --values                # per-step timeline
sn flow logs <context_sys_id> --level warn                      # engine log lines
sn flow why-not "Delegate Roles in Group" --record change_request:CHG0030421
sn flow tail "Change - Normal - Assess" --errors --duration 600 # live, JSONL
```

Output uses raw values (`ERROR`, not `Error`) and UTC timestamps, so a value can be fed
straight back into a query.

**Steps exist only when the run was reported.** Per-step rows are written only when
the system property `com.snc.process_flow.reporting.level` is `BASIC` (states and
timings) or `FULL` (adds input/output values) at the time the flow runs, and it ships
as `OFF`. `steps` on an unreported run is an error that says so, and `debug` carries
the same explanation in its `notes` — an empty step list would read as "nothing ran".
Newer releases may not use `sys_flow_report` even then: on an Australia instance a run
recorded at `FULL` left it empty and wrote its values to `sys_flow_report_value`, which
REST cannot read even as admin. For such a run the message points at the UI
(`sn open sys_flow_context <sys_id>`) instead of at the property.
Log lines at error level are shown by `debug` even when the run ended `COMPLETE`: a
script step that catches and logs its own failure leaves exactly that shape.

**`why-not`** reads the flow's published runtime trigger (`sys_hub_flow.remote_trigger_id`
→ `sys_flow_record_trigger`: table, condition, insert/update/delete, active) and
checks, in order: the flow is active and published, the trigger is active, the record
is in the trigger's table (or an extension of it, when the trigger runs on extended
tables), and the condition matches — clause by clause, so the output names the clause
that fails. It also lists any runs the flow *did* have for the record. Its limits are
reported, not hidden: change operators (`CHANGESTO`, `VALCHANGES`, `CHANGESFROM`) test
the triggering write and cannot be evaluated against stored values; the condition is
checked against the record as it is now, not as it was at the write; and a clause that
matches every row of the table is flagged, because ServiceNow silently drops a query
term it cannot parse.

All of these tables are row-ACL protected: a profile without `flow_operator`/`admin`
sees no executions (and a 404 for one read by sys_id).

## API discovery

`sn schema` answers "what does this table look like?"; `sn api` answers "is there an API for this?"
It reads the same catalogue the instance's REST API Explorer does:

```bash
sn api list                          # every namespace, with API and endpoint counts
sn api list --namespace sn_chg_rest  # the APIs in one namespace
sn api search attachment             # matching endpoints, with method and route
sn api search cart --namespace sn_sc --method POST
sn api spec "Table API"              # the OpenAPI 3 document
sn api spec "Table API" --format yaml > table-api.yaml
```

`search` matches case-insensitively across namespace, API name, route and both descriptions, and
each row carries what a call needs — `route` is relative to `/api`, so
`/now/attachment/{sys_id}` is `sn raw DELETE /api/now/attachment/<sys_id>`. `list` and `search`
summarize; `--output raw` prints the catalogue endpoint's own response instead (several hundred KB)
for piping to `jq`, and `--output table` renders either as columns.

`spec` takes the name `list` reports; a unique case-insensitive substring is enough, and an
ambiguous one exits 1 listing the candidates with their namespaces so `--namespace` can break the
tie. `--format yaml` goes to stdout verbatim and ignores `--pretty`/`--compact`/`--output`.

An unknown `--namespace` is a usage error naming the near miss — the endpoint answers a bad
namespace with `{"result":{}}` and HTTP 200, which would otherwise be indistinguishable from "no
matches". A genuine 404 keeps the instance's own explanation ("Version v99 not found for now/Table
API") instead of being rewritten as a guess about the release.

## Code search

`sn codesearch` finds a string in script and code fields across the instance in one call, through
the Code Search API that Studio uses:

```bash
sn codesearch MyUtil                                   # every configured table, every scope
sn codesearch "new GlideRecord('incident')" --table sys_script_include
sn codesearch MyUtil --scope x_acme_app --limit 50
```

Each row is one matching field — `{table, sys_id, name, field, count, lines}` — and `lines` holds
each matching line plus its neighbours, with `match` telling them apart. `table` and `sys_id` feed
straight into `sn table get` or `sn open`. The match is a case-insensitive literal substring.

- Results only include records you can read, so a non-admin often gets an empty array.
- `--limit` (default 500, also the instance's stock ceiling) counts records the instance examines,
  not hits. When a result may have been cut short, stderr says which tables to narrow.
- A `--table` that Code Search doesn't cover exits 1 and lists the tables it does; a term
  containing `^` is refused, because the instance would split it into separate query terms.
- `--timeout` defaults to 120s for this command: a search across every table can take a while.

## Inspect and connect

```bash
# Auth + identity + latency + build — check the selected profile
sn ping
# {"ok":true,"profile":"prod","instance":"acme.service-now.com","username":"admin",
#  "identity_source":"sg/impersonation/session","user_sys_id":null,"user_display_name":null,
#  "admin":true,"can_impersonate":true,"impersonating":false,"original_user":"admin",
#  "latency_ms":134,"build_name":null,"build_tag":null}

# The caller's own sys_user record
sn user me
```

`username` is **the instance's answer, not the configured one** — `sn ping` asks an endpoint that
names the caller, because echoing the profile back verifies nothing about identity and the two
disagree exactly when the profile is wrong. `identity_source` says which endpoint answered
(`sg/impersonation/session`, `ui/user/current_user`, `sys_user`, or `profile` for the configured
name as a last resort). `admin`, `can_impersonate`, `impersonating` and `original_user` come from
the same probe and are `null` when the endpoint that carries them is absent; `impersonating` is
`true` only when two present, non-blank names differ. `build_name`/`build_tag` need their own
`sys_properties` read and are `null` when it returns nothing — a Zurich PDI carries neither
`glide.buildname` nor `glide.buildtag`, so null there means "the instance doesn't publish it", not
a failure.

```bash
# Preflight: is this account set up for what I'm about to do? One round trip.
sn doctor --need-role itil --need-plugin com.snc.change_management --need-property glide.servlet.uri
```

`sn doctor` reports identity, whether the session is admin, which kinds of check this account can
run, and a `checks` array of `pass` / `fail` / `unavailable`. It exits 0 only when every check
passes; otherwise the report still goes to stdout and the command exits 2. A role passes only if it
exists — an admin session would otherwise "hold" any name you type — and plugin/property checks are
`unavailable` for non-admin accounts, because the instance answers them with `null`. See the
[agent guide](agent-guide.md#sn-doctor) for the full shape.

`sn user me` resolves the caller's sys_id and reads that one record — no `javascript:` term on the
wire, so a term the instance cannot evaluate cannot be silently dropped and leave you holding a
stranger's record. On an instance without that endpoint it falls back to the scripted `sys_user`
read and exits 2 if the filter was evidently dropped.

## Acting as another user

```bash
# Run one command as abel.tuter, then end the impersonation
sn impersonate abel.tuter -- table get incident <sys_id>   # "why can't they see this?"
sn impersonate abel.tuter -- ping                          # impersonating: true, original_user: you
sn impersonate <sys_id> --profile prod --output table -- table list incident --limit 5
```

The server evaluates roles and ACLs as the target, so a record they cannot read comes back empty
or 403 exactly as it would for them. The profile's user needs the admin or impersonator role;
without it the command exits 4 before anything is attempted.

The command after `--` is any `sn` command (a leading `sn` is optional) except the ones that
manage local state or open a session of their own: `init`, `profile`, `watch`, `flow tail`,
`open`, `script`, `cache`, `completion`, `introspect`, and `impersonate` itself. Its stdout, stderr and exit code are its own.
Connection options (`--profile`, `--proxy`, `--timeout`, TLS) go before `--`, since the session is
opened first; output options work on either side.

The impersonation lives on a session private to this one process: minted with the profile's
credentials, then driven by cookie alone (a per-request credential would re-authenticate as you and
undo it), and never written to disk. Your browser and every other `sn` invocation are unaffected.
It ends on every exit — success, a failing command, Ctrl-C (exit 130) — by switching back, logging
the session out, and checking the session is gone. The switch is also verified before the command
runs: the instance answers the impersonate call with success even when it refused it.

## Open a record or list in the web UI

```bash
sn open incident <sys_id>                # any table; opens the form in your default browser
sn open incident:INC0010001              # record references work too
sn open incident                         # the table's list view
sn open incident -q "active=true^priority=1"   # a filtered list view
sn open incident <sys_id> --print-url    # print the URL instead of opening it
```

A second positional (or a `table:id` reference) names a record; a bare table
names its list. `-q` filters the list only, so combining it with a record is a
usage error (exit 1). The query is encoded for you — pass it exactly as you would
to `sn table list -q`.

Opening emits `{"opened": true, "url": "..."}` and honors `--output`. `--print-url`
deliberately does not: it writes the bare URL and nothing else, under every
`--output` mode, so `$(sn open … --print-url)` is directly usable in a shell.

## Raw REST passthrough

Call an endpoint directly using the selected profile. The response must be JSON;
`sn raw` keeps its envelope but parses and formats it rather than copying raw bytes:

```bash
sn raw GET /api/now/v2/table/incident -q sysparm_limit=5 -q sysparm_query=active=true
sn raw POST /api/now/table/incident --data '{"short_description":"From sn raw"}'
sn raw PATCH /api/now/table/incident/abc123 --field state=2
sn raw DELETE /api/now/table/incident/abc123
sn raw GET /api/now/table/incident -H 'X-no-response-body: true' -H 'X-Trace: 1'
```

`sn raw` has no confirmation guard, including for `DELETE`, and takes no `--yes` flag.
Headers can be added with `--header` / `-H`, but `Authorization` must come from the
profile. Body inputs are JSON; changing `Content-Type` does not change their encoding.

## Background scripts

`sn script run` executes server-side JavaScript as the profile's user — the CLI's
equivalent of *System Definition › Scripts - Background* — and returns what it printed.
It is arbitrary code execution with that user's privileges (normally admin), so it is
gated like every destructive command: `--yes` is required whenever stdin is not a terminal.

```bash
sn script run 'gs.info(new GlideRecord("incident").getRowCount())' --yes
sn script run @cleanup.js --rollback --yes             # record the run so its writes can be undone
echo 'gs.info(gs.getUserName())' | sn script run @- --yes
sn script run @job.js --scope x_acme_app --timeout 300 --yes
```

The result is one JSON object:

```json
{"ok": true, "scope": "global", "output": ["42"], "messages": [], "error": null,
 "elapsed_ms": 951, "history_id": "48169e6a…", "rollback_context": null}
```

- `output` — each line the script logged (`gs.info`/`warn`/`error`/`debug`, a source-less
  `gs.log`, and `gs.print` in global), one element per call. Print
  `JSON.stringify(x)` to hand back structured data.
- `messages` — anything else the platform printed during the run (slow-business-rule
  notices, `gs.log(msg, source)` lines, SQL debug), kept out of `output`.
- `error` — `{type: "compilation"|"execution", message, line, detail}`. A script error is
  still an HTTP 200, so the object above goes to stdout with `ok: false` (output printed
  before the error included) and the command exits 2 with `status_code: 200`.
- `history_id` — the run's `sys_script_execution_history` row; `rollback_context` — with
  `--rollback`, the `sys_rollback_context` its writes can be rolled back from (`null` when
  the script wrote nothing).

`--scope` takes a scope name, display name, or sys_id and is resolved before anything runs:
the instance silently runs an unrecognised scope in global. Store application scopes are
refused by the instance even for admin. A user without the role to run background scripts
exits 4. The script runs synchronously, bounded by `--timeout` (default 30s) on this side
and the instance's transaction quota on the other; a client timeout does **not** stop it —
its result still lands in `sys_script_execution_history`.

## Human-readable table output

Most read commands accept `--output table` for columns instead of JSON — for interactive browsing; keep the default JSON for scripts and pipelines (don't pipe it):

```bash
sn table list incident --setlimit 5 --output table
sn schema columns incident --writable --output table
sn aggregate incident --count --group-by state --output table
sn api search attachment --method DELETE --output table
sn table list incident --query "active=true" --all --array --output table
```

`aggregate`, `scores list`, `scores favorite` and `open` accepted `--output table` and silently
ignored it in earlier releases; all four go through the same renderer as every other command now.
`--all` still refuses it — a table cannot size a column without seeing every row, so buffer with
`--array` as above.

## Shell completions

```bash
# zsh — write to a dir on your fpath, then enable compinit
mkdir -p ~/.zsh/completions
sn completion zsh > ~/.zsh/completions/_sn
# add these two lines to ~/.zshrc (once), then restart your shell:
#   fpath=(~/.zsh/completions $fpath)
#   autoload -Uz compinit && compinit

# bash (requires the bash-completion package)
mkdir -p ~/.local/share/bash-completion/completions
sn completion bash > ~/.local/share/bash-completion/completions/sn

# fish
mkdir -p ~/.config/fish/completions
sn completion fish > ~/.config/fish/completions/sn.fish
```

Supported shells: `bash`, `zsh`, `fish`, `powershell`, `elvish`. The `${fpath[1]}` shortcut some tools suggest fails when that directory doesn't exist (common on Apple Silicon Homebrew) — the dir-on-fpath recipe above is portable.

### Dynamic completion: table and column names

`--dynamic` emits a script that asks `sn` itself for candidates on every TAB, so it can also
complete table names (`sn table list incid<TAB>`, `sn gr`, `sn aggregate`, `sn watch`, …; CMDB
classes only for `sn cmdb`) and column names (`-f`, `--field`, the field of the last `-q` term,
`--group-by`) from the [offline schema cache](#offline-schema-cache). Build the cache first with
`sn cache refresh`; without one, those positions simply offer nothing. Completion never touches the
network.

```bash
# zsh / bash — load at shell startup (not saved to a file, so it tracks upgrades).
# eval, not `source <(…)`: macOS's stock bash 3.2 cannot source a process substitution.
echo 'eval "$(sn completion zsh --dynamic)"' >> ~/.zshrc
echo 'eval "$(sn completion bash --dynamic)"' >> ~/.bashrc

# fish
echo 'sn completion fish --dynamic | source' > ~/.config/fish/completions/sn.fish
```

The script calls back into `sn` with `SN_COMPLETE=<shell>` set; that variable is what switches the
binary into completion mode, so don't export it yourself.

## Output contract

API data is JSON by default, pretty-printed on a terminal and compact when piped.
`--pretty` and `--compact` override that formatting. Most REST commands unwrap
ServiceNow's `{"result": ...}` envelope; `--output raw` keeps it, and `--output table`
renders columns for interactive reading.

| Command | Default stdout |
|---|---|
| `table list`, schema lists | JSON array; `table list --all` streams JSONL unless `--array` is set |
| `table get/create/update` | One record object |
| `get` | `{table, sys_id, record, variables, journal}`; `--output raw` is unsupported |
| `cmdb get` | Object containing `attributes` and relationships |
| Typed `delete` commands | Empty on success |
| `aggregate` | Stats object, or an array of groups with `--group-by` |
| `journal` | Entry array, or a JSON string with `--raw` |
| `variables get/set` | Variable array / verified change report |
| `context` | Scope and update-set object; setters also include `previous` |
| `graphql` | Unwrapped `data`; errors exit 2, but partial data is still emitted |
| `decision show/run` | One composed object (see [Decision tables](#decision-tables)); `--output raw` is refused, as for `get` |
| `script run` | `{ok, scope, output, messages, error, elapsed_ms, history_id, rollback_context}`; a script error still prints it (`ok: false`) and exits 2 |
| `gr` | Record array, or `{"count": N}` with `--count`; errors exit 2 without partial data |
| Async CICD operations / `progress` | Progress object; see [CICD operations](#cicd-operations) |
| `api list/search` | Summary array; `--output raw` keeps the catalogue response |
| `api spec` | OpenAPI JSON, or YAML text with `--format yaml` |
| `cache tables/columns` | Array of names; `cache refresh/status` return a summary object (`instance`, `path`, `tables`, `columns`, `columns_indexed`, `built_at`) |
| `raw` | Parsed JSON response, with no envelope unwrapping |
| `profile add/use/remove` | JSON result; removing a missing profile returns `removed:false` and exits 0 |
| `scores favorite/unfavorite` | Endpoint response; `unfavorite` supplies `{ok, uuid}` if there is no body |

Some commands deliberately produce other formats: `attachment download` writes file
bytes (or `{path, size}` JSON with `--out`); `open --print-url` writes a bare URL;
`completion` writes a shell script; help and version output are text. `sn init` reports
its result on stderr, and interactive setup prompts can appear on stdout.

`--all --output raw` is unsupported, including with `--array`.
`--all --output table` requires `--array`. Options such as `open --print-url` and
`api spec --format yaml` select their own format regardless of `--output`.

Errors go to stderr in this shape:

```json
{"error":{"message":"...","detail":"...","status_code":403}}
```

Only `message` is guaranteed. Optional keys are `detail`, `status_code`,
`transaction_id`, `sn_error` (the instance's error payload), and `resume_from` (only
on an interrupted `table list --all` stream). `status_code` is absent
when no HTTP status describes the failure, and it can be `200` for a failure reported
inside a successful HTTP response. Branch on the exit code first.

Argument-parsing errors use JSON when stderr is redirected and human-readable text
when stderr is a terminal. Runtime errors use JSON in either case. Warnings, OAuth
login messages, and `-d` diagnostics can also appear on stderr, so the entire stream
is not guaranteed to be one JSON document.

`--timeout` defaults to 30 seconds per HTTP request. Attachment downloads use a
per-read idle timeout for the body. Multi-request commands can take longer overall;
use `--wait-timeout` for CICD polling and the watch-specific duration flags for streams.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Usage or config error — including a destructive command refused for want of `--yes`, and a config write that could not take the directory lock within 10s |
| 2 | API error (4xx/5xx, non-auth), or a failure the instance reported inside an HTTP 200 |
| 3 | Network / transport error |
| 4 | Auth error — every 401 and every 403 |

Exit 4 is wider than "wrong password": a 403 from an ACL, a field the role cannot write, or an
expired token all land here, and `status_code` is the only thing distinguishing them. There is no
exit 2 with `status_code: 403`. A 403 can reflect an ACL or API access policy, not just a missing role.
Check the error details before refreshing credentials; logging in again does not grant
additional access.

Ctrl-C during an attachment download exits `130`. `sn watch` exits `0` on Ctrl-C,
and a closed stdout pipe also exits `0`; a successful exit therefore does not always
mean every possible record was consumed.

## Parameters

The following Table API flags map to `sysparm_*` parameters. Other command groups
support subsets; check their `--help` before reusing flags:

| Friendly | Short | Alias | Values |
|---|---|---|---|
| `--query` | `-q` | `--sysparm-query` | Encoded query string |
| `--fields` | `-f` | `--sysparm-fields` | Comma-separated field list |
| `--setlimit` |  | `--limit`, `--setLimit`, `--sysparm-limit`; `--page-size` on `table list` only | Max records returned. Default 1000 on `table`/`change`/`cmdb` list; 100 on `gr`, `change task list`, `attachment list`, `catalog categories`, `catalog items` |
| `--offset` |  | `--sysparm-offset` | Starting offset |
| `--display-value` |  | `--sysparm-display-value` | `true` (default), `false`, `all` |
| `--exclude-reference-link` |  | `--sysparm-exclude-reference-link` | Flag (presence ⇒ true) |
| `--view` |  | `--sysparm-view` | Named UI view |
| `--input-display-value` |  | `--sysparm-input-display-value` | Flag (presence ⇒ true; writes) |
| `--suppress-auto-sys-field` |  | `--sysparm-suppress-auto-sys-field` | Flag (presence ⇒ true; writes) |
| `--suppress-pagination-header` |  | `--sysparm-suppress-pagination-header` | Flag (presence ⇒ true) |
| `--query-category` |  | `--sysparm-query-category` | Index-selection hint (string) |
| `--query-no-domain` |  | `--sysparm-query-no-domain` | Flag (presence ⇒ true) |
| `--no-count` |  | `--sysparm-no-count` | Flag (presence ⇒ true) |
| `--output` |  | (CLI only) | `default` (unwrapped JSON), `raw` (full envelope), or `table` (columnar — interactive only) |

## Debugging

```bash
sn -d   table list incident     # HTTP method, URL, status
sn -dd  table list incident     # + response headers
sn -ddd table list incident     # + request/response bodies (auth headers, cookies, OAuth tokens masked)
sn -v                           # print version (-V also works)
```
