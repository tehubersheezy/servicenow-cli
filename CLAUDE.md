# CLAUDE.md

Guidance for Claude Code (claude.ai/code) working in this repository.

## What this project is

`sn` is a single-binary Rust CLI wrapping ServiceNow's REST APIs: Table, Change Management, Attachment, CMDB, Import Set, Service Catalog, Identification & Reconciliation, CICD (App Repository, Update Sets, ATF), Aggregate, Performance Analytics, and two undocumented schema-discovery endpoints. Built for LLM agents: stable JSON on stdout, structured JSON errors on stderr, deterministic exit codes, no interactive surprises unless opted into (`sn init`).

## Build, test, lint

```bash
cargo build                    # dev build
cargo build --release          # release (stripped, LTO)
cargo test --workspace         # unit + integration
cargo test --lib query::       # one module
cargo test --test pagination   # one integration file
cargo clippy --all-targets --all-features -- -D warnings   # must pass before commit
cargo fmt --all -- --check
```

Integration tests use `wiremock` to mock ServiceNow and `assert_cmd` to drive the compiled binary. Tests calling `reqwest::blocking::Client` inside `#[tokio::test]` **must** wrap both client construction and method calls in `tokio::task::spawn_blocking`, or the blocking runtime panics on drop inside an async context.

## Architecture

### Module layout

```
src/
  main.rs           → parse Cli, set verbosity, dispatch, map Error → ExitCode
  lib.rs            → pub mod {body, cli, client, config, error, observability, output, output_table, query} — register new modules here
  error.rs          → Error enum (5 variants), exit_code(), to_stderr_json()
  output.rs         → emit_value (JSON), emit_jsonl (JSONL), emit_error (stderr)
  output_table.rs   → write_table (renders JSON as a comfy-table columnar view for `--output table`)
  config.rs         → Config/Credentials TOML types, load/save, resolve_profile(); OAuth types (AuthMethod, OAuthConfig, OAuthGrant, TokenSet) + token persistence
  client.rs         → reqwest blocking client (proxy/TLS), Auth enum (Basic/Bearer/None), Paginator iterator
  oauth.rs          → OAuth 2.0 for SSO: PKCE, loopback redirect server, token exchange (authorization_code/refresh/client_credentials), ensure_access_token()
  query.rs          → ListQuery/GetQuery/WriteQuery/DeleteQuery → Vec<(String,String)>
  body.rs           → --data / --field parsing into serde_json::Value
  observability.rs  → global AtomicU8 verbosity, log helpers (set_level called in main; log_request/response/body wired into client.rs)
  cli/
    mod.rs          → Cli struct, GlobalFlags, all Subcommand enums + arg structs
    init.rs         → sn init (interactive profile setup — basic or oauth — + verification; oauth branch reuses auth::complete_oauth_login)
    auth.rs         → sn auth login/logout/status/refresh (OAuth session commands; login runs the flow for a configured oauth profile, no config mutation)
    profile.rs      → sn profile list/show/remove/use
    table.rs        → sn table list/get/create/update/replace/delete + shared helpers
    schema.rs       → sn schema tables/columns/choices (undocumented SN endpoints)
    introspect.rs   → sn introspect (dumps clap command tree as JSON)
    progress.rs     → sn progress + finish_cicd (poll async CICD ops)
    app.rs          → sn app install/publish/rollback (App Repository)
    update_set.rs   → sn updateset create/retrieve/preview/commit/commit-multiple/back-out
    atf.rs          → sn atf run/results (Automated Test Framework)
    aggregate.rs    → sn aggregate (server-side stats/counts/averages)
    scores.rs       → sn scores list/favorite/unfavorite (Performance Analytics)
    change.rs       → sn change list/get/create/update/delete + task/ci/conflict/nextstates/approvals/risk/schedule/models/templates
    attachment.rs   → sn attachment list/get/upload/download/delete (binary)
    cmdb.rs         → sn cmdb list/get/create/update/replace/meta + relation add/delete
    import.rs       → sn import create/bulk/get (staging-table imports)
    catalog.rs      → sn catalog list/get/categories/items/order/cart/checkout/wishlist
    identify.rs     → sn identify create-update/query + enhanced variants (CI reconciliation)
    user.rs         → sn user me (authenticated user via gs.getUserName())
    ping.rs         → sn ping (auth + latency + ServiceNow build version)
    open_record.rs  → sn open <table> <sys_id> (opens the form in the browser)
    raw.rs          → sn raw <method> <path> (REST passthrough for unmodeled endpoints)
    completion.rs   → sn completion <shell> (clap_complete)
```

### Key data flow

1. `main.rs` parses `Cli` (clap derive), sets verbosity, destructures `Cli { global, command }`; each handler receives `&GlobalFlags` + its typed args struct.
2. `build_profile(&GlobalFlags)` resolves instance + credentials — a profile is the whole unit of identity (`--profile` > `default_profile`; no per-field argv overrides).
3. `build_client(&profile, timeout)` builds a reqwest blocking client (auth, proxy, TLS).
4. Query structs (`ListQuery`, etc.) convert friendly flags → `sysparm_*` pairs.
5. Responses unwrap `{"result": ...}` by default (`--output raw` keeps the envelope); errors go to stderr as `{"error": {message, detail?, status_code?, transaction_id?, sn_error?}}`.

