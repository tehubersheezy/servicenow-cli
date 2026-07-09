---
name: sn
description: Use when the user asks about ServiceNow data, incidents, change requests, problems, CIs, attachments, CMDB, service catalog, import sets, or any SNOW/SN operations. Also use when user says "sn", "servicenow", or references a ServiceNow instance, CICD operations (app install/publish/rollback, update sets, ATF tests), aggregate statistics, Performance Analytics scorecards, or CI reconciliation.
---

# sn — ServiceNow CLI

Single binary wrapping ServiceNow's REST APIs (one per section below). Machine contract: JSON on stdout, JSON errors on stderr, deterministic exit codes, no interactive surprises.

Install: `brew install tehubersheezy/sn/sn` or https://github.com/tehubersheezy/servicenow-cli

## Setup & profiles

```bash
sn init                                          # interactive: prompts auth method, instance, credentials
sn init --profile prod --instance X --username Y --password Z   # scripted basic auth
sn ping                                          # verify connectivity + credentials + build version
sn --profile prod table list incident           # pick profile per command
sn profile list                                  # also: show <name> / use <name> / remove <name>
```

Profile selection: `--profile` > `default_profile` in config > error (no implicit fallback). `SN_CONFIG_DIR` overrides the config dir (`config.toml`/`credentials.toml`). No env var sets credentials or selects a profile (proxy/TLS env vars excepted — see Proxy & TLS).

## OAuth / SSO

For instances behind an external IdP (Okta, Azure AD, ADFS) basic auth can't work — use OAuth (`auth = "oauth"`). Configure via `sn init`; `sn auth login` runs the flow and caches tokens. The default `authorization_code` flow is public PKCE (no secret); `client_credentials` is non-interactive and needs a secret.

```bash
sn init --auth oauth --client-id <id>            # authorization_code (SSO, default)
sn init --auth oauth --grant client_credentials --client-id <id> --client-secret <secret>
sn auth login    # runs flow: OPENS A BROWSER, blocks up to 300s on human login — not agent-safe
sn auth status   # token state (never prints secrets)
sn auth refresh  # force refresh
sn auth logout   # drop cached tokens
```

Data commands never open a browser — tokens refresh transparently. A missing/expired token fails exit 4: mint one via `client_credentials` if a secret is configured, else ask the human to `sn auth login`.

## Output, exit codes & flags

