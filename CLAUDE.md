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
  lib.rs            → pub mod {body, cli, client, config, error, observability, output, output_table, query} — add new modules here too
  error.rs          → Error enum (5 variants), exit_code(), to_stderr_json()
  output.rs         → emit_value (JSON), emit_jsonl (JSONL), emit_error (stderr)
  output_table.rs   → write_table (renders JSON object/array as a comfy-table columnar view for `--output table`)
  config.rs         → Config/Credentials TOML types, load/save, resolve_profile(); OAuth types (AuthMethod, OAuthConfig, OAuthGrant, TokenSet) + token persistence helpers
  client.rs         → reqwest blocking client (proxy/TLS), Auth enum (Basic/Bearer/None), Paginator iterator
  oauth.rs          → OAuth 2.0 for SSO instances: PKCE, loopback redirect server, token exchange (authorization_code/refresh/client_credentials), ensure_access_token()
  query.rs          → ListQuery/GetQuery/WriteQuery/DeleteQuery → Vec<(String,String)>
  body.rs           → --data / --field parsing into serde_json::Value
  observability.rs  → global AtomicU8 verbosity, log helpers (set_level called in main; log_request/response not yet wired into client)
  cli/
    mod.rs          → Cli struct, GlobalFlags, all Subcommand enums + arg structs
    init.rs         → sn init (interactive profile setup — basic or oauth — + credential verification; oauth branch reuses auth::complete_oauth_login)
    auth.rs         → sn auth login/logout/status/refresh (pure OAuth session commands; login runs the flow for an already-configured oauth profile — no config mutation)
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
    user.rs         → sn user me (resolves the authenticated user via gs.getUserName())
    ping.rs         → sn ping (auth + latency + ServiceNow build version)
    open_record.rs  → sn open <table> <sys_id> (launches the form in the default browser)
    raw.rs          → sn raw <method> <path> (REST passthrough for unmodeled endpoints)
    completion.rs   → sn completion <shell> (clap_complete script generation)
```

### CICD async pattern

CICD operations (`app`, `updateset`, `atf`) are async — they return a `progress_id` immediately and the operation runs in the background on the ServiceNow instance. The preferred way to wait for completion is `--wait`, which blocks the command until the operation succeeds or fails (polling `GET /api/sn_cicd/progress/{id}` every 2 seconds) and then emits the final progress result — eliminating the need for manual `sn progress` polling. `--wait-timeout <SECS>` (requires `--wait`) bounds the total wait; on expiry the command exits 3 with a pointer to `sn progress`. Without `--wait`, the command returns immediately with the initial progress object. For operations already in flight, poll manually with `sn progress <progress_id>`. The progress response includes a `state` field (`running`, `complete`, `failed`) and a `percentComplete` indicator. All command groups share the same tail via `cli/progress.rs::finish_cicd` (progress-link extraction + polling + emission through `write_response`, so `--output table` works under `--wait`) — new async commands must route through it rather than open-coding the wait block.

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
3. `build_profile(&GlobalFlags)` resolves which ServiceNow instance + credentials to use (a profile is the whole unit of identity: `--profile` > `default_profile`; no per-field argv overrides).
4. `build_client(&profile, timeout)` creates a reqwest blocking client with basic auth, proxy, and TLS settings.
5. Query structs (`ListQuery`, etc.) convert friendly flags to `sysparm_*` query pairs.
6. Responses are unwrapped from `{"result": ...}` by default; `--output raw` preserves the envelope.
7. Errors always go to stderr as `{"error": {"message", "detail?", "status_code?", "transaction_id?", "sn_error?"}}`.

### Exit codes

`0` success, `1` usage/config, `2` API 4xx/5xx (non-auth), `3` network/transport, `4` auth (401/403). Clap parse errors are intercepted in `main.rs` (`handle_clap_error`) so they honor this contract too: exit 1, with the JSON error envelope on stderr when stderr is not a TTY (clap's human-readable text when it is). `--help`/`--version` still exit 0.

### Profile resolution precedence

A profile is the single unit of identity: commands either manage profiles (`sn init`, `sn profile *`) or use exactly one. Selection is `--profile` flag > `default_profile` in config.toml; if neither resolves, `resolve_profile_name` (in `config.rs`) returns `Error::Config("no profile selected; pass --profile <name> or run \`sn init\`")` (exit 1) — there is no phantom literal-`"default"` fallback. There are **no** per-field argv overrides: the old `--instance-override`, `--username`, and `--password` global flags have been deleted (they grafted argv fragments onto disk state and, on OAuth profiles, `--instance-override` could exfiltrate the refresh token + client secret to an arbitrary host). Change identity by editing the profile via `sn init` or selecting a different one via `--profile`. A profile with an empty/whitespace `instance` is rejected rather than silently yielding a `"https://"` base URL. There are deliberately no env vars for credential values or profile selection. Proxy/TLS env vars (`SN_PROXY` etc.) and the `SN_CONFIG_DIR` config-directory override (see below) still exist.

### OAuth / SSO authentication

A profile authenticates via one of two methods, selected by `auth = "basic"` (default) or `auth = "oauth"` in its `config.toml` entry. OAuth is the path for instances fronted by external SSO (Okta/Azure AD/ADFS), where a human's password lives in the IdP — so HTTP Basic and the OAuth password grant cannot work.

- **Non-secret OAuth config** (client_id, redirect_uri, endpoint overrides, grant, pkce) lives in `config.toml` under `[profiles.<name>.oauth]`. **The client secret and cached tokens** live in `credentials.toml` (`chmod 0600`), mirroring the username/password split.
- **Two grants:** `authorization_code` (interactive — opens a browser, runs a loopback redirect server per RFC 8252, uses PKCE S256 by default; registered as a **public client**, so `sn init` neither needs nor prompts for a secret — pass `--client-secret` only for a confidential authorization-code client) and `client_credentials` (non-interactive, confidential, always requires a secret). The loopback `redirect_uri` (default `http://localhost:8400/callback`) **must be registered exactly** in ServiceNow's Application Registry.
- **Endpoints:** authorization `GET /oauth_auth.do`, token `POST /oauth_token.do` (overridable per profile).
- **Commands:** `sn auth login` (a pure session command with no flags — it resolves the selected profile, requires `auth = "oauth"` with an `[oauth]` block, runs the flow using the stored grant, and caches tokens; a basic profile errors with "does not use oauth; run \`sn init\`"), `sn auth status`, `sn auth refresh`, `sn auth logout`. All four emit their success JSON to **stdout**. OAuth profiles are configured (client_id, secret, grant, etc.) via `sn init --auth oauth`, not via login flags. `sn auth test` no longer exists — use `sn ping` (auth + latency + instance version). **Transparent refresh:** `build_client` (in `cli/table.rs`) calls `oauth::ensure_access_token` for OAuth profiles before every request — it returns a cached token, refreshes a stale one via the refresh token (or mints a fresh one for client_credentials), and persists any new tokens. All command handlers get this for free with no call-site changes. The `Client` itself is auth-agnostic: a single `Auth` enum (`Basic`/`Bearer`/`None`) is applied in `send()`.

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

