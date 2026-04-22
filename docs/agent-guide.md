# `sn` agent usage guide

Read this once at the start of a task. It covers everything an LLM agent needs
to read, create, update, and delete ServiceNow records via the `sn` CLI.

## What `sn` is

`sn` is a Rust CLI that wraps the ServiceNow Table API (`/api/now/table/...`)
and two schema-discovery endpoints (`/api/now/doc/table/schema` for tables and
`/api/now/ui/meta/{table}` for column/choice/reference metadata). It speaks
JSON on stdout and structured JSON errors on stderr, uses stable exit codes,
and exposes schema and choice lookups
so an agent can discover a table's shape on demand. Assume zero prior
ServiceNow knowledge: every operation below is runnable end-to-end from a
cold start after `sn init`.

## Output contract (read this first)

This is the part you must internalize before issuing any command.

**stdout is JSON, always.**
- TTY: pretty-printed (2-space indent).
- Piped / non-TTY: compact (single-line). Parse with `jq` or any JSON parser.
- Default shape is **unwrapped** — the CLI unwraps ServiceNow's envelope for you.

| Verb | Default stdout shape |
|---|---|
| `table list` | JSON array of record objects (the `result` field, unwrapped) |
| `table get` | A single record object |
| `table create` | The created record object |
| `table update` | The updated record object |
| `table replace` | The replaced record object |
| `table delete` | No stdout (empty); exit code indicates success |
| `schema tables` | JSON array of table metadata |
| `schema columns` | JSON array of column metadata |
| `schema choices` | JSON array of choice values |
| `auth test` | `{"ok": true, "user": "...", "instance": "..."}` |

**Opt-in raw mode.** `--output raw` preserves the full ServiceNow response
envelope instead of unwrapping:

```bash
sn table get incident abc123 --output raw
```
```json
{
  "result": {
    "sys_id": "abc123",
    "number": "INC0010001",
    "short_description": "Mail server down"
  }
}
```

**stderr is JSON for errors, always:**

```json
{
  "error": {
    "message": "Record not found",
    "detail": "No record with sys_id 'abc123' in table 'incident'",
    "status_code": 404,
    "transaction_id": "3f4ab12c8d0001",
    "sn_error": {
      "message": "No Record found",
      "detail": "Record doesn't exist or ACL restricts the record retrieval"
    }
  }
}
```

`sn_error` is the original ServiceNow payload verbatim (may be null for
transport/CLI errors). `transaction_id` is ServiceNow's correlation id when
present, useful for support requests.

**Exit codes — branch on these first, parse stdout second:**

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Usage / config / parse error (bad flags, unreadable file, malformed JSON input) |
| 2 | API error — ServiceNow returned a 4xx/5xx other than auth |
| 3 | Network / transport error (DNS, connection refused, timeout, TLS) |
| 4 | Auth error (401 or 403) |

Recommended agent pattern:

```bash
out=$(sn table get incident "$sysid" 2>/tmp/sn.err)
case $? in
  0) jq -r '.short_description' <<<"$out" ;;
  2) # API error — inspect JSON on stderr, decide whether to retry or surface
     jq -r '.error.message' /tmp/sn.err ;;
  4) echo "auth failed, re-init profile" >&2; exit 1 ;;
  *) echo "transport or config failure" >&2; exit 1 ;;
esac
```

Verbose logging on stderr (see `-v`) is debug-only; never required to parse it.

## Setup (one-time per instance)

```bash
sn init                           # interactive: prompts for instance, username, password
sn init --profile prod            # add a second profile named "prod"
sn auth test                      # verify credentials against /api/now/table/sys_user?sysparm_limit=1
```

`sn init` writes credentials to `~/.config/sn/credentials.toml` (chmod 600
on Unix; on Windows the per-user `%APPDATA%` ACL is the access boundary).
Non-secret profile config lives in `~/.config/sn/config.toml`. v1 uses
plaintext TOML with file permissions as the access boundary; OS keychain
storage is on the roadmap but not shipped.

Sample `auth test` output:

```json
{
  "ok": true,
  "instance": "https://dev12345.service-now.com",
  "username": "api.user",
  "user_sys_id": "a1b2c3d4e5f6",
  "profile": "default"
}
```

