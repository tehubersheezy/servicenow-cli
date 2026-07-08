---
name: sn
description: Use when the user asks about ServiceNow data, incidents, change requests, problems, CIs, attachments, CMDB, service catalog, import sets, or any SNOW/SN operations. Also use when user says "sn", "servicenow", or references a ServiceNow instance, CICD operations (app install/publish/rollback, update sets, ATF tests), aggregate statistics, Performance Analytics scorecards, or CI reconciliation.
allowed-tools: Bash(sn *)
---

# sn — ServiceNow CLI

Single-binary CLI wrapping ServiceNow REST APIs: Table, Change Management, Attachment, CMDB, Import Set, Service Catalog, Identification & Reconciliation, CICD, Aggregate, Performance Analytics, and schema discovery. JSON on stdout, errors on stderr, deterministic exit codes. Installed at `sn`.

## Prerequisites

Install `sn` first: `brew install tehubersheezy/sn/sn` or see https://github.com/tehubersheezy/servicenow-cli

## Setup

```bash
sn init                                    # interactive (prompts for auth method, instance, credentials)
sn init --profile prod --instance X --username Y --password Z   # scripted (basic auth)
sn ping                                    # verify connectivity + credentials
sn --profile prod table list incident      # select profile per command
sn profile list                            # profiles: list / show <name> / use <name> / remove <name>
```

Proxy/TLS env vars: `SN_PROXY`, `SN_NO_PROXY`, `SN_INSECURE`, `SN_CA_CERT`, `SN_PROXY_CA_CERT`. `SN_CONFIG_DIR` overrides the config directory (the folder holding `config.toml`/`credentials.toml`). There are no env vars for credential values or profile selection.

## OAuth / SSO instances

For instances fronted by an external IdP (Okta, Azure AD, ADFS), basic auth cannot work — use OAuth (`auth = "oauth"` on the profile).

Configure the OAuth profile with `sn init` (all config lives there); `sn auth login` is a pure session command that runs the flow and caches tokens.

```bash
sn init --auth oauth --client-id <id>                     # configure authorization_code (SSO, default)
sn init --auth oauth --grant client_credentials --client-id <id> --client-secret <secret>   # non-interactive grant
sn auth login                                             # run the flow: OPENS A BROWSER,
                                                          # blocks up to 300s on human login — not agent-safe
sn auth status                                            # token state (never prints secrets)
sn auth refresh                                           # force a refresh
sn auth logout                                            # drop cached tokens
```

Data commands never launch a browser: tokens refresh transparently, and an expired/missing token fails with exit 4 and `"run \`sn auth login\`"` in the error message. Recovery loop for agents: on exit 4 with an OAuth profile, tell the human to run `sn auth login` (or use the `client_credentials` grant yourself if a secret is configured).

## Discovery flow (use this when you don't know the schema)

```bash
sn schema tables --filter incident          # 1. find the table
sn schema columns incident --writable       # 2. learn writable fields
sn schema choices incident state            # 3. get valid values for choice fields
sn table create incident --field short_description="x" --field state=2  # 4. write with confidence
```

## Table CRUD

```bash
sn table list incident --query "active=true^priority=1" --fields "number,state" --setlimit 10
sn table get incident <sys_id>
sn table get incident <sys_id> --display-value all    # human-readable choice/reference values
sn table create incident --field short_description="x" --field urgency=2
sn table create incident --data @body.json             # or --data '{"key":"val"}'
sn table update incident <sys_id> --field state=6      # PATCH (partial)
sn table replace incident <sys_id> --data @full.json   # PUT (full overwrite — dangerous)
sn table delete incident <sys_id> --yes                # without --yes on non-TTY stdin: clean JSON error, exit 1
```

## Encoded query cheatsheet (--query)

ServiceNow encoded queries are the most error-prone part of any invocation:

- `^` = AND, `^OR` = OR, `^NQ` = new top-level query: `active=true^priority=1^ORpriority=2`
- Operators: `=`, `!=`, `>`, `>=`, `<`, `<=`, `IN` (`stateIN1,2,3`), `LIKE` (`short_descriptionLIKEdisk`), `STARTSWITH`, `ENDSWITH`
- Empty checks: `assigned_toISEMPTY`, `assigned_toISNOTEMPTY`
- Dot-walk references: `caller_id.name=Abel Tuter`, `cmdb_ci.location.city=Cary`
- Dates: `sys_created_on>javascript:gs.daysAgoStart(7)`, `opened_atONToday@javascript:gs.beginningOfToday()@javascript:gs.endOfToday()`
- Sort inside the query: `ORDERBYDESCsys_created_on`, `ORDERBYnumber`
- Values are raw (no quotes); spaces are fine: `short_descriptionLIKEdisk full`

## Pagination

```bash
sn table list incident --all                           # JSONL stream (one record per line)
sn table list incident --all --array                   # single JSON array
sn table list incident --all --max-records 5000        # safety cap (default 100000)
sn table list incident --all | jq -r '.number'         # pipe JSONL through jq
```

## Output contract

- **stdout**: unwrapped JSON (`list` = array, `get`/`create`/`update` = object, `delete` = empty). `--output raw` preserves `{"result": ...}` envelope.
- **stderr** (when piped): JSON errors `{"error": {"message", "detail?", "status_code?", "transaction_id?", "sn_error?"}}`. `sn_error` carries ServiceNow's own error body — read it to self-correct (bad field name, missing role, etc.). On a TTY, usage errors print clap's human text instead.
- **Exit codes**: `0` ok, `1` usage/config (including bad flags), `2` API error, `3` network/transport (including `--wait-timeout` expiry), `4` auth (401/403 — for OAuth profiles, prompt a human to `sn auth login`).
- Branch on exit code first, parse stdout second.

## Key flags

Every `sysparm_*` has a friendly name and raw alias (e.g. `--query` / `--sysparm-query`). Notable:

- `--display-value true|false|all` — resolve choice/reference fields to labels
- `--setlimit N` (default 1000, aliases `--limit`, `--page-size`) — max records returned
- `--input-display-value` — set fields by display value on writes
- `--timeout SECS` — per-request HTTP timeout (default 30)
- `--pretty` / `--compact` — force JSON formatting (default: pretty on TTY, compact when piped)
- `-v` / `-vv` / `-vvv` — requests / +headers / +bodies logged to stderr (auth always masked); `-V` = version

## Proxy and TLS

```bash
sn --proxy http://proxy:8080 table list incident       # HTTP proxy (also socks5://)
sn --insecure table list incident                      # skip TLS cert verification
sn --ca-cert /path/to/ca.pem table list incident       # custom CA
sn --no-proxy table list incident                      # bypass configured proxy
```

Env vars: `SN_PROXY`, `SN_NO_PROXY`, `SN_INSECURE=1`, `SN_CA_CERT`, `SN_PROXY_CA_CERT`. Per-profile in `config.toml`: `proxy`, `no_proxy`, `insecure`, `ca_cert`, `proxy_ca_cert`; proxy auth in `credentials.toml`. Precedence: CLI flag > env var > profile config.

## Aggregate queries

Server-side statistics without fetching individual records:

```bash
sn aggregate incident --count --group-by state
sn aggregate incident --avg-fields reassignment_count --query "active=true"
sn aggregate incident --sum-fields reassignment_count --min-fields priority --max-fields priority
```

## Change Management

