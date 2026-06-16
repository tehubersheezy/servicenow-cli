# Changelog

## 0.6.0 (2026-06-16)

### OAuth 2.0 / SSO authentication

- Profiles can now authenticate via OAuth 2.0 (`auth = "oauth"`) in addition to
  HTTP Basic — the supported path for instances fronted by external SSO
  (Okta/Azure AD/ADFS), where a human's password lives in the IdP and Basic auth
  cannot work.
- Two grants:
  - **`authorization_code`** — interactive browser flow with a loopback redirect
    server (RFC 8252) and PKCE S256 by default. The redirect URI defaults to
    `http://localhost:8400/callback` and must be registered exactly in
    ServiceNow's Application Registry.
  - **`client_credentials`** — non-interactive service-to-service tokens.
- New commands:
  - **`sn auth login`** — configure OAuth, run the flow, cache tokens, and verify
    (`--client-id`, `--client-secret`, `--redirect-uri`, `--scope`, `--grant`,
    `--no-pkce`, `--instance`).
  - **`sn auth status`** — show the resolved auth method and token expiry.
  - **`sn auth refresh`** — force a token refresh now.
  - **`sn auth logout`** — discard cached tokens.
- **`sn init`** now offers `basic` or `oauth` setup interactively (and via the
  same flags), so a profile can be stood up end to end in one command.
- Tokens are refreshed (or minted, for client-credentials) transparently before
  every request; new tokens are persisted automatically. Non-secret OAuth config
  lives in `config.toml`; the client secret and tokens live in
  `credentials.toml` (chmod 0600 on Unix).

Backward compatible: existing `config.toml` files without an `auth` field
continue to behave as `basic` profiles.

## 0.4.1 (2026-04-25)

### Fixes

- **Release pipeline** (v0.4.0 was tagged but never published).
  - `wix/main.wxs` was regenerated after the repo rename so the MSI's
    `ARPHELPLINK` ("More info") points at the new
    `tehubersheezy/servicenow-cli` URL. `dist plan` rejected v0.4.0 because
    the WXS template hadn't been refreshed alongside `Cargo.toml`'s
    `homepage` field.
  - ARM64 Windows builds now run on a native Windows runner
    (`windows-latest`) via `[dist.github-custom-runners]`. The default
    Linux runner couldn't cross-compile `ring` because its build script
    emits MSVC `/imsvc` flags that clang on Linux rejects.

## 0.4.0 (2026-04-25)

### New commands

- **`sn user me`** — returns the currently authenticated user's record. Resolves
  the identity via `gs.getUserName()`, so it works regardless of auth method
  (basic auth, OAuth, etc.).
- **`sn ping`** — one-shot health check. Returns auth status, instance URL,
  username, end-to-end latency in ms, and the ServiceNow build name/tag if
  reachable. Useful as the first thing to run when something breaks.
- **`sn open <table> <sys_id>`** — opens the ServiceNow web UI form for a record
  in the default browser via `nav_to.do?uri=...`. Pass `--print-url` to print
  the URL to stdout instead of launching a browser.
- **`sn raw <method> <path>`** — generic REST passthrough for endpoints that
  aren't yet modeled as typed commands. Accepts arbitrary methods (case
  insensitive), `--query key=value` (repeatable), and the same `--data` /
  `--field` body sources as the typed commands. Response is emitted exactly as
  ServiceNow returns it (no envelope unwrapping). The escape hatch for the long
  tail of ServiceNow's API surface.
- **`sn completion <shell>`** — generate tab-completion scripts for `bash`,
  `zsh`, `fish`, `powershell`, and `elvish` via `clap_complete`.

### New output mode

- **`--output table`** — render JSON results as a human-readable columnar table
  using `comfy-table`. Suitable for interactive browsing; for scripts and
  pipelines, leave the default JSON output. Single objects render as a
  two-column key/value table; arrays of objects render as a wide table with the
  union of keys as headers; empty arrays render as `(no records)`.

### Internal

- New shared helper `cli::table::write_response(global, value)` centralizes
  output dispatch so each command's emit site is a one-liner. All read/write
  command call sites now route through it instead of constructing
  `emit_value(...)` chains.
- Six new modules: `src/cli/{user,ping,open_record,raw,completion}.rs` and
  `src/output_table.rs`.
- New deps: `clap_complete = "4"`, `webbrowser = "1"`, `comfy-table = "7"`.

## 0.3.3 (2026-04-25)

### Distribution

- **Windows MSI installers.** The release pipeline now builds signed-ready
  `.msi` installers for both x86_64 and ARM64 Windows
  (`sn-x86_64-pc-windows-msvc.msi`, `sn-aarch64-pc-windows-msvc.msi`).
  Suitable for unattended deployment via SCCM/Intune/Group Policy:
  `msiexec /i sn-...msi /qn`.
