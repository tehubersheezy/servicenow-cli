---
name: sn
description: Run ServiceNow operations through the `sn` CLI — read and write incidents, changes, problems, requests, CMDB CIs, catalog items, attachments, journal comments, update sets and ATF runs against the user's instance. Use whenever the user says sn, ServiceNow or SNOW, names an instance, pastes a record number (INC…, CHG…, RITM…, PRB…, TASK…), asks to look up or file or update a ticket, wants live record updates, or asks a question whose answer lives in a ServiceNow table.
allowed-tools: Bash(sn *)
---

# sn — ServiceNow from the command line

API data is JSON by default, with stable exit codes and JSON runtime errors on stderr.
Warnings and diagnostics can share stderr. Commands never prompt when stdin isn't a terminal. `sn <group> --help` is accurate and complete — read it for syntax.
This file covers only what the binary can't tell you: how the *instance* misleads you.

## The one dangerous silent failure

**ServiceNow can drop an invalid query term and apply only the remaining terms**,
returning more records than you intended with exit 0. If no valid filter remains, the
result can cover the whole table. Behavior also depends on instance configuration.

Check field names with `sn schema` and inspect returned values before making a decision
or write that depends on a filter. Compare counts to catch a suspiciously broad result;
`sn aggregate <table> --count` is one request without record rows:

```bash
sn aggregate incident --count                                        # 70  baseline
sn aggregate incident --count -q "assigned_to.name=Abel Tuter"       # 0   term survived
sn aggregate incident --count -q "assigned_two.name=Abel Tuter"      # 70  typo, dropped
```

Equal counts are a clue, not proof: a valid filter can match every record, and a
partly invalid query can still narrow the result.

**An empty result needs a check too.** "Nothing matched" and "my query broke"
look identical, so show the query *can* match — confirm the entity exists, or run the same
shape against a value you know is there.

`sn change list` surfaces ignored fields when the API reports them: a dropped term is a stderr warning naming it
(`sn: warning: ServiceNow ignored query field(s): assigned_two`). The full account — the
query the instance actually ran — is the `__meta` element kept under `--output raw`.

**Trust the CLI's own defenses rather than working around them.** `sn variables set` refuses
unknown names before writing, `sn cmdb` stringifies numeric attributes for you, `sn ping`
/ `sn user me` fail closed rather than naming a stranger, and a `table:number` record
reference that cannot be resolved errors instead of matching an arbitrary row. Those traps
are already shut.

## Shape traps — where the obvious jq returns nothing

Silent: `jq` prints empty and exits 0, so a wrong path reads as "no data".

| Command | The obvious path | The real path |
|---|---|---|
| `schema tables` | `.name` (always null) | **`.value`** |
| `schema columns` | `.default_value`, `.choice_field` | **`.default`**, and `type=="choice"` with options in **`.choices[]`** |
| `change *` | `.number` | **`.number.value`** — the Change API wraps *every* field as `{display_value, value}`; `state.value` is a float (`-5.0`) |
| `change nextstates` | a list of `{value,label}` | three keys: `available_states`, `state_label`, **`state_transitions`** (conditions + `transition_available`) |
| `cmdb get` | `.name` | **`.attributes.name`** — top level is only `attributes` + relations |
| `sn get` | `.number` | **`.record.number`** — top level is `{table, sys_id, record, variables, journal}` |
| `aggregate --group-by` | `.stats.groupby_fields` | top level becomes an **array**; `groupby_fields` is a **sibling** of `stats` |
| `aggregate --sum-fields` | `.stats.sum` | **`.stats.sum.<field>`** — sum/avg/min/max nest per field |
| `aggregate --count` | a number | a **string**: `"70"`; `gr --count` instead returns `{count: 70}` |
| `watch` | every line is an event | a line with **`sn_watch`** is a gap marker |

## Exit codes and permission failures

`0` ok · `1` usage/config · `2` instance refused or couldn't answer · `3` network/transport ·
`4` auth. Branch on the code first, parse stdout second. `sn_error` on stderr carries
ServiceNow's own error body — read it and self-correct instead of retrying blind.

**Exit 4 does not always mean "log in again."** Both 401 and 403 map to it. A successful
`sn ping` confirms that its probes worked, but another endpoint can still reject the
request because of roles, ACLs, or API access policies. A failing ping can also be a
network or configuration error. Check the exit code and error details before retrying.
`sn doctor --need-role <role>` names a missing role. Don't test roles with
`getMatchingRoles` or `gs.hasRole` yourself: under admin they answer yes to any name,
real or not.

`status_code` may be absent on exit 2 (a failure reported inside a 200). Test for the key,
don't default it.

## Values that don't round-trip

`--display-value` **defaults to `true`** on `table`, `get`, `gr`, `change`, `aggregate`, and `scores`, so you
get labels — and dates localized to the caller's timezone. A localized date fed back into
`--query` will not match. When a value will be *used* rather than shown, read it with
`--display-value false`.

## Working as an agent

- **Ask the instance before guessing a field name.** `sn schema tables --filter X` →
  `sn schema columns X --writable` → `sn schema choices X <field>`. A guessed name is how you
  land in the dropped-term case above.
- **Bound every watch** (`--max-events` / `--duration` / `--idle-timeout`), and note it
  requires `-q`; there is no bare "watch this table" form.
- **`updateset back-out` and `app rollback` deserve a human.** They can affect
  many configuration records. Establish the target and intended reversal from the user's
  request before running either command.
- **Pipe secrets** (`--password-stdin`, `--client-secret-stdin`, `--api-key-stdin`,
  `--token-stdin`); argv is visible to `ps`.
- **Prefer `sn profile add` over `sn init`** — it emits JSON, never prompts off a TTY, and
  leaves `default_profile` alone.
- **Authorization-code login needs a person** — `sn profile login` opens a browser and waits
  for authorization. `client_credentials`, `jwt_bearer`, and API keys (`--auth apikey`)
  support headless setup. Data commands never open a browser.
- **Journal has no write verb**: add a note with
  `sn table update incident <sys_id> --field work_notes="..."`.

## Where to go next

| File | Covers |
|---|---|
| `references/shapes-queries.md` | encoded-query hazards, `^OR`/`^NQ` precedence, `INSTANCEOF`, dates, aggregates |
| `references/change.md` | Change Management: routing by type, state transitions, tasks, conflicts |
| `references/watch.md` | live AMB streams: event anatomy, gap markers, `--on-change` caveats |
| `references/cicd.md` | `app`/`updateset`/`atf`/`progress` — the async `--wait` contract |

Other surfaces are well covered by `--help`; a few notes worth having anyway: `sn api search
<term>` discovers what endpoints the instance actually publishes (use it before hand-writing
`sn raw`); `sn codesearch <term>` answers "where is X referenced in code" across every
script table and scope in one call (rows are ACL-filtered, so a non-admin sees few or none);
`sn gr <table> -f number,caller_id.manager.email` reads dot-walked reference fields
in one round trip without writing GraphQL (`--count` for just the match count). Both
`sn graphql` and `sn gr` map GraphQL errors to exit 2 even under HTTP 200; only
`sn graphql` emits partial `data` on failure. `sn raw` has no confirmation guard,
including for DELETE; `sn attachment download --out` stages and renames, so a
failed download never leaves a truncated file, and reports `{"path","size"}`; `sn flow get <flow>
--outline` is the compact view of a Flow Designer model (the full one runs to hundreds of KB,
and each read takes tens of seconds); `sn identify
query` shows what the IRE *would* match before `create-update` writes; `sn catalog
item-variables` names what an order must carry, and the cart is server-side state that
survives a failed run.
