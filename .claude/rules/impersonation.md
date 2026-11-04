---
paths:
  - "src/cli/impersonate.rs"
  - "tests/impersonate.rs"
---

# Impersonation

## Modules

- `src/cli/impersonate.rs` → sn impersonate <user> -- <cmd> (mints one cookie-only session, hops to the user, verifies, runs the wrapped command in-process via main.rs's `run`, then reverts + logs out on every exit incl. Ctrl-C; see "Impersonation")

`sn impersonate <user> -- <cmd>` is the one command that holds a **server-side session** across requests, because impersonation is session state. Any per-request credential (Basic or bearer — measured for both) re-authenticates as the credential holder on every call and wipes the impersonation, so the flow is: one credentialed `GET /api/now/sg/impersonation/session` (`Client::get_secret_with_cookies`) mints the session and reads its `SessionToken`; from then on every request goes out as `Auth::Session(Arc<SessionJar>)` — `Cookie` + `X-UserToken`, both `set_sensitive`, **no `Authorization`** — and `send()` folds each response's `Set-Cookie` back into the jar. The wrapped command runs **in-process** (parsed with `cli::parse_from`, dispatched through the `run` fn pointer `main.rs` passes down), and `kernel::SessionScope` makes every `build_client` in that window return a session client (no OAuth refresh, no API key) and clears on drop. Connection globals must precede `--` (they are copied onto the inner `GlobalFlags` so it resolves the same profile); presentation globals merge.

Measured on dev421992 (Australia), and why the code looks the way it does:

- **`POST /api/now/ui/impersonate/{sys_id}` answers `201` when it did nothing** — a caller without the role gets 201 and an unchanged session. So `CanImpersonate: false` is refused up front (exit 4, `NO_HTTP_STATUS`), and after the POST the session is re-read and must name the target *and* the original user, or the command never runs (`Error::Instance`).
- The POST did **not** require `X-UserToken`, and the token did **not** rotate across a hop (the issue reported both). The token is sent anyway and re-read after the hop; nothing relies on either observation.
- **Ending** = POST back to the original user's sys_id, `GET /logout.do` (302 → `logout_success.do`; the cookies answer 401 afterwards), then re-read the session and require the 401. `Hop::end` is idempotent under a mutex shared by an `EndOnDrop` guard (early returns, panics) and a `ctrlc` handler (ends, then exits 130) — a signal mid-request waits for nothing. The session's cookies live only in memory, so a SIGKILL leaves an unreachable session that lapses at the idle timeout. The ctrlc handler is process-wide and single, so a wrapped `attachment download` loses its staging-file unlink on Ctrl-C; ending the impersonation is the one that wins.
- The API-only-session reaper that `sn watch` fights does **not** touch this: an impersonated session polled over REST every 15s stayed alive and impersonating for 137s.
- A cookie-authenticated response carries `x-usertoken-request` (echoing the token sent) and `x-usertoken-response`; `observability::header_display_value` masks every `x-usertoken*` header at `-dd`, and `get_secret*` never logs the session body.
- Refused as wrapped commands before any request: `init`, `profile` (local state), `watch` and `flow tail` (their own websocket session), `open` (the browser's session is not this one), `script` (it mints its own UI session for `sys.scripts.do`, so it would run as the profile's user, not the impersonated one), `cache` (the schema cache is shared per instance and must hold the profile's view), `completion`/`introspect` (no instance), `impersonate` (no nesting). `refusal` ends in a catch-all, so **a new command that opens its own session must be added there** — it is otherwise silently allowed.
