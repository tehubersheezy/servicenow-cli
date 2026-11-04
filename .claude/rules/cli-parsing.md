---
paths:
  - "src/cli/mod.rs"
  - "src/main.rs"
  - "tests/cli_surface.rs"
  - "tests/usage_errors.rs"
---

# Argv parsing: implied verbs and subcommand errors

- **The read verb is optional on `table` and `cmdb`** (`IMPLIED_VERB_GROUPS`): `sn table incident <sys_id>` → `get`, `sn table incident` → `list`, `sn table incident:INC0010001` → `get`. Implemented in `cli::parse` as a *retry after failure*, not `allow_external_subcommands` — argv is re-parsed with a verb spliced in only when the first parse raised `InvalidSubcommand`. Two properties carry it, and both must survive any change here:
  - **The verb is decided, not guessed.** A `:` in the token marks a record reference (no table name can contain one), so only `get` is offered; a plain token tries `list` before `get`, and `list` rejects a second positional, so a trailing sys_id still lands on `get`. Ordering carries the bare-noun-means-list rule because `get`'s sys_id positional is optional now. Never infer a write.
  - **A typo must not become a shorthand.** clap fills `ContextKind::SuggestedSubcommand` only when the token is within edit distance of a real verb, so a non-empty suggestion aborts the rewrite — that is why `sn table lst incident` still says "did you mean 'list'" instead of GETting a table named `lst`. Don't replace that check with a hand-rolled distance function; it would drift from clap's.
  - Groups with non-CRUD subcommands (`change nextstates`, `catalog checkout`) are excluded on purpose: their first token isn't reliably a noun.
- A top-level `InvalidSubcommand` for a name that moved under `sn cicd` (`app`, `updateset`, `atf`, `progress`) gets "moved to `sn cicd <name>`" (`cli::moved_command_hint`) instead. Other `InvalidSubcommand` errors append the group's real subcommand list (`main.rs::subcommand_hint` → `cli::valid_subcommands`), because clap names none of them unless the token is a near-miss.
- **`sn get` takes a noun subcommand *or* a REF positional** (`args_conflicts_with_subcommands` + `subcommand_negates_reqs` on `GetArgs`). clap matches a subcommand name first, and no reference can equal a noun (a reference has a `:` or a counter's digits), so nothing is guessed. A digit-free, colon-free token that is not a noun gets the noun list appended to the REF error (`get::noun_hint`). `usage_line` prints one line per form for such a command (`sn get <REF> [OPTIONS]` / `sn get <NOUN> [OPTIONS]`).
