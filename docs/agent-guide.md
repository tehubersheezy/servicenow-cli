# `sn` agent usage guide

Read this once at the start of a task. It covers everything an LLM agent needs
to read, create, update, and delete ServiceNow records via the `sn` CLI.

## What `sn` is

`sn` is a Rust CLI that wraps ServiceNow's REST APIs: Table API, Change
Management, Attachment, CMDB (Instance + Meta), Import Set, Service Catalog,
Identification & Reconciliation, CICD (App Repository, Update Sets, ATF),
Aggregate, Performance Analytics, and two schema-discovery endpoints. It speaks
JSON on stdout and structured JSON errors on stderr, uses stable exit codes,
and exposes schema and choice lookups so an agent can discover a table's shape
on demand. Assume zero prior ServiceNow knowledge: every operation below is
runnable end-to-end from a cold start after `sn init`.

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
| `aggregate` | Stats object with count/sum/avg/min/max and optional groupby results |
| `app install` / `publish` / `rollback` | Progress object with `status_label` and `links.progress.id` |
| `updateset create` | New Update Set record object |
| `updateset retrieve` / `preview` / `commit` / `back-out` | Progress object |
| `updateset commit-multiple` | Array of progress objects |
| `atf run` | Progress object with `links.progress.id` |
| `atf results` | Test suite result object |
| `progress` | Progress status object (`status_label`, `percent_complete`, `status_message`) |
| `scores list` | JSON array of scorecard objects |
| `scores favorite` / `unfavorite` | Updated scorecard object |
| `change list` | JSON array of change records |
| `change get` / `create` / `update` | Single change record object |
| `change delete` | No stdout (empty) |
| `change nextstates` / `schedule` / `models` / `templates` | JSON object or array |
| `change task list` / `ci list` / `conflict get` | JSON array |
| `attachment list` | JSON array of attachment metadata |
| `attachment get` / `upload` | Single attachment metadata object |
| `attachment download` | Binary content (raw bytes to stdout or file via `--output`) |
| `attachment delete` | No stdout (empty) |
| `cmdb list` | JSON array of CI records |
| `cmdb get` / `create` / `update` / `replace` | Single CI record with relations |
| `cmdb meta` | Class metadata object |
| `import create` / `bulk` | Import result array (includes transform results) |
| `import get` | Single import set record |
| `catalog list` / `items` / `categories` | JSON array |
| `catalog get` / `item` / `category` / `item-variables` | Single object |
| `catalog order` / `checkout` / `submit-order` | Order/request result object |
| `catalog cart` / `wishlist` | Cart/wishlist object |
| `identify create-update` / `query` | Reconciliation result with items array |

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
  2) # API error — inspect JSON on stderr, decide whether to surface or handle
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
2. `default_profile` in `config.toml`
3. Literal `default` profile

```bash
sn --profile prod table list incident --limit 5
```

**Per-invocation CLI overrides** (bypass profile entirely; useful for CI or
ephemeral agent sessions):

```bash
sn --instance-override https://dev12345.service-now.com \
   --username api.user \
   --password 's3cr3t' \
   table list incident --limit 1
```

Precedence for credential fields: CLI flag > profile file. If
`--instance-override` is set but username/password are not, the CLI falls
back to the active profile for the missing pieces. The `--username` and
`--password` flags are hidden from `--help` (visible in `ps` output and
shell history); prefer `sn init` + `--profile` for everyday use. There are
deliberately no env vars for credentials — the previous `SN_INSTANCE` /
`SN_USERNAME` / `SN_PASSWORD` overrides silently hijacked stored profile
credentials and have been removed.

**Proxy and TLS overrides** (useful when the agent runs behind a corporate proxy):

```bash
SN_PROXY=http://proxy.corp:8080 sn table list incident
SN_INSECURE=1 sn table list incident              # skip TLS cert verification
sn --proxy socks5://proxy:1080 table list incident # SOCKS5
sn --ca-cert /path/to/ca.pem table list incident   # custom CA
sn --no-proxy table list incident                  # bypass configured proxy
```

