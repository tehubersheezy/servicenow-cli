# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this project is

`sn` is a single-binary Rust CLI that wraps the ServiceNow Table API and two undocumented schema-discovery endpoints. It is designed to be invoked by LLM agents — stable JSON on stdout, structured JSON errors on stderr, deterministic exit codes, no interactive surprises unless explicitly opted into (`sn init`).

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
  lib.rs            → re-exports all modules for integration tests
  error.rs          → Error enum (5 variants), exit_code(), to_stderr_json()
  output.rs         → emit_value (JSON), emit_jsonl (JSONL), emit_error (stderr)
  config.rs         → Config/Credentials TOML types, load/save, resolve_profile()
  client.rs         → reqwest blocking client (proxy/TLS), Paginator iterator
  query.rs          → ListQuery/GetQuery/WriteQuery/DeleteQuery → Vec<(String,String)>
  body.rs           → --data / --field parsing into serde_json::Value
  observability.rs  → global AtomicU8 verbosity, log_request/response/headers/body
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
```

### CICD async pattern

CICD operations (`app`, `updateset`, `atf`) are async — they return a `progress_id` immediately and the operation runs in the background on the ServiceNow instance. Poll for completion with `sn progress <progress_id>`. The progress response includes a `state` field (`running`, `complete`, `failed`) and a `percentComplete` indicator. All three command groups share the same polling mechanism via `cli/progress.rs`.

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

`--profile` flag > `SN_PROFILE` env > `default_profile` in config.toml > literal "default" profile. Per-field overrides: `SN_INSTANCE`, `SN_USERNAME`, `SN_PASSWORD` override the resolved profile's values.

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

`client.paginate()` returns a `Paginator` iterator that follows `Link: rel="next"` headers. Default output is JSONL (one record per line); `--array` buffers into a JSON array. `--max-records` caps total output. The page size is fixed at whatever `--setlimit` sets (default 1000).

### Schema endpoints (undocumented)

`GET /api/now/doc/table/schema` — list all accessible tables.
`GET /api/now/ui/meta/{table}` — column metadata including choices and references.
These are not in ServiceNow's OpenAPI specs but are used by the platform UI. They may return 404 on very old instances.

## Conventions

- Every sysparm_* parameter has a friendly flag name (e.g. `--query`) and a raw alias (`--sysparm-query`). Both map to the same field. Defined in `cli/mod.rs` via clap's `alias` attribute.
- `update` = PATCH (partial), `replace` = PUT (full overwrite). Separate verbs prevent accidental field-wipe.
- `pub(crate)` helpers in `cli/table.rs` (`build_profile`, `build_client`, `bool_opt`, `format_from_flags`, `unwrap_or_raw`) are shared by all command modules.

## CI/CD

- `.github/workflows/ci.yml` — fmt + clippy + test on ubuntu/macos/windows, triggered on PRs and pushes to main.
- `.github/workflows/release.yml` — cargo-dist release on tag push. Builds for x86_64-linux, aarch64-linux, x86_64-macos, aarch64-macos, x86_64-windows. Produces shell/powershell/homebrew installers.
- `dist-workspace.toml` — cargo-dist configuration (target triples, installer types).
