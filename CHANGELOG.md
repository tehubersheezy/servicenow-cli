# Changelog

## 0.8.0 (2026-07-11)

An adversarial review of the docs — checked against the compiled CLI, ServiceNow's
official API docs, the published release assets, and a live instance — drove this
release: every documented claim is now either verified true or fixed, plus two
classes of code defects the review surfaced.

### Breaking changes

- **Every destructive `delete` now requires confirmation.** `change delete`,
  `change task delete`, `attachment delete`, and `cmdb relation delete` gain the
  guard `table delete` already had: a `[y/N]` prompt on a TTY, and a required
  `--yes`/`-y` when stdin is not a terminal (exit 1 with a usage error instead of
  deleting silently). Scripts calling these commands must add `--yes`.

### Added

- **Single-letter short flags** on the highest-traffic parameters: `-q`
  (`--query`), `-f` (`--fields`), `-D` (`--data`), `-F` (`--field`). Capitals
  mirror curl's `-d`/`-F` mnemonics; lowercase `-d` is the verbosity ladder and
  `-f` belongs to `--fields`.
- **`.claude-plugin/marketplace.json`** — the repo is now its own Claude Code
  plugin marketplace, so the documented install flow works as written:
  `claude plugin marketplace add tehubersheezy/servicenow-cli`, then
  `claude plugin install sn`.

### Fixed

- **Verbose logging no longer leaks secrets.** `-ddd` printed OAuth
  token-endpoint responses — live access and refresh tokens — in cleartext;
  token values are now masked (metadata like `token_type` / `expires_in` stays
  readable). `-dd` masked only `Authorization`; it now also masks `Set-Cookie`
  session tokens, and the mask label no longer claims `Basic` for Bearer auth.
- **Docs no longer claim PUT blanks omitted fields.** `replace` was documented
  as "full overwrite — omitted fields are blanked"; ServiceNow actually applies
  PUT as a partial update (verified against a live instance and the official
  Table API docs). The docs now say so and explain how to clear a field
  explicitly.
- **The `cmdb relation add` example payload was unusable** — bare
  `type`/`target` keys; the API requires them wrapped in
  `outbound_relations`/`inbound_relations`. Fixed in the README, agent guide,
  and both skills.
- **The documented Claude-plugin install command didn't exist**
  (`claude plugin install --dir`); replaced with the real marketplace flow.
- Documentation gaps closed: the stderr envelope's `sn_error` field, the global
  `--timeout`, the Parameters table's missing rows
  (`--suppress-pagination-header`, `--query-category`) and per-command
  `--setlimit` defaults, the `attachment download` `--output` file-path
  double-meaning, and the TOC's missing Shell completions entry.

## 0.7.1 (2026-07-08)

### Fixes

- **`updateset create` sent the wrong query parameter.** It posted `name=…`, but
  the CICD Update Set API's required parameter is `update_set_name` (verified
  against the official docs on the australia/zurich/yokohama families). ServiceNow
  ignores unknown query params, so the required name never arrived. The `--name`
  flag is unchanged (with `--update-set-name` as an alias); only the wire parameter
  was corrected.
- **`updateset retrieve` ignored its source selectors.** The flags sent
  `source_id` / `source_instance_id`, but the API expects `update_source_id` /
  `update_source_instance_id` — so the selectors were silently dropped and retrieve
  always fell back to ServiceNow's own source resolution. Flags renamed to
  `--update-source-id` / `--update-source-instance-id` and the wire parameters
  corrected. (These flags never functioned before, so nothing that worked breaks.)
- **`sn ping` now honors `--output table`.** It emitted JSON regardless of the
  output mode; its final emission routes through `write_response` like every other
  command, so `--output table`/`--pretty`/`--compact` all apply.

### Changed

- **`-v` prints the version** (with `-V` kept as an alias); the verbose logging
  ladder moves to **`-d` / `-dd` / `-ddd`** (long form `--verbose`). This reverses
  the 0.6.1 `-v`-is-verbose choice in favor of the more common version-on-`-v`
  convention.
- **`sn init --auth oauth` registers a public PKCE client by default** and no longer
  prompts for a client secret on the interactive authorization-code flow. Pass
  `--client-secret` explicitly for a confidential authorization-code client;
  `client_credentials` still requires one.

## 0.7.0 (2026-07-08)

A coherence pass on authentication and profile handling. A profile is now the
single unit of identity: commands either **manage** profiles (`sn init`,
`sn profile *`) or **use** exactly one (`--profile` > `default_profile`). Nothing
mixes stored profile state with per-invocation argv fragments anymore.

### Breaking changes

- **Removed the `--instance-override`, `--username`, and `--password` global
  flags.** They grafted argv fragments onto a stored profile's identity, producing
  chimeras — half from disk, half from the command line. On an OAuth profile,
  `--instance-override` redirected the token endpoint, sending the refresh token
  and client secret to an arbitrary host. Change identity by editing the profile
  (`sn init`) or selecting another (`--profile`).
