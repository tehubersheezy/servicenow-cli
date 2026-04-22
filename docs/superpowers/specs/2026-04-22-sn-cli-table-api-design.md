# sn: Rust CLI for the ServiceNow Table API - Design Specification

Date: 2026-04-22
Status: Approved for implementation
Audience: Implementing engineers, the planning agent that will turn this into an implementation plan, and future maintainers needing the durable record of design decisions.

## 1. Overview and guiding principle

`sn` is a single-binary Rust CLI that wraps the ServiceNow Table API. The product target is "agent-forward" use: the primary consumer is an LLM-driven agent (or a script written by one), with a human operator as a secondary but fully supported audience.

"Agent-forward" is a concrete design contract, not a slogan. It means:

- **Stable command tree.** Subcommand names, flag names, and global flag set are versioned and do not silently change. New flags are additive.
- **Predictable JSON on stdout.** Every command that returns data emits valid JSON (or JSONL for streamed pagination). The shape of that JSON is documented per command and does not depend on terminal width, locale, or color settings.
- **Structured JSON errors on stderr.** Errors never appear on stdout. The error envelope is a single, documented schema (see section 6).
- **Deterministic exit codes.** Five codes, each with a single meaning (see section 6). Agents branch on these without parsing text.
- **No interactive surprises.** No command prompts the user unless explicitly interactive (`sn init` only). No pagers. No color-escape pollution of piped output. No "press any key" affordances.
- **Machine-readable help.** Beyond `clap`'s `--help`, `sn introspect` dumps the entire command and flag tree as JSON suitable for generating MCP tool schemas or other agent bindings.

A human-friendly path exists: TTY-aware pretty-printing, readable `--help`, an interactive `sn init`. None of these degrade the machine surface. The rule is: if stdout is not a TTY, behavior is identical regardless of how the binary was launched.

## 2. Command tree

The complete v1 command surface:

```
sn init [--profile NAME] [--instance URL] [--username U] [--password P]
sn auth test [--profile NAME]

sn profile list
sn profile show [NAME]
sn profile remove <NAME>
sn profile use <NAME>

sn table list    <table> [query/output flags]
sn table get     <table> <sys_id> [output flags]
sn table create  <table> [body flags] [output flags]
sn table update  <table> <sys_id> [body flags] [output flags]   # PATCH
sn table replace <table> <sys_id> [body flags] [output flags]   # PUT
sn table delete  <table> <sys_id> [--yes] [--query-no-domain]

sn schema tables  [--filter SUBSTR] [--reference-only] [output flags]
sn schema columns <table> [--filter SUBSTR] [--type TYPE] [--mandatory]
                          [--writable] [--choices-only] [--references-only]
                          [output flags]
sn schema choices <table> <field>

sn introspect           # dump full command tree + flags as JSON for MCP/agent generation
sn --version
```

Global flags apply to every subcommand:

| Flag | Purpose |
| --- | --- |
| `--profile NAME` | Select a named profile from `config.toml`. |
| `--instance-override URL` | Override the selected profile's `instance` for this invocation. |
| `--output json\|raw` | `json` (default) emits the unwrapped payload; `raw` emits the full SN response body verbatim. |
| `--pretty` | Force pretty-printed JSON regardless of TTY detection. |
| `--compact` | Force compact (single-line) JSON regardless of TTY detection. |
| `-v` / `-vv` / `-vvv` | Increase logging verbosity (see section 9). |
| `--timeout SECS` | Per-request timeout. Default 30s. Also settable via `SN_TIMEOUT`. |
| `--no-retry` | Disable the default 429/5xx retry loop. |

### 2.1 Rationale for `update` (PATCH) vs `replace` (PUT)

The Table API exposes both `PATCH /api/now/table/{table}/{sys_id}` (partial update) and `PUT /api/now/table/{table}/{sys_id}` (full replace). They take an identical set of `sysparm_*` query parameters but have very different mutation semantics: PATCH merges the supplied fields onto the existing record, while PUT replaces the record body, blanking any field absent from the request payload.

A single verb that switched on a flag (e.g. `--full`) would invite catastrophic accidents - an agent that forgot the flag could wipe most of a record's writable fields with one call. Splitting the operation into two distinct verbs forces the caller to make the choice deliberately. The names `update` and `replace` were chosen over `patch` and `put` so the verb describes intent rather than HTTP method.