**Multi-profile selection** (in precedence order, highest first):
1. `--profile <name>` flag
2. `SN_PROFILE` environment variable
3. `default` profile

```bash
sn --profile prod table list incident --limit 5
SN_PROFILE=prod sn table list incident --limit 5
```

**Per-invocation env overrides** (bypass profile entirely; great for CI or
ephemeral agent sessions):

```bash
SN_INSTANCE=https://dev12345.service-now.com \
SN_USERNAME=api.user \
SN_PASSWORD='s3cr3t' \
  sn table list incident --limit 1
```

Precedence for credential fields: env var > profile file.
If `SN_INSTANCE` is set but username/password are not, the CLI falls back to
the active profile for the missing pieces.

## Discovery flow (the agent's superpower)

When you don't know a table's schema, do this before writing:

```bash
# 1. Find the table — fuzzy match table name or label
sn schema tables --filter incident
```
```json
[
  {
    "name": "incident",
    "label": "Incident",
    "super_class": "task",
    "is_extendable": true,
    "sys_id": "d17b2c4773..."
  },
  {
    "name": "incident_task",
    "label": "Incident Task",
    "super_class": "task",
    "is_extendable": false,
    "sys_id": "8a9e..."
  }
]
```

```bash
# 2. Learn the writable schema (mandatory fields, types, references)
sn schema columns incident --writable
```
```json
[
  {
    "name": "short_description",
    "label": "Short description",
    "type": "string",
    "max_length": 160,
    "mandatory": true,
    "read_only": false,
    "reference": null,
    "choice_field": false
  },
  {
    "name": "caller_id",
    "label": "Caller",
    "type": "reference",
    "mandatory": false,
    "read_only": false,
    "reference": "sys_user",
    "choice_field": false
  },
  {
    "name": "state",
    "label": "State",
    "type": "integer",
    "mandatory": true,
    "read_only": false,
    "reference": null,
    "choice_field": true,
    "default_value": "1"
  },
  {
    "name": "priority",
    "label": "Priority",
    "type": "integer",
    "mandatory": false,
    "read_only": false,
    "reference": null,
    "choice_field": true
  }
]
```

Useful filters: `--writable` (excludes read-only), `--mandatory`,
`--filter <substring>` (matches name or label), `--references-only` (reference
fields only), `--choices-only` (choice fields only), `--type <type>`
(filter by column type, e.g. `string`, `integer`, `reference`).

```bash
# 3. For any choice field, list the valid values
sn schema choices incident state
```
```json
[
  {"value": "1", "label": "New",          "sequence": 1},
  {"value": "2", "label": "In Progress",  "sequence": 2},
  {"value": "3", "label": "On Hold",      "sequence": 3},
  {"value": "6", "label": "Resolved",     "sequence": 6},
  {"value": "7", "label": "Closed",       "sequence": 7},
  {"value": "8", "label": "Canceled",     "sequence": 8}
]
```

`state=2` means "In Progress" — the numeric `value` is what you send to the
write APIs. The human label is what `--display-value true` returns on reads.

```bash
# 4. Now create/update with confidence
sn table create incident \
  --field short_description="server down" \
  --field state=2 \
  --field priority=1
```

(Example values in this document are illustrative; real values depend on your
instance.)

## Reading records (`list`, `get`)

### Simple list with a cap

```bash
sn table list incident --page-size 5
```

(`--limit` is accepted as an alias for `--page-size`, matching the
ServiceNow docs for `sysparm_limit`. Default is 1000 records per page;
override any time.)
```json
[
  {
    "sys_id": "a1b2c3d4e5f6",
    "number": "INC0010001",
    "short_description": "Mail server down",
    "state": "2",
    "priority": "1",
    "sys_created_on": "2026-04-22 09:14:11"
  },
  {
    "sys_id": "b2c3d4e5f6a7",
    "number": "INC0010002",
    "short_description": "VPN disconnects intermittently",
    "state": "1",
    "priority": "3",
    "sys_created_on": "2026-04-22 09:17:02"
  }
]
```

### Filter + project columns

```bash
sn table list incident \
  --query "active=true^priority=1" \
  --fields "number,short_description,state" \
  --page-size 10
```
```json
[
  {"number": "INC0010001", "short_description": "Mail server down",        "state": "2"},
  {"number": "INC0010044", "short_description": "Auth service 500s",       "state": "1"}
]
```

