---
paths:
  - "src/cli/script.rs"
  - "tests/script.rs"
---

# `sn script`

## Modules

- `src/cli/script.rs` → sn script run (server-side JS via the classic-UI sys.scripts.do processor: UI session minted by Client::ui_session, form POSTed by Client::post_ui_form through a non-redirect-following client, HTML result parsed into {ok, scope, output, messages, error, elapsed_ms, history_id, rollback_context}; gated by confirm_destructive)

**Endpoints** (`script` (undocumented)): `GET /api/now/sg/impersonation/session` (mints the session); `POST /sys.scripts.do` (form-encoded)

Not REST: the *Scripts - Background* processor, which demands a UI session. The session GET sets the cookies and its `SessionToken` **is** the session's `g_ck` CSRF token, so nothing is scraped from HTML; `Client::ui_session` never logs that body, and `post_ui_form` logs form field names only. The POST carries cookie + `X-UserToken` + `sysparm_ck` (either token spelling alone is accepted, neither → a 200 whose body is `not authorized`; no cookie → an empty 200 and the script did **not** run) **and keeps the profile's auth header** — basic, OAuth and API-key all work, the script runs as that user. Every verdict about the *script* is inside a 200 of HTML (all measured on dev421992): `[H:MM:SS.mmm] …Script completed in scope <scope>: script…sys_script_execution_history.do?sys_id=<id>…<PRE>entry<BR/>…</PRE>` — "completed" is printed for failures too; entries are HTML-escaped, so splitting on the literal `<BR/>` is exact; errors are entries starting `Script compilation error:` / `Script execution error:`, and every entry after one is its detail. Script output is identified by prefix, and **the prefix is the scope**: `*** Script: ` in global, `<scope>: ` in an app scope (`gs.print` is refused in app scopes); everything else (slow-BR notices, SQL debug, `gs.log(msg, source)`'s `source: msg`) goes to `messages`. The issue's printed-sentinel design was not used: it would break in app scopes, vanish on compile errors, and shift reported line numbers. `sys_scope` wants a **sys_id** — an unknown value or a scope *name* silently runs in global — so `--scope` is resolved first (`context::resolve_scope`) and the reported scope checked after; store-app scopes answer `not authorized` even for admin. Refusals: non-admin basic → 403 (empty body); non-admin OAuth → `302 → /logout_redirect.do`, which is why the command's client is built by `kernel::connect_without_redirects` (following it yields a 200 login page). `record_for_rollback=on` (`--rollback`) makes the history row's `rollback_context` point at a `sys_rollback_context` — populated by the time the POST returns, empty when the script wrote nothing; `GlideRollbackWorker` (what the context form's Rollback action runs) undoes it. The run is synchronous: a client timeout does not stop the script
