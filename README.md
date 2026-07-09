# sn

[![CI](https://github.com/tehubersheezy/servicenow-cli/actions/workflows/ci.yml/badge.svg)](https://github.com/tehubersheezy/servicenow-cli/actions/workflows/ci.yml)
[![Security](https://github.com/tehubersheezy/servicenow-cli/actions/workflows/security.yml/badge.svg)](https://github.com/tehubersheezy/servicenow-cli/actions/workflows/security.yml)
[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/tehubersheezy/servicenow-cli/badge)](https://scorecard.dev/viewer/?uri=github.com/tehubersheezy/servicenow-cli)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Latest release](https://img.shields.io/github/v/release/tehubersheezy/servicenow-cli?display_name=tag&sort=semver)](https://github.com/tehubersheezy/servicenow-cli/releases/latest)

A fast, single-binary CLI for ServiceNow, built for LLM agents and human operators alike.

It wraps ServiceNow's REST APIs — Table, Change Management, Attachment, CMDB, Import Set, Service Catalog, Identification & Reconciliation, CICD, Aggregate, Performance Analytics, and schema discovery — behind one predictable interface: stable JSON on stdout, structured errors on stderr, and deterministic exit codes.

```bash
sn init                                      # connect to an instance
sn table list incident --setlimit 5          # read records as JSON
sn table create incident --field short_description="Disk full on prod-db-01"
sn ping                                       # auth + latency health check
```

## Contents

- [Installation](#installation)
- [Setup](#setup)
  - [Basic auth](#basic-auth)
  - [OAuth / SSO](#oauth--sso)
- [Usage](#usage)
  - [Reading records](#reading-records)
  - [Writing records](#writing-records)
  - [Pagination](#pagination)
  - [Schema discovery](#schema-discovery)
  - [Aggregate queries](#aggregate-queries)
  - [Change Management](#change-management)
  - [Attachments](#attachments)
  - [CMDB](#cmdb)
  - [Import Sets](#import-sets)
  - [Service Catalog](#service-catalog)
  - [Identification & Reconciliation](#identification--reconciliation)
  - [CICD operations](#cicd-operations)
  - [Performance Analytics scorecards](#performance-analytics-scorecards)
  - [Inspect and connect](#inspect-and-connect)
  - [Open a record in the web UI](#open-a-record-in-the-web-ui)
  - [Raw REST passthrough](#raw-rest-passthrough)
  - [Human-readable table output](#human-readable-table-output)
  - [Agent integration](#agent-integration)
- [Output contract](#output-contract)
  - [Exit codes](#exit-codes)
- [Parameters](#parameters)
- [Configuration](#configuration)
  - [Environment variables](#environment-variables)
- [Proxy and TLS](#proxy-and-tls)
- [Debugging](#debugging)
- [License](#license)

## Installation

### Homebrew (macOS / Linux)

```bash
brew install tehubersheezy/sn/sn
# or: brew tap tehubersheezy/sn && brew install sn   (upgrade later with: brew upgrade sn)
```

### Shell installer (macOS / Linux)

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/tehubersheezy/servicenow-cli/releases/latest/download/sn-installer.sh | sh
```

### Windows (MSI or PowerShell)

Download `sn-x86_64-pc-windows-msvc.msi` (64-bit Intel/AMD) or `sn-aarch64-pc-windows-msvc.msi` (ARM64 — Surface Pro X, Copilot+ PCs) from the [latest release](https://github.com/tehubersheezy/servicenow-cli/releases/latest) and double-click. For unattended/SCCM/Intune deployment use `msiexec /i sn-x86_64-pc-windows-msvc.msi /qn`. Or install via PowerShell:

```powershell
powershell -ExecutionPolicy ByPass -c "irm https://github.com/tehubersheezy/servicenow-cli/releases/latest/download/sn-installer.ps1 | iex"
```

### Pre-built binaries

Download from [Releases](https://github.com/tehubersheezy/servicenow-cli/releases): Linux (x86_64, ARM64), macOS (Intel, Apple Silicon), and Windows (x86_64, ARM64) — the latter as a portable `.zip` (no install) or `.msi` installer.

## Setup

`sn` supports **basic auth** (username + password) for most instances, and **OAuth / SSO** for instances fronted by an external identity provider (Okta, Azure AD, ADFS), where the password lives in the IdP and basic auth cannot work.

### Basic auth

```bash
sn init
# Instance (e.g. acme.service-now.com): mycompany.service-now.com
# Username: admin
# Password: ********
# profile 'default' saved and verified.
```

Add more instances as named profiles, then pick one per command or set a default:

```bash
sn init --profile prod --instance prod.service-now.com --username svc-user
sn --profile prod table list incident --setlimit 5
sn profile use prod                  # make it the default
```

### OAuth / SSO

Configure the profile with `sn init --auth oauth`, then run the flow with `sn auth login`:

```bash
# Authorization-code + PKCE (default): a PUBLIC client — no secret needed or prompted for.
sn init --profile sso --auth oauth --instance acme.service-now.com --client-id <id>

# Non-interactive server-to-server: client_credentials is a CONFIDENTIAL client and needs a secret
# (prompted if --client-secret is omitted).
sn init --profile svc --auth oauth --instance acme.service-now.com \
  --grant client_credentials --client-id <id> --client-secret <secret>

sn --profile sso auth login          # run the OAuth flow, cache tokens
```

**One-time admin setup** (if the instance has no registry entry yet): **System OAuth → Application Registry → New → "Create an OAuth API endpoint for external clients"**; set the redirect URL to `http://localhost:8400/callback` — which must match `--redirect-uri` **exactly** — and copy the client ID. For the default authorization-code flow, enable **Public Client / PKCE required** so no secret is needed; only `client_credentials` needs the generated secret.

After login, tokens refresh transparently. Manage the session with `sn auth status` (method + token expiry), `sn auth refresh`, and `sn auth logout`. The client ID and redirect URI live in `config.toml`; the secret and tokens in `credentials.toml` (chmod 600).

Verify either auth method at any time with `sn ping`.

## Usage

### Reading records

```bash
# List incidents (default: up to 1000 records)
sn table list incident

# Filter, select fields, limit
sn table list incident --query "active=true^priority=1" \
  --fields "number,short_description,state" --setlimit 10

# One record; --display-value swaps internal codes for readable labels
sn table get incident <sys_id> --display-value all
```

### Writing records

`create`, `update`, and `replace` take either `--data` or `--field` (mutually exclusive):

- `--data '<json>'` — inline JSON object (`@file.json` reads a file, `@-` reads stdin)
- `--field key=value` — repeatable key/value pairs (`key=@file` reads the value from a file)

```bash
# Key/value pairs, or inline JSON, or piped from another tool
sn table create incident --field short_description="Disk full on prod-db-01" --field urgency=2
sn table create incident --data '{"short_description":"Server down","priority":"1"}'
echo '{"short_description":"from pipe"}' | sn table create incident --data @-

# update = PATCH (changes only the fields you name); replace = PUT (omitted fields are blanked)
sn table update incident <sys_id> --field state=2
sn table replace incident <sys_id> --data @full-record.json

# Delete
sn table delete incident <sys_id> --yes
```

### Pagination

```bash
# Stream every match as JSONL (one record per line)
sn table list incident --query "active=true" --all

# ...or buffer into one JSON array; cap the total with --max-records
sn table list incident --all --array --max-records 5000

# Pipe to jq
sn table list incident --all | jq -r '.number'
```

### Schema discovery

Explore an unfamiliar instance:

```bash
sn schema tables --filter incident        # find tables by keyword
sn schema columns incident --writable     # writable columns for a table
sn schema choices incident state          # valid values for a choice field
```

### Aggregate queries

Server-side statistics, without fetching individual records:

```bash
# Count records grouped by state, with readable labels
sn aggregate incident --count --group-by state --display-value true

# Average a field, filtered
sn aggregate incident --avg-fields reassignment_count --query "active=true"

# Several aggregations in one call
sn aggregate incident --sum-fields reassignment_count --min-fields priority --max-fields priority
```

### Change Management

Normal, emergency, and standard change requests across their lifecycle:

```bash
# List; create (standard changes require --template); update; delete
sn change list --type normal --query "state=1" --setlimit 10
sn change create --type normal --field short_description="DB migration" --field category=software
sn change create --type standard --template <template_sys_id> --field short_description="Routine patching"
sn change update <sys_id> --field state=2
sn change delete <sys_id>

# Workflow helpers
sn change nextstates <sys_id>                          # valid next states
sn change approvals <sys_id> --field approval="approved"
sn change risk <sys_id> --data '{"risk_value":"moderate"}'
sn change schedule <sys_id>
sn change models                                       # change models
sn change templates                                    # standard-change templates
```

#### Change tasks, CIs, and conflicts

```bash
# Tasks
sn change task list <change_sys_id>
sn change task create <change_sys_id> --field short_description="Pre-check"
sn change task update <change_sys_id> <task_sys_id> --field state=2
sn change task delete <change_sys_id> <task_sys_id>

# CIs and conflicts
sn change ci add <change_sys_id> --data '{"cmdb_ci_sys_id":"<ci_id>"}'
sn change conflict get <sys_id>
sn change conflict remove <sys_id>
```

### Attachments

Files on any record:

```bash
sn attachment list --query "table_name=incident"
sn attachment get <sys_id>

# Upload a file (optionally override its name and content type)
sn attachment upload --table incident --record <record_sys_id> --file ./screenshot.png
sn attachment upload --table incident --record <record_sys_id> --file ./data.csv \
  --file-name "export_2026.csv" --content-type text/csv

# Download to a file, or to stdout for piping
sn attachment download <sys_id> --output ./downloaded.png
sn attachment download <sys_id> | gzip > backup.gz

sn attachment delete <sys_id>
```

### CMDB

CRUD and relationships on Configuration Items of any class:

```bash
sn cmdb list cmdb_ci_server --query "operational_status=1" --setlimit 20
sn cmdb get cmdb_ci_server <sys_id>                                     # includes relations
sn cmdb create cmdb_ci_server --field name=web-server-01 --field ip_address=10.0.1.50
sn cmdb update cmdb_ci_server <sys_id> --field operational_status=2     # PATCH
sn cmdb replace cmdb_ci_server <sys_id> --data @ci.json                 # PUT (full overwrite)
sn cmdb meta cmdb_ci_server                                             # class schema

# Relations
sn cmdb relation add cmdb_ci_server <sys_id> --data '{"type":"<rel_type_id>","target":"<target_ci_id>"}'
sn cmdb relation delete cmdb_ci_server <sys_id> <rel_sys_id>
```

### Import Sets

Insert into staging tables for transform-based imports:

```bash
sn import create u_staging_table --field u_name="Server-01" --field u_ip="10.0.1.1"
sn import bulk u_staging_table --data '[{"u_name":"Server-01"},{"u_name":"Server-02"}]'
sn import get u_staging_table <sys_id>
```

### Service Catalog

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

# ...or work the cart (cart-update / cart-remove / cart-empty also available)
sn catalog add-to-cart <item_sys_id> --data '{"sysparm_quantity":"1"}'
sn catalog cart
sn catalog checkout
sn catalog submit-order

sn catalog wishlist
```

### Identification & Reconciliation

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

### CICD operations

`app`, `updateset`, and `atf run` are asynchronous — they return a `progress_id` and run in the background. Add `--wait` to block until the operation finishes and emit the final result, and `--wait-timeout <SECS>` to bound that wait (on expiry `sn` exits 3 with a pointer to `sn progress`). Without `--wait`, poll manually with `sn progress <progress_id>`.

```bash
# App Repository lifecycle
sn app install  --scope x_myapp --version 1.2.0 --wait
sn app publish  --scope x_myapp --version 1.3.0 --dev-notes "Bug fixes" --wait
sn app rollback --scope x_myapp --version 1.1.0 --wait

# Update sets
sn updateset create --name "My Changes" --description "Sprint 42 work"
sn updateset retrieve --update-set-id <id> --auto-preview
sn updateset preview <remote_update_set_id> --wait
sn updateset commit  <remote_update_set_id> --wait
sn updateset commit-multiple --ids id1,id2,id3
sn updateset back-out --update-set-id <id> --wait

# ATF suites
sn atf run --suite-name "Regression Suite" --wait --wait-timeout 900
sn atf results <result_id>

# Poll an operation already in flight
sn progress <progress_id>
```

### Performance Analytics scorecards

```bash
# List scorecards (paged and sorted)
sn scores list --per-page 20 --sort-by VALUE --sort-dir DESC

# Historical scores for one indicator
sn scores list --uuid <indicator_id> --include-scores --from 2026-01-01 --to 2026-04-01

sn scores favorite <uuid>
sn scores unfavorite <uuid>
```

### Inspect and connect

```bash
# Latency + auth + ServiceNow build version — one-shot health check (either auth method)
sn ping
# {"ok":true,"profile":"prod","instance":"https://acme.service-now.com","username":"admin","latency_ms":134,"build_name":"Vancouver",...}

# The authenticated user, resolved via gs.getUserName()
sn user me
```

### Open a record in the web UI

```bash
sn open incident <sys_id>                # any table; opens the form in your default browser
sn open incident <sys_id> --print-url    # print the URL instead of opening it
```

### Raw REST passthrough

An escape hatch for endpoints not yet modeled as typed commands — returned exactly as ServiceNow sends it, no envelope unwrapping:

```bash
sn raw GET /api/now/v2/table/incident -q sysparm_limit=5 -q sysparm_query=active=true
sn raw POST /api/now/table/incident --data '{"short_description":"From sn raw"}'
sn raw PATCH /api/now/table/incident/abc123 --field state=2
sn raw DELETE /api/now/table/incident/abc123
```

### Human-readable table output

Most read commands accept `--output table` for columns instead of JSON — for interactive browsing; keep the default JSON for scripts and pipelines (don't pipe it):

```bash
sn table list incident --setlimit 5 --output table
sn schema columns incident --writable --output table
```

### Shell completions

```bash
# zsh — write to a dir on your fpath, then enable compinit
mkdir -p ~/.zsh/completions
sn completion zsh > ~/.zsh/completions/_sn
# add these two lines to ~/.zshrc (once), then restart your shell:
#   fpath=(~/.zsh/completions $fpath)
#   autoload -Uz compinit && compinit

# bash (requires the bash-completion package)
sn completion bash > ~/.local/share/bash-completion/completions/sn

# fish
sn completion fish > ~/.config/fish/completions/sn.fish
```

Supported shells: `bash`, `zsh`, `fish`, `powershell`, `elvish`. The `${fpath[1]}` shortcut some tools suggest fails when that directory doesn't exist (common on Apple Silicon Homebrew) — the dir-on-fpath recipe above is portable.

### Agent integration

`sn` ships as a Claude Code plugin — install it so Claude can run `sn` commands automatically (it pre-approves `Bash(sn *)`):

```bash
claude plugin install --dir /path/to/sn
```

In a clone of this repo, the skill at `.claude/skills/sn.md` is picked up automatically — invoke with `/sn`.

`sn introspect` dumps the full command tree as JSON — for auto-generating MCP tool definitions or function-call schemas:

```bash
sn introspect | jq '.subcommands[] | {name, about}'
```

## Output contract

Commands emit JSON on stdout by a few consistent rules:

- `list` / `schema tables` / `columns` / `choices` → a JSON array (JSONL with `--all`).
- `get` / `create` / `update` / `replace` → the single record object (`cmdb get` includes relations).
- `delete` → nothing.
- `aggregate` → a stats object; `scores` → scorecard records.
- Async CICD (`app`, `updateset`, `atf run`, `progress`) → a progress object with `progress_id`, `state`, and `percentComplete`.
- `attachment download` → raw bytes (or metadata JSON when you pass `--output <file>`).

Across every command:

- `--output raw` preserves ServiceNow's `{"result": ...}` envelope; `--output table` renders columns (interactive only).
- Output is pretty-printed on a TTY, compact when piped — override with `--pretty` / `--compact`.
- Errors always go to stderr: `{"error": {"message", "detail?", "status_code?", "transaction_id?"}}`.

### Exit codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Usage or config error |
| 2 | API error (4xx/5xx, non-auth) |
| 3 | Network / transport error |
| 4 | Auth error (401/403) |

## Parameters

Every `sysparm_*` parameter has both a friendly name and a raw alias:

| Friendly | Alias | Values |
|---|---|---|
| `--query` | `--sysparm-query` | Encoded query string |
| `--fields` | `--sysparm-fields` | Comma-separated field list |
| `--setlimit` | `--limit`, `--sysparm-limit`, `--page-size` | Max records returned (default 1000) |
| `--offset` | `--sysparm-offset` | Starting offset |
| `--display-value` | `--sysparm-display-value` | `true`, `false`, `all` |
| `--exclude-reference-link` | `--sysparm-exclude-reference-link` | Boolean |
| `--view` | `--sysparm-view` | Named UI view |
| `--input-display-value` | `--sysparm-input-display-value` | Boolean (writes) |
| `--suppress-auto-sys-field` | `--sysparm-suppress-auto-sys-field` | Boolean (writes) |
| `--query-no-domain` | `--sysparm-query-no-domain` | Boolean |
| `--no-count` | `--sysparm-no-count` | Boolean |
| `--output` | (CLI only) | `default` (unwrapped JSON), `raw` (full envelope), or `table` (columnar — interactive only) |

## Configuration

Credentials use a two-file, AWS CLI-style split:

| File | Contains | Location (Linux) |
|---|---|---|
| `config.toml` | Instance URLs, default profile, non-secret OAuth config | `~/.config/sn/` |
| `credentials.toml` | Usernames, passwords, secrets, cached tokens (chmod 600) | `~/.config/sn/` |

macOS uses `~/Library/Application Support/sn/` and Windows `%APPDATA%\sn\`.

A **profile** is the single unit of identity — create one with `sn init` (see [Setup](#setup)) and select it with `--profile NAME` or `sn profile use NAME`. Resolution is `--profile` flag > `default_profile` in `config.toml` > a clear error. No env var or global flag can override a profile field — preventing chimera identities and, for OAuth, token leakage to an arbitrary host.

Point `sn` at a different config directory (for testing or sandboxing) with `SN_CONFIG_DIR`.

### Environment variables

| Env var | Description |
|---|---|
| `SN_CONFIG_DIR` | Override the config directory. Points **directly** at the folder holding `config.toml` and `credentials.toml` (no `sn` subdirectory appended). Cross-platform; when unset, the platform-native location is used. |
| `SN_PROXY` | HTTP/HTTPS/SOCKS5 proxy URL |
| `SN_NO_PROXY` | Comma-separated hosts to bypass the proxy |
| `SN_INSECURE=1` | Disable TLS certificate verification |
| `SN_CA_CERT` | Path to a custom CA cert for ServiceNow |
| `SN_PROXY_CA_CERT` | Path to a custom CA cert for the proxy |

```bash
SN_PROXY=http://proxy:8080 sn table list incident
SN_INSECURE=1 sn table list incident    # skip cert verification
```

There are deliberately no environment variables for credential values or profile selection — use profiles (`sn init`, `--profile`) instead.

## Proxy and TLS

Route through a proxy or adjust TLS per invocation:

```bash
sn --proxy http://proxy.corp:8080 table list incident   # also socks5://proxy:1080
sn --no-proxy table list incident                        # bypass a configured proxy for one call
sn --insecure table list incident                        # skip cert verification (dev/self-signed certs)
sn --ca-cert /path/to/ca.pem table list incident         # custom CA certificate
```

Any of these can live in a profile — non-secrets in `config.toml`, proxy credentials in `credentials.toml`:

```toml
# config.toml
[profiles.dev]
instance = "dev.example.com"
proxy = "http://proxy.corp:8080"
no_proxy = "localhost,127.0.0.1"
insecure = false
ca_cert = "/etc/ssl/custom-ca.pem"
proxy_ca_cert = "/etc/ssl/proxy-ca.pem"

# credentials.toml
[profiles.dev]
proxy_username = "proxy-user"
proxy_password = "proxy-pass"
```

Precedence for every proxy/TLS setting: CLI flag > env var (`SN_PROXY`, `SN_INSECURE=1`, …) > profile config.

## Debugging

```bash
sn -d   table list incident     # HTTP method, URL, status
sn -dd  table list incident     # + response headers
sn -ddd table list incident     # + request/response bodies (auth masked)
sn -v                           # print version (-V also works)
```

## License

MIT