### Get one record by sys_id

```bash
sn table get incident a1b2c3d4e5f6
```
```json
{
  "sys_id": "a1b2c3d4e5f6",
  "number": "INC0010001",
  "short_description": "Mail server down",
  "state": "2",
  "priority": "1",
  "caller_id": "6816f79cc0a8016401c5a33be04be441",
  "assigned_to": "",
  "sys_created_on": "2026-04-22 09:14:11"
}
```

### Display values — critical for agents

By default reference fields return sys_ids and choice fields return raw
values. That makes `state: "2"` unreadable without a choice lookup. Use
`--display-value` to ask ServiceNow to resolve them:

```bash
sn table get incident a1b2c3d4e5f6 --display-value all
```
```json
{
  "sys_id": "a1b2c3d4e5f6",
  "number": "INC0010001",
  "short_description": "Mail server down",
  "state": "In Progress",
  "priority": "1 - Critical",
  "caller_id": "Alice Example",
  "assigned_to": "",
  "sys_created_on": "2026-04-22 09:14:11"
}
```

Values for `--display-value`:

| Value | Effect | Use when |
|---|---|---|
| `false` (default) | raw values everywhere | writing back, scripting |
| `true` | display values everywhere | human-readable output |
| `all` | returns both — each field becomes `{"value": "...", "display_value": "..."}` | you need both at once |

With `--display-value all` the shape changes:

```json
{
  "state":    {"value": "2", "display_value": "In Progress"},
  "priority": {"value": "1", "display_value": "1 - Critical"}
}
```

When you plan to echo a value back into an update, always use the raw
`value`, not the `display_value`.

### Auto-pagination

ServiceNow caps any single response; for large queries use `--all`:

```bash
# Streams every matching record as JSONL (one JSON object per line)
sn table list incident --query "active=true" --all
```
```
{"sys_id":"a1...","number":"INC0010001","short_description":"Mail server down","state":"2"}
{"sys_id":"b2...","number":"INC0010002","short_description":"VPN disconnects","state":"1"}
...
```

Why JSONL: you can pipe it to `jq -c` line-by-line without buffering the
whole result set. For huge tables this is the only safe default.

```bash
# Single JSON array instead of JSONL — buffers everything in memory
sn table list incident --query "active=true" --all --array
```

```bash
# Safety cap (stops after N records even if more match)
sn table list incident --query "active=true" --all --max-records 1000
```

Internal paging follows the `Link: rel="next"` header ServiceNow emits.
`--page-size` controls the per-API-call batch size (default 1000), and
`--offset` is ignored in `--all` mode since pagination walks the full
result set from the start.

### Encoded query syntax cheat sheet

ServiceNow uses "encoded queries" for `--query`. The primitives:

| Operator | Meaning | Example |
|---|---|---|
| `=` | equals | `state=2` |
| `!=` | not equals | `state!=7` |
| `>` / `>=` / `<` / `<=` | numeric/date comparison | `priority<=2` |
| `LIKE` | contains (case-insensitive) | `short_descriptionLIKEmail` |
| `STARTSWITH` | prefix match | `numberSTARTSWITHINC001` |
| `ENDSWITH` | suffix match | `numberENDSWITH42` |
| `IN` | value in comma list | `stateIN1,2,3` |
| `NOT IN` | value not in list | `stateNOT IN7,8` |
| `ISEMPTY` / `ISNOTEMPTY` | null check | `assigned_toISEMPTY` |
| `^` | AND | `active=true^priority=1` |
| `^OR` | OR (same field) | `state=1^ORstate=2` |
| `^NQ` | new query (OR across groups) | `state=1^NQstate=2^priority=1` |
| `^EQ` | end of OR group (rarely needed) | `state=1^ORstate=2^EQ` |
| `ORDERBY` | ascending sort | `ORDERBYnumber` |
| `ORDERBYDESC` | descending sort | `ORDERBYDESCsys_created_on` |

Worked examples:

```bash
# Priority 1 or 2, active, sorted newest first
sn table list incident \
  --query "active=true^priority=1^ORpriority=2^ORDERBYDESCsys_created_on" \
  --limit 20

# Short description contains "mail", not yet resolved
sn table list incident \
  --query "short_descriptionLIKEmail^state!=6^state!=7"

# Assigned to a specific user (sys_id) or unassigned
sn table list incident \
  --query "assigned_to=6816f79cc0a8016401c5a33be04be441^ORassigned_toISEMPTY"
```

