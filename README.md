# sn

A fast, single-binary CLI for ServiceNow. Designed for LLM agents and human operators alike.

`sn` wraps ServiceNow's REST APIs — Table, Change Management, Attachment, CMDB, Import Set, Service Catalog, Identification & Reconciliation, CICD, Aggregate, Performance Analytics, and schema discovery — into a predictable command-line interface with stable JSON output, structured error reporting, and deterministic exit codes.

## Installation

### Homebrew (macOS / Linux)

```bash
brew install tehubersheezy/sn/sn
```

Or tap once, then install/upgrade by short name:

```bash
brew tap tehubersheezy/sn
brew install sn
brew upgrade sn
```

### Shell installer (macOS / Linux)

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/tehubersheezy/sn/releases/latest/download/sn-installer.sh | sh
```

### MSI installer (Windows)

Download the appropriate `.msi` from the [latest release](https://github.com/tehubersheezy/sn/releases/latest):

- `sn-x86_64-pc-windows-msvc.msi` — 64-bit Intel/AMD
- `sn-aarch64-pc-windows-msvc.msi` — ARM64 (Surface Pro X, Copilot+ PCs)

Double-click to install, or for unattended/SCCM/Intune deployment:

```powershell
msiexec /i sn-x86_64-pc-windows-msvc.msi /qn
```

### PowerShell installer (Windows)

```powershell
powershell -ExecutionPolicy ByPass -c "irm https://github.com/tehubersheezy/sn/releases/latest/download/sn-installer.ps1 | iex"
```

### Cargo (from source)

```bash
cargo install sn
```

### Pre-built binaries

Download from [Releases](https://github.com/tehubersheezy/sn/releases). Binaries are available for:

- Linux (x86_64, ARM64)
- macOS (Intel, Apple Silicon)
- Windows (x86_64, ARM64) — both as standalone `.zip` (portable, no install) and `.msi` installer

## Setup

Connect to a ServiceNow instance:

```bash
sn init
# Instance (e.g. acme.service-now.com): mycompany.service-now.com
# Username: admin
# Password: ********
# profile 'default' saved and verified.
```

Add additional instances with named profiles:

```bash
sn init --profile prod --instance prod.service-now.com --username svc-user
sn init --profile dev  --instance dev.service-now.com  --username admin
```

Select a profile per command or set a default:

```bash
sn --profile prod table list incident --setlimit 5
sn profile use prod                  # set as default
SN_PROFILE=dev sn table list change_request   # env override
```

Verify credentials:

```bash
sn auth test
```

## Usage

### Reading records

```bash
# List incidents (default: up to 1000 records)
sn table list incident

# Filter + select fields
sn table list incident \
  --query "active=true^priority=1" \
  --fields "number,short_description,state" \
  --setlimit 10

# Get a single record
sn table get incident <sys_id>

# Display human-readable values instead of internal codes
sn table get incident <sys_id> --display-value all
```

### Writing records

All three write verbs (`create`, `update`, `replace`) accept either `--data` or `--field` (mutually exclusive):

- `--data '<json>'` — inline JSON object
- `--data @file.json` — read JSON from file
- `--data @-` — read JSON from stdin
- `--field key=value` — repeatable; builds a JSON object from key-value pairs
- `--field key=@file` — field value read from file

```bash
# Create with key-value pairs
sn table create incident \
  --field short_description="Disk full on prod-db-01" \
  --field urgency=2

# Create with inline JSON
sn table create incident --data '{"short_description": "Server down", "priority": "1"}'

# Create from file
sn table create incident --data @body.json

# Create from stdin (pipe from another tool)
echo '{"short_description": "from pipe"}' | sn table create incident --data @-

# Update (PATCH) — only changes the fields you name
sn table update incident <sys_id> --field state=2
sn table update incident <sys_id> --data '{"state": "6", "close_notes": "Resolved"}'

# Replace (PUT) — overwrites the entire record (omitted fields are blanked)
sn table replace incident <sys_id> --data @full-record.json

# Delete
sn table delete incident <sys_id> --yes
```

### Pagination

```bash
# Stream all records as JSONL (one per line)
sn table list incident --query "active=true" --all

# Collect into a single JSON array
sn table list incident --query "active=true" --all --array

# Cap total records
sn table list incident --all --max-records 5000

# Process with jq
sn table list incident --all | jq -r '.number'
```

### Schema discovery

Discover tables and their structure without prior ServiceNow knowledge:

```bash
# Find tables matching a keyword
sn schema tables --filter incident