Settings can also be stored per-profile in `config.toml` (`proxy`, `no_proxy`, `insecure`, `ca_cert`, `proxy_ca_cert`) and `credentials.toml` (`proxy_username`, `proxy_password`). Precedence: CLI flag > env var > profile config.

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
sn table list incident --setlimit 5
```

(`--limit` is accepted as an alias for `--setlimit`, matching the
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
  --setlimit 10
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
`--setlimit` controls the per-API-call batch size (default 1000), and
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
| `--setlimit <N>` | `sysparm_limit` | list | Max records returned; default 1000. Aliases: `--limit`, `--page-size`, `--sysparm-limit`. |
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
| `-v` / `-vv` / `-vvv` | (CLI only) | all | Debug logging to stderr |

## Pagination patterns

```bash
# Manual single-page read — page 3 at size 50
sn table list incident --setlimit 50 --offset 100 --query "active=true"
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
sn table list incident --query "active=true" --all --setlimit 5000
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

## Aggregate queries

`sn aggregate` calls `GET /api/now/stats/{table}` — a single round trip that returns counts, sums, averages, mins, and maxes. Use this instead of paginating and counting client-side.

```bash
# Count all active incidents, grouped by state
sn aggregate incident --count --group-by state --display-value true
```
```json
{
  "stats": {
    "count": "142",
    "groupby_fields": [
      {"field": "state", "value": "In Progress", "count": "87"},
      {"field": "state", "value": "New",          "count": "55"}
    ]
  }
}
```

```bash
# Average reassignment count on active incidents
sn aggregate incident --avg-fields reassignment_count --query "active=true"
```
```json
{
  "stats": {
    "avg": {"reassignment_count": "1.83"}
  }
}
```

```bash
# Multiple aggregations in one call — sum, min, max
sn aggregate incident \
  --sum-fields reassignment_count \
  --min-fields priority \
  --max-fields priority
```
```json
{
  "stats": {
    "sum": {"reassignment_count": "260"},
    "min": {"priority": "1"},
    "max": {"priority": "4"}
  }
}
```

Key flags for `aggregate`:

| Flag | Effect |
|---|---|
| `--count` | Include a total record count |
| `--group-by <csv>` | Group results by one or more fields |
| `--avg-fields <csv>` | Average these numeric fields |
| `--sum-fields <csv>` | Sum these numeric fields |
| `--min-fields <csv>` | Minimum value of these fields |
| `--max-fields <csv>` | Maximum value of these fields |
| `--query <EQ>` | Encoded query filter (same syntax as `table list`) |
| `--having <expr>` | Post-aggregation HAVING filter |
| `--order-by <csv>` | Sort the grouped results |
| `--display-value true\|false\|all` | Resolve choice/reference display labels |

## CICD operations

### The async pattern

`app`, `updateset`, and `atf run` are asynchronous. The recommended approach for agents is `--wait`: it blocks the command until the operation succeeds or fails (polling `GET /api/sn_cicd/progress/{id}` every 2 seconds internally) and then emits the final progress result. This eliminates the polling loop entirely.

**With `--wait` (preferred for agents):**

```bash
# Blocks until the install completes or fails, then prints the final progress result
result=$(sn app install --scope x_myapp --version 1.2.0 --wait)
status=$(echo "$result" | jq -r '.status_label')
if [ "$status" != "Complete" ]; then
  echo "Install failed: $(echo "$result" | jq -r '.status_message')" >&2
  exit 1
fi
```

The final progress result shape emitted by `--wait`:

```json
{
  "status": "2",
  "status_label": "Complete",
  "status_message": "Application installed successfully",
  "status_detail": "Install complete",
  "percent_complete": 100
}
```