Build queries incrementally: run with `--limit 1` first to sanity-check
syntax, then widen.

## Writing records (`create`, `update`, `replace`)

### Create with inline JSON

```bash
sn table create incident --data '{"short_description": "Printer jam in 3B", "urgency": "3"}'
```
```json
{
  "sys_id": "c7d8e9f0a1b2",
  "number": "INC0010087",
  "short_description": "Printer jam in 3B",
  "urgency": "3",
  "state": "1",
  "sys_created_on": "2026-04-22 14:02:44"
}
```

### Create from a file

```bash
sn table create incident --data @body.json
```

Where `body.json` is:
```json
{
  "short_description": "DB replica lag alert",
  "caller_id": "6816f79cc0a8016401c5a33be04be441",
  "category": "software",
  "urgency": "2"
}
```

`--data @-` reads from stdin, so you can pipe:

```bash
jq -n '{short_description: "from pipe", urgency: "3"}' | sn table create incident --data @-
```

### Create with repeated `--field` flags

Cleaner for a handful of fields and avoids JSON escaping:

```bash
sn table create incident \
  --field short_description="Server CPU spike" \
  --field caller_id=6816f79cc0a8016401c5a33be04be441 \
  --field urgency=2 \
  --field category=hardware
```

`--field` takes `name=value`. Values are sent as strings — ServiceNow
coerces them per the column type. Use `--data` when you need nested objects,
arrays, or explicit nulls.

### Update (PATCH — only the fields you name)

```bash
sn table update incident c7d8e9f0a1b2 --field state=2 --field work_notes="Picked up, investigating"
```

Only `state` and `work_notes` are sent. All other fields on the record are
untouched. This is almost always what you want.

### Replace (PUT — overwrites the entire record)

```bash
sn table replace incident c7d8e9f0a1b2 --data @full.json
```

**Danger.** `replace` sends a PUT. Any field you omit from the payload is
reset to its dictionary default (often empty). Use `replace` only when you
have freshly read the record, modified the full payload, and intend to write
the whole thing back. For partial changes, always prefer `update`.

### Delete

```bash
sn table delete incident c7d8e9f0a1b2 --yes
```

Returns exit 0 with empty stdout on success. Without `--yes`, the CLI
prints a confirmation prompt to stderr and reads `y/N` from stdin. In any
non-interactive context (CI, agent, pipe), always pass `--yes` or the
command will hang.

### Writing by display value

If you have a human-readable label ("In Progress") instead of a raw value
("2"), pass `--input-display-value` so ServiceNow resolves labels on input:

```bash
sn table update incident c7d8e9f0a1b2 \
  --input-display-value \
  --field state="In Progress" \
  --field assigned_to="Alice Example"
```

ServiceNow's display-value resolution can be ambiguous (two users named
"Alice Example"); prefer raw sys_ids for references when you can.

## Parameter reference

Friendly flags map directly to ServiceNow `sysparm_*` query parameters. Use
whichever name you remember; both work in this table.

