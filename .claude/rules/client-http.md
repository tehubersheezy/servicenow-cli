---
paths:
  - "src/client.rs"
  - "src/cli/attachment.rs"
  - "src/cli/raw.rs"
  - "tests/attachment_download.rs"
  - "tests/raw_headers.rs"
---

# HTTP client: binary methods, download timeouts, extra headers

## Modules

- `src/client.rs` → reqwest blocking client (proxy/TLS), Auth enum (Basic/Bearer/ApiKey/Session/None), SessionJar, Paginator iterator (Link/offset and keyset modes; see "Pagination")
- `src/cli/attachment.rs` → sn attachment list/get/upload/download/delete (binary)
- `src/cli/raw.rs` → sn raw <method> <path> (REST passthrough for unmodeled endpoints; --header, --query, --data/--field)

`client.rs` adds three methods beyond the JSON HTTP verbs, for the Attachment API:
- `upload_file(path, query, body: Vec<u8>, content_type)` — POST raw binary with a custom Content-Type.
- `download_file(path) -> Download` — GET binary with the **body unread**; the caller drains it with `Download::copy_to(sink)` through a fixed 64 KiB buffer (`DOWNLOAD_BUFFER_BYTES`), so peak memory is independent of file size (measured: 800 MB at 19 MB peak RSS). The *status* is still resolved eagerly, and that split is the point — a 404 or 401 must fail before the caller touches the filesystem, so a failed download never creates a file to clean up. `copy_to`'s `DownloadError` names the failing side: `Source` is a transport error (exit 3) leaving the destination undefined, `Sink` is local and belongs to the caller, the only party that knows what the destination is.
- `delete_json(path, query) -> Value` — DELETE expecting a JSON body (vs `delete()` returning `()`).

**Reading the body through `Read` changes what `--timeout` means for a download, and that is deliberate.** reqwest's blocking client keeps its timeout on the blocking side only (the inner async client is built without one) and enforces it by wrapping whichever future the caller is parked on. `Response::bytes()` parks on the *whole body*, so the 30s cap covered the entire transfer and a large-but-perfectly-healthy download died at the cap; `Read::read` parks on *one chunk*, so the same 30s becomes a **per-read idle timeout** — slow-but-alive completes, a stalled socket still dies 30s after the last byte. The header/connect phase is unchanged (one bounded wait in reqwest's `execute_request`) and **no non-download request's timeout semantics moved**. `read_timeout` was the obvious primitive, but reqwest 0.12.28 exposes it only on the async builder.

`sn attachment download --out <PATH>` stages into a dot-prefixed `.{base}.sn{pid}-{nanos}.part` in the destination's **own** directory and renames on success (same filesystem, hence atomic), because a truncated file sitting under the name the caller asked for is indistinguishable from a complete one — the failure has to be loud. A pre-existing destination therefore survives a failed download untouched, which is the property staging exists for and what its test asserts (`!dest.exists()` also passes for code that writes straight through and unlinks). The staging name is capped at `NAME_MAX` by truncating the readable base on a char boundary; uniqueness lives in pid+nanos. Ctrl-C unlinks the staging file and exits 130 — without that, every retry stranded another hidden `.part`. stdout has no such option: bytes on a pipe cannot be recalled, so a mid-stream failure is exit 3 with an envelope naming how many truncated bytes already went out.

`Client` also carries an `extra_headers: HeaderMap` for `sn raw --header`, applied **last** in `send()` — after auth and after the `Content-Type` that `request()` sets on bodied calls. Merging caller headers into `ClientBuilder`'s defaults instead is silently overridden for exactly `Content-Type`, since reqwest ranks request-level headers above client defaults; the request goes out under the old value with no error. `RequestBuilder::headers` replaces rather than appends, so a caller's `Content-Type` supplants ours instead of arriving as a second value. `build_client_with_headers` (in `cli/kernel.rs`) is a separate entry point so no other call site changed. `Authorization` never reaches here — `raw` rejects it at parse time, because a profile is the whole unit of identity and a credential in argv is visible to `ps`.