`status` codes: `0` = Pending, `1` = Running, `2` = Successful, `3` = Failed, `4` = Cancelled.

**Without `--wait` (manual polling — use for already-running operations):**

Every triggering command returns a progress object immediately:

```json
{
  "links": {
    "progress": {
      "id": "9e8d7c6b5a4f3e2d1c0b",
      "href": "https://dev12345.service-now.com/api/now/progress/9e8d7c6b5a4f3e2d1c0b"
    }
  },
  "status": "0",
  "status_label": "Pending",
  "status_detail": "Pending",
  "status_message": ""
}
```

Poll `sn progress <progress_id>` until `status_label` is `"Complete"` or `"Failed"`:

```bash
progress_id="9e8d7c6b5a4f3e2d1c0b"
while true; do
  result=$(sn progress "$progress_id")
  status=$(echo "$result" | jq -r '.status_label')
  echo "Status: $status"
  case "$status" in
    Complete) echo "Done."; break ;;
    Failed)   echo "Failed: $(echo "$result" | jq -r '.status_message')" >&2; exit 1 ;;
    *)        sleep 5 ;;
  esac
done
```

### App lifecycle

Install, publish, and roll back scoped applications from the ServiceNow App Repository. All three are identified by `--scope` (e.g. `x_acme_myapp`) or `--sys-id`.

```bash
# Install a specific version and wait for completion
sn app install --scope x_myapp --version 1.2.0 --wait
```
```json
{
  "status": "2",
  "status_label": "Complete",
  "status_message": "Application installed successfully",
  "percent_complete": 100
}
```

```bash
# Publish to the app repo with release notes
sn app publish --scope x_myapp --version 1.3.0 --dev-notes "Fix null pointer in approval flow" --wait

# Roll back to a previous version (--version is required)
sn app rollback --scope x_myapp --version 1.1.0 --wait
```

### Update Set lifecycle

Update Sets move configuration changes between ServiceNow instances. The typical flow is: create → (make changes on the instance) → retrieve → preview → commit.

```bash
# Create a new Update Set on this instance
sn updateset create --name "Sprint 42 changes" --description "ITSM form tweaks"
```
```json
{
  "sys_id": "a1b2c3d4e5f6",
  "name": "Sprint 42 changes",
  "state": "in progress"
}
```

```bash
# Retrieve a remote Update Set (pulls it from another instance)
sn updateset retrieve --update-set-id <remote_sys_id> --auto-preview
```

`--auto-preview` kicks off preview automatically after retrieval (saves a round trip). Use `--wait` on each step to block until it completes before proceeding.

```bash
# Preview a retrieved Update Set (checks for collisions/errors)
sn updateset preview <remote_update_set_id> --wait

# Commit after a clean preview
sn updateset commit <remote_update_set_id> --wait

# Commit multiple Update Sets in one call
sn updateset commit-multiple --ids id1,id2,id3

# Undo an applied Update Set
sn updateset back-out --update-set-id <sys_id> --wait
```

`back-out` also accepts `--rollback-installs` to undo any app installs that were part of the Update Set.

### ATF test execution

Run Automated Test Framework suites by name or sys_id:

```bash
sn atf run --suite-name "Regression Suite" --wait
```
```json
{
  "status": "2",
  "status_label": "Complete",
  "status_message": "Test suite completed",
  "percent_complete": 100
}
```

Once the run completes, fetch the detailed results:

```bash
# Retrieve test results by the result sys_id (available after completion)
sn atf results <result_id>
```
```json
{
  "sys_id": "result123",
  "name": "Regression Suite",
  "status": "success",
  "test_suite_name": "Regression Suite",
  "duration": "00:02:14",
  "tests_total": 38,
  "tests_passed": 38,
  "tests_failed": 0
}
```

Optional ATF run flags: `--suite-id <sys_id>` (use instead of `--suite-name`), `--browser-name chrome`, `--run-in-cloud`, `--performance-run`.