| Friendly flag | sysparm name | Applies to | Notes |
|---|---|---|---|
| `--query <EQ>` | `sysparm_query` | list | Encoded query string |
| `--fields <csv>` | `sysparm_fields` | list, get, create, update, replace | Comma-separated columns to return |
| `--page-size <N>` | `sysparm_limit` | list | Per-page/result-cap; default 1000. `--limit` is an accepted alias. |
| `--offset <N>` | `sysparm_offset` | list | Page offset |
| `--display-value <false\|true\|all>` | `sysparm_display_value` | list, get, create, update, replace | See display values above |
| `--input-display-value` | `sysparm_input_display_value=true` | create, update, replace | Resolve labels in request body |
| `--exclude-reference-link` | `sysparm_exclude_reference_link=true` | list, get, create, update, replace | Drops the `link` URL from reference fields |
| `--suppress-pagination-header` | `sysparm_suppress_pagination_header=true` | list | Skips `X-Total-Count` calculation (faster) |
| `--view <name>` | `sysparm_view` | list, get | Apply a named form/list view |
| `--query-category <cat>` | `sysparm_query_category` | list | Query category for index selection |
| `--query-no-domain` | `sysparm_query_no_domain=true` | list, get, update, replace, delete | Cross-domain access if authorized |
| `--output <default\|raw>` | (CLI only) | all | `raw` keeps the SN envelope |
| `--profile <name>` | (CLI only) | all | Select profile |
| `--all` | (CLI only) | list | Auto-paginate |
| `--array` | (CLI only) | list | With `--all`, emit one array instead of JSONL |
| `--max-records <N>` | (CLI only) | list | Cap `--all` output |
| `--suppress-auto-sys-field` | `sysparm_suppress_auto_sys_field=true` | create, update, replace | Skip auto-generation of system fields |
| `--no-count` | `sysparm_no_count=true` | list | Skip the count query (faster for large tables) |
| `--yes` / `-y` | (CLI only) | delete | Skip confirmation |
| `--no-retry` | (CLI only) | all | Disable automatic 429/5xx retries |
| `-v` / `-vv` / `-vvv` | (CLI only) | all | Debug logging to stderr |

## Pagination patterns

```bash
# Manual single-page read — page 3 at size 50
sn table list incident --page-size 50 --offset 100 --query "active=true"
```

```bash
# Stream everything as JSONL — safe for 100k+ records
sn table list incident --query "active=true" --all
```

```bash
# Same, but cap the total to avoid runaway queries
sn table list incident --query "active=true" --all --max-records 5000
```

```bash
# Buffer everything into a single array (use only when you know it's bounded)
sn table list incident --query "active=true" --all --array
```

```bash
# Tune per-API batch size for --all (fewer, larger pages)
sn table list incident --query "active=true" --all --page-size 5000
```

### Processing JSONL with jq

```bash
# Count records
sn table list incident --query "active=true" --all | wc -l

# Extract just numbers
sn table list incident --query "active=true^priority=1" --all \
  | jq -r '.number'

# Filter client-side (when encoded queries can't express it)
sn table list incident --all \
  | jq -c 'select(.short_description | test("mail"; "i"))'

# Group by state, count
sn table list incident --all \
  | jq -s 'group_by(.state) | map({state: .[0].state, count: length})'

# Stream into parallel updates (careful with rate limits)
sn table list incident --query "state=6^ORstate=7" --all \
  | jq -r '.sys_id' \
  | while read -r sid; do
      sn table update incident "$sid" --field active=false
    done
```

## Error handling

### Branch on exit code

```bash
sn table get incident "$sysid" > out.json 2> err.json
case $? in
  0)
    jq -r '.number' out.json
    ;;
  1)
    echo "usage error — fix the command" >&2
    jq -r '.error.message' err.json >&2
    exit 1
    ;;
  2)
    status=$(jq -r '.error.status_code' err.json)
    if [ "$status" = "404" ]; then
      echo "record not found, nothing to do" >&2
      exit 0
    fi
    echo "API error: $(jq -r '.error.message' err.json)" >&2
    exit 1
    ;;
  3)
    echo "network failure — safe to retry the whole command" >&2
    ;;
  4)
    echo "auth failed — re-run 'sn init' or check SN_PASSWORD" >&2
    exit 1
    ;;
esac
```

### Parse the stderr JSON

```bash
sn table get incident bogus_id 2>&1 >/dev/null | jq '.error'
```
```json
{
  "message": "Record not found",
  "detail": "No record with sys_id 'bogus_id' in table 'incident'",
  "status_code": 404,
  "transaction_id": "a7b8c9d0e1f2",
  "sn_error": {
    "message": "No Record found",
    "detail": "Record doesn't exist or ACL restricts the record retrieval"
  }
}
```

### Common error scenarios

