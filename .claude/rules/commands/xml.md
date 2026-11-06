---
paths:
  - "src/cli/xml.rs"
  - "tests/xml.rs"
---

# `sn xml`

`src/cli/xml.rs` → `sn xml export <TABLE> <QUERY>` and `sn xml import <FILE>`. The file is passed through unparsed in both directions, by decision: nothing validates the XML.

**Endpoints** (undocumented): `GET /<table>.do?UNL&sysparm_query=…[&sysparm_record_count=N]`; `POST /sys_upload.do` (multipart: `sysparm_ck`, `sysparm_target`, `attachFile`), inside a `Client::ui_session`. Both use a no-redirect client. `?XML` is a different format: an `<xml>` root with no `action` attributes or related rows.

## Measured (Brazil PDI dev457896, 2026-11-05; basic and OAuth client_credentials, not API key)

- **A term on a nonexistent column is dropped.** The export and `/api/now/stats` both return the whole table for `zzz_nope=1`, and `caller_id.zzz_nope=1` likewise, so neither can check the other. `check_columns` walks each query column, dot-walks included, through `ui/meta` before exporting.
- `sysparm_record_count` is honoured as a cap; `sysparm_sys_id=` is not a parameter the processor reads and exports the whole table.
- An unknown table answers `200` with an empty body, and `ui/meta` answers `{"columns":{}}`.
- The export includes related rows (`sys_journal_field`, `sys_attachment`, `sys_attachment_doc`), and a parent table's rows arrive under their child class (`task` yields `<change_request …>`).
- **`sys_upload.do` reports nothing.** A load, a missing or bad CSRF token, garbage, an unknown table and an itil caller all answer an empty `200` (a `302` to `sysparm_referring_url` when that is sent). So exit 0 means uploaded, not loaded. Reading rows back was built and removed: it needs the file parsed.
- `sysparm_target` must be non-empty and its value is ignored (blank or absent loads nothing).
- A load is synchronous and restores a row exactly: `sys_updated_on` and `sys_mod_count` included. It inserts a deleted row with its sys_id.
- `action="DELETE"` deletes the row, hence the `--yes` gate. A missing `action` loads as an upsert, and a renamed `<xml>` root still loaded.
- **`.do?UNL` refuses a bearer token** (`401`, `WWW-Authenticate: Basic`), and `?XML` likewise: with the token alone, token plus session cookie, and cookie alone until the session has loaded a page. After one `GET /home.do`, cookies alone export; adding the token back is `401` again. `unload_client` does this for bearer and API-key profiles (API key unmeasured) and basic goes straight in. `sn impersonate` is already cookie-only, so it only needs the page load.
- `sys_upload.do` and `sys.scripts.do` accept the same session with the bearer token attached, so `import` needs no page load.
- A delete followed by an import restores the attachment too: `sys_attachment` and `sys_attachment_doc` rows reload and the file downloads byte-identical. Restored rows keep their `sys_id`; `sys_mod_count` is restored too, as exported.
- itil: export 403 (basic, OAuth and under `sn impersonate`), import the silent no-op.
