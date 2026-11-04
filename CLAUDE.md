# sn

`sn` is a single-binary Rust CLI wrapping ServiceNow's REST, GraphQL and several undocumented APIs. It is built for scripts and coding agents: API data is JSON on stdout, runtime errors are JSON on stderr, and the exit code says what kind of failure it was. User-facing behavior is documented in `docs/usage.md`, `docs/setup.md` and `docs/agent-guide.md`.

## Commands

```bash
cargo build                    # dev build
cargo test --workspace         # unit + integration
cargo test --lib query::       # one module
cargo test --test pagination   # one integration file
cargo clippy --all-targets --all-features -- -D warnings   # must pass before commit
cargo fmt --all -- --check     # must pass before commit
```

- Integration tests mock ServiceNow with `wiremock` and drive the compiled binary with `assert_cmd`.
- A test that uses `reqwest::blocking::Client` inside `#[tokio::test]` must wrap client construction and every call in `tokio::task::spawn_blocking`, or the blocking runtime panics on drop.
- Tests isolate config with `SN_CONFIG_DIR`, pointed directly at a temp dir holding `config.toml`/`credentials.toml` (no `sn` subdir). Don't gate config tests on `target_os`.

## Layout

```
src/main.rs            parse Cli, dispatch, map Error → exit code
src/lib.rs             module list — register new top-level modules here
src/error.rs           Error enum, exit_code(), stderr JSON envelope
src/client.rs          reqwest blocking client, Auth enum, Paginator
src/config.rs          profiles, credentials, atomic 0600 writes, .sn.lock
src/oauth.rs, jwt.rs, external_token.rs    token acquisition
src/query.rs, body.rs  flags → sysparm_* pairs; --data/--field → JSON
src/output.rs, output_table.rs             JSON / JSONL / table writers
src/amb.rs             websocket client behind `sn watch`
src/schema_cache.rs    offline schema index behind `sn cache`
src/cli/mod.rs         Cli, GlobalFlags, command registration, implied verbs
src/cli/kernel.rs      shared preamble and emitters every command uses
src/cli/args.rs        shared clap arg structs
src/cli/<command>.rs   one module per command group
hooks/, skills/, .claude-plugin/           the Claude Code plugin
```

A command handler receives `&GlobalFlags` plus its args struct, calls `kernel::connect(global)` (profile → client), and hands the response to `kernel::emit(global, resp)`.

## Rules that apply everywhere

**Exit codes are a contract.** `0` success, `1` usage/config, `2` API 4xx/5xx or the instance failing to answer, `3` network/transport, `4` auth (401/403). `status_code` in the error envelope reports what HTTP actually said; when the failure carried no HTTP status, omit it (`NO_HTTP_STATUS`, `Error::Instance`) rather than invent one.

**ServiceNow silently drops query terms it cannot evaluate and returns unfiltered rows.** A lookup that must match at most one row asks for two (`sysparm_limit=2` or `_rowCount`) and treats a second row as `Error::Instance`. Keep these canaries, and add one to any new lookup. A report of who or what something is must fail closed.

**Behavior marked "measured" came from a live instance, not from docs.** Don't change code that depends on it without re-measuring, and record what you measured in that area's rule file.

**A profile is the whole unit of identity.** Selection is `--profile` > `default_profile`. There are no per-field argv overrides and no env vars for credentials or profile selection.

**Check argv before touching the network.** Usage errors and destructive-command gates run before `build_profile`/`build_client`, so an argv mistake never reaches the instance or an IdP.

**Destructive commands go through `kernel::confirm_destructive(yes, verb, what)`,** using the verb the caller typed. Non-TTY stdin without `--yes` is refused.

**All JSON output goes through `kernel::emit` / `write_response`,** which is what makes `--output table` and `--output raw` work. Streaming commands use `output::write_jsonl_line`, which flushes per record — never flush again after it.

**Every clap arg has a `///` doc comment** (`sn introspect` publishes it). Declare fields in order of usefulness; raw `sysparm_*` passthroughs get `help_heading = ADVANCED`. Build the command tree with `cli::command()`, never `Cli::command()`.

