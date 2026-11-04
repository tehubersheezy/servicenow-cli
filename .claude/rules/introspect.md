---
paths:
  - "src/cli/introspect.rs"
  - "tests/introspect.rs"
  - "tests/cli_surface.rs"
  - "docs/agent-guide.md"
  - "docs/agent-integration.md"
---

# The introspect contract

## Modules

- `src/cli/introspect.rs` → sn introspect (dumps clap command tree as JSON; see "The introspect contract")

`sn introspect` is the machine-readable face of the command tree — MCP and function-call schema generators consume it — so its **output shape is a contract**, and changing it is a breaking change for anything caching generated schemas. Emit through `cli::command()` (not `Cli::command()`) then `build()`, and through `write_response` like every other command.

- **Nodes** are `{name, about, args[], subcommands[]}`, recursive. The root additionally carries `version` (from `CARGO_PKG_VERSION`) and `global_args`.
- **A command's effective flags are its own `args` plus the root's `global_args`.** clap propagates the 11 globals onto every node; serializing them there was 1,480 of 1,852 argument entries — three quarters of a 481 KB tree. Partition with `Arg::is_global_set()`. Non-global args do not propagate, so there is no ancestor chain to walk.
- **`build()` is required *and* is the trap.** It finalizes actions, defaults, and value names (without it every arg looks like it takes a value), but it also generates the `help` subcommand, which mirrors the whole tree as argument-less stubs. Emitting that put 274 of 391 nodes there and gave every real command a same-named twin. `describe_command` filters `help` and hidden subcommands back out.
- **Drop `--help`/`--version` by clap *action*, never by name.** `sn cicd app install --version <VERSION>` is a real value-taking option that a name-based filter deletes.
- **`conflicts_with` comes from the command** (`Command::get_arg_conflicts_with`), which is why `describe_arg` takes one; it is sorted so the tree is byte-stable across runs. Its inverse is unavailable: clap keeps `requires` `pub(crate)`, so `--wait-timeout` needing `--wait` survives only as prose in that flag's help. Don't go looking for the getter.

Guards live in `tests/introspect.rs` (no node named `help`, unique command paths, globals not duplicated, builtins gone but `app install --version` intact). Note that `tests/cli_surface.rs::every_argument_has_help_text` walks this tree: it chains the root's `global_args`, because checking `args` alone would silently stop covering 11 flags rather than fail.

Two places document this schema by hand and must move together: `docs/agent-guide.md` and `docs/agent-integration.md` (the README links there but carries no schema detail itself). The agent skills used to restate it and deliberately no longer do — anything `sn introspect --help` and these docs already cover was cut from them, because a hand-copy of a machine-readable contract is a second source of truth that silently goes stale.