## 3. Parameter mapping (sysparm to friendly)

Every Table API `sysparm_*` query parameter has a friendly long flag and a verbatim alias. The verbatim alias exists so authors of existing scripts and ServiceNow developers familiar with the underlying parameter name do not have to consult a translation table. `clap` rejects flags that do not apply to the chosen verb at parse time, via per-verb argument structs - typos and misuse fail before any network round-trip. Enums are validated at parse time with the same fail-fast guarantee.

| Friendly flag | Raw alias | Type / enum | Applies to |
| --- | --- | --- | --- |
| `--query` | `--sysparm-query` | string | `list` |
| `--fields` | `--sysparm-fields` | CSV string | `list`, `get`, `create`, `update`, `replace` |
| `--page-size` | `--sysparm-limit`, `--limit` | u32 | `list` |
| `--offset` | `--sysparm-offset` | u32 | `list` (see section 13) |
| `--display-value` | `--sysparm-display-value` | enum: `true` \| `false` \| `all` | `list`, `get`, `create`, `update`, `replace` |
| `--exclude-reference-link` | `--sysparm-exclude-reference-link` | bool | `list`, `get`, `create`, `update`, `replace` |
| `--suppress-pagination-header` | `--sysparm-suppress-pagination-header` | bool | `list` |
| `--view` | `--sysparm-view` | string | `list`, `get`, `create`, `update`, `replace` |
| `--query-category` | `--sysparm-query-category` | string | `list` |
| `--query-no-domain` | `--sysparm-query-no-domain` | bool | `list`, `get`, `update`, `replace`, `delete` |
| `--no-count` | `--sysparm-no-count` | bool | `list` |
| `--input-display-value` | `--sysparm-input-display-value` | bool | `create`, `update`, `replace` |
| `--suppress-auto-sys-field` | `--sysparm-suppress-auto-sys-field` | bool | `create`, `update`, `replace` |

Notes:
- "CSV string" means a comma-separated list parsed by the CLI and passed verbatim to ServiceNow as `sysparm_fields=a,b,c`.
- Enums are case-insensitive at parse time but normalized to lowercase on the wire.
- Boolean flags follow the standard `clap` convention: presence implies `true`. To pass `false` explicitly (where it differs from the SN default), use `--flag=false`.
- `--page-size` defaults to `1000`. The primary name is `--page-size`; `--limit` and `--sysparm-limit` are accepted aliases because existing ServiceNow documentation and users reach for that name. All three map to the same `sysparm_limit` wire value. The CLI overrides the ServiceNow default (10,000) with 1,000 to produce friendlier default output for agents and humans.

## 4. Request body input (create/update/replace)

The three mutating verbs share a single body-construction grammar. There are two top-level input modes plus one repeatable kv flag, modelled after `curl` for muscle-memory:

| Flag | Behavior |
| --- | --- |
| `--data '<json>'` | Inline JSON object. Parsed and validated before sending. |
| `--data @path.json` | Read JSON from the file at `path.json`. |
| `--data @-` | Read JSON from stdin. |
| `--field name=value` | Repeatable. Builds an object from key/value pairs. |
| `--field name=@file` | Repeatable. The field's value is the contents of `file` (read as UTF-8 string). |

Precedence and merging:

- `--data` and `--field` are mutually exclusive. If `--data` is present, any `--field` flag is a usage error (exit 1).
- Multiple `--field` flags merge into a single JSON object. A duplicate field name is a usage error (exit 1) so partial overwrites can never happen silently.
- Values from `--field name=value` are sent as JSON strings unless they parse cleanly as `true`, `false`, `null`, an integer, or a float - in which case they are sent as the corresponding JSON scalar. To force a string that looks numeric, use `--data` instead.
- All input must produce a JSON object at the top level. Arrays or scalars are a usage error.

Example:

```
sn table create incident \
  --field short_description="Disk full on prod-db-01" \
  --field urgency=2 \
  --field caller_id=admin
```

## 5. Auth and configuration

### 5.1 Config directory resolution

Resolved via `directories::ProjectDirs::from("", "", "sn")`:

- Linux: `~/.config/sn/` (honors `$XDG_CONFIG_HOME`)
- macOS: `~/Library/Application Support/sn/`
- Windows: `%APPDATA%\sn\` (e.g. `C:\Users\<you>\AppData\Roaming\sn\`)

### 5.2 File layout (AWS-CLI split)

Two TOML files. The split mirrors the AWS CLI rationale: connection metadata is checked into dotfile repos by some users; credentials never should be.

`config.toml`:

```toml
default_profile = "dev"

