---
paths:
  - "src/error.rs"
  - "src/main.rs"
  - "tests/usage_errors.rs"
  - "tests/output_contract.rs"
---

# Exit codes and `status_code`

## Modules

- `src/error.rs` → Error enum (8 variants: Usage/Config/Api/Auth/Transport/BrokenPipe/Instance, plus Interrupted — a wrapper adding `resume_from` to an interrupted `--all` stream), NO_HTTP_STATUS, exit_code(), to_stderr_json()

`0` success, `1` usage/config, `2` API 4xx/5xx (non-auth), `3` network/transport, `4` auth (401/403). Clap parse errors are intercepted in `main.rs` (`handle_clap_error`) to honor this contract: exit 1, JSON error envelope on stderr when stderr is not a TTY (clap's human text when it is). `--help`/`--version` still exit 0.

**`status_code` reports what HTTP said, and is omitted rather than invented.** A failure that never carried an HTTP status must not publish one — in particular never `0`, which no response can carry. Two distinct mechanisms produce a statusless exit 2, and they are deliberately *not* collapsed into one variant because keeping one would have to lie about the other's cause:

- **`Error::Api` with the `NO_HTTP_STATUS` sentinel** (`0`, in `error.rs`) — the HTTP call succeeded, the *operation* inside it failed. Five producers: a `--wait` on a CICD op whose progress ends `"3"` (failed) or `"4"` (cancelled), an AMB subscribe refusal (`amb.rs` — the only HTTP exchange there was the `101` upgrade, and the refusal is an in-band Bayeux reply), a `sn doctor` check that did not pass (the full report is already on stdout; the exit is what a CI step branches on), a record-reference number that resolves to nothing (`record_ref.rs`/`get_record.rs` — the lookup 200'd with zero rows, and fabricating a 404 would claim an HTTP verdict that never happened; `sn get`'s message also hedges "(or not readable by this profile)" because GraphQL row ACLs make absent and unreadable the same bytes), and `sn playbook list`'s `null` answer (same hedge, same reason: its resolver reads the parent through `GlideRecordSecure`). A sentinel rather than `Option<u16>` because every other `Error::Api` site has a real status and would become `Some(..)` noise; `to_stderr_json` drops the key and `Display` drops the `" (404)"` suffix.
- **`Error::Instance { message, detail }`** — an HTTP 200 whose *body* does not answer the question asked, so it must not be handed back as if it did. `sn user me`'s scripted fallback returning two rows (proof the `javascript:` term was silently dropped and the rows are strangers) is the case, and record-reference number resolution trips it the same way: `number=` is unique where it exists, so a second row under `sysparm_limit=2` (or `_rowCount > 1` in `sn get`'s GraphQL document) proves the term was dropped and the rows are unfiltered. `sn get`'s `sys_number` prefix lookup trips it on a returned row whose `prefix` is not the one asked about (prefixes are not unique, so there it is a per-row check, not a row count).

Both exit 2 — this is still the instance failing to serve the request — and both are pinned by tests in `error.rs`.

`Error::Auth` takes the same sentinel for a permission the instance denies *in band*: `sn impersonate` on a session whose `CanImpersonate` is `false` (inside a 200) exits 4 with no `status_code`.

**`status_code: 200` is not this, and is correct where it appears.** `graphql.rs`, `journal.rs`, `playbook.rs`, `variables.rs`, `script.rs` and `decision.rs` detect failures ServiceNow reports *in band* inside a genuinely successful 200, and they build `Error::Api { status: 200 }` on purpose — the HTTP status really was 200, and hiding it would misdescribe the exchange. The rule is only that a status must not be *fabricated*; reporting a real one is never wrong. So `status_code` is diagnostic, and the exit code is what a caller branches on.