- **Removed the phantom `"default"` profile fallback.** With no `--profile` and no
  `default_profile`, `sn` used to invent a profile named `"default"` that nobody
  created, surfacing errors about a phantom. It now fails fast: `no profile
  selected; pass --profile <name> or run \`sn init\``.
- **`sn auth login` is now a pure session command with no flags.** It previously
  doubled as a second, partial `sn init` — writing `client_id`/`grant`/
  `redirect_uri`, force-converting a profile to OAuth, and able to persist an empty
  instance while minting tokens against an `--instance-override` host. It now
  resolves the selected profile, requires `auth = "oauth"` with an `[oauth]` block
  (a basic profile errors with `does not use oauth; run \`sn init\``), runs the flow
  with the stored grant, and caches tokens. Configure OAuth via `sn init --auth
  oauth`.
- **Removed `sn auth test`.** Use `sn ping` — it verifies auth and adds latency and
  the ServiceNow build version.
- **Empty/whitespace `instance` is rejected** instead of silently producing a
  scheme-only `https://` base URL.

### Added

- **`SN_CONFIG_DIR`** — points directly at the directory holding `config.toml` and
  `credentials.toml` (no `sn` subdirectory appended), overriding the platform-native
  location on every OS. This is the cross-platform config-isolation mechanism,
  superseding the Linux-only `XDG_CONFIG_HOME` hack; config-dependent integration
  tests are no longer `#[cfg(target_os = "linux")]`-gated.
- **Richer `sn profile list` / `sn profile show`.** `list` reports each profile's
  `auth` method and a `default` marker; `show` surfaces the auth method and, for
  OAuth profiles, the client_id, grant, redirect_uri, pkce, and token state
  (`loggedIn`/`hasRefreshToken`/`expiresAt`) with all secret material redacted.

### Changed

- **`sn auth login` / `logout` / `refresh` now emit success JSON to stdout**
  (joining `status`), honoring the machine contract; all four also honor
  `--output`/`--pretty`/`--compact`.
- **Re-running `sn init` over an existing profile is non-destructive** — it merges
  onto the stored profile, clears only the secrets of the auth method being switched
  away from, and preserves `proxy_username`/`proxy_password`.

### Migration

| Old | New |
|---|---|
| `sn --instance-override URL --username U --password P table list …` | `sn init` a profile once, then `sn --profile NAME table list …` |
| `sn auth login --client-id … --grant … --instance …` | `sn init --auth oauth …`, then `sn auth login` |
| `sn auth test` | `sn ping` |
| relying on the implicit `"default"` profile | `sn profile use NAME` (sets `default_profile`) or pass `--profile` |
| `XDG_CONFIG_HOME` (Linux-only) for config isolation | `SN_CONFIG_DIR` (all platforms) |

## 0.6.1 (2026-07-04)

### Fixes

- **Exit-code contract at the CLI edge.** Clap parse errors (unknown flags,
  missing args) are now intercepted via `try_parse` so usage mistakes exit `1`
  — clap's default `2` is reserved for API errors — and emit the JSON error
  envelope on stderr when stderr is piped. `--help`/`--version` still exit `0`.
- **`-v` is now `--verbose`** (as the help text always claimed); `--version`
  moves to `-V` per clap convention.
- **`import bulk`** accepts the README-documented bare JSON array and wraps it
  as `{"records": [...]}` for `insertMultiple`; pre-wrapped objects still pass
  through unchanged.
- **`introspect`** builds the clap command before describing it, so boolean
  flags no longer report `takes_value: true` with `["true","false"]` (which led
  agents to emit `--all true`); adds `positional`, `repeatable`, and
  `default_values` fields.
- **`--wait-timeout <SECS>`** now bounds the CICD poll loop (exit `3` on
  expiry); all eight async CICD call sites route their final emission through a
  shared `finish_cicd`, so `--output table` works under `--wait`.
- **`-vvv` body logging** truncates on a char boundary instead of panicking
  mid-UTF-8 sequence.

### OAuth

- `sn init`'s OAuth branch prompts for the client secret immediately after the
  client id, and skips the redirect-URI prompt under `client_credentials`.
- **OAuth scope removed entirely** (flag, config field, request parameter).
  ServiceNow grants scopes through the Application Registry entry an admin
  configures, so a client-requested scope granted nothing and only invited
  misconfiguration. Existing `config.toml` files with a leftover `scope=` line
  still parse (serde ignores unknown keys).

### Dependencies

- Bump `quinn-proto` to 0.11.15 (RUSTSEC-2026-0185).

### CI

- The security workflow's `cargo audit` job now installs a prebuilt cargo-audit
  binary (via `taiki-e/install-action`) instead of compiling it from source,
  which had been failing intermittently on crates.io index fetches.

### Docs

- README gains a table of contents, an at-a-glance command block, and an
  OAuth / SSO setup section documenting `sn auth login/status/refresh/logout`.

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