[profiles.dev]
instance = "acmedev.service-now.com"

[profiles.prod]
instance = "acme.service-now.com"
```

`credentials.toml` (chmod `0600` on Unix; on Windows we rely on the per-user ACL of `%APPDATA%`):

```toml
[profiles.dev]
username = "admin"
password = "..."
```

Both files are written with platform-appropriate line endings. On read, both LF and CRLF are accepted.

### 5.3 Profile selection precedence

Highest wins:

1. `--profile NAME` flag.
2. `SN_PROFILE` environment variable.
3. `default_profile` field in `config.toml`.
4. Fallback to a profile literally named `default` if `[profiles.default]` exists.

If none resolves, any command requiring credentials exits 1 with a usage error directing the user to run `sn init`.

### 5.4 Per-value environment overrides

The following environment variables override the resolved profile's value for a single invocation:

| Env var | Overrides |
| --- | --- |
| `SN_INSTANCE` | `instance` URL |
| `SN_USERNAME` | basic-auth username |
| `SN_PASSWORD` | basic-auth password |
| `SN_TIMEOUT` | request timeout in seconds |

These are useful for CI runners and ephemeral agent environments that should not write to disk.

### 5.5 `sn init`

Interactive by default. Each value can be supplied via flag (`--profile`, `--instance`, `--username`, `--password`) to skip its prompt. When prompted, the password uses hidden input via the `rpassword` crate.

There is intentionally no `--password-stdin` flag; users with automation needs should write `credentials.toml` directly or set `SN_PASSWORD`.

After collection, `sn init` writes the profile to both files (creating them if needed), sets `0600` on `credentials.toml` on Unix, and runs an `sn auth test` against the new profile.

### 5.6 `sn auth test`

Performs `GET /api/now/table/sys_user?sysparm_limit=1` against the selected profile's instance. Reports success or failure on stderr. Exit code follows section 6 mapping (`0` ok, `4` on 401/403, `2` on other 4xx/5xx, `3` on transport error).

### 5.7 Auth method

HTTP Basic only in v1. The Authorization header is built per-request from the resolved username and password. OAuth 2.0 and OS-keychain storage are explicitly deferred (section 14).

## 6. Output contract

### 6.1 Stdout shape

| Verb | Default stdout |
| --- | --- |
| `table list` | The `result` array, unwrapped. |
| `table get` | The `result` object, unwrapped. |
| `table create` | The created record object, unwrapped from `result`. |
| `table update` | The updated record object, unwrapped from `result`. |
| `table replace` | The replaced record object, unwrapped from `result`. |
| `table delete` | Nothing. Exit 0 on success. |
| `schema tables` | Array of table descriptors. |
| `schema columns` | Array of column descriptors. |
| `schema choices` | Array of choice entries. |
| `profile list` | Array of profile names. |
| `profile show` | Profile object (no password). |
| `introspect` | Single command-tree object. |

### 6.2 `--output raw`

Emits the full ServiceNow response body verbatim, preserving the `result` envelope and any other top-level fields (e.g. `_links`). Useful when the agent needs the envelope for reasons the wrapper cannot anticipate.

### 6.3 TTY detection and formatting

When stdout is a TTY, output is pretty-printed with 2-space indentation. When stdout is piped or redirected, output is compact (single-line JSON, or one record per line for JSONL pagination). `--pretty` and `--compact` force the respective mode regardless of detection. `is-terminal` is the detection crate.

### 6.4 Errors on stderr

All errors emit a single JSON object on stderr, regardless of `--output` mode. The schema:

```json
{
  "error": {
    "message": "...",
    "detail": "...",
    "status_code": 404,
    "transaction_id": "abc123",
    "sn_error": { }
  }
}
```

Field semantics:

- `message`: short, human-readable summary.
- `detail`: longer description when available.
- `status_code`: HTTP status from ServiceNow if the failure was an HTTP response. Omitted for transport, config, and usage errors.
- `transaction_id`: contents of the `X-Transaction-ID` response header when present.
- `sn_error`: the verbatim `error` object from a ServiceNow error response, when present.

### 6.5 Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Success. |
| `1` | Usage error or local config error (bad flag combination, missing profile, malformed `--data`, etc.). |
| `2` | API error (4xx or 5xx that is not auth-related). |
| `3` | Network or transport error (DNS, TLS, timeout, connection reset). |
| `4` | Auth error (HTTP 401 or 403). |

Codes are stable across versions.

## 7. Pagination

`sn table list` supports three pagination modes. A single flag, `--page-size N` (default `1000`, aliases `--limit` and `--sysparm-limit`), controls the wire-level `sysparm_limit` on every mode.

1. **Manual**: `--page-size` and `--offset` pass through directly to a single API call. One request, one response of up to `--page-size` records.
2. **Auto streaming**: `--all` follows the `Link: rel="next"` header that ServiceNow emits and streams results. `--page-size` controls the per-API-call batch size; `--offset` is ignored because `--all` walks the full result set from the beginning. Default streaming format is JSONL (one record per line) so an agent or pipeline can begin processing immediately and memory does not blow up on large result sets.
3. **Auto array**: `--all --array` collects all pages into a single JSON array before emitting. Buffers in memory; use only when the consumer requires array shape.

Additional flag:

- `--max-records N` is a hard cap on total records returned. Default `100000`. `0` disables the cap (use with care). Applies to both manual and `--all` modes, though in manual mode it is usually redundant with `--page-size`.

When the cap is hit, the command exits 0 and emits a stderr warning (still as a structured JSON object with a `warning` key) so the consumer knows the result was truncated.

## 8. Errors, retries, and timeouts

- Default per-request timeout: `30s`. Override via `--timeout SECS` or `SN_TIMEOUT`.
- Retry policy: 429, 502, 503, 504. Three attempts total. Exponential backoff with jitter, starting at `500ms`. The `backoff` crate provides the schedule.
- `--no-retry` disables the retry loop; the first failure is the final result.
- 4xx other than 429 are never retried.
- The `X-Transaction-ID` header from ServiceNow is captured on every response and included in the error envelope when present, regardless of whether the request was retried.
- Connection-level errors (DNS, TLS, refused, reset) are not retried in v1; they exit 3. (Revisit if real-world agent traffic shows transient TCP issues are common.)

## 9. Observability

Verbosity is controlled by `-v` flags and emits to stderr (never stdout). Log lines are plain-text key=value pairs, not JSON, since they are diagnostic and not part of the machine contract.

| Level | Emits |
| --- | --- |
| (none) | Errors only. |
| `-v` | HTTP method, URL, status, elapsed time. |
| `-vv` | All of the above plus request and response headers. |
| `-vvv` | All of the above plus request body and response body. |

The `Authorization` header is always masked (`Authorization: Basic ****`) regardless of verbosity. Bodies are emitted verbatim at `-vvv`; users running with that level are expected to know they are dumping potentially sensitive payloads.

There is no log file output by default. Agents and CI pipelines capture stderr separately.

## 10. Project layout

Single Rust crate, binary-only for v1. A library split is deferred until a second consumer needs the internals (YAGNI).

```
Cargo.toml
src/
  main.rs               # clap entry, dispatch
  cli/
    mod.rs              # Cli struct, global flags
    init.rs
    auth.rs
    profile.rs
    table.rs            # list/get/create/update/replace/delete subcommands
    schema.rs           # tables/columns/choices subcommands
    introspect.rs
  config.rs             # TOML load/save, paths, chmod
  client.rs             # reqwest blocking client, retry/backoff, auth header
  query.rs              # builds sysparm_* query string from friendly flags
  body.rs               # --data / --field merging
  output.rs             # TTY detection, pretty/compact, JSON/JSONL emitter
  error.rs              # thiserror-based error types, exit-code mapping