**Secrets never reach logs, error messages or a child's argv.** Config files are written atomically at 0600, and any load → modify → save runs inside `config::with_config_lock`.

**Defaults that surprise:** `--display-value` defaults to `true`; updates are PATCH (ServiceNow treats PUT as partial too, so there is no `replace`).

## When you add something

- A new command module: register it in `src/cli/mod.rs` and `src/main.rs`, and document it in `docs/usage.md` and the README command table.
- A command that opens its own session: add it to `refusal` in `src/cli/impersonate.rs`. That list ends in a catch-all, so an unlisted command is silently allowed under `sn impersonate`.
- A new command, positional or value-taking flag: regenerate the panel's catalog with `UPDATE_PLUGIN_CATALOG=1 cargo test --test plugin_catalog` (the test fails until you do).
- An async CICD command: route it through `cli/progress.rs::finish_cicd`.
- A change to `sn introspect`'s output shape is breaking; update `docs/agent-guide.md` and `docs/agent-integration.md` together.
- A skill change: keep `skills/sn/` and `.claude/skills/sn/` identical except the `allowed-tools` line (`diff -r`). The skill holds only what `--help` cannot teach.

## Releases

- The top section of `CHANGELOG.md` is published verbatim as the GitHub release body. Write one line per entry, state the fix rather than the problem, and keep the section under ~4,000 characters.
- IMPORTANT: never write `Bearer` followed by a word in `CHANGELOG.md`. The runner masks it and the release publishes zero assets while reporting success. Write "bearer token" in lowercase.
- Run `gh workflow run mask-check.yml` before tagging, and `gh workflow run attach-provenance.yml -f tag=<tag>` after every release.
- `release.yml` is hand-edited on top of cargo-dist's output; `dist generate` reverts those edits.

## Where the detail lives

Design rationale, protocol notes and measured instance behavior live in `.claude/rules/`. Each file loads automatically when you open a file it is scoped to. If you are planning or answering a question without opening those files, read the matching rule first.

| Rule file | Covers |
|---|---|
| `exit-codes.md` | `NO_HTTP_STATUS`, `Error::Instance`, when `status_code: 200` is correct |
| `cli-conventions.md` | kernel, flag naming, help tiers, destructive gate |
| `cli-parsing.md` | implied read verb on `table`/`cmdb`, subcommand hints |
| `output.md` | batch vs stream writers |
| `record-refs.md` | `table:number` references, bare-number prefix resolution, `sn get` |
| `pagination.md` | keyset vs offset, `^NQ`/`^OR` cursor placement, resume |
| `client-http.md` | upload/download, per-read timeout, staging files, `raw --header` |
| `proxy-tls.md` | flag/env/config precedence table |
| `config-files.md` | config dir, atomic writes, lock sidecar, write ordering |
| `profiles.md` | profile resolution, `sn init` vs `sn profile add` |
| `auth-oauth.md` | OAuth grants, JWT bearer, API key, external token command |
| `identity.md` | how `whoami`/`ping`/`user me` name the caller |
| `impersonation.md` | session-held hop, verification, teardown |
| `cicd-async.md` | `--wait`, progress status values |
| `watch-amb.md` | AMB websocket protocol, session reaping, rotation, markers |
| `schema-cache.md` | `sn cache refresh` build, dynamic completion |
| `introspect.md` | the introspect output contract |
| `plugin.md` | skill policy, `sn table` panel hooks |
| `ci.md` | workflows, lockfile refresh, action pinning |
| `changelog.md` | release-note style, the masked-output trap in full |
| `commands/<name>.md` | endpoints and quirks for `change`, `cmdb`, `api`, `codesearch`, `graphql`/`gr`, `doctor`, `flow`, `script`, `decision`, `playbook`, `context`, `get` (nouns and `sn set`), and `catalog`/`import`/`identify`/`schema` |

Keep this file under 200 lines. It holds only what every session needs; anything specific to one area goes in that area's rule file.
