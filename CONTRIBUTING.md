# Contributing to sn

## Development setup

```bash
git clone https://github.com/tehubersheezy/sn.git
cd sn
cargo build
```

## Before submitting a PR

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace
```

All three must pass — CI enforces them on every PR.

## Testing

Integration tests use `wiremock` to mock ServiceNow and `assert_cmd` to drive the compiled binary. Tests that use `reqwest::blocking::Client` inside `#[tokio::test]` must wrap client calls in `tokio::task::spawn_blocking`.

```bash
cargo test --workspace              # all tests
cargo test --test new_apis          # one test file
cargo test --lib query::            # tests in a module
```

## Adding a new API

1. Add arg structs and subcommand enum to `src/cli/mod.rs`
2. Create a handler module in `src/cli/` (follow existing patterns — `build_profile`, `build_client`, `unwrap_or_raw`, `emit_value`)
3. Add the module declaration to `src/cli/mod.rs`
4. Wire up dispatch in `src/main.rs`
5. Add integration tests in `tests/`
6. Update documentation: README.md, CLAUDE.md, docs/agent-guide.md, .claude/skills/sn.md, skills/sn/SKILL.md

## Conventions

- `update` = PATCH (partial), `replace` = PUT (full overwrite)
- Every `sysparm_*` gets a friendly flag name and a raw `--sysparm-*` alias
- `--data` / `--field` for request bodies (mutually exclusive)
- `--wait` for async CICD operations
- JSON on stdout, structured errors on stderr, exit codes 0/1/2/3/4
