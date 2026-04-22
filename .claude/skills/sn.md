---
name: sn
description: Use when the user asks about ServiceNow data, incidents, change requests, problems, CIs, or any SNOW/SN table operations. Also use when user says "sn", "servicenow", or references a ServiceNow instance, CICD operations (app install/publish/rollback, update sets, ATF tests), aggregate statistics on SN tables, or Performance Analytics scorecards.
---

# sn — ServiceNow CLI

Single-binary CLI wrapping the ServiceNow Table API + schema discovery. JSON on stdout, errors on stderr, deterministic exit codes. Installed at `sn`.

## Discovery flow (use this when you don't know the schema)

```bash
sn schema tables --filter incident          # 1. find the table
sn schema columns incident --writable       # 2. learn writable fields
sn schema choices incident state            # 3. get valid values for choice fields
sn table create incident --field short_description="x" --field state=2  # 4. write with confidence
```

## CRUD

```bash
sn table list incident --query "active=true^priority=1" --fields "number,state" --page-size 10
sn table get incident <sys_id>
sn table get incident <sys_id> --display-value all    # human-readable choice/reference values
sn table create incident --field short_description="x" --field urgency=2
sn table create incident --data @body.json             # or --data '{"key":"val"}'
sn table update incident <sys_id> --field state=6      # PATCH (partial)
sn table replace incident <sys_id> --data @full.json   # PUT (full overwrite — dangerous)
sn table delete incident <sys_id> --yes                # --yes required in non-interactive contexts
```

## Pagination

```bash
sn table list incident --all                           # JSONL stream (one record per line)
sn table list incident --all --array                   # single JSON array
sn table list incident --all --max-records 5000        # safety cap
sn table list incident --all | jq -r '.number'         # pipe JSONL through jq
```

## Output contract

- **stdout**: unwrapped JSON (`list` = array, `get`/`create`/`update` = object, `delete` = empty). `--output raw` preserves `{"result": ...}` envelope.
- **stderr**: always JSON errors: `{"error": {"message", "status_code?", "transaction_id?"}}`.
- **Exit codes**: `0` ok, `1` usage/config, `2` API error, `3` network, `4` auth (401/403).
- Branch on exit code first, parse stdout second.

## Setup

```bash
sn init                                    # interactive (prompts for instance, user, password)
sn init --profile prod --instance X --username Y --password Z   # scripted
sn auth test                               # verify credentials
sn --profile prod table list incident      # select profile per command
```

Env overrides: `SN_INSTANCE`, `SN_USERNAME`, `SN_PASSWORD`, `SN_PROFILE`.

## Key flags

Every `sysparm_*` has a friendly name and raw alias (e.g. `--query` / `--sysparm-query`). Run `sn table list --help` for the full set. Notable:

- `--display-value true|false|all` — resolve choice/reference fields to labels
- `--page-size N` (default 1000, alias `--limit`) — records per API call
- `--input-display-value` — set fields by display value on writes
- `-v` / `-vv` / `-vvv` — debug logging to stderr (auth always masked)

## Common mistakes

- Using `replace` (PUT) when you mean `update` (PATCH) — wipes omitted fields.
- Forgetting `--yes` on delete in non-interactive contexts — hangs on stdin.
- Forgetting `--display-value true` — get cryptic numbers instead of labels.
- Mixing `--data` and `--field` — mutually exclusive, exits 1.
- Using `--query` on `get` — only works on `list`; use `list --query "..." --page-size 1` instead.

## Aggregate queries

Server-side statistics without fetching individual records:

```bash
sn aggregate incident --count --group-by state
sn aggregate incident --avg-fields reassignment_count --query "active=true"
sn aggregate incident --sum-fields reassignment_count --min-fields priority --max-fields priority
```

## CICD operations

App, updateset, and atf are async — they return a `progress_id`. Poll with `sn progress <id>`.

```bash
# App lifecycle
sn app install --scope x_myapp --version 1.2.0
sn app publish --scope x_myapp --version 1.3.0 --dev-notes "Bug fixes"
sn app rollback --scope x_myapp --version 1.1.0

# Update sets
sn updateset create --name "Changes" --description "Sprint work"
sn updateset retrieve --update-set-id <id> --auto-preview
sn updateset preview <remote_update_set_id>
sn updateset commit <remote_update_set_id>
sn updateset commit-multiple --ids id1,id2,id3
sn updateset back-out --update-set-id <id>

# ATF testing
sn atf run --suite-name "Regression Suite"   # returns progress_id
sn atf results <result_id>

# Poll any async operation
sn progress <progress_id>
```

## Scorecards

Performance Analytics scorecard queries:

```bash
sn scores list --per-page 20 --sort-by VALUE --sort-dir DESC
sn scores list --uuid <indicator_id> --include-scores --from 2026-01-01 --to 2026-04-01
sn scores favorite <uuid>
sn scores unfavorite <uuid>
```

## Introspection

`sn introspect` dumps the full command tree as JSON (for MCP/tool generation).