## Performance Analytics scorecards

`sn scores list` queries `GET /api/now/pa/scorecards`. Results are paginated; use `--per-page` and `--page` to walk through them.

```bash
# List first 20 scorecards sorted by value descending
sn scores list --per-page 20 --sort-by VALUE --sort-dir DESC
```
```json
[
  {
    "uuid": "indicator-uuid-1",
    "name": "MTTR - Incidents",
    "value": 4.2,
    "target": 6.0,
    "direction": "minimize",
    "frequency": "Daily",
    "date": "2026-04-22"
  },
  {
    "uuid": "indicator-uuid-2",
    "name": "First Contact Resolution Rate",
    "value": 78.5,
    "target": 80.0,
    "direction": "maximize",
    "frequency": "Weekly",
    "date": "2026-04-20"
  }
]
```

```bash
# Fetch historical scores for a specific indicator
sn scores list \
  --uuid <indicator_id> \
  --include-scores \
  --from 2026-01-01 \
  --to 2026-04-01
```

Useful filters:

| Flag | Effect |
|---|---|
| `--uuid <csv>` | Filter to specific indicator UUID(s) |
| `--favorites` | Return only favorited scorecards |
| `--key` | Return only key indicators |
| `--target` | Return only indicators with a target set |
| `--contains <csv>` | Substring match on indicator name |
| `--sort-by VALUE\|CHANGE\|CHANGEPERC\|GAP\|NAME\|DATE\|…` | Sort field |
| `--sort-dir ASC\|DESC` | Sort direction |
| `--include-scores` | Attach historical score data to each result |
| `--from` / `--to` | ISO-8601 date range for `--include-scores` |
| `--include-available-breakdowns` | List breakdowns available for each indicator |
| `--include-realtime` | Attach real-time score data |

```bash
# Mark an indicator as a favorite
sn scores favorite <uuid>

# Remove from favorites
sn scores unfavorite <uuid>
```

## Change Management

`sn change` wraps the Change Management API (`/api/sn_chg_rest/change`). Changes have three types: **normal**, **emergency**, and **standard**. Use `--type` to target a type-specific endpoint; omit for the generic endpoint.

### CRUD

```bash
# List normal changes, filtered
sn change list --type normal --query "state=1^priority<=2" --setlimit 10
```
```json
[
  {"sys_id": "chg001", "number": "CHG0010001", "type": "normal", "state": "1"}
]
```

```bash
# Get a specific change
sn change get chg001 --type normal
```

```bash
# Create a normal change
sn change create --type normal \
  --field short_description="DB migration" \
  --field category=software
```

```bash
# Create a standard change from a template
sn change create --type standard --template <template_sys_id>
```

Standard changes **require** `--template`. Emergency changes need only `--type emergency`.

```bash
# Update (PATCH)
sn change update chg001 --field state=2

# Delete
sn change delete chg001
```

### Workflow operations

```bash
# Valid state transitions — call this before updating state to avoid errors
sn change nextstates chg001
```
```json
[
  {"value": "-4", "label": "Scheduled"},
  {"value": "3", "label": "Implement"}
]
```

```bash
# Update approvals
sn change approvals chg001 --field approval="approved"

# Update risk assessment
sn change risk chg001 --data '{"risk_value": "moderate"}'

# View schedule
sn change schedule chg001

# Browse models and standard templates
sn change models
sn change templates
```

### Sub-resources (tasks, CIs, conflicts)

```bash
# Change tasks
sn change task list <change_sys_id>
sn change task create <change_sys_id> --field short_description="Pre-check"
sn change task update <change_sys_id> <task_sys_id> --field state=2
sn change task delete <change_sys_id> <task_sys_id>

# CIs affected by a change
sn change ci list <change_sys_id>
sn change ci add <change_sys_id> --data '{"cmdb_ci_sys_id": "<ci_id>"}'

# Conflicts
sn change conflict get <sys_id>
sn change conflict add <sys_id> --data '{"..."}'
sn change conflict remove <sys_id>
```

