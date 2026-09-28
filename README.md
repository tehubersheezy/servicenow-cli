# sn

[![CI](https://github.com/tehubersheezy/servicenow-cli/actions/workflows/ci.yml/badge.svg)](https://github.com/tehubersheezy/servicenow-cli/actions/workflows/ci.yml)
[![Security](https://github.com/tehubersheezy/servicenow-cli/actions/workflows/security.yml/badge.svg)](https://github.com/tehubersheezy/servicenow-cli/actions/workflows/security.yml)
[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/tehubersheezy/servicenow-cli/badge)](https://scorecard.dev/viewer/?uri=github.com/tehubersheezy/servicenow-cli)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Latest release](https://img.shields.io/github/v/release/tehubersheezy/servicenow-cli?display_name=tag&sort=semver)](https://github.com/tehubersheezy/servicenow-cli/releases/latest)

`sn` is a command-line tool for ServiceNow. Look up incidents, create change requests,
upload attachments, move update sets, and run tests from your terminal.

It connects to your instance through ServiceNow's APIs and runs as a single binary,
with nothing to install on the instance. Use it for a quick lookup, as part of a shell
script or CI pipeline, or with an AI coding agent. JSON is the default output; add
`--output table` when you'd rather read columns in the terminal.

## Quickstart

Install with Homebrew on macOS or Linux:

```bash
brew install tehubersheezy/sn/sn
```

For Windows, shell installers, and pre-built binaries, see the
[setup guide](docs/setup.md#installation).

For OAuth or SSO, we recommend [creating an Application Registry entry and using its
client ID](docs/setup.md#create-an-oauth-application-registry-entry).

Run `sn init` to connect to your instance. It asks for the instance URL and credentials,
checks the connection, and saves a profile for future commands.

```bash
sn init
sn ping
```

Try a few reads, using an incident number from your instance:

```bash
# List up to five active incidents
sn table list incident --query "active=true" --setlimit 5

# Read one incident, including its catalog variables and work notes
sn get INC0010001

# See which incident fields you can write
sn schema columns incident --writable
```

To create an incident or watch for changes:

```bash
sn table create incident -F short_description="Disk full on prod-db-01"
sn watch incident --query "active=true"  # Ctrl-C to stop
```

## What it can do

The [usage guide](docs/usage.md) has examples for each command group.

| Commands | What they cover |
|---|---|
| [`sn get` / `sn table`](docs/usage.md#reading-records) | Look up records by number or sys_id; create, update, and delete records |
| [`sn watch`](docs/usage.md#watching-records-live) | Stream record changes as they happen |
| [`sn schema`](docs/usage.md#schema-discovery) | Find tables, columns, and available choice values |
| [`sn journal`](docs/usage.md#journal-comments-and-work-notes) | Read comments and work notes as structured entries |
| [`sn aggregate`](docs/usage.md#aggregate-queries) | Get counts, sums, and averages without downloading every record |
| [`sn graphql`](docs/usage.md#graphql) | Run GraphQL queries and mutations |
| [`sn gr`](docs/usage.md#dot-walked-reads-sn-gr) | Read fields from related records using dot-walked references |
| [`sn change`](docs/usage.md#change-management) | Manage change requests, tasks, affected CIs, conflicts, and approvals |
| [`sn attachment`](docs/usage.md#attachments) | Upload and download record attachments |
| [`sn cmdb`](docs/usage.md#cmdb) | Manage configuration items and their relationships; inspect class schemas |
| [`sn import`](docs/usage.md#import-sets) | Load data into import staging tables |
| [`sn catalog`](docs/usage.md#service-catalog) | Browse the Service Catalog and place orders |
| [`sn variables`](docs/agent-guide.md#catalog-variables-variables) | Read catalog variables, or write them and verify the result |
| [`sn identify`](docs/usage.md#identification--reconciliation) | Identify and reconcile configuration items |
| [`sn app` / `sn updateset` / `sn atf`](docs/usage.md#cicd-operations) | Install and publish apps, move update sets, and run Automated Test Framework suites |
| [`sn context`](docs/agent-guide.md#session-context-context) | View or switch the session's application scope and update set |
| [`sn scores`](docs/usage.md#performance-analytics-scorecards) | Read Performance Analytics scorecards |
| [`sn api`](docs/usage.md#api-discovery) | Find REST endpoints on your instance and retrieve their OpenAPI specs |
| [`sn script run`](docs/usage.md#background-scripts) | Run server-side JavaScript and get what it printed back as JSON |
| [`sn codesearch`](docs/usage.md#code-search) | Find where a script include, table, or function is referenced in the instance's code |
| [`sn raw`](docs/usage.md#raw-rest-passthrough) | Call REST endpoints directly |
| [`sn impersonate`](docs/usage.md#acting-as-another-user) | Run a command as another user to see what their roles and ACLs allow |
| [`sn ping` / `sn doctor` / `sn open`](docs/usage.md#inspect-and-connect) | Check the connection, preflight required roles, plugins and properties, or open a record or list view in your browser |

## Using it in scripts

Record data goes to stdout as JSON, and errors go to stderr. On `sn table list`, use
`--all` to stream one record per line, ready to pipe into `jq`:

```bash
sn table list incident --query "active=true" --all | jq -r '.number'
```

Exit codes distinguish success (`0`), usage or configuration errors (`1`), API errors
(`2`), network errors (`3`), and authentication or permission errors (`4`). Commands
won't prompt when stdin isn't a terminal. Destructive operations require `--yes` in
that case; in an interactive terminal, they ask for confirmation. `sn raw` sends the
requested HTTP method directly and has no confirmation guard.

See the [output contract](docs/usage.md#output-contract) for response formats and the
[non-interactive setup guide](docs/setup.md#non-interactive-setup-ci-containers-agents)
for configuring profiles in CI.

## Using it with AI agents

An agent can use `sn schema` to check available fields before writing and parse command
results and errors as JSON. The [agent usage guide](docs/agent-guide.md) covers these
workflows and can be added to an agent's context.

The repo also includes a [Claude Code plugin](docs/agent-integration.md#claude-code-plugin).
For other integrations, `sn introspect` exports the command tree as JSON to help generate
MCP tool definitions or function-call schemas.

## Documentation

| Doc | Contents |
|---|---|
| [Setup guide](docs/setup.md) | Installation, profiles, authentication, config files, proxy and TLS |
| [Usage guide](docs/usage.md) | Command examples, output formats, exit codes, and debugging |
| [Agent integration](docs/agent-integration.md) | The Claude Code plugin and `sn introspect` |
| [Agent usage guide](docs/agent-guide.md) | Workflows and reference material for coding agents |
| [Changelog](CHANGELOG.md) | Release notes and breaking changes to check before upgrading |

## License

[MIT](LICENSE)
