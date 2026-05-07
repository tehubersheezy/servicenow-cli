---
name: sn
description: Use when the user asks about ServiceNow data, incidents, change requests, problems, CIs, attachments, CMDB, service catalog, import sets, or any SNOW/SN operations. Also use when user says "sn", "servicenow", or references a ServiceNow instance, CICD operations (app install/publish/rollback, update sets, ATF tests), aggregate statistics, Performance Analytics scorecards, or CI reconciliation.
---

# sn — ServiceNow CLI

Single-binary CLI wrapping ServiceNow REST APIs: Table, Change Management, Attachment, CMDB, Import Set, Service Catalog, Identification & Reconciliation, CICD, Aggregate, Performance Analytics, and schema discovery. JSON on stdout, errors on stderr, deterministic exit codes. Installed at `sn`.

## Discovery flow (use this when you don't know the schema)

```bash
sn schema tables --filter incident          # 1. find the table
sn schema columns incident --writable       # 2. learn writable fields
sn schema choices incident state            # 3. get valid values for choice fields
sn table create incident --field short_description="x" --field state=2  # 4. write with confidence
```

## CRUD

```bash
sn table list incident --query "active=true^priority=1" --fields "number,state" --setlimit 10
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

CLI overrides for one-off use: `--instance-override URL`, `--username USER`, `--password PASSWORD` (the latter two are hidden from `--help`; prefer `sn init` + `--profile`). Proxy/TLS env vars: `SN_PROXY`, `SN_NO_PROXY`, `SN_INSECURE`, `SN_CA_CERT`, `SN_PROXY_CA_CERT`. There are no env vars for credentials or profile selection.

## Key flags

Every `sysparm_*` has a friendly name and raw alias (e.g. `--query` / `--sysparm-query`). Notable:

- `--display-value true|false|all` — resolve choice/reference fields to labels
- `--setlimit N` (default 1000, aliases `--limit`, `--page-size`) — max records returned
- `--input-display-value` — set fields by display value on writes
- `-v` / `-vv` / `-vvv` — debug logging to stderr (auth always masked)

## Proxy and TLS

```bash
sn --proxy http://proxy:8080 table list incident       # HTTP proxy
sn --proxy socks5://proxy:1080 table list incident     # SOCKS5 proxy
sn --insecure table list incident                      # skip TLS cert verification
sn --ca-cert /path/to/ca.pem table list incident       # custom CA
sn --no-proxy table list incident                      # bypass configured proxy
```

Env vars: `SN_PROXY`, `SN_NO_PROXY`, `SN_INSECURE=1`, `SN_CA_CERT`, `SN_PROXY_CA_CERT`.
Per-profile in `config.toml`: `proxy`, `no_proxy`, `insecure`, `ca_cert`, `proxy_ca_cert`. Proxy auth in `credentials.toml`: `proxy_username`, `proxy_password`.
Precedence: CLI flag > env var > profile config.

## Common mistakes

- Using `replace` (PUT) when you mean `update` (PATCH) — wipes omitted fields.
- Forgetting `--yes` on delete in non-interactive contexts — hangs on stdin.
- Forgetting `--display-value true` — get cryptic numbers instead of labels.
- Mixing `--data` and `--field` — mutually exclusive, exits 1.
- Using `--query` on `get` — only works on `list`; use `list --query "..." --setlimit 1`.

## Aggregate queries

Server-side statistics without fetching individual records:

```bash
sn aggregate incident --count --group-by state
sn aggregate incident --avg-fields reassignment_count --query "active=true"
sn aggregate incident --sum-fields reassignment_count --min-fields priority --max-fields priority
```

## CICD operations

App, updateset, and atf are async. Use `--wait` to block until done (preferred); poll manually with `sn progress <id>` for already-running operations.

```bash
# App lifecycle
sn app install --scope x_myapp --version 1.2.0 --wait     # blocks until done
sn app publish --scope x_myapp --version 1.3.0 --dev-notes "Bug fixes" --wait
sn app rollback --scope x_myapp --version 1.1.0 --wait

