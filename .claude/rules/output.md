---
paths:
  - "src/output.rs"
  - "src/output_table.rs"
  - "src/cli/kernel.rs"
  - "src/cli/watch.rs"
  - "tests/output_contract.rs"
---

# Output: the batch/stream split

## Modules

- `src/output.rs` → write_value (batched JSON), write_jsonl_line (one record, flushed), emit_value/emit_error (raw writers)
- `src/output_table.rs` → write_table (renders JSON as a comfy-table columnar view for `--output table`)

- **The batch/stream split in `output.rs` is load-bearing, and it lives in the functions rather than in a comment at the call sites** — the call sites are where it kept getting lost. `write_value` batches a finished payload through a 64 KiB `BufWriter` with an explicit, propagated flush (`BufWriter::drop` discards a failing flush, which would turn a closed pipe or a full disk into silent truncation with exit 0). `write_jsonl_line` is the streaming path and **flushes after every record**: a line sitting in a buffer is indistinguishable from a hung process, so `sn watch | jq` would look frozen. **Callers must not flush again.** A second flush is a second syscall on a pipe the reader may already have closed, so `sn watch … | head -1` attributes the broken pipe to the *following* record instead of the one that hit it, and the session skips its `/meta/disconnect` — that is a real merge defect, not a hypothetical: `Stream::write` was flushing on top of `write_jsonl_line`, and the test caught it only after both branches landed. There is deliberately no whole-iterator JSONL helper; one existed, nothing in the binary called it, and its tests read like coverage of the streaming path while pinning dead code.
