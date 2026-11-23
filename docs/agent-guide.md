# `sn` agent usage guide

One-time read for an LLM agent that reads, creates, updates, and deletes
ServiceNow records via `sn`. The [usage guide](usage.md) is the command
reference and `sn <group> <command> --help` is the syntax for the installed
version. This guide holds what an agent gets wrong without being told: output
shapes, failure handling, and values that don't round-trip.

Configure a profile using the [setup guide](setup.md). The examples use
illustrative record IDs and values; instance plugins, roles, and ACLs determine
which operations are available.

## Output, errors & exit codes (read first)

**API data is JSON by default**, pretty-printed on a TTY and compact when piped.
Most REST commands unwrap ServiceNow's `{"result": ...}` envelope. Table reads
return arrays or record objects; other commands can have different shapes. See the
[output contract](usage.md#output-contract) for the complete format rules.

Three output modes via `--output`:
- `default` — unwrapped JSON (above).
- `raw` — preserves the full `{"result": ...}` envelope.
- `table` — columnar text table for humans (don't parse it).

Non-obvious shapes worth knowing. A wrong `jq` path prints nothing and exits 0,
so it reads as "no data":

| Command | stdout |
|---|---|
| `table delete`, `change delete`, `attachment delete` | empty (exit code signals success) |
| `get` | `{table, sys_id, record, variables, journal}` — the number is `.record.number` |
| `get updateset` | `.in_progress` is the boolean; the stored `state` is `"in progress"`, with a space |
| `schema tables` | the table name is `.value`, not `.name` |
| `change *` | every field is a `{display_value, value}` pair — see [Change Management](#change-management) |
| `cmdb get` | the CI's fields are under `.attributes` — see [CMDB](#cmdb) |
| `aggregate` | `{"stats":{...}}` — but with `--group-by`, an **array** of `{groupby_fields,stats}` — see [Aggregate](#aggregate) |
| `gr --count` | `{"count": N}`, with a JSON number |
| `scores list` | `direction` and `frequency` are integer codes; the words are in `direction_label` / `frequency_label` |
| `cicd app` / `cicd updateset` / `cicd atf run` | progress object with `status_label` + `links.progress.id` |
| `cicd progress` | `{status_label, percent_complete, status_message}` |
| `scores unfavorite` | the endpoint's body, or `{"ok":true,"uuid":"..."}` when there is none. `scores favorite` passes its body through unchanged, which is `null` where the instance answers with no content |
| `attachment download` | raw bytes, or `{"path","size"}` with `--out <file>` |
| `profile use` | `{"ok":true,"profile":"ci","default":true}` |
| `profile remove` | `{"ok":true,"profile":"ci","removed":true,"wasDefault":false}` — `removed:false` (still exit 0) when there was no such profile, so removal is idempotent |
| `init` | Result on stderr; interactive setup prompts may use stdout. Use `profile add` for a JSON result. |
| `open --print-url`, `completion`, help/version | Plain text |
| `apisearch spec --format yaml` | YAML text |
| `ping` | 14 keys, and `username` is the *instance's* answer |
| `doctor` | the report is on stdout **even on exit 2**; `checks[].status` is `pass`, `fail` or `unavailable` |
| `watch` | JSONL; a line with `sn_watch` is a gap marker, not an event |

Runtime errors use a JSON envelope on stderr. Argument-parsing errors use the same
envelope when stderr is redirected and human-readable text on a terminal. Warnings,
OAuth prompts, and verbose diagnostics can appear alongside errors; don't assume
that all of stderr is one JSON document. A typical error is:

```json
{
  "error": {
    "message": "Record not found",
    "detail": "No record with sys_id 'abc123' in table 'incident'",
    "status_code": 404,
    "transaction_id": "3f4ab12c8d0001",
    "sn_error": {"message": "No Record found", "detail": "ACL restricts the record retrieval"}
  }
}
```

`sn_error` contains the instance's error payload when available. It is usually
absent for transport or CLI errors; check `.error.message` first. `transaction_id` is SN's correlation id,
useful for support requests. `resume_from` appears only on a
`table list --all` stream that failed part-way (see [Pagination](#pagination--bulk-processing)).

**Every key but `message` may be absent**, and `status_code` in particular. Two
examples that omit it because no HTTP status describes the failure are: a CICD
operation the instance reported as failed *inside* a 200 under `--wait`, and a
read whose scripted query the instance silently dropped (`sn get me`). It is
never `0` — no HTTP response carries that — so a `jq` test has to tolerate a
missing key (`jq -r '.error.status_code'` prints `null`, which no comparison
against a status will match).

`status_code: 200` is a *different* thing and does occur: `sn graphql`, `sn gr`,
`sn journal`, `sn playbook`, `sn variables set`, `sn script run` and `sn decision` detect failures ServiceNow reports in-band,
inside a genuinely successful HTTP 200, and they report that 200 truthfully
rather than hiding it. So `status_code` answers "what did HTTP say", not "did
this succeed" — **branch on the exit code, and treat `status_code` as
diagnostic detail.**

**Exit codes — branch on these before parsing stdout:**

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Usage / config / parse error (bad flags, unreadable file, malformed JSON, mixing `--data` + `--field`, bad proxy URL / CA file) |
| 2 | API error — ServiceNow 4xx/5xx other than auth (400 bad table, 404 not-found, 429 rate-limit, 5xx), or a failure the instance reported inside an HTTP 200 (`status_code` is then either `200` or absent — see above; do not branch on it) |
| 3 | Network / transport (DNS, connection refused, timeout, TLS handshake, proxy unreachable) |
| 4 | Auth — **every** 401 and **every** 403 |

**Exit 4 is not only "the password is wrong."** `sn` maps 401 and 403 alike to
exit 4, so a row-level ACL denial, a field the profile's role may not write, and
an expired token all arrive the same way — with `status_code` telling you which
status it was and nothing telling you which cause. There is no exit 2 with
`status_code: 403`; that shape cannot occur. So do not wire exit 4 straight to
"re-login". Read the error details and check the account's roles, ACLs, and API
access policies. Re-authentication does not grant additional permissions.

```bash
err_file=$(mktemp)
trap 'rm -f "$err_file"' EXIT
if out=$(sn table get incident "$sysid" 2>"$err_file"); then
  jq -r '.short_description' <<<"$out"
else
  status=$?
  case "$status" in
    1) echo "check the command and profile configuration" >&2 ;;
    2) echo "the instance could not complete the request" >&2 ;;
    3) echo "check connectivity and timeouts" >&2 ;;
    4) echo "check credentials, roles, ACLs, and API access policies" >&2 ;;
    *) echo "command interrupted or failed with exit $status" >&2 ;;
  esac
  cat "$err_file" >&2
  exit "$status"
fi
```

Turn on `-d` after an exit 2/3 to see the exact URL built — a sysparm typo
producing a malformed path is a common 404 cause (`-dd` adds headers, `-ddd`
bodies with secrets masked). Verbose text is free-form and may change between
versions; only the stderr error object is structured.

## Profiles

**`sn profile add` is the one to reach for**, not `sn init`. It emits JSON on
stdout, and it **never prompts when stdin is not a terminal** — a missing field
is exit 1 naming the flag, not a blocked read. `sn init` is a human wizard that
also takes over `default_profile`.

```bash
sn profile add ci --instance dev12345 --username svc --password-stdin < secret.txt
# → {"auth":"basic","default":false,"instance":"dev12345.service-now.com","next":"sn profile use ci",
#    "ok":true,"profile":"ci","user":"svc","verified":true}
```

| situation | exit | effect |
|---|---|---|
| ok | 0 | profile written, `"verified":true` |
| profile already exists | 1 | nothing written — pass `--force` (which rebuilds proxy/TLS from its own flags: omitting `--insecure` clears it) |
| required flag missing (no TTY) | 1 | nothing written — message names the flag |
| credentials rejected | 4 | profile changes rolled back |
| `--no-verify` | 0 | written unverified, no network call |

- **Pipe secrets in** (`--password-stdin`, `--client-secret-stdin`,
  `--api-key-stdin`, `--token-stdin`) rather than passing them as flags, which
  any process can read out of `ps` and which land in shell history.
- **Headless auth:** basic, API key, `--grant client_credentials`,
  `--grant jwt_bearer`, and `--auth token`. The default `authorization_code`
  grant needs a browser, so `add` refuses on a non-TTY rather than save an
  untested profile: register it with `--no-verify` and have a human run
  `sn profile login --profile <name>`. See the [setup guide](setup.md) for each
  method.
- **Selection** is `--profile <name>` (`-p`, before or after the subcommand) →
  `default_profile` → error (`no profile selected`, exit 1). A profile is the
  single unit of identity: there is no way to graft a one-off
  instance/username/password onto a command, and there are no env vars for
  credentials or profile selection.
- **Sandboxing:** `SN_CONFIG_DIR` relocates the config directory (used as-is —
  no `sn` subdir appended); it's the supported way to isolate `sn` for CI or
  ephemeral sessions.
- Parallel invocations that write profiles or refresh OAuth tokens serialize on
  a lock instead of losing each other's work, and a writer that cannot take the
  lock within 10s exits **1** rather than hanging.

## Who am I, and what can I do

`sn ping` reports the identity and privileges the *instance* attributes to the
credentials, which is the cheapest way to tell a dead credential from a live one
that simply lacks the role. `sn get me` returns the caller's record, roles and
groups. `sn doctor --need-role … --need-plugin … --need-property …` is a
preflight that exits 0 only when every check passes. See
[Inspect and connect](usage.md#inspect-and-connect) for the keys and rules.

**Never derive identity from `sn table list sys_user --limit 1`.** A bare limited
read of `sys_user` returns whichever row sorts first — a stranger, reported with
full confidence. `sn ping` and `sn get me` both ask endpoints that name the
caller directly, and both refuse to answer rather than hand back an arbitrary row.

In `sn doctor`, **`unavailable` is not `pass`**: plugin and property checks need
admin, and a check the account cannot answer fails the preflight.

## Discovery flow

When you don't know a table's schema, discover it before writing.

```bash
sn schema tables --filter incident        # fuzzy match name or label
```
```json
[{"value":"incident","label":"Incident","rawLabel":"Incident","reference":false,"sequence":-1,
  "image":"","missing":false,"selected":false,"used":false}]
```

⚠️ **The table name is `value`, not `name`.** This endpoint returns a picker-style
list, so `jq -r '.[].name'` yields `null` for every row. Use `jq -r '.[].value'`.
There is no `super_class`, `is_extendable`, or `sys_id` here.

```bash
sn schema columns incident --writable      # mandatory fields, types, references
```
```json
[
  {"name":"short_description","type":"string","internal_type":"string","max_length":160,
   "mandatory":true,"read_only":false,"default":"","label":"Short description"},
  {"name":"caller_id","type":"reference","mandatory":false,"reference":"sys_user",
   "reference_display_field":"name","label":"Caller"},
  {"name":"state","type":"choice","internal_type":"integer","mandatory":true,"default":"1",
   "choice_type":"dropdown","choices":[{"value":"1","label":"New"},{"value":"2","label":"In Progress"}]}
]
```

⚠️ There is **no `choice_field`** and **no `default_value`**. The default is `default`.
A choice column is one whose **`type` is `"choice"`** — note its `internal_type` may
still be `"integer"` — and its options are inlined in a **`choices[]`** array, so you
often don't need a second `schema choices` call. `reference` is *absent* on
non-reference columns rather than `null`.

`columns` filters: `--writable` (excludes read-only), `--mandatory`,
`--filter <substr>` (name or label), `--references-only`, `--choices-only`,
`--type <type>` (e.g. `string`, `integer`, `reference`).

```bash
sn schema choices incident state           # valid values for a choice field
```
```json
[{"value":"1","label":"New","rawLabel":"New","sequence":1,"image":"","missing":false,
  "reference":false,"selected":false,"used":false,"parameters":{…}}]
```

Rows are the same picker shape as `schema tables`; read `value` and `label`. The
numeric `value` is what you send to write APIs; the `label` is what
`--display-value true` returns on reads. Now write with confidence:

```bash
sn table create incident --field short_description="server down" --field state=2 --field priority=1
```

For endpoints rather than tables, use `sn apisearch <term>` before assuming an
operation has no API — the answer for anything `sn` doesn't model is usually
`sn raw` against a route it names for you (`route` is relative to `/api`). See
[API discovery](usage.md#api-discovery).

## Reading records

```bash
sn table list incident --query "active=true^priority=1" --fields "number,short_description,state" --setlimit 10
sn table get incident a1b2c3d4e5f6      # by sys_id (get has no --query flag)
sn table get incident:INC0010001        # by number
sn get INC0010001                       # the record plus its variables and journal entries
```

- `--limit` aliases `--setlimit`; the default is 1000 on the main list commands.
  Drop it low (`--setlimit 5`) for exploration.
- **When a query is supposed to identify exactly one record, use `--limit 2` and
  check the result count.** ServiceNow drops a query term it cannot evaluate and
  returns unfiltered rows.
- `sn get <noun>` reads state an agent otherwise assembles by hand (`scope`,
  `updateset`, `property`, `script`, `choices`, `me`, `instance`, `mine`, `last`,
  `logs`). See [`sn get` nouns](usage.md#sn-get-nouns).
- The read verb may be omitted on `table` and `cmdb` (`sn table incident` lists).
  The flip side when debugging: a first token that is *not* close to any real
  verb is taken as a table name. If you get an unexpected "invalid table" on
  `table` or `cmdb`, check whether you misspelled the *verb*.

### Display values

**`--display-value` defaults to `true`.** Reads come back as readable labels —
`state` is `"In Progress"`, not `"2"`; `assigned_to` is `"Abel Tuter"`, not a
sys_id.

| Value | Effect | Use when |
|---|---|---|
| `true` (default) | display labels | reading, reporting, showing a human |
| `false` | raw values and sys_ids | feeding a value back into a query or a write |
| `all` | both — each field becomes `{"value","display_value"}` | you need to show one and send the other |

```bash
sn table get incident a1b2c3d4e5f6 --display-value all
# ... "state":{"value":"2","display_value":"In Progress"}, "priority":{"value":"1","display_value":"1 - Critical"}
```

**Two consequences that will bite a script.** Dates are also rewritten, into the
calling user's timezone and locale — `2026-08-04 14:22:01` comes back as
`08/04/2026 10:22:01 AM`, which **will not parse back into an encoded query**.
And a reference field yields a name where you needed the sys_id to chain into
the next call.

So: **anything you intend to send back to ServiceNow must be read with
`--display-value false`** (or `all`, reading the `value` side). Never echo a
`display_value` into an update or a `--query`.

```bash
sn table get incident a1b2c3d4e5f6 --display-value false     # raw, round-trippable
sn table list incident --query "sys_created_on>2026-08-01" --display-value false --fields sys_id
```

The default applies to `table`, `get`, `gr`, `change`, `aggregate`, and `scores`. `sn watch`
has no such flag — an event's `record` already carries each field as a
`{display_value, value}` pair.

### Pagination & bulk processing

On `sn table list`, `--all` streams every match as JSONL, one object per line,
in `sys_id` order. It stops at `--max-records` (100,000 by default; `0` removes
the cap). Other command groups do not support `--all`. Don't compute offsets by
hand.

```bash
sn table list incident --query "active=true^priority=1" --all | jq -r '.number'           # extract a field
sn table list incident --all | jq -c 'select(.short_description|test("mail";"i"))'         # client-side filter
sn table list incident --all | jq -s 'group_by(.state)|map({state:.[0].state,count:length})' # group + count
sn table list incident --query "state=6^ORstate=7" --all | jq -r '.sys_id' \
  | while read -r sid; do sn table update incident "$sid" --field active=false; done        # stream into updates
```

If a stream fails part-way, stdout holds every record up to the failure and the
error names where to continue:

```json
{"error":{"message":"…","status_code":503,"resume_from":"8b031135474321009db4b5b08b9a7152"}}
```

Rerun the same command with `--resume-from <that sys_id>` and append. `--all`
refuses `--output raw` and `--output table` (exit 1); add `--array` for a table.
See [Pagination](usage.md#pagination) for how the walk works.

### Encoded query syntax

`--query` takes a ServiceNow "encoded query." Build incrementally — run with
`--limit 1` first to sanity-check syntax, then widen.

| Operator | Meaning | Example |
|---|---|---|
| `=` / `!=` | equals / not equals | `state=2`, `state!=7` |
| `>` `>=` `<` `<=` | numeric/date compare | `priority<=2` |
| `LIKE` / `STARTSWITH` / `ENDSWITH` | contains / prefix / suffix | `short_descriptionLIKEmail` |
| `IN` / `NOT IN` | value in / not in comma list | `stateIN1,2,3` |
| `ISEMPTY` / `ISNOTEMPTY` | null check | `assigned_toISEMPTY` |
| `^` / `^OR` / `^NQ` | AND / OR / new query (OR across groups) | `active=true^priority=1` |
| `ORDERBY` / `ORDERBYDESC` | ascending / descending sort | `ORDERBYDESCsys_created_on` |

```bash
# Priority 1 or 2, active, newest first
sn table list incident --query "active=true^priority=1^ORpriority=2^ORDERBYDESCsys_created_on" --limit 20
# Assigned to a user (sys_id) or unassigned
sn table list incident --query "assigned_to=6816f79c...^ORassigned_toISEMPTY"
```

## Writing records

**Body input** — two mutually exclusive ways (mixing them is exit 1):
- `--field name=value` (repeatable): cleanest for a few fields, no JSON
  escaping. Values are sent as strings; ServiceNow coerces per column type.
- `--data`: full JSON payload — needed for nested objects, arrays, or explicit
  nulls. Accepts inline JSON, `@file`, or `@-` (stdin).

```bash
sn table create incident --field short_description="Server CPU spike" --field caller_id=6816f79c... --field urgency=2
jq -n '{short_description:"from pipe",urgency:"3"}' | sn table create incident --data @-
sn table update incident c7d8e9f0a1b2 --field state=2 --field work_notes="Investigating"
sn table delete incident c7d8e9f0a1b2 --yes
```

- **`update` = PATCH** — only the named fields change. There is no `replace`. To
  clear a field, send it explicitly empty: `--field description=""`.
- **Destructive commands need `--yes` on a non-TTY**, or they exit 1 with a usage
  error naming the verb and the target. The guard is not only on `delete`: see
  the [confirmation guard](usage.md#confirmation-guard) for the list.
- **Writing by display value:** if you have a label ("In Progress") instead of a
  raw value ("2"), add `--input-display-value` so ServiceNow resolves labels on
  input. Resolution can be ambiguous (two users named "Alice"); prefer raw
  sys_ids for references.
- On writes, `--fields` narrows only the *response*, never the request body.
- **Tracked writes** (`sys_script`, `sys_properties`, dictionary, ACLs, …) are
  captured under the account's current application scope and update set. Check
  `sn get scope` and `sn get updateset` first; see
  [Session context](usage.md#session-context).

## Aggregate

`sn aggregate` counts, sums and averages server-side in one round trip, instead
of paginating and counting client-side.

```bash
sn aggregate incident --count                       # ungrouped → ONE object
```
```json
{"stats":{"count":"142"}}
```

```bash
sn aggregate incident --count --group-by state --display-value false   # grouped → an ARRAY
```
```json
[
  {"groupby_fields":[{"field":"state","value":"1"}],"stats":{"count":"15"}},
  {"groupby_fields":[{"field":"state","value":"2"}],"stats":{"count":"20"}},
  {"groupby_fields":[{"field":"state","value":"7"}],"stats":{"count":"27"}}
]
```

⚠️ **`--group-by` changes the top-level type from object to array**, and
`groupby_fields` is a **sibling** of `stats`, not a member of it. The count for a
group lives at `.stats.count`, and the group's value at
`.groupby_fields[0].value` — so `jq '.stats.groupby_fields[]'` returns nothing.
With the default `--display-value true`, that value is the label (`"New"`).

```bash
sn aggregate incident --count --group-by state \
  | jq -r '.[] | "\(.groupby_fields[0].value)\t\(.stats.count)"'
```

Counts are strings (`"142"`). `sum`/`avg`/`min`/`max` nest **per field** rather
than being scalars:
`{"stats":{"sum":{"reassignment_count":"24"},"min":{"priority":"1"}}}`.

## Change Management

`--type` targets a type-specific endpoint (`normal`, `emergency`, `standard`;
omit for the generic one). Standard changes **require** `--template`.

⚠️ **Unlike the Table API, the Change API returns every field as a
`{display_value, value}` pair.** `.number` is an *object*, not a string:

```json
{"number":{"display_value":"CHG0000024","value":"CHG0000024"},
 "state":{"display_value":"Closed","value":3.0}}
```

So `jq -r '.number'` prints a JSON blob, not `CHG0000024` — you want
`jq -r '.number.value'`. Note `state.value` comes back as a **number** (`3.0`),
while the Table API would give you the string `"3"`.

Call `nextstates` before changing state to avoid invalid-transition errors:

```bash
sn change nextstates chg001
# {"available_states":["3"],"state_label":{"3":"Closed"},"state_transitions":[]}
#   ^ an OBJECT, not a list of {value,label}: the legal next states are the strings
#     in .available_states, and .state_label maps each to its display name.
#     e.g.  jq -r '.available_states[] as $s | "\($s)\t\(.state_label[$s])"'
```

`sn change conflict remove <sys_id>` clears **every** conflict on the change — it
takes no conflict id. See [Change Management](usage.md#change-management) for
the commands.

## CMDB

Give `create`/`update` flat fields exactly as on `table`; `sn` builds the
`{"attributes": {...}, "source": "..."}` envelope the API requires and sends
values as strings. `--source` is the record's provenance and defaults to
`"Manual Entry"`; name a real discovery source only when standing in for that
tool. See [CMDB](usage.md#cmdb) for the rules.

⚠️ **`cmdb get` nests the CI's fields under `attributes`.** The top level has
exactly three keys — `attributes`, `inbound_relations`, `outbound_relations` — so
the CI's name is `.attributes.name`, not `.name`:

```bash
sn cmdb get cmdb_ci_server ci001 | jq -r '.attributes.name'
sn cmdb get cmdb_ci_server ci001 | jq '.outbound_relations[] | {type, target}'
```

## CICD: waiting and polling

`cicd app`, `cicd updateset`, and `cicd atf run` are **asynchronous** — they return a progress
object with `links.progress.id` immediately. `status` codes: `0` Pending, `1`
Running, `2` Successful, `3` Failed, `4` Cancelled. See
[CICD operations](usage.md#cicd-operations) for the commands.

**Preferred: `--wait`** blocks until the operation finishes, then emits the final
progress result. `--wait-timeout <SECS>` bounds the wait; on expiry the command
exits 3 pointing you to `sn cicd progress <id>`.

**Branch on the exit code, never on `status_label`.** For an operation with a returned progress link, `--wait` returns 0 when
`status` reaches `2`. If the initial response has no `links.progress.id`, the CLI
emits it without polling, so check the response before assuming completion. A failed operation is **exit 2 with empty stdout** (the progress
object is on stderr, under `.error.sn_error`); a timeout is **exit 3, also empty
stdout**. So reading the command's stdout on a failure branch gets you an empty
string:

```bash
if out=$(sn cicd app install --scope x_myapp --version 1.2.0 --wait --wait-timeout 300 2>/tmp/sn.err); then
  echo "installed"                                    # exit 0 ⇒ status "2", nothing else to check
else
  case $? in
    2) jq -r '.error.message' /tmp/sn.err >&2 ;;      # failed — details in .error.sn_error
    3) echo "polling timed out or a network request failed; check the error" >&2 ;;
    *) cat /tmp/sn.err >&2 ;;
  esac
  exit 1
fi
```

Final `--wait` stdout on success:
`{"status":"2","status_label":"Successful","status_message":"...","percent_complete":"100"}`.

⚠️ **`status_label` is ServiceNow's own string, passed through verbatim — `sn` never
normalizes it.** It varies by instance and operation ("Successful", "Complete",
"Succeeded"…). Matching on it is how you write a poll loop that never terminates.
The numeric `status` is the contract; the label is for humans.

**Manual polling** (for an operation already in flight) — key off `status`:

```bash
while true; do
  r=$(sn cicd progress "$id") || exit $?
  case "$(jq -r '.status' <<<"$r")" in
    2)   break ;;                                              # successful
    3|4) jq -r '.status_message' <<<"$r" >&2; exit 1 ;;        # failed / cancelled
    *)   sleep 5 ;;                                            # 0 pending, 1 running
  esac
done
```

## Other commands

Each of these is documented in the usage guide; the note is the part that bites.

| Command | Worth knowing |
|---|---|
| [`sn watch`](usage.md#watching-records-live) | Bound every stream (`--max-events` / `--duration` / `--idle-timeout`). An event carries only the fields that changed. |
| [`sn gr`](usage.md#dot-walked-reads-sn-gr) | Dot-walked reference fields in one request. On GraphQL errors it exits 2 without partial records. |
| [`sn graphql`](usage.md#graphql) | Errors arrive in-band under HTTP 200: exit 2, the full array under `sn_error`, partial `data` still on stdout. Prefer `--var` over splicing values into the document. |
| [`sn journal`](usage.md#journal-comments-and-work-notes) | There is no `journal add`: write an entry with `sn table update … --field work_notes="…"`. |
| [`sn variables`](usage.md#catalog-variables) | `set` refuses unknown names before writing and verifies by re-read. Names are case-sensitive. |
| [`sn attachment`](usage.md#attachments) | `download --out` never leaves a truncated file; to stdout, a mid-stream failure is exit 3 with bytes already written. Check the exit code. |
| [`sn catalog`](usage.md#service-catalog) | Call `item-variables` before ordering: variables with `mandatory: true` must be in the payload. |
| [`sn identify`](usage.md#identification--reconciliation) | `query` identifies without modifying anything; run it before `create-update`. |
| [`sn xml`](usage.md#xml-export-and-import) | `import` exit 0 means the file was uploaded, so check the records afterwards. `export` stops at `glide.xml.export.limit` without saying so. |
| [`sn flow`](usage.md#flow-designer) | `get --outline` is usually all an agent needs out of a model that can run to hundreds of KB. `steps` on an unreported run is an error, not an empty list. |
| [`sn decision`](usage.md#decision-tables) | `run` checks inputs against the table first (exit 1). `matches: []` is an answer, exit 0. |
| [`sn playbook`](usage.md#playbooks) | Branch on `sn_error.errorType`. `trigger --only-if-none` keeps a retry from stacking a second run. |
| [`sn script run`](usage.md#background-scripts) | Arbitrary code as the profile's user, so it needs `--yes`. A script error prints the result with `ok: false` and exits 2 with `status_code: 200`. |
| [`sn impersonate`](usage.md#acting-as-another-user) | Runs one command as another user to see what their roles and ACLs allow. Needs the admin or impersonator role (exit 4 without it). |
| [`sn codesearch`](usage.md#code-search) | Read access filters results silently: exit 0 with few or no rows is the ACL answer, not "not referenced". `--limit` counts records examined, not hits. |
| [`sn appmanager`](usage.md#application-manager) | `show` on an unknown id is exit 2, never `installed: false`. `install` is plugins only and irreversible. |
| [`sn raw`](usage.md#raw-rest-passthrough) | No confirmation guard, including for `DELETE`. Request body and response must be JSON; the envelope is kept. |
| [`sn cache`](usage.md#offline-schema-cache) | After `sn cache refresh` (needs admin), `cache tables` and `cache columns` answer with no network. |
| [`sn introspect`](agent-integration.md#sn-introspect-the-machine-readable-command-tree) | The command tree as one recursive JSON object, for generating MCP tools or function-call schemas. |

## Common mistakes

- **Echoing a read straight back into a write or a `--query`** → `--display-value`
  defaults to **`true`**, so you are holding `"In Progress"` and a localized date
  like `08/04/2026 10:22:01 AM`, neither of which ServiceNow will accept back.
  Read with `--display-value false` (or `all`, and send the `value` side).
- **Reaching for `replace`** → there is none. `update` is the existing-record
  update verb. Clear a field explicitly (`--field x=""`).
- **Mixing `--data` and `--field`** → exit 1. Pick one.
- **`--query` on `get`** → use a sys_id or `table:number` reference. For other
  criteria, use `table list --query "..."` and check how many records matched.
- **Missing `--yes` on a destructive command** in CI/agent contexts → immediate
  exit 1 usage error (non-TTY never prompts). See the
  [confirmation guard](usage.md#confirmation-guard).
- **Treating every exit 4 as "log in again"** → 403 is exit 4 too, and an ACL
  denial does not get better with a fresh token. Check `.error.status_code`.
- **Combining `--all` with `--output raw` or `--output table`** → exit 1. Use
  `--array` for a table; page manually if you need the envelope.
- **Comparing `.error.status_code` without allowing for its absence** → it is
  omitted, not zeroed, when the failure carried no HTTP status.
- **Sending a display value as raw** → `--field state="In Progress"` without
  `--input-display-value` fails; send `state=2`.
- **Paginating by hand** → use `--all` (with `--max-records` as a guard rail);
  never compute offsets.
- **Trusting `sn_error` on transport errors (exit 3)** → it's null/absent; check
  `.error.message`.
- **Pulling more than you need** → `--setlimit` defaults to 1000 on the main list commands; lower it for
  exploration.

## Quick reference

Use `sn --help` for command groups and `sn <group> <command> --help` for the
installed version's arguments. `sn introspect` provides the same command tree as
JSON. This avoids treating flags from one command as valid on another.

| Task | Start here |
|---|---|
| Connect to an instance | [Profiles](#profiles) |
| Find tables or fields | [Discovery flow](#discovery-flow) |
| Read by number or sys_id | [Reading records](#reading-records) |
| Read fields through references | [Dot-walked reads](usage.md#dot-walked-reads-sn-gr) |
| Create, update, or delete | [Writing records](#writing-records) |
| Select scope and update set | [Session context](usage.md#session-context) |
| Watch for changes | [Live watching](usage.md#watching-records-live) |
| Run deployment or test operations | [CICD](#cicd-waiting-and-polling) |
| Interpret output or failure | [Output and exit codes](#output-errors--exit-codes-read-first) |
| Install the Claude Code plugin | [Agent integration](agent-integration.md#claude-code-plugin) |
