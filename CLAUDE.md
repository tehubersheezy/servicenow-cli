# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this project is

`sn` is a single-binary Rust CLI that wraps ServiceNow's REST APIs: Table API, Change Management, Attachment, CMDB, Import Set, Service Catalog, Identification & Reconciliation, CICD (App Repository, Update Sets, ATF), Aggregate, Performance Analytics, and two undocumented schema-discovery endpoints. Designed for LLM agents — stable JSON on stdout, structured JSON errors on stderr, deterministic exit codes, no interactive surprises unless explicitly opted into (`sn init`).

## Build, test, lint

```bash
cargo build                    # dev build
cargo build --release          # release build (stripped, LTO)
cargo test --workspace         # all unit + integration tests
cargo test --lib query::       # run tests in a specific module
cargo test --test pagination   # run a specific integration test file
cargo clippy --all-targets --all-features -- -D warnings   # lint (must pass before commit)
cargo fmt --all -- --check     # format check
```

Integration tests use `wiremock` to mock ServiceNow and `assert_cmd` to drive the compiled binary. Tests that call `reqwest::blocking::Client` inside `#[tokio::test]` **must** wrap both client construction and method calls in `tokio::task::spawn_blocking` — otherwise the blocking runtime panics on drop inside an async context.

## Architecture

### Module layout

```
src/
  main.rs           → parse Cli, set verbosity, dispatch, map Error → ExitCode
  lib.rs            → pub mod {body, cli, client, config, error, observability, output, query} — add new modules here too
  error.rs          → Error enum (5 variants), exit_code(), to_stderr_json()
  output.rs         → emit_value (JSON), emit_jsonl (JSONL), emit_error (stderr)
  config.rs         → Config/Credentials TOML types, load/save, resolve_profile()
  client.rs         → reqwest blocking client (proxy/TLS), Paginator iterator
  query.rs          → ListQuery/GetQuery/WriteQuery/DeleteQuery → Vec<(String,String)>
  body.rs           → --data / --field parsing into serde_json::Value
  observability.rs  → global AtomicU8 verbosity, log helpers (set_level called in main; log_request/response not yet wired into client)
  cli/
    mod.rs          → Cli struct, GlobalFlags, all Subcommand enums + arg structs
    init.rs         → sn init (interactive profile setup + credential verification)
    auth.rs         → sn auth test (GET sys_user with limit=1)
    profile.rs      → sn profile list/show/remove/use
    table.rs        → sn table list/get/create/update/replace/delete + shared helpers
    schema.rs       → sn schema tables/columns/choices (undocumented SN endpoints)
    introspect.rs   → sn introspect (dumps clap command tree as JSON)
    progress.rs     → sn progress (poll async CICD operations by progress_id)
    app.rs          → sn app install/publish/rollback (App Repository lifecycle)
    update_set.rs   → sn updateset create/retrieve/preview/commit/commit-multiple/back-out
    atf.rs          → sn atf run/results (Automated Test Framework)
    aggregate.rs    → sn aggregate (server-side stats/counts/averages on table data)
    scores.rs       → sn scores list/favorite/unfavorite (Performance Analytics scorecards)
    change.rs       → sn change list/get/create/update/delete + task/ci/conflict/nextstates/approvals/risk/schedule/models/templates
    attachment.rs   → sn attachment list/get/upload/download/delete (binary file support)
    cmdb.rs         → sn cmdb list/get/create/update/replace/meta + relation add/delete
    import.rs       → sn import create/bulk/get (staging table imports)
    catalog.rs      → sn catalog list/get/categories/items/order/cart/checkout/wishlist
    identify.rs     → sn identify create-update/query + enhanced variants (CI reconciliation)
```

### CICD async pattern

CICD operations (`app`, `updateset`, `atf`) are async — they return a `progress_id` immediately and the operation runs in the background on the ServiceNow instance. The preferred way to wait for completion is `--wait`, which blocks the command until the operation succeeds or fails (polling `GET /api/sn_cicd/progress/{id}` every 2 seconds) and then emits the final progress result — eliminating the need for manual `sn progress` polling. Without `--wait`, the command returns immediately with the initial progress object. For operations already in flight, poll manually with `sn progress <progress_id>`. The progress response includes a `state` field (`running`, `complete`, `failed`) and a `percentComplete` indicator. All three command groups share the same polling mechanism via `cli/progress.rs`.

### Client binary methods

`client.rs` includes three methods beyond the standard JSON HTTP verbs for the Attachment API:
- `upload_file(path, query, body: Vec<u8>, content_type)` — POST raw binary with custom Content-Type
- `download_file(path) -> (Vec<u8>, Option<String>)` — GET binary response, returns bytes + Content-Type
- `delete_json(path, query) -> Value` — DELETE that expects a JSON response body (vs `delete()` which returns `()`)

### Change Management API

Uses `/api/sn_chg_rest/change` with type-specific sub-paths (`/normal`, `/emergency`, `/standard`). The `--type` flag routes to the correct endpoint. Standard change creation requires `--template <id>`. Supports nested sub-resources: tasks (`/task`), CIs (`/ci`), conflicts (`/conflict`), plus state-related operations (nextstates, approvals, risk, schedule).

### Service Catalog API

Uses `/api/sn_sc/servicecatalog`. Supports the full shopping cart workflow: browse catalogs/categories/items → add to cart → checkout/submit order. Also supports direct ordering via `order` (bypasses cart). Item variables endpoint exposes the form fields required before ordering.

### CMDB APIs