# List writable columns for a table
sn schema columns incident --writable

# Get valid values for a choice field
sn schema choices incident state
```

### Aggregate queries

Run server-side statistics against any table without fetching individual records:

```bash
# Count records grouped by state (with human-readable labels)
sn aggregate incident --count --group-by state --display-value true

# Average a field, filtered
sn aggregate incident --avg-fields reassignment_count --query "active=true"

# Multiple aggregations in one call
sn aggregate incident \
  --sum-fields reassignment_count \
  --min-fields priority \
  --max-fields priority
```

### Change Management

Full lifecycle management for normal, emergency, and standard change requests:

```bash
# List all normal changes
sn change list --type normal --query "state=1" --setlimit 10

# Create a normal change
sn change create --type normal \
  --field short_description="DB migration" \
  --field category=software

# Create a standard change from a template
sn change create --type standard --template <template_sys_id> \
  --field short_description="Routine patching"

# Update a change
sn change update <sys_id> --field state=2

# Get valid next states (useful for workflow automation)
sn change nextstates <sys_id>

# Delete a change
sn change delete <sys_id>
```

#### Change tasks, CIs, and conflicts

```bash
# Task management
sn change task list <change_sys_id>
sn change task create <change_sys_id> --field short_description="Pre-check"
sn change task update <change_sys_id> <task_sys_id> --field state=2
sn change task delete <change_sys_id> <task_sys_id>

# CI relationships
sn change ci list <change_sys_id>
sn change ci add <change_sys_id> --data '{"cmdb_ci_sys_id": "<ci_id>"}'

# Conflicts
sn change conflict get <sys_id>
sn change conflict add <sys_id> --data '{"...": "..."}'
sn change conflict remove <sys_id>

# Approval and risk
sn change approvals <sys_id> --field approval="approved"
sn change risk <sys_id> --data '{"risk_value": "moderate"}'

# Schedule and models
sn change schedule <sys_id>
sn change models                  # list all change models
sn change templates               # list standard change templates
sn change templates <sys_id>      # get a specific template
```

### Attachments

Upload, download, and manage file attachments on any ServiceNow record:

```bash
# List attachments for a table
sn attachment list --query "table_name=incident"

# Get attachment metadata
sn attachment get <sys_id>

# Upload a file to a record
sn attachment upload \
  --table incident \
  --record <record_sys_id> \
  --file ./screenshot.png

# Upload with custom name and content type
sn attachment upload \
  --table incident \
  --record <record_sys_id> \
  --file ./data.csv \
  --file-name "export_2026.csv" \
  --content-type text/csv

# Download attachment content
sn attachment download <sys_id> --output ./downloaded.png

# Download to stdout (pipe to another tool)
sn attachment download <sys_id> | gzip > backup.gz

# Delete an attachment
sn attachment delete <sys_id>
```

### CMDB

Query, create, and manage Configuration Items and their relationships:

```bash
# List CIs of a specific class
sn cmdb list cmdb_ci_server --query "operational_status=1" --setlimit 20

# Get a CI with its relations
sn cmdb get cmdb_ci_server <sys_id>

# Create a CI
sn cmdb create cmdb_ci_server \
  --field name=web-server-01 \
  --field ip_address=10.0.1.50

# Update a CI (PATCH)
sn cmdb update cmdb_ci_server <sys_id> --field operational_status=2

# Replace a CI (PUT — full overwrite)
sn cmdb replace cmdb_ci_server <sys_id> --data @ci.json

# Get class metadata (schema for a CMDB class)
sn cmdb meta cmdb_ci_server

# Manage relations
sn cmdb relation add cmdb_ci_server <sys_id> \
  --data '{"type": "<rel_type_id>", "target": "<target_ci_id>"}'
sn cmdb relation delete cmdb_ci_server <sys_id> <rel_sys_id>
```

### Import Sets

Insert records into staging tables for transform-based imports:

```bash
# Insert a single record
sn import create u_staging_table \
  --field u_name="Server-01" \
  --field u_ip="10.0.1.1"

# Bulk insert multiple records
sn import bulk u_staging_table \
  --data '[{"u_name":"Server-01"},{"u_name":"Server-02"}]'

# Retrieve an import set record
sn import get u_staging_table <sys_id>
```

### Service Catalog

Browse catalogs, search items, and place orders:

```bash
# Browse catalogs
sn catalog list
sn catalog get <catalog_sys_id>

# Browse categories
sn catalog categories <catalog_sys_id>
sn catalog category <category_sys_id>