### Exit codes

`0` success, `1` usage/config, `2` API 4xx/5xx (non-auth), `3` network/transport, `4` auth (401/403). Clap parse errors are intercepted in `main.rs` (`handle_clap_error`) to honor this contract: exit 1, JSON error envelope on stderr when stderr is not a TTY (clap's human text when it is). `--help`/`--version` still exit 0.

### REST endpoint map

Base paths and quirks not obvious from the module list:

| Group | Base path(s) | Notes |
|---|---|---|
| `change` | `/api/sn_chg_rest/change` + `/normal`, `/emergency`, `/standard` | `--type` routes to the sub-path; standard requires `--template <id>`; nested `/task`, `/ci`, `/conflict` + state ops (nextstates, approvals, risk, schedule) |
| `catalog` | `/api/sn_sc/servicecatalog` | full cart flow (browse → add → checkout) or direct `order` (bypasses cart); item-variables endpoint exposes required form fields |
| `cmdb` | `/api/now/cmdb/instance/{className}` (CRUD + relations); `/api/now/cmdb/meta/{className}` (schema) | className is a positional arg |
| `import` | `/api/now/import/{stagingTableName}` | single create + bulk via `insertMultiple`; staging table is positional |
| `identify` | `/api/now/identifyreconcile` | POST-only; CI create/update + read-only query; enhanced variants accept `--options`; payload via `--data` |
| schema (undocumented) | `GET /api/now/doc/table/schema` (tables); `GET /api/now/ui/meta/{table}` (columns/choices/refs) | not in SN's OpenAPI specs; may 404 on very old instances |

### CICD async pattern

CICD operations (`app`, `updateset`, `atf`) are async: they return a `progress_id` and run in the background on the instance. Prefer `--wait` — it blocks (polling `GET /api/sn_cicd/progress/{id}` every 2s) until the operation completes, then emits the final progress result. `--wait-timeout <SECS>` (requires `--wait`) bounds the wait; on expiry the command exits 3 with a pointer to `sn progress`. Without `--wait`, the command returns the initial progress object immediately; poll in-flight operations with `sn progress <progress_id>`. Progress responses carry `state` (`running`/`complete`/`failed`) and `percentComplete`. All groups share one tail — `cli/progress.rs::finish_cicd` (progress-link extraction + polling + emission via `write_response`, so `--output table` works under `--wait`); new async commands must route through it, not open-code the wait.

### Client binary methods

`client.rs` adds three methods beyond the JSON HTTP verbs, for the Attachment API:
- `upload_file(path, query, body: Vec<u8>, content_type)` — POST raw binary with a custom Content-Type.
- `download_file(path) -> (Vec<u8>, Option<String>)` — GET binary, returns bytes + Content-Type.
- `delete_json(path, query) -> Value` — DELETE expecting a JSON body (vs `delete()` returning `()`).

### Profile resolution precedence

A profile is the single unit of identity: commands either manage profiles (`sn init`, `sn profile *`) or use exactly one. Selection is `--profile` flag > `default_profile` in config.toml; if neither resolves, `resolve_profile_name` (in `config.rs`) returns `Error::Config("no profile selected; pass --profile <name> or run \`sn init\`")` (exit 1). There are **no** per-field argv overrides — change identity by editing the profile via `sn init` or selecting a different one via `--profile`. A profile with an empty/whitespace `instance` is rejected. There are deliberately no env vars for credential values or profile selection; proxy/TLS env vars (`SN_PROXY` etc.) and the `SN_CONFIG_DIR` config-directory override (see below) still exist.

### OAuth / SSO authentication

A profile authenticates via `auth = "basic"` (default) or `auth = "oauth"` in its `config.toml` entry. OAuth serves SSO-fronted instances (Okta/Azure AD/ADFS) where the password lives in the IdP, so Basic and the password grant can't work.

- **Split by secrecy:** non-secret OAuth config (client_id, redirect_uri, endpoint overrides, grant, pkce) lives in `config.toml` under `[profiles.<name>.oauth]`; the client secret and cached tokens live in `credentials.toml` (`chmod 0600`), mirroring the username/password split.
- **Two grants:** `authorization_code` (interactive browser flow, loopback redirect server per RFC 8252, PKCE S256; registered as a **public client** — `sn init` neither needs nor prompts for a secret; pass `--client-secret` only for a confidential client) and `client_credentials` (non-interactive, confidential, requires a secret). The loopback `redirect_uri` (default `http://localhost:8400/callback`) **must be registered exactly** in the Application Registry.
- **Endpoints:** authorization `GET /oauth_auth.do`, token `POST /oauth_token.do` (overridable per profile).
- **Commands:** `sn auth login` (pure session command, no flags: resolves the selected profile, requires `auth = "oauth"` with an `[oauth]` block — a basic profile errors `does not use oauth; run \`sn init\`` — runs the flow with the stored grant, caches tokens), plus `sn auth status`/`refresh`/`logout`. All four emit success JSON to **stdout**; OAuth profiles are configured via `sn init --auth oauth`.
- **Transparent refresh:** `build_client` (in `cli/table.rs`) calls `oauth::ensure_access_token` before every request for OAuth profiles — returns a cached token, refreshes a stale one via the refresh token (or mints a fresh one for client_credentials), persists new tokens. Call sites need no changes; the `Client` is auth-agnostic (one `Auth` enum `Basic`/`Bearer`/`None`, applied in `send()`).

### Proxy and TLS

Same precedence as profile fields: CLI flag > env var > profile config file.

| CLI flag | Env var | config.toml field | Description |
|---|---|---|---|
| `--proxy <URL>` | `SN_PROXY` | `proxy` | HTTP/HTTPS/SOCKS5 proxy URL |
| `--no-proxy` | — | — | Bypass proxy for this invocation |
| — | `SN_NO_PROXY` | `no_proxy` | Comma-separated hosts to bypass |
| `--insecure` | `SN_INSECURE=1` | `insecure` | Disable TLS cert verification (off by default) |
| `--ca-cert <PATH>` | `SN_CA_CERT` | `ca_cert` | Custom CA cert for ServiceNow |
| `--proxy-ca-cert <PATH>` | `SN_PROXY_CA_CERT` | `proxy_ca_cert` | Custom CA cert for the proxy |

Proxy auth is stored per-profile in `credentials.toml` (`proxy_username`, `proxy_password`).

### Config file locations

Resolved by `config::config_dir()`. A non-empty `SN_CONFIG_DIR` is used as-is — the documented cross-platform override pointing **directly** at the directory holding `config.toml`/`credentials.toml` (no `sn` subdir appended). Otherwise `directories::ProjectDirs::from("", "", "sn")` gives the platform-native path: Linux `~/.config/sn/`, macOS `~/Library/Application Support/sn/`, Windows `%APPDATA%\sn\`.

`credentials.toml` is `chmod 0600` on Unix. `SN_CONFIG_DIR` is the cross-platform config-isolation mechanism for tests, so config-dependent integration tests are not `#[cfg(target_os = "linux")]`-gated. Point it at a temp dir containing the seeded config files (never a `/sn` subdir).

### Pagination (--all)

`client.paginate()` returns a `Paginator` iterator that follows `Link: rel="next"` headers. Default output is JSONL (one record per line); `--array` buffers into a JSON array. Page size is whatever `--setlimit` sets (default 1000); `--max-records` caps total output (default 100,000; 0 = unlimited).

## Conventions

- Every `sysparm_*` parameter has a friendly flag (e.g. `--query`) and a raw alias (`--sysparm-query`) mapping to the same field, via clap's `alias` attribute in `cli/mod.rs`.
- `update` = PATCH, `replace` = PUT — different HTTP methods, but ServiceNow treats both as partial updates (PUT does not blank omitted fields); clear a field by setting it explicitly.
- Shared `pub(crate)` helpers live in `cli/table.rs`: `build_profile`, `build_client`, `bool_opt`, `format_from_flags`, `unwrap_or_raw`, `write_response`.
- Every command's final emission goes through `cli::table::write_response(global, &value)`, which routes to `output::emit_value` (JSON, default/raw) or `output_table::write_table` (`--output table`). `unwrap_or_raw` treats `Table` like `Default` (envelope unwrapped, then rendered).

## Claude Code plugin

This repo is also a Claude Code plugin and its own marketplace (`.claude-plugin/plugin.json` + `marketplace.json` + `skills/sn/SKILL.md`). The plugin skill mirrors `.claude/skills/sn.md` but adds `allowed-tools: Bash(sn *)` for auto-approved CLI access — keep the two files in sync. `.claude/skills/sn.md` is for in-repo use (`/sn`); `skills/sn/SKILL.md` is for external distribution.

## CI/CD

- `ci.yml` — fmt + clippy + test on ubuntu/macos/windows (PRs + pushes to main).
- `security.yml` — `cargo audit` (RustSec) + `cargo deny check all` (license/source/banned-crate policy in `deny.toml`); on dep changes + daily 06:00 UTC.
- `scorecard.yml` — OSSF Scorecard; weekly + push to main; publishes to scorecard.dev.
- `release.yml` — cargo-dist on `v*` tag: builds {x86_64,aarch64}-{linux,macos,windows}, shell/PowerShell/MSI installers, Homebrew formula, and signed provenance attestations (`gh attestation verify`); auto-publishes the formula to `tehubersheezy/homebrew-sn` via `HOMEBREW_TAP_TOKEN`.
- Config: `dist-workspace.toml` (cargo-dist), `wix/main.wxs` (MSI template; stable GUIDs in `[package.metadata.wix]` of `Cargo.toml`), `.github/dependabot.yml` (weekly grouped dep PRs for cargo + github-actions).