tests/
  cli.rs                # assert_cmd integration tests
  client.rs             # wiremock-backed HTTP tests
docs/
  agent-guide.md        # hands-on agent usage
```

### 10.1 Key dependencies

Runtime:

- `clap` (with `derive` and `suggest` features) - argument parsing, help generation, did-you-mean suggestions.
- `reqwest` (blocking client, `rustls-tls` only - no system OpenSSL dependency).
- `serde`, `serde_json` - JSON serialization and parsing.
- `toml` - config file format.
- `directories` - cross-platform config dir resolution.
- `rpassword` - hidden password prompt.
- `thiserror` - error type derivation.
- `is-terminal` - TTY detection.
- `backoff` - retry schedule with jitter.

Dev-only:

- `wiremock` - HTTP mocking for client tests.
- `assert_cmd` - integration testing of the CLI binary.
- `predicates` - assertions over command output.

## 11. Cross-platform and distribution

Supported targets:

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `x86_64-pc-windows-msvc`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`

Distribution channels:

- `cargo-dist` produces release artifacts (tarballs, zip, signed where applicable) and maintains a Homebrew tap.
- `cargo install sn` for users who already have a Rust toolchain.

CI (GitHub Actions):

- On every PR: `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check`.
- On tag push: release workflow that builds artifacts for every supported target and publishes via `cargo-dist`.