# Search and view items
sn catalog items --text "laptop" --catalog <catalog_id>
sn catalog item <item_sys_id>
sn catalog item-variables <item_sys_id>   # form fields required to order

# Order immediately (bypasses cart)
sn catalog order <item_sys_id> --data '{"sysparm_quantity": "1"}'

# Cart workflow
sn catalog add-to-cart <item_sys_id> --data '{"sysparm_quantity": "1"}'
sn catalog cart                           # view current cart
sn catalog cart-update <cart_item_id> --field quantity=2
sn catalog cart-remove <cart_item_id>
sn catalog cart-empty <cart_sys_id>
sn catalog checkout
sn catalog submit-order

# Wishlist
sn catalog wishlist
```

### Identification & Reconciliation

Create, update, or identify CIs through the reconciliation engine:

```bash
# Create or update a CI
sn identify create-update --data '{
  "items": [{
    "className": "cmdb_ci_server",
    "values": {"name": "web-01", "ip_address": "10.0.1.1"}
  }]
}'

# Identify a CI without modifying it
sn identify query --data '{
  "items": [{
    "className": "cmdb_ci_server",
    "values": {"name": "web-01"}
  }]
}'

# Enhanced variants with options
sn identify create-update-enhanced \
  --data @payload.json \
  --data-source "discovery" \
  --options "partial_payload:true,partial_commits:true"

sn identify query-enhanced --data @query.json --data-source "discovery"
```

### CICD operations

#### App lifecycle

Install, publish, and roll back scoped applications from the ServiceNow App Repository:

```bash
sn app install --scope x_myapp --version 1.2.0 --wait
sn app publish --scope x_myapp --version 1.3.0 --dev-notes "Bug fixes" --wait
sn app rollback --scope x_myapp --version 1.1.0 --wait
```

#### Update sets

```bash
# Create a new Update Set
sn updateset create --name "My Changes" --description "Sprint 42 work"

# Retrieve a remote Update Set into this instance
sn updateset retrieve --update-set-id <id> --auto-preview

# Preview and commit (--wait blocks until each step completes)
sn updateset preview <remote_update_set_id> --wait
sn updateset commit <remote_update_set_id> --wait

# Commit several at once
sn updateset commit-multiple --ids id1,id2,id3

# Undo an applied Update Set
sn updateset back-out --update-set-id <id> --wait
```

#### ATF testing

Run Automated Test Framework suites and retrieve their results:

```bash
sn atf run --suite-name "Regression Suite" --wait
sn atf results <result_id>
```

#### Polling async progress

`app`, `updateset`, and `atf run` are asynchronous. Pass `--wait` to any of these commands and it will block until the operation completes (or fails), then emit the final progress result — no manual polling needed:

```bash
sn app install --scope x_myapp --version 1.2.0 --wait
sn atf run --suite-name "Regression Suite" --wait
```

To check the status of an already-running operation, use `sn progress` with the `progress_id` from the initial response:

```bash
sn progress <progress_id>
```

### Performance Analytics scorecards

```bash
# List scorecards (20 per page, sorted by value descending)
sn scores list --per-page 20 --sort-by VALUE --sort-dir DESC

# Fetch historical scores for a specific indicator
sn scores list --uuid <indicator_id> --include-scores --from 2026-01-01 --to 2026-04-01