Instance API (`/api/now/cmdb/instance/{className}`) provides CRUD + relation management on any CMDB class. The class name is a positional arg. Meta API (`/api/now/cmdb/meta/{className}`) returns schema metadata for a class. Both are combined under the `sn cmdb` command group.

### Import Set API

Uses `/api/now/import/{stagingTableName}`. Supports single record creation and bulk insert via `insertMultiple`. The staging table name is a positional arg.

### Identification & Reconciliation API

Uses `/api/now/identifyreconcile`. POST-only pattern for CI creation/updates and read-only queries. Enhanced variants accept `--options` for partial payload/commit support. All operations take `--data` for the items payload.

### Key data flow

1. `main.rs` parses `Cli` via clap derive, sets observability level, destructures `Cli { global, command }`.
2. Each command handler receives `&GlobalFlags` and its typed args struct.
3. `build_profile(&GlobalFlags)` resolves which ServiceNow instance + credentials to use (flag > env > config file precedence).
4. `build_client(&profile, timeout)` creates a reqwest blocking client with basic auth, proxy, and TLS settings.
5. Query structs (`ListQuery`, etc.) convert friendly flags to `sysparm_*` query pairs.
6. Responses are unwrapped from `{"result": ...}` by default; `--output raw` preserves the envelope.
7. Errors always go to stderr as `{"error": {"message", "detail?", "status_code?", "transaction_id?", "sn_error?"}}`.

### Exit codes

`0` success, `1` usage/config, `2` API 4xx/5xx (non-auth), `3` network/transport, `4` auth (401/403).

### Profile resolution precedence

`--profile` flag > `SN_PROFILE` env > `default_profile` in config.toml > literal "default" profile. Per-field overrides: `SN_INSTANCE`, `SN_USERNAME`, `SN_PASSWORD` override the resolved profile's values. `--instance-override URL` overrides the selected profile's instance URL for a single invocation (useful for one-off commands against a different instance without switching profiles).

### Proxy and TLS

Proxy and TLS settings follow the same precedence as profile fields: CLI flag > env var > profile config file.

| CLI flag | Env var | config.toml field | Description |
|---|---|---|---|
| `--proxy <URL>` | `SN_PROXY` | `proxy` | HTTP/HTTPS/SOCKS5 proxy URL |
| `--no-proxy` | — | — | Bypass proxy for this invocation |
| — | `SN_NO_PROXY` | `no_proxy` | Comma-separated hosts to bypass proxy |
| `--insecure` | `SN_INSECURE=1` | `insecure` | Disable TLS cert verification |
| `--ca-cert <PATH>` | `SN_CA_CERT` | `ca_cert` | Custom CA cert for ServiceNow |
| `--proxy-ca-cert <PATH>` | `SN_PROXY_CA_CERT` | `proxy_ca_cert` | Custom CA cert for proxy |

Proxy authentication is stored in `credentials.toml` per-profile via `proxy_username` and `proxy_password` fields.

### Config file locations

Resolved via `directories::ProjectDirs::from("", "", "sn")`:
- Linux: `~/.config/sn/{config.toml, credentials.toml}`
- macOS: `~/Library/Application Support/sn/...`
- Windows: `%APPDATA%\sn\...`

`credentials.toml` is `chmod 0600` on Unix. The `XDG_CONFIG_HOME` override only works on Linux (the `directories` crate uses platform-native paths on macOS/Windows), which is why `tests/init.rs` is `#[cfg(target_os = "linux")]`.

### Pagination (--all)

`client.paginate()` returns a `Paginator` iterator that follows `Link: rel="next"` headers. Default output is JSONL (one record per line); `--array` buffers into a JSON array. `--max-records` caps total output. The page size is fixed at whatever `--setlimit` sets (default 1000). `--max-records` caps total output (default 100,000; 0 = unlimited).

### Schema endpoints (undocumented)

`GET /api/now/doc/table/schema` — list all accessible tables.
`GET /api/now/ui/meta/{table}` — column metadata including choices and references.
These are not in ServiceNow's OpenAPI specs but are used by the platform UI. They may return 404 on very old instances.

## Conventions

- Every sysparm_* parameter has a friendly flag name (e.g. `--query`) and a raw alias (`--sysparm-query`). Both map to the same field. Defined in `cli/mod.rs` via clap's `alias` attribute.
- `update` = PATCH (partial), `replace` = PUT (full overwrite). Separate verbs prevent accidental field-wipe.
- `pub(crate)` helpers in `cli/table.rs` (`build_profile`, `build_client`, `bool_opt`, `format_from_flags`, `unwrap_or_raw`) are shared by all command modules.

## Claude Code plugin

This repo is also a Claude Code plugin (`.claude-plugin/plugin.json` + `skills/sn/SKILL.md`). The plugin skill mirrors `.claude/skills/sn.md` but adds `allowed-tools: Bash(sn *)` for auto-approved CLI access. The local skill at `.claude/skills/sn.md` is for in-repo use (invoked as `/sn`); the plugin skill at `skills/sn/SKILL.md` is for external distribution.

## CI/CD

- `.github/workflows/ci.yml` — fmt + clippy + test on ubuntu/macos/windows, triggered on PRs and pushes to main.
- `.github/workflows/release.yml` — cargo-dist release on tag push. Builds for x86_64-linux, aarch64-linux, x86_64-macos, aarch64-macos, x86_64-windows. Produces shell/powershell/homebrew installers.
- `dist-workspace.toml` — cargo-dist configuration (target triples, installer types).
