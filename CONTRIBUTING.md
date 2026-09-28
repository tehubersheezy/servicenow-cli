# Contributing to sn

## Development setup

Install Rust 1.88 or newer, then clone and build the project:

```bash
git clone https://github.com/tehubersheezy/servicenow-cli.git
cd servicenow-cli
cargo build
```

## Before submitting a PR

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features --workspace
```

All three must pass — CI enforces them on every PR.

## Testing

Integration tests use `wiremock` to mock ServiceNow and `assert_cmd` to drive the compiled binary. Tests that use `reqwest::blocking::Client` inside `#[tokio::test]` must wrap both client construction and calls in `tokio::task::spawn_blocking`.

```bash
cargo test --all-features --workspace # unit and mock integration tests
cargo test --test new_apis          # one test file
cargo test --lib query::            # tests in a module
```

The tests in `tests/live_dev380385.rs` are ignored by default. They require a configured
development instance; see that file before opting into a live run. The `fuzz/` crate
is a separate workspace and is not included in these commands.

## Adding a new API

1. Create a handler module in `src/cli/` with its argument structs and subcommand enum.
2. Reuse shared flags from `src/cli/args.rs` and the helpers in `src/cli/kernel.rs`:
   `connect`, `emit`, and `write_response`. These keep authentication and output consistent.
3. Register the module and command in `src/cli/mod.rs`, then wire dispatch in `src/main.rs`.
4. Add integration tests in `tests/`, including output and error behavior.
5. Update `docs/usage.md`, the README command index, and relevant setup or agent guidance.
   Update `CLAUDE.md` for architectural changes and `CHANGELOG.md` for the release.

The local skill is `.claude/skills/sn/SKILL.md`; the distributable copy is
`skills/sn/SKILL.md`. Keep their bodies and all files in `references/` identical.
Only the distributable entrypoint has `allowed-tools: Bash(sn *)`. Update these
skills when a change adds guidance an agent cannot learn from `--help`.

## Conventions

- `table update` and `cmdb update` use PATCH. `replace` was removed in 0.11.0;
  ServiceNow treated its PUT requests as partial updates too.
- Exposed `sysparm_*` parameters use friendly flags and `--sysparm-*` aliases where supported.
- `--data` / `--field` for request bodies (mutually exclusive)
- `--wait` for async CICD operations
- Follow the [output contract](docs/usage.md#output-contract), including command-specific
  exceptions to JSON output and the exit-code meanings.
