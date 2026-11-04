---
paths:
  - "src/cli/auth.rs"
  - "src/cli/ping.rs"
  - "src/cli/user.rs"
  - "tests/identity.rs"
---

# Identity: who the caller is

## Modules

- `src/cli/user.rs` → sn user me (authenticated user: `ui/user/current_user` names the sys_id, then a direct `sys_user/{sys_id}` read; the scripted query is only the fallback)
- `src/cli/ping.rs` → sn ping (auth + latency + build version + the instance's own account of who the caller is)

**Endpoints** (identity (undocumented)): `GET /api/now/ui/user/current_user` (names the caller: `user_name`, `user_sys_id`, `user_display_name`); `GET /api/now/sg/impersonation/session` (`CurrentUser`, `OriginalUser`, `admin`, `CanImpersonate`)

the instance's own answer to "who is this request from", with no query to misparse and no `sys_user` read ACL to satisfy. **`sg/impersonation/session` is not `result`-wrapped** — it is a bare object, and a shape that is (a proxy's JSON, a future release) fails the `CurrentUser` check and falls back rather than reporting a name. Its response also carries `SessionToken`, a **live CSRF credential**: it is copied into no output field, and `ping::get_without_body_log` drops response-body logging for exactly that request so `-ddd` cannot print it either. Only a 404 earns the `sys_user` fallback — 401/403 is a real auth verdict (exit 4) and a ping that shopped around for an endpoint willing to answer would report `ok` for dead credentials

- **Whose identity is it:** `auth::identify` (which `whoami`, `sn ping` and `sn profile add`/`sn init` verification all sit on) asks `/api/now/ui/user/current_user` first and only falls back to the `user_name=javascript:gs.getUserName()` read of `sys_user`. A bare `sysparm_limit=1` read of that table returns whichever row sorts first — an arbitrary account, not the caller — so never use it to report "who you logged in as" (that bug shipped for several releases, and then came back through a different door: an instance that cannot evaluate the scripted term **silently drops it**, leaving exactly that bare limit. Measured on a PDI, a bogus term returns `survey.user` and `lucius.bagnoli`, neither of them the caller). The fallback therefore asks for `sysparm_limit=2`: `user_name` is unique, so an evaluated filter matches at most one row and a second row is proof the term is gone — that case reports no user (or, in `sn user me`, `Error::Instance`) rather than a stranger. Every path here either produces an identity it can defend or produces `None`; a report of who you are must fail closed.