## Attachments

`sn attachment` wraps the Attachment API (`/api/now/attachment`). Supports binary upload/download for any ServiceNow record.

```bash
# List attachments on incidents
sn attachment list --query "table_name=incident" --setlimit 20
```
```json
[
  {
    "sys_id": "att001",
    "file_name": "screenshot.png",
    "table_name": "incident",
    "table_sys_id": "inc001",
    "size_bytes": "245760",
    "content_type": "image/png"
  }
]
```

```bash
# Get metadata for a specific attachment
sn attachment get att001

# Upload a file to a record
sn attachment upload --table incident --record <record_sys_id> --file ./report.pdf
```
```json
{
  "sys_id": "att002",
  "file_name": "report.pdf",
  "table_name": "incident",
  "table_sys_id": "inc001",
  "size_bytes": "102400",
  "content_type": "application/pdf"
}
```

Content type is auto-detected from file extension; override with `--content-type`.

```bash
# Download to a file
sn attachment download att001 --output ./downloaded.png
```
```json
{"path": "./downloaded.png", "size": 245760}
```

```bash
# Download to stdout (pipe to another tool)
sn attachment download att001 > file.bin

# Delete
sn attachment delete att001
```

## CMDB

`sn cmdb` combines the CMDB Instance API (`/api/now/cmdb/instance/{class}`) and Meta API (`/api/now/cmdb/meta/{class}`). The CMDB class name (e.g. `cmdb_ci_server`, `cmdb_ci_linux_server`) is always the first positional argument.

```bash
# List CIs of a class
sn cmdb list cmdb_ci_server --query "operational_status=1" --setlimit 10
```
```json
[
  {"sys_id": "ci001", "name": "web-server-01", "ip_address": "10.0.1.50"}
]
```

```bash
# Get a CI (includes relations)
sn cmdb get cmdb_ci_server ci001

# Create a CI
sn cmdb create cmdb_ci_server \
  --field name=web-server-02 \
  --field ip_address=10.0.1.51

# Update (PATCH)
sn cmdb update cmdb_ci_server ci001 --field operational_status=2

# Replace (PUT — full overwrite)
sn cmdb replace cmdb_ci_server ci001 --data @ci-full.json

# Class metadata (schema, available fields)
sn cmdb meta cmdb_ci_server
```

### Relations

```bash
# Create a relation between CIs
sn cmdb relation add cmdb_ci_server ci001 \
  --data '{"type": "<rel_type_sys_id>", "target": "<target_ci_sys_id>"}'

# Delete a relation
sn cmdb relation delete cmdb_ci_server ci001 <rel_sys_id>
```

## Import Sets

`sn import` wraps the Import Set API (`/api/now/import/{stagingTable}`). Used for loading data through transform maps.

```bash
# Insert a single record into a staging table
sn import create u_my_staging_table --field u_name="Server-01" --field u_ip="10.0.1.1"
```
```json
[
  {
    "sys_id": "imp001",
    "table": "cmdb_ci_server",
    "display_name": "web-server-01",
    "status": "inserted",
    "sys_import_set": "ISET001"
  }
]
```

The result includes transform map outcomes — `status` will be `inserted`, `updated`, `skipped`, or `error`.

```bash
# Bulk insert multiple records
sn import bulk u_my_staging_table --data '[
  {"u_name": "Server-01", "u_ip": "10.0.1.1"},
  {"u_name": "Server-02", "u_ip": "10.0.1.2"}
]'

# Or from file
sn import bulk u_my_staging_table --data @records.json

# Retrieve an import set record
sn import get u_my_staging_table imp001
```

## Service Catalog

`sn catalog` wraps the Service Catalog API (`/api/sn_sc/servicecatalog`). Supports browsing, searching, cart management, and ordering.

### Browsing