```bash
sn change list --type normal --query "state=1" --setlimit 10
sn change get <sys_id> --type normal
sn change create --type normal --field short_description="DB migration"
sn change create --type standard --template <template_id>   # standard requires --template
sn change update <sys_id> --field state=2
sn change delete <sys_id>                                   # deletes immediately — no --yes guard
sn change nextstates <sys_id>                               # valid state transitions
sn change approvals <sys_id> --field approval="approved"
sn change risk <sys_id> --data '{"risk_value":"moderate"}'
sn change schedule <sys_id>
sn change models                                            # list change models
sn change templates                                         # list standard templates
sn change task list <change_sys_id>
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
sn attachment download <sys_id> --output ./file.pdf         # here --output is the FILE PATH, or stdout without it
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
sn import bulk u_staging_table --data '[{"u_name":"A"},{"u_name":"B"}]'   # array is auto-wrapped as {"records": [...]}
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

## CICD operations

App, updateset, and atf are async. Use `--wait` to block until done (preferred); add `--wait-timeout <SECS>` so a stalled job can't hang you forever (exit 3 on expiry). Poll manually with `sn progress <id>` for already-running operations.

```bash
sn app install --scope x_myapp --version 1.2.0 --wait --wait-timeout 900
sn app publish --scope x_myapp --version 1.3.0 --dev-notes "Bug fixes" --wait
sn app rollback --scope x_myapp --version 1.1.0 --wait
sn updateset create --name "Changes" --description "Sprint work"
sn updateset retrieve --update-set-id <id> --auto-preview
sn updateset preview <remote_update_set_id> --wait
sn updateset commit <remote_update_set_id> --wait
sn updateset commit-multiple --ids id1,id2,id3
sn updateset back-out --update-set-id <id> --wait
sn atf run --suite-name "Regression Suite" --wait --wait-timeout 1800
sn atf results <result_id>
sn progress <progress_id>
```

## Scorecards

```bash
sn scores list --per-page 20 --sort-by VALUE --sort-dir DESC
sn scores list --uuid <indicator_id> --include-scores --from 2026-01-01 --to 2026-04-01
sn scores favorite <uuid>
sn scores unfavorite <uuid>
```

## Common mistakes

- Using `replace` (PUT) when you mean `update` (PATCH) — wipes omitted fields
- Omitting `--yes` on `table delete` in non-interactive contexts — clean JSON usage error, exit 1 (it does not hang)
- Forgetting `--display-value true` — get cryptic numbers instead of labels
- Mixing `--data` and `--field` — mutually exclusive, exits 1
- Using `--query` on `get` — only works on `list`; use `list --query "..." --setlimit 1`
- Standard changes require `--template` — will error without it
- Running `sn auth login` (authorization_code) non-interactively — it blocks on a browser; use `--grant client_credentials` or ask the human

## Inspect / connectivity

```bash
sn ping                                   # auth + latency + ServiceNow build version, JSON output
sn user me                                # the currently authenticated user record
```

## Open a record in the web UI

```bash
sn open incident <sys_id>                 # launches the form in the default browser
sn open incident <sys_id> --print-url     # prints the URL instead (for chaining/scripts)
```

## Raw REST passthrough

For endpoints not yet modeled as typed commands, use `sn raw`:

```bash
sn raw GET /api/now/v2/table/incident -q sysparm_limit=5
sn raw POST /api/now/table/incident --data '{"short_description":"From sn raw"}'
sn raw PATCH /api/now/table/incident/<sys_id> --field state=2
sn raw DELETE /api/now/table/incident/<sys_id>
```

Response is emitted exactly as ServiceNow returns it (no envelope unwrapping). Method is case-insensitive.

## Output modes

The global `--output` flag accepts:

- `default` (omitted) — unwrapped JSON, suitable for piping to `jq`
- `raw` — full SN envelope (`{"result": ...}`) preserved
- `table` — human-readable columnar output (interactive use only, do not pipe)

## Shell completions

```bash
sn completion bash > /usr/local/etc/bash_completion.d/sn
sn completion zsh > "${fpath[1]}/_sn"
sn completion fish > ~/.config/fish/completions/sn.fish
```

Supported: `bash`, `zsh`, `fish`, `powershell`, `elvish`.

## Introspection

`sn introspect` dumps the full command tree as JSON (for MCP/tool generation): args carry `takes_value`, `positional`, `repeatable`, `default_values`, `aliases`, and `possible_values`. Flags report `takes_value: false` — pass them bare (`--all`, not `--all true`).