| Scenario | Exit | Status | Notes |
|---|---|---|---|
| Bad sysparm value (e.g. `--limit abc`) | 1 | — | Caught at parse time; nothing sent |
| Malformed `--data` JSON | 1 | — | Validated before the request |
| Mixing `--data` and `--field` | 1 | — | Mutually exclusive |
| Unknown table | 2 | 400 | ServiceNow rejects the path |
| sys_id not found | 2 | 404 | Record missing or ACL blocks you |
| ACL denies read/write | 2 | 403 | Distinct from auth (401) — credentials are fine, permissions aren't |
| Bad credentials | 4 | 401 | Re-init profile |
| Session expired / MFA | 4 | 401 | Same handling as bad creds |
| TLS handshake failure | 3 | — | Usually `SN_INSTANCE` typo or proxy issue |
| DNS / connection refused | 3 | — | Network; safe to retry |
| Timeout | 3 | — | See retry behavior below |
| Rate limited | 2 or 0 after retry | 429 | Auto-retried 3x with backoff |
| Internal server error | 2 or 0 after retry | 5xx | Auto-retried 3x with backoff |

Distinguishing 403-from-ACL vs 401-from-auth matters: code 4 says "your
credentials are wrong," code 2 with `status_code: 403` says "the user is
authenticated but not allowed to do that."

### Retry behavior

- `429 Too Many Requests` and `5xx` are automatically retried up to 3
  times with exponential backoff (roughly 0.5s, 1s, 2s plus jitter),
  honoring `Retry-After` if present.
- Network errors (exit 3) are **not** retried — the CLI surfaces them so
  the caller can decide.
- Pass `--no-retry` to disable automatic retries entirely (useful when
  your own orchestrator handles backoff).

```bash
sn --no-retry table list incident --limit 1
```

## Verbosity for debugging

| Flag | What it adds to stderr |
|---|---|
| (none) | Nothing — stderr silent on success |
| `-v` | HTTP method, URL, status code, elapsed time per request |
| `-vv` | Response headers |
| `-vvv` | Request and response bodies (pretty-printed; Authorization always masked) |

```bash
sn -vv table get incident a1b2c3d4e5f6 2>/tmp/trace.log
```

Rules of thumb for agents:
- Never parse stderr in verbose mode — stdout is still the only contract.
- `-vvv` is safe to log: auth headers are masked to `Authorization: Basic ***`.
- Turn on `-v` when you get an exit 2 or 3 and need to see the URL that
  was built (common cause of 404s is a sysparm typo producing a weird URL).

## Building tools/MCP servers on top of `sn`

`sn introspect --json` emits the complete command tree — every subcommand,
flag, value type, and help text — as machine-readable JSON. This is the
canonical way to auto-generate MCP tool definitions, OpenAI function-call
schemas, or any other structured wrapper.

```bash
sn introspect --json | jq '.commands[] | {name, summary}'
```
```json
{"name": "table list",     "summary": "List records from a table"}
{"name": "table get",      "summary": "Get one record by sys_id"}
{"name": "table create",   "summary": "Create a record"}
{"name": "table update",   "summary": "Patch an existing record"}
{"name": "table replace",  "summary": "Replace (PUT) a record"}
{"name": "table delete",   "summary": "Delete a record"}
{"name": "schema tables",  "summary": "List tables on the instance"}
{"name": "schema columns", "summary": "List columns for a table"}
{"name": "schema choices", "summary": "List choice values for a choice column"}
{"name": "auth test",      "summary": "Verify credentials"}
{"name": "introspect",     "summary": "Emit command metadata"}
```

A full command entry looks like:

```json
{
  "name": "table list",
  "summary": "List records from a table",
  "args": [
    {"name": "table", "required": true, "description": "Table name, e.g. 'incident'"}
  ],
  "flags": [
    {"name": "--query",         "value_type": "string",  "sysparm": "sysparm_query"},
    {"name": "--fields",        "value_type": "string",  "sysparm": "sysparm_fields"},
    {"name": "--limit",         "value_type": "integer", "sysparm": "sysparm_limit"},
    {"name": "--display-value", "value_type": "enum",    "values": ["false","true","all"]},
    {"name": "--all",           "value_type": "bool"},
    {"name": "--output",        "value_type": "enum",    "values": ["default","raw"]}
  ],
  "exit_codes": [0, 1, 2, 3, 4]
}
```

## Common mistakes

Things that bite agents repeatedly:

- **Forgetting `--display-value true` on reads meant for humans.** You'll
  report `state=2` instead of `state=In Progress` and confuse everyone.
  For writes, always use raw values.
- **Using `replace` (PUT) when you meant `update` (PATCH).** `replace`
  wipes every field you didn't include. Default to `update` unless you
  explicitly need full-record semantics.
