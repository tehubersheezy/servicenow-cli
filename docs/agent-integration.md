# Agent integration

`sn` works with coding agents and shell scripts. Both can use the same discovery commands,
output formats, and exit codes:

- **JSON output by default** — see the [output contract](usage.md#output-contract) for
  formats, exceptions, and stderr diagnostics.
- **Deterministic [exit codes](usage.md#exit-codes)** — branch on the code before parsing
  anything: `2` API error, `3` network, `4` auth.
- **No prompts when stdin isn't a terminal** — setup needs complete flags, and destructive
  commands require `--yes`. Authorization-code login still needs a browser and a person;
  use a prepared profile or headless authentication for unattended work.
- **Discovery built in** — `sn schema` tells an agent what a table looks like before it
  writes; `sn introspect` tells it what the CLI itself looks like.

For the full agent-facing playbook — discovery flow, encoded-query syntax, common mistakes —
see the [agent usage guide](agent-guide.md), which is written to be dropped into an agent's
context.

## Claude Code plugin

The repo ships as a Claude Code plugin (plugin name `sn`, in `.claude-plugin/`). Its
skill declares `allowed-tools: Bash(sn *)`; the host's permission settings still apply.
Install the `sn` binary separately using the [setup guide](setup.md#installation).
This repo is its own plugin marketplace:

```bash
claude plugin marketplace add tehubersheezy/servicenow-cli   # or a local clone path
claude plugin install sn
```

In a clone of this repo, the local skill is
[`.claude/skills/sn/SKILL.md`](../.claude/skills/sn/SKILL.md); invoke it with `/sn`.
The distributable copy is [`skills/sn/SKILL.md`](../skills/sn/SKILL.md).

## `sn introspect`: the machine-readable command tree

`sn introspect` dumps the full command tree as JSON — for auto-generating MCP tool
definitions or function-call schemas:

```bash
sn introspect | jq '.subcommands[] | {name, about}'

# Flags that cannot be combined, across the whole tree:
sn introspect | jq '[.. | objects | select(.conflicts_with? // [] | length > 0)
                     | {name, conflicts_with}] | unique'
```

Each `args[]` entry carries `name`, `long`, `short`, `help`, `help_heading`, `required`, `takes_value`, `value_name`, `positional`, `repeatable`, `aliases`, `default_values`, `possible_values`, and `conflicts_with`. `--help` and `--version` are omitted — they exit before any handler runs — and nothing named `help` appears in the tree.

The root carries two extra keys: `version` (the binary that produced the tree) and
`global_args`. **A command's effective flags are its own `args` plus the root's
`global_args`.** Read these from the installed binary instead of hardcoding a flag or
command count:

```bash
# Everything `table list` accepts:
sn introspect | jq '[.global_args[], (.subcommands[] | select(.name=="table")
                     | .subcommands[] | select(.name=="list") | .args[])] | map(.name)'
```

The schema does not include every constraint. In the clap version used here, `requires`
is not exposed through a public getter, so `--wait-timeout` requiring `--wait` appears
only in help text. Generated integrations must still handle usage errors.