- **ARM64 Windows binary.** Native build for Surface Pro X and Copilot+ PCs
  (`sn-aarch64-pc-windows-msvc.zip`), avoiding x86 emulation overhead.

### Internal

- Added `authors` field and `[package.metadata.wix]` GUIDs to `Cargo.toml`
  (required for stable MSI upgrade behavior across releases).
- Added `wix/main.wxs` (cargo-wix's MSI definition template, generated by
  `dist init`).

## 0.3.2 (2026-04-25)

### Distribution

- **Homebrew tap.** `sn` is now installable via Homebrew:

  ```bash
  brew install tehubersheezy/sn/sn
  ```

  The release workflow auto-publishes the cargo-dist-generated formula
  to [tehubersheezy/homebrew-sn](https://github.com/tehubersheezy/homebrew-sn)
  on every tagged release.

## 0.3.1 (2026-04-24)

### Documentation

- All write subcommands (`table`, `cmdb`, `catalog`, `change`, `import`,
  `identify`) now show consistent `--data` and `--field` help text covering
  the `@file` and `@-` (stdin) idioms. The binary always supported these,
  but only `sn table create` documented them — every other write command
  was mute, leading users to invent shell-quoting workarounds for
  multi-line content.
- `sn --help` now ends with a `BODY INPUT` reference and three concrete
  examples covering multi-line file bodies, file-backed field values
  (`--field description=@notes.md`), and stdin-piped input
  (`jq … | sn … --data @-`).

### Tests

- Added integration tests pinning `sn table update --data @file.json` and
  `sn table update --field name=@file.txt` so the multi-line write paths
  stay regression-tested.

## 0.3.0 (2026-04-23)

### Breaking

- `-v` is now the short flag for `--version` (was `--verbose`). Use `--verbose`
  (or `-vv`, `-vvv`) for verbosity levels. Scripts relying on `sn -v <cmd>` for
  verbose output must switch to `sn --verbose <cmd>`.

### Improvements

- **Observability is live.** `--verbose` logs `METHOD url` + elapsed ms to
  stderr. `-vv` adds response headers. `-vvv` adds request/response bodies
  (truncated). The logger functions existed previously but were never wired in.
- **HTTP error bodies no longer disappear.** Non-JSON 5xx responses (proxy
  errors, WAF blocks, upstream HTML) now surface the first 500 chars of the
  body as `detail` in the error envelope instead of collapsing to `HTTP 502`.
- **Broken-pipe handling.** `sn … | head` exits 0 silently instead of exit 1
  with a `{"error": {"message": "stdout: ..."}}` envelope on stderr.
- **`sn init` respects global proxy/TLS flags.** `sn init --proxy … --insecure
  --ca-cert …` now uses those settings for credential verification *and*
  persists them to the saved profile so future invocations pick them up.

### Internal

- Single `Client::request` method replaces four near-duplicate HTTP verb
  methods.
- Per-command arg structs (`*Args`, `*Sub`) now live alongside their handler
  modules; `cli/mod.rs` is a ~240-line entry point + re-exports (was 1,477).
- Unused `url` crate dependency removed.

## 0.1.0 (2026-04-22)

Initial release.

### Command groups

- **table** — CRUD on any ServiceNow table (list, get, create, update, replace, delete)
- **schema** — schema discovery (tables, columns, choices)
- **aggregate** — server-side stats (count, sum, avg, min, max, group-by)
- **change** — Change Management (normal/emergency/standard, tasks, CIs, conflicts, approvals, risk, schedule, models, templates)
- **attachment** — file upload/download (binary support)
- **cmdb** — CMDB Instance + Meta (CRUD, relations, class metadata)
- **import** — Import Set (single/bulk insert, retrieve)
- **catalog** — Service Catalog (browse, order, cart workflow, wishlist)
- **identify** — Identification & Reconciliation (CI create/update/query, enhanced variants)
- **app** — App Repository (install, publish, rollback)
- **updateset** — Update Set lifecycle (create, retrieve, preview, commit, back-out)
- **atf** — Automated Test Framework (run suites, get results)
- **scores** — Performance Analytics scorecards (list, favorite, unfavorite)
- **progress** — poll async CICD operations
- **introspect** — dump command tree as JSON

### Features

- Named profiles with config/credentials split (chmod 600)
- `--wait` flag for async CICD operations (auto-polls progress)
- Auto-pagination with `--all` (JSONL or `--array`)
- Proxy and TLS support (HTTP/HTTPS/SOCKS5, custom CA certs)
- Claude Code plugin for agent integration