- **Pulling way more than you need.** `sn` defaults `--page-size` to
  1000, which is much friendlier than SN's native 10000. For quick
  exploration, drop it lower (`--page-size 5`). For bulk work, prefer
  `--all` with `--max-records` as a guard rail.
- **Mixing `--data` and `--field` on the same command.** The CLI rejects
  this with exit 1. Pick one: `--data` for full JSON payloads, `--field`
  for a handful of simple key/values.
- **Using `--query` on `get`.** `get` takes a sys_id only. For
  "find one by criteria," use `list --limit 1 --query "..."` and read
  `[0]` from the result array.
- **Forgetting `--yes` on `delete` in non-interactive contexts.** The
  command will block forever waiting on stdin.
- **Sending display values as raw values.** `--field state="In Progress"`
  without `--input-display-value` will fail ("Invalid value for integer
  field"). Either pass the raw value (`state=2`) or opt in to display
  resolution.
- **Trusting `sn_error` to always be present.** For transport errors
  (exit 3) there is no ServiceNow response; `sn_error` will be null or
  absent. Always check `.error.message` first.
- **Treating stderr verbose output as structured.** Only stderr JSON
  error objects are structured. `-v`/`-vv`/`-vvv` output is free-form
  debug text and may change between versions.
- **Paginating by hand when `--all` exists.** Don't compute offsets
  yourself; use `--all` with `--max-records` for safety.
- **Assuming `--fields` narrows the request body for writes.** On
  create/update/replace, `--fields` only narrows the *response*. The
  request body is whatever you sent via `--data` or `--field`.

## Quick reference card

```
sn init [--profile NAME]
sn auth test [--profile NAME]

sn schema tables  [--filter SUBSTR]
sn schema columns TABLE [--writable] [--mandatory] [--filter SUBSTR]
                        [--references-only] [--choices-only] [--type TYPE]
sn schema choices TABLE COLUMN

sn table list TABLE
  [--query EQ] [--fields CSV]
  [--page-size N (default 1000; alias --limit)] [--offset N]
  [--display-value false|true|all]
  [--exclude-reference-link]
  [--all [--array] [--max-records N]]
  [--view NAME] [--query-category CAT] [--query-no-domain] [--no-count]
  [--suppress-pagination-header]
  [--output default|raw]

sn table get TABLE SYS_ID
  [--fields CSV] [--display-value false|true|all]
  [--exclude-reference-link] [--view NAME] [--query-no-domain]
  [--output default|raw]

sn table create TABLE (--data JSON|@FILE|@- | --field K=V [--field K=V ...])
  [--fields CSV] [--display-value ...] [--input-display-value]
  [--exclude-reference-link] [--suppress-auto-sys-field]
  [--view NAME] [--output default|raw]

sn table update TABLE SYS_ID (--data ...|--field K=V ...)
  [--fields CSV] [--display-value ...] [--input-display-value]
  [--exclude-reference-link] [--suppress-auto-sys-field]
  [--view NAME] [--query-no-domain] [--output default|raw]

sn table replace TABLE SYS_ID (--data ...|--field K=V ...)
  [--fields CSV] [--display-value ...] [--input-display-value]
  [--exclude-reference-link] [--suppress-auto-sys-field]
  [--view NAME] [--query-no-domain] [--output default|raw]

sn table delete TABLE SYS_ID [--yes] [--query-no-domain]

sn introspect --json

Global flags (any command):
  --profile NAME   select credential profile
  --no-retry       disable 429/5xx auto-retry
  -v / -vv / -vvv  verbose logging on stderr

Environment variables:
  SN_PROFILE       profile override
  SN_INSTANCE      https://<name>.service-now.com
  SN_USERNAME      basic-auth username
  SN_PASSWORD      basic-auth password

Exit codes:
  0 success   1 usage/config   2 api (4xx/5xx)   3 network   4 auth (401/403)

Canonical output shapes (stdout):
  list     -> [ {record}, {record}, ... ]
  get      -> {record}
  create   -> {record}
  update   -> {record}
  replace  -> {record}
  delete   -> (empty)
  schema * -> [ {meta}, ... ]
  --output raw preserves { "result": ... } envelope

Error shape (stderr, all non-zero exits):
  { "error": { "message", "detail", "status_code",
               "transaction_id", "sn_error" } }
```