- **stdout**: unwrapped JSON — `list`=array, `get`/`create`/`update`=object, `delete`=empty.
- **stderr** (when piped): `{"error": {"message", "detail?", "status_code?", "transaction_id?", "sn_error?"}}`. `sn_error` carries ServiceNow's own error body — read it to self-correct (bad field, missing role). On a TTY, usage errors print clap's human text instead.
- **Exit codes**: `0` ok · `1` usage/config (incl. bad flags) · `2` API error · `3` network/transport (incl. `--wait-timeout` expiry) · `4` auth (401/403; OAuth → prompt human to `sn auth login`). Branch on exit code first, parse stdout second.
- **`--output default|raw|table`**: `default` = unwrapped JSON (pipe to jq); `raw` = full `{"result": ...}` envelope; `table` = human columns (interactive only, don't pipe).

Flags (global unless noted; every `sysparm_*` has a friendly name + raw `--sysparm-*` alias):

- `--display-value true|false|all` (list/get) — resolve choice/reference fields to labels
- `--setlimit N` (list; default 1000; aliases `--limit`, `--page-size`) — max records
- `--input-display-value` (writes) — set fields by display value
- `--timeout SECS` (default 30) · `--pretty`/`--compact` (default: pretty on TTY, compact when piped)
- `-d`/`-dd`/`-ddd` — log requests / +headers / +bodies to stderr (auth masked); `-v`/`-V` = version
- `--data` and `--field` are mutually exclusive on writes (exit 1)

## Discovery flow (when you don't know the schema)

```bash
sn schema tables --filter incident          # 1. find the table
sn schema columns incident --writable       # 2. writable fields
sn schema choices incident state            # 3. valid values for a choice field
sn table create incident --field short_description="x" --field state=2   # 4. write
```

## Table CRUD

```bash
sn table list incident --query "active=true^priority=1" --fields "number,state" --setlimit 10
sn table get incident <sys_id>
sn table get incident <sys_id> --display-value all     # human-readable choice/reference values
sn table create incident --field short_description="x" --field urgency=2
sn table create incident --data @body.json             # or --data '{"key":"val"}'
sn table update incident <sys_id> --field state=6      # PATCH (partial)
sn table replace incident <sys_id> --data @full.json   # PUT (full overwrite — wipes omitted fields)
sn table delete incident <sys_id> --yes                # --yes required on non-TTY, else clean JSON error exit 1
```

`get` takes no `--query`; filter with `list --query "..." --setlimit 1`.

## Encoded query (--query)

The most error-prone part of any invocation:

- `^`=AND, `^OR`=OR, `^NQ`=new top-level query: `active=true^priority=1^ORpriority=2`
- Operators: `=`, `!=`, `>`, `>=`, `<`, `<=`, `IN` (`stateIN1,2,3`), `LIKE` (`short_descriptionLIKEdisk`), `STARTSWITH`, `ENDSWITH`
- Empty checks: `assigned_toISEMPTY`, `assigned_toISNOTEMPTY`
- Dot-walk references: `caller_id.name=Abel Tuter`, `cmdb_ci.location.city=Cary`
- Dates: `sys_created_on>javascript:gs.daysAgoStart(7)`, `opened_atONToday@javascript:gs.beginningOfToday()@javascript:gs.endOfToday()`
- Sort in-query: `ORDERBYDESCsys_created_on`, `ORDERBYnumber`
- Values are raw (no quotes); spaces are fine: `short_descriptionLIKEdisk full`

## Pagination

```bash
sn table list incident --all                     # JSONL stream (one record/line)
sn table list incident --all --array             # single JSON array
sn table list incident --all --max-records 5000  # safety cap (default 100000)
sn table list incident --all | jq -r '.number'   # pipe JSONL through jq
```

## Aggregate

Server-side stats, no record fetch:

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
sn change models
sn change templates                                        # standard-change templates
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
sn attachment download <sys_id> --output ./file.pdf    # --output is the FILE PATH here (stdout without it)
sn attachment delete <sys_id>
```

## CMDB

```bash
sn cmdb list cmdb_ci_server --query "operational_status=1"
sn cmdb get cmdb_ci_server <sys_id>
sn cmdb create cmdb_ci_server --field name=web-01 --field ip_address=10.0.1.1
sn cmdb update cmdb_ci_server <sys_id> --field operational_status=2
sn cmdb replace cmdb_ci_server <sys_id> --data @ci.json     # PUT full overwrite
sn cmdb meta cmdb_ci_server
sn cmdb relation add cmdb_ci_server <sys_id> --data '{"type":"<rel_type>","target":"<ci>"}'
sn cmdb relation delete cmdb_ci_server <sys_id> <rel_sys_id>
```

## Import Sets

```bash
sn import create u_staging_table --field u_name=Server-01
sn import bulk u_staging_table --data '[{"u_name":"A"},{"u_name":"B"}]'   # array auto-wrapped as {"records":[...]}
sn import get u_staging_table <sys_id>
```

## Service Catalog

Browse → cart → checkout → submit, or `order` directly:

```bash
sn catalog list
sn catalog items --text "laptop"                 # search items
sn catalog item <sys_id>
sn catalog item-variables <sys_id>               # required form fields
sn catalog order <item_sys_id> --data '{"sysparm_quantity":"1"}'   # order immediately (bypass cart)
sn catalog add-to-cart <item_sys_id>
sn catalog cart
sn catalog checkout                              # validate
sn catalog submit-order                          # place order
sn catalog wishlist
```

## Identification & Reconciliation

```bash
sn identify create-update --data '{"items":[{"className":"cmdb_ci_server","values":{"name":"web-01"}}]}'
sn identify query --data '{"items":[{"className":"cmdb_ci_server","values":{"name":"web-01"}}]}'
sn identify create-update-enhanced --data @payload.json --data-source "discovery" --options "partial_payload:true"
sn identify query-enhanced --data @query.json
```

## CICD (app / updateset / atf)

Async — `--wait` blocks until done (add `--wait-timeout <SECS>` to bound a stall, exit 3 on expiry). Poll running ops with `sn progress <id>`.

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

## Scorecards (Performance Analytics)

```bash
sn scores list --per-page 20 --sort-by VALUE --sort-dir DESC
sn scores list --uuid <indicator_id> --include-scores --from 2026-01-01 --to 2026-04-01
sn scores favorite <uuid>
sn scores unfavorite <uuid>
```

## Utility & escape hatches

```bash
sn ping                                    # auth + latency + build version (JSON)
sn user me                                 # currently authenticated user record
sn open incident <sys_id>                  # open the form in the default browser
sn open incident <sys_id> --print-url      # print the URL instead (for scripts)
sn raw GET /api/now/v2/table/incident -q sysparm_limit=5     # REST passthrough for unmodeled endpoints
sn raw POST /api/now/table/incident --data '{"short_description":"x"}'
sn raw PATCH /api/now/table/incident/<sys_id> --field state=2
sn raw DELETE /api/now/table/incident/<sys_id>
sn completion bash|zsh|fish|powershell|elvish   # zsh: > ~/.zsh/completions/_sn (dir on fpath + compinit)
sn introspect                              # full command tree as JSON (for MCP/tool generation)
```

`raw` emits the response exactly as ServiceNow returns it (no unwrapping); method is case-insensitive. `introspect` args carry `takes_value`, `positional`, `repeatable`, `default_values`, `aliases`, `possible_values`; flags report `takes_value: false` — pass them bare (`--all`, not `--all true`).

## Proxy & TLS

```bash
sn --proxy http://proxy:8080 table list incident    # http/https/socks5://
sn --insecure table list incident                   # skip TLS cert verification
sn --ca-cert /path/ca.pem table list incident       # custom CA
sn --no-proxy table list incident                   # bypass configured proxy
```

Env: `SN_PROXY`, `SN_NO_PROXY`, `SN_INSECURE=1`, `SN_CA_CERT`, `SN_PROXY_CA_CERT`. Per-profile in `config.toml`: `proxy`, `no_proxy`, `insecure`, `ca_cert`, `proxy_ca_cert` (proxy auth in `credentials.toml`). Precedence: CLI flag > env var > profile config.