```bash
# List catalogs
sn catalog list
sn catalog list --text "IT"   # search by keyword

# Get a specific catalog
sn catalog get <catalog_sys_id>

# List categories in a catalog
sn catalog categories <catalog_sys_id>
sn catalog categories <catalog_sys_id> --top-level-only

# Get a category
sn catalog category <category_sys_id>

# Search items
sn catalog items --text "laptop" --catalog <catalog_id>
sn catalog items --category <category_id> --setlimit 20

# Get item details and required variables (form fields)
sn catalog item <item_sys_id>
sn catalog item-variables <item_sys_id>
```

### Ordering

Two patterns: **order now** (immediate, bypasses cart) or **cart workflow**.

```bash
# Order immediately
sn catalog order <item_sys_id> --data '{"sysparm_quantity": "1", "variables": {"urgency": "high"}}'
```
```json
{
  "request_number": "REQ0010001",
  "request_id": "req001"
}
```

```bash
# Cart workflow
sn catalog add-to-cart <item_sys_id> --data '{"sysparm_quantity": "1"}'
sn catalog cart                           # view cart
sn catalog cart-update <cart_item_id> --field quantity=2
sn catalog cart-remove <cart_item_id>     # remove one item
sn catalog cart-empty <cart_sys_id>       # empty entire cart
sn catalog checkout                       # validate and proceed
sn catalog submit-order                   # place the order

# Wishlist
sn catalog wishlist
```

**Agent tip:** Call `sn catalog item-variables <id>` before ordering to discover required form fields. Variables with `mandatory: true` must be included in the order payload.

## Identification & Reconciliation

`sn identify` wraps the Identification and Reconciliation API (`/api/now/identifyreconcile`). Used for programmatic CI lifecycle management through ServiceNow's reconciliation engine.

All operations are POST-only and take `--data` for the items payload.

```bash
# Create or update a CI (reconciliation decides based on identification rules)
sn identify create-update --data '{
  "items": [{
    "className": "cmdb_ci_server",
    "values": {"name": "web-01", "ip_address": "10.0.1.1"}
  }]
}'
```
```json
{
  "items": [
    {
      "sysId": "ci001",
      "className": "cmdb_ci_server",
      "operation": "INSERT",
      "identifierEntrySysId": "id001"
    }
  ]
}
```

```bash
# Query / identify a CI without modifying it
sn identify query --data '{
  "items": [{
    "className": "cmdb_ci_server",
    "values": {"name": "web-01"}
  }]
}'
```

### Enhanced variants

The enhanced endpoints support partial payloads and partial commits:

```bash
sn identify create-update-enhanced \
  --data @payload.json \
  --data-source "my_discovery" \
  --options "partial_payload:true,partial_commits:true"

sn identify query-enhanced --data @query.json
```

`--data-source` tags the operation for audit trail purposes. `--options` accepts comma-separated `key:value` pairs.

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
    echo "network failure — check connectivity, then retry manually" >&2
    ;;
  4)
    echo "auth failed — re-run 'sn init' or pass --username/--password" >&2
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
| TLS handshake failure | 3 | — | Usually a typo in the profile's instance URL or a proxy issue |
| DNS / connection refused | 3 | — | Network; check connectivity |
| Timeout | 3 | — | Network timeout |
| Rate limited | 2 | 429 | Back off and retry manually |
| Internal server error | 2 | 5xx | ServiceNow error |
| Invalid proxy URL | 1 | — | Bad `--proxy` URL format |
| Proxy connection failed | 3 | — | Proxy unreachable; check `SN_PROXY` |
| Bad CA certificate | 1 | — | File missing or invalid PEM format |

Distinguishing 403-from-ACL vs 401-from-auth matters: code 4 says "your
credentials are wrong," code 2 with `status_code: 403` says "the user is
authenticated but not allowed to do that."

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

## Claude Code plugin