Windows-specific concessions:

- `chmod` is skipped on `credentials.toml`; the per-user ACL of `%APPDATA%` is the access boundary.
- Long-path prefix (`\\?\`) handling for config paths longer than 260 chars.
- TOML reader accepts both LF and CRLF line endings.

## 12. Agent documentation plan

Agents receive three layers of documentation, each authoritative for a different consumer:

1. **`clap`-generated `--help`.** Every flag, every enum value, every short description is generated from the same code that parses the arguments. This is parse-time accurate by construction. Reachable via `sn --help`, `sn table --help`, `sn table list --help`, etc.

2. **`sn introspect --json`.** Dumps the full command and flag tree as a single JSON document. Each command node lists its subcommands, positional arguments, named flags, flag types, allowed enum values, required/optional status, and short descriptions. This is the canonical input for an MCP server builder or any agent framework that needs to generate tool schemas without parsing free-form `--help` text.

3. **`docs/agent-guide.md`.** Hand-written usage patterns aimed at LLM agents: common workflows, when to prefer `update` over `replace`, how to read pagination warnings, etc. Authored separately from this spec.

### 12.1 Recommended agent discovery flow

When an agent is asked to manipulate an unfamiliar ServiceNow table:

1. `sn schema tables --filter <hint>` to find the table by name fragment.
2. `sn schema columns <table> --writable` to learn what fields can be set.
3. `sn schema choices <table> <field>` for any choice (enum) field about to be set, so the agent picks a valid value.
4. `sn table create <table> --field ...` (or `update` / `replace`) with confidence.

This flow is documented in the agent guide and is what `sn introspect` is designed to make discoverable to autonomous tools.

## 13. Spec deviations

Each deviation from the OpenAPI spec at `/Users/abey/Projects/servicenow-tools/openapi/now-Table_API.json` is documented here so future readers understand why the wrapper does not match the published contract one-for-one.

- **`sysparm_offset` added.** Absent from the OpenAPI file but present in the live API. It is required for `--all` pagination and is widely used in practice. The wrapper exposes it as `--offset` / `--sysparm-offset`.
- **`update` vs `replace` verbs.** The OpenAPI spec exposes PATCH and PUT as distinct operations. We split them into two named CLI verbs because they share the same `sysparm_*` parameter set but differ in mutation semantics. Forcing the caller to pick a verb prevents accidental field-wipe via PUT.
- **JSON only.** The OpenAPI spec advertises `application/xml` responses. The wrapper always sends `Accept: application/json` and only parses JSON. XML support is not planned.
- **`sn schema` endpoints are undocumented in OpenAPI.** `GET /api/now/doc/table/schema` and `GET /api/now/ui/meta/{tablename}` are widely used (the platform UI relies on them) and stable in practice but absent from the OpenAPI catalog. The wrapper surfaces a clear error if the endpoint returns 404 on a given instance. Source: https://davidmac.pro/posts/2021-11-26-schema-meta-api/

## 14. Out of scope for v1

The following are deliberately deferred:

- **OAuth 2.0.** Basic auth only in v1.
- **OS keychain storage for credentials.** Plaintext TOML with `chmod 0600` on Unix, ACL-protected `%APPDATA%` on Windows. Keychain integration arrives with OAuth.
- **Other ServiceNow APIs.** The Attachment, Aggregate, Import Set, and Batch APIs each get their own subcommand later. Each has distinct semantics that justify dedicated surface area.
- **Shell completion scripts.** Trivial to add via `clap_complete` once the command tree is stable; not needed for v1.
- **Schema response caching.** One extra HTTP call per agent workflow is cheap. Revisit only if real-world traffic shows the schema endpoints are being hammered.