Resolved by `config::config_dir()`. If `SN_CONFIG_DIR` is set to a non-empty value it is used as-is — the documented, cross-platform override that points **directly** at the directory holding `config.toml`/`credentials.toml` (no `sn` subdir is appended). Otherwise `directories::ProjectDirs::from("", "", "sn")` gives the platform-native location:
- Linux: `~/.config/sn/{config.toml, credentials.toml}`
- macOS: `~/Library/Application Support/sn/...`
- Windows: `%APPDATA%\sn\...`

`credentials.toml` is `chmod 0600` on Unix. `SN_CONFIG_DIR` is the config-isolation mechanism for tests on every platform (superseding the old Linux-only `XDG_CONFIG_HOME` hack), so config-dependent integration tests are no longer `#[cfg(target_os = "linux")]`-gated. Set `SN_CONFIG_DIR` directly at a temp dir containing the seeded config files (never a `/sn` subdir).

### Pagination (--all)

`client.paginate()` returns a `Paginator` iterator that follows `Link: rel="next"` headers. Default output is JSONL (one record per line); `--array` buffers into a JSON array. `--max-records` caps total output. The page size is fixed at whatever `--setlimit` sets (default 1000). `--max-records` caps total output (default 100,000; 0 = unlimited).

### Schema endpoints (undocumented)

`GET /api/now/doc/table/schema` — list all accessible tables.
`GET /api/now/ui/meta/{table}` — column metadata including choices and references.
These are not in ServiceNow's OpenAPI specs but are used by the platform UI. They may return 404 on very old instances.

## Conventions

- Every sysparm_* parameter has a friendly flag name (e.g. `--query`) and a raw alias (`--sysparm-query`). Both map to the same field. Defined in `cli/mod.rs` via clap's `alias` attribute.
- `update` = PATCH (partial), `replace` = PUT (full overwrite). Separate verbs prevent accidental field-wipe.
- `pub(crate)` helpers in `cli/table.rs` (`build_profile`, `build_client`, `bool_opt`, `format_from_flags`, `unwrap_or_raw`, `write_response`) are shared by all command modules.
- Output mode dispatch: every command's final emission goes through `cli::table::write_response(global, &value)`, which routes to either `output::emit_value` (JSON, default/raw) or `output_table::write_table` (`--output table`). `unwrap_or_raw` treats `OutputMode::Table` like `Default` (envelope unwrapped, then rendered as a column table).

## Claude Code plugin

This repo is also a Claude Code plugin (`.claude-plugin/plugin.json` + `skills/sn/SKILL.md`). The plugin skill mirrors `.claude/skills/sn.md` but adds `allowed-tools: Bash(sn *)` for auto-approved CLI access. The local skill at `.claude/skills/sn.md` is for in-repo use (invoked as `/sn`); the plugin skill at `skills/sn/SKILL.md` is for external distribution.

## CI/CD

- `.github/workflows/ci.yml` — fmt + clippy + test on ubuntu/macos/windows, triggered on PRs and pushes to main.
- `.github/workflows/security.yml` — `cargo audit` (RustSec advisories) + `cargo deny check all` (license/source/banned-crate policy via `deny.toml`). Runs on dep changes and daily at 06:00 UTC.
- `.github/workflows/scorecard.yml` — OSSF Scorecard analysis. Weekly cron + on push to main. Publishes to scorecard.dev.
- `.github/workflows/release.yml` — cargo-dist release on `v*` tag push. Builds for {x86_64,aarch64}-{linux,macos,windows}. Produces shell/PowerShell installers, Windows MSI installers, Homebrew formula, and signed build provenance attestations (verifiable with `gh attestation verify`). Auto-publishes the formula to `tehubersheezy/homebrew-sn` via `HOMEBREW_TAP_TOKEN`.
- `dist-workspace.toml` — cargo-dist configuration (target triples, installer types, tap, attestations).
- `wix/main.wxs` — WiX template for the Windows MSI installer (generated by `dist init`; GUIDs in `[package.metadata.wix]` of `Cargo.toml` keep MSI upgrades stable across versions).
- `.github/dependabot.yml` — weekly grouped dep-update PRs for cargo + github-actions ecosystems.