# Favorite / unfavorite
sn scores favorite <uuid>
sn scores unfavorite <uuid>
```

### Agent integration

#### Claude Code plugin

`sn` ships as a Claude Code plugin. Install it so Claude can use `sn` commands automatically:

```bash
claude plugin install --dir /path/to/sn
```

Or for projects that clone this repo, the skill at `.claude/skills/sn.md` is picked up automatically — invoke with `/sn`.

The plugin pre-approves `Bash(sn *)` so Claude won't prompt for permission on each command.

#### Introspection

`sn introspect` dumps the full command tree as structured JSON, suitable for auto-generating MCP tool definitions or function-call schemas:

```bash
sn introspect | jq '.subcommands[] | {name, about}'
```

## Output contract

| Verb | stdout |
|---|---|
| `table list` | JSON array of records (JSONL with `--all`) |
| `table get` / `create` / `update` / `replace` | Single record object |
| `table delete` | Nothing (empty) |
| `schema tables` / `columns` / `choices` | JSON array |
| `aggregate` | Stats object (counts, sums, averages, grouped results) |
| `app install` / `publish` / `rollback` | Progress object with `progress_id` |
| `updateset create` | New Update Set record |
| `updateset retrieve` / `preview` / `commit` / `back-out` | Progress object with `progress_id` |
| `updateset commit-multiple` | Array of progress objects |
| `atf run` | Progress object with `progress_id` |
| `atf results` | Test suite result object |
| `progress` | Progress status object |
| `scores list` | JSON array of scorecard records |
| `scores favorite` / `unfavorite` | Updated scorecard object |
| `change list` | JSON array of change records |
| `change get` / `create` / `update` | Single change object |
| `change delete` | Nothing (empty) |
| `change nextstates` / `schedule` / `models` / `templates` | JSON object or array |
| `change task list` / `ci list` | JSON array |
| `change task get` / `task create` / `task update` | Single task object |
| `attachment list` | JSON array of attachment metadata |
| `attachment get` | Single attachment metadata object |
| `attachment upload` | Created attachment metadata object |
| `attachment download` | Binary file content (or JSON metadata with `--output`) |
| `attachment delete` | Nothing (empty) |
| `cmdb list` | JSON array of CI records |
| `cmdb get` / `create` / `update` / `replace` | Single CI object with relations |
| `cmdb meta` | Class metadata object |
| `import create` | Import result array |
| `import bulk` | Import result array |
| `import get` | Single import set record |
| `catalog list` / `items` | JSON array |
| `catalog get` / `category` / `item` | Single object |
| `catalog order` / `checkout` / `submit-order` | Order result object |
| `identify create-update` / `query` | Reconciliation result object |

- `--output raw` preserves ServiceNow's `{"result": ...}` envelope.
- Pretty-printed when stdout is a TTY; compact when piped. Override with `--pretty` / `--compact`.
- Errors are always JSON on stderr: `{"error": {"message", "detail?", "status_code?", "transaction_id?"}}`.

### Exit codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Usage or config error |
| 2 | API error (4xx/5xx, non-auth) |
| 3 | Network / transport error |
| 4 | Auth error (401/403) |

## Parameters

Every ServiceNow `sysparm_*` parameter has both a friendly name and a raw alias:

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
| `--instance-override` | (CLI only) | Override instance URL for this invocation |

## Configuration

Credentials are stored in two files (AWS CLI-style split):

| File | Contains | Location (Linux) |
|---|---|---|
| `config.toml` | Instance URLs, default profile | `~/.config/sn/` |
| `credentials.toml` | Usernames, passwords (chmod 600) | `~/.config/sn/` |

Environment variables override profile values:

```bash
SN_INSTANCE=https://myco.service-now.com \
SN_USERNAME=api-user \
SN_PASSWORD=secret \
  sn table list incident --setlimit 1
```

Proxy and TLS environment variables:

```bash
SN_PROXY=http://proxy:8080 sn table list incident
SN_INSECURE=1 sn table list incident    # skip cert verification
```

## Proxy and TLS

Route traffic through a proxy:

```bash
sn --proxy http://proxy.corp:8080 table list incident --setlimit 5

# SOCKS5 proxy
sn --proxy socks5://proxy:1080 table list incident

# Bypass a configured proxy for one call
sn --no-proxy table list incident
```

Disable TLS certificate verification (for dev/test instances with self-signed certs):

```bash
sn --insecure table list incident
```

Use a custom CA certificate:

```bash
sn --ca-cert /path/to/ca.pem table list incident
sn --proxy-ca-cert /path/to/proxy-ca.pem --proxy http://proxy:8080 table list incident
```

All proxy/TLS settings can be set per-profile in `config.toml`:

```toml
[profiles.dev]
instance = "dev.example.com"
proxy = "http://proxy.corp:8080"
no_proxy = "localhost,127.0.0.1"
insecure = false
ca_cert = "/etc/ssl/custom-ca.pem"
proxy_ca_cert = "/etc/ssl/proxy-ca.pem"
```

Proxy credentials go in `credentials.toml` (since they're secrets):

```toml
[profiles.dev]
username = "sn-user"
password = "sn-pass"
proxy_username = "proxy-user"
proxy_password = "proxy-pass"
```

Environment variables: `SN_PROXY`, `SN_NO_PROXY`, `SN_INSECURE=1`, `SN_CA_CERT`, `SN_PROXY_CA_CERT`.

Precedence: `--proxy` flag > `SN_PROXY` env > profile config (same for all settings).

## Debugging

```bash
sn -v table list incident       # show HTTP method, URL, status
sn -vv table list incident      # add response headers
sn -vvv table list incident     # add request/response bodies (auth masked)
```

## License

MIT
