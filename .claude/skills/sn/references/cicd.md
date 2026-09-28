# CICD — the async contract

`app`, `updateset` and `atf` operations run in the background on the instance. `--help` has the
verbs; this is the part that breaks scripts.

## Branch on the exit code, never on `status_label`

`--wait` follows the returned progress link and polls every 2s. For a polled operation:

| Outcome | Exit | stdout |
|---|---|---|
| Succeeded | `0` | the final progress result |
| Operation failed or was cancelled | `2` | **empty** — progress object is on stderr under `.error.sn_error` |
| `--wait-timeout` expired | `3` | **empty** — pointer to `sn progress <id>` |

So reading stdout on a failure branch gets you nothing. A failed operation carries **no
`status_code`**: the HTTP call succeeded, the operation didn't.

`status_label` is ServiceNow's verbatim string and varies by instance — "Successful",
"Complete", "Succeeded". Matching on it is how you write a poll loop that never terminates.

## Polling manually

Key off the numeric `status`, which is a **string** holding a digit:

| `status` | Meaning |
|---|---|
| `"0"` | pending |
| `"1"` | running |
| `"2"` | successful |
| `"3"` | failed |
| `"4"` | cancelled |

Alongside it: `status_message`, `status_detail`, `percent_complete` (snake_case). **The progress
id lives at `links.progress.id`** — there is no top-level `progress_id` in the response, despite
that being the CLI's argument name.

```bash
id=$(sn app install --scope x_myapp --version 1.2.0 | jq -r '.links.progress.id')
sn progress "$id"
```

If the initial response has no `links.progress.id`, the CLI emits that response
without polling, even with `--wait`. Inspect the response shape before assuming a
background operation was observed to completion.

Prefer `--wait` with a `--wait-timeout` when you can: one command, and the timeout bounds a
stall instead of hanging. `--wait` honors `--output raw`.

## The two that deserve a human

Both require `--yes` without a terminal and prompt on a terminal unless it is supplied.
Establish the target and intended reversal before running them:

- **`updateset back-out` reverses configuration changes from an applied update set**;
  conflicts can require manual resolution.
- **`app rollback` replaces an installed app** without rolling back what the newer version
  wrote to data.

These operations can affect many configuration records and run asynchronously. A
successful request does not mean the reversal has completed; inspect its progress.