`sn` ships as a Claude Code plugin. When installed, the plugin pre-approves
`Bash(sn *)` so the agent can run any `sn` command without per-call
permission prompts.

**For projects that clone the repo:** the local skill at `.claude/skills/sn.md`
is loaded automatically. Invoke with `/sn`.

**For standalone use:** install the plugin so it's available across all
projects:

```bash
claude plugin install --dir /path/to/sn
```

The plugin provides schema discovery, output contract, and usage examples
for every command group so the agent can operate ServiceNow without prior
knowledge of the API.

## Building tools/MCP servers on top of `sn`

`sn introspect` emits the complete command tree — every subcommand,
flag, value type, and help text — as machine-readable JSON. This is the
canonical way to auto-generate MCP tool definitions, OpenAI function-call
schemas, or any other structured wrapper.

```bash
sn introspect | jq '.commands[] | {name, summary}'
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
- **Pulling way more than you need.** `sn` defaults `--setlimit` to
  1000, which is much friendlier than SN's native 10000. For quick
  exploration, drop it lower (`--setlimit 5`). For bulk work, prefer
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
  [--setlimit N (default 1000; alias --limit)] [--offset N]
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

sn change list [--type normal|emergency|standard] [--query EQ] [--fields CSV]
               [--setlimit N] [--offset N] [--display-value ...]
sn change get SYS_ID [--type ...] [--fields CSV] [--display-value ...]
sn change create [--type normal|emergency|standard] [--template ID]
                 (--data ...|--field K=V ...)
sn change update SYS_ID [--type ...] (--data ...|--field K=V ...)
sn change delete SYS_ID [--type ...]
sn change nextstates SYS_ID
sn change approvals SYS_ID (--data ...|--field K=V ...)
sn change risk SYS_ID (--data ...|--field K=V ...)
sn change schedule SYS_ID
sn change models [SYS_ID]
sn change templates [SYS_ID]
sn change task list CHANGE_SYS_ID [--fields CSV] [--setlimit N]
sn change task get CHANGE_SYS_ID TASK_SYS_ID
sn change task create CHANGE_SYS_ID (--data ...|--field K=V ...)
sn change task update CHANGE_SYS_ID TASK_SYS_ID (--data ...|--field K=V ...)
sn change task delete CHANGE_SYS_ID TASK_SYS_ID
sn change ci list CHANGE_SYS_ID
sn change ci add CHANGE_SYS_ID (--data ...|--field K=V ...)
sn change conflict get SYS_ID
sn change conflict add SYS_ID (--data ...|--field K=V ...)
sn change conflict remove SYS_ID

sn attachment list [--query EQ] [--setlimit N] [--offset N]
sn attachment get SYS_ID
sn attachment upload --table TABLE --record SYS_ID --file PATH
                     [--file-name NAME] [--content-type MIME]
sn attachment download SYS_ID [--output PATH]
sn attachment delete SYS_ID

sn cmdb list CLASS [--query EQ] [--setlimit N] [--offset N]
sn cmdb get CLASS SYS_ID
sn cmdb create CLASS (--data ...|--field K=V ...)
sn cmdb update CLASS SYS_ID (--data ...|--field K=V ...)
sn cmdb replace CLASS SYS_ID (--data ...|--field K=V ...)
sn cmdb meta CLASS
sn cmdb relation add CLASS SYS_ID (--data ...|--field K=V ...)
sn cmdb relation delete CLASS SYS_ID REL_SYS_ID

sn import create STAGING_TABLE (--data ...|--field K=V ...)
sn import bulk STAGING_TABLE --data JSON|@FILE|@-
sn import get STAGING_TABLE SYS_ID

sn catalog list [--text TEXT]
sn catalog get SYS_ID
sn catalog categories CATALOG_SYS_ID [--setlimit N] [--top-level-only]
sn catalog category SYS_ID
sn catalog items [--text TEXT] [--category ID] [--catalog ID]
                 [--item-type TYPE] [--setlimit N]
sn catalog item SYS_ID
sn catalog item-variables SYS_ID
sn catalog order ITEM_SYS_ID (--data ...|--field K=V ...)
sn catalog add-to-cart ITEM_SYS_ID (--data ...|--field K=V ...)
sn catalog cart
sn catalog cart-update CART_ITEM_ID (--data ...|--field K=V ...)
sn catalog cart-remove CART_ITEM_ID
sn catalog cart-empty CART_SYS_ID
sn catalog checkout
sn catalog submit-order
sn catalog wishlist

sn identify create-update (--data ...|--field K=V ...) [--data-source NAME]
sn identify query (--data ...|--field K=V ...) [--data-source NAME]
sn identify create-update-enhanced (--data ...|--field K=V ...)
                                   [--data-source NAME] [--options KEY:VAL,...]
sn identify query-enhanced (--data ...|--field K=V ...)
                           [--data-source NAME] [--options KEY:VAL,...]

sn aggregate TABLE [--count] [--avg-fields CSV] [--sum-fields CSV]
                   [--min-fields CSV] [--max-fields CSV]
                   [--group-by CSV] [--query EQ] [--having EXPR]
                   [--order-by CSV] [--display-value ...]

sn app install [--scope S|--sys-id ID] [--version V] [--wait]
sn app publish [--scope S|--sys-id ID] [--version V] [--dev-notes T] [--wait]
sn app rollback [--scope S|--sys-id ID] --version V [--wait]

sn updateset create --name NAME [--description T] [--scope S]
sn updateset retrieve --update-set-id ID [--auto-preview] [--wait]
sn updateset preview REMOTE_ID [--wait]
sn updateset commit REMOTE_ID [--wait]
sn updateset commit-multiple --ids CSV [--wait]
sn updateset back-out --update-set-id ID [--rollback-installs] [--wait]

sn atf run [--suite-id ID|--suite-name N] [--wait]
sn atf results RESULT_ID

sn scores list [--uuid CSV] [--per-page N] [--page N] [--sort-by ...] [--sort-dir ...]
               [--include-scores] [--from DATE] [--to DATE] [--favorites] [--key]
sn scores favorite UUID
sn scores unfavorite UUID

sn progress PROGRESS_ID
sn introspect

Global flags (any command):
  --profile NAME          select credential profile
  --instance-override URL override instance URL for this invocation
  --username USER         override profile's username (hidden; for tests/CI)
  --password PASSWORD     override profile's password (hidden; for tests/CI)
  --proxy URL             HTTP/HTTPS/SOCKS5 proxy
  --no-proxy              bypass configured proxy
  --insecure              disable TLS cert verification
  --ca-cert PATH          custom CA certificate
  --proxy-ca-cert PATH    custom proxy CA certificate
  --timeout SECS          request timeout
  -v / -vv / -vvv         verbose logging on stderr

Environment variables (proxy/TLS only — credential and profile env vars
were removed; use --profile or the per-field CLI flags above):
  SN_PROXY           proxy URL
  SN_NO_PROXY        comma-separated bypass hosts
  SN_INSECURE        set to 1 to skip TLS verification
  SN_CA_CERT         path to custom CA cert
  SN_PROXY_CA_CERT   path to proxy CA cert

Exit codes:
  0 success   1 usage/config   2 api (4xx/5xx)   3 network   4 auth (401/403)

Canonical output shapes (stdout):
  list     -> [ {record}, {record}, ... ]
  get      -> {record}
  create   -> {record}
  update   -> {record}
  replace  -> {record}
  delete   -> (empty)
  download -> binary bytes (or JSON metadata with --output)
  schema * -> [ {meta}, ... ]
  --output raw preserves { "result": ... } envelope

Error shape (stderr, all non-zero exits):
  { "error": { "message", "detail", "status_code",
               "transaction_id", "sn_error" } }
```