# Update sets
sn updateset create --name "Changes" --description "Sprint work"
sn updateset retrieve --update-set-id <id> --auto-preview
sn updateset preview <remote_update_set_id> --wait         # blocks until done
sn updateset commit <remote_update_set_id> --wait          # blocks until done
sn updateset commit-multiple --ids id1,id2,id3
sn updateset back-out --update-set-id <id> --wait

# ATF testing
sn atf run --suite-name "Regression Suite" --wait          # blocks until done
sn atf results <result_id>

# Poll an already-running operation
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

## Change Management

```bash
sn change list --type normal --query "state=1" --setlimit 10
sn change get <sys_id> --type normal
sn change create --type normal --field short_description="DB migration"
sn change create --type standard --template <template_id>   # standard requires --template
sn change update <sys_id> --field state=2
sn change delete <sys_id>
sn change nextstates <sys_id>                               # valid state transitions
sn change approvals <sys_id> --field approval="approved"
sn change risk <sys_id> --data '{"risk_value":"moderate"}'
sn change schedule <sys_id>
sn change models                                            # list change models
sn change templates                                         # list standard templates
sn change task list <change_sys_id>                         # tasks, CIs, conflicts
sn change task create <change_sys_id> --field short_description="Pre-check"
sn change ci list <change_sys_id>
sn change ci add <change_sys_id> --data '{"cmdb_ci_sys_id":"<id>"}'
sn change conflict get <sys_id>
```

## Attachments

```bash
sn attachment list --query "table_name=incident"
sn attachment get <sys_id>
sn attachment upload --table incident --record <record_id> --file ./report.pdf
sn attachment download <sys_id> --output ./file.pdf         # or stdout without --output
sn attachment delete <sys_id>
```

## CMDB

```bash
sn cmdb list cmdb_ci_server --query "operational_status=1"
sn cmdb get cmdb_ci_server <sys_id>
sn cmdb create cmdb_ci_server --field name=web-01 --field ip_address=10.0.1.1
sn cmdb update cmdb_ci_server <sys_id> --field operational_status=2
sn cmdb replace cmdb_ci_server <sys_id> --data @ci.json     # PUT full overwrite
sn cmdb meta cmdb_ci_server                                 # class metadata
sn cmdb relation add cmdb_ci_server <sys_id> --data '{"type":"<rel_type>","target":"<ci>"}'
sn cmdb relation delete cmdb_ci_server <sys_id> <rel_sys_id>
```

## Import Sets

```bash
sn import create u_staging_table --field u_name=Server-01
sn import bulk u_staging_table --data '[{"u_name":"A"},{"u_name":"B"}]'
sn import get u_staging_table <sys_id>
```

## Service Catalog

```bash
sn catalog list                                              # list catalogs
sn catalog items --text "laptop"                             # search items
sn catalog item <sys_id>                                     # item details
sn catalog item-variables <sys_id>                           # required form fields
sn catalog order <item_sys_id> --data '{"sysparm_quantity":"1"}'  # order immediately
sn catalog add-to-cart <item_sys_id>                         # cart workflow
sn catalog cart                                              # view cart
sn catalog checkout                                          # validate
sn catalog submit-order                                      # place order
sn catalog wishlist
```

## Identification & Reconciliation

```bash
sn identify create-update --data '{"items":[{"className":"cmdb_ci_server","values":{"name":"web-01"}}]}'
sn identify query --data '{"items":[{"className":"cmdb_ci_server","values":{"name":"web-01"}}]}'
sn identify create-update-enhanced --data @payload.json --data-source "discovery" --options "partial_payload:true"
sn identify query-enhanced --data @query.json
```

## Introspection

`sn introspect` dumps the full command tree as JSON (for MCP/tool generation).
