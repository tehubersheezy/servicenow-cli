---
paths:
  - "src/cli/progress.rs"
  - "src/cli/app.rs"
  - "src/cli/update_set.rs"
  - "src/cli/atf.rs"
  - "tests/cicd.rs"
---

# CICD async pattern (`--wait`)

## Modules

- `src/cli/progress.rs` → sn cicd progress + finish_cicd (poll async CICD ops)
- `src/cli/app.rs` → sn cicd app install/publish/rollback (App Repository)
- `src/cli/update_set.rs` → sn cicd updateset create/retrieve/preview/commit/commit-multiple/back-out
- `src/cli/atf.rs` → sn cicd atf run/results (Automated Test Framework)

Async CICD operations return a progress link at `links.progress.id` and run in the background on the instance. Prefer `--wait` — it blocks (polling `GET /api/sn_cicd/progress/{id}` every 2s) until the operation completes, then emits the final progress result. `--wait-timeout <SECS>` (requires `--wait`) bounds the wait; on expiry the command exits 3 with a pointer to `sn cicd progress`. Without `--wait`, the command returns the initial progress object immediately; poll in-flight operations with `sn cicd progress <id>`. Progress responses carry `status` — a numeric **string**, not an enum name: `"0"` pending, `"1"` running, `"2"` successful, `"3"` failed, `"4"` cancelled — plus `status_message`/`status_detail` and `percent_complete` (snake_case). The id is at `links.progress.id`; there is no top-level `progress_id` in the response (that name is only the CLI's positional arg). If the response has no progress link, `--wait` emits it without polling. All groups share one tail — `cli/progress.rs::finish_cicd` (progress-link extraction + polling + emission via `write_response`, so `--output table` works under `--wait`); new async commands must route through it, not open-code the wait. `--wait` honors `--output raw` rather than hardcoding `Default`: it used to emit the unwrapped result *and* look for the progress link only at the top level, which under raw is inside the envelope — so `--output raw --wait` was a silent no-op that emitted the initial, unpolled response. A wait that ends in a failed operation raises `Error::Api` with `NO_HTTP_STATUS`, so no `status_code` is published for a failure reported inside a 200 (see "Exit codes").
