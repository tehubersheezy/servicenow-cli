---
paths:
  - "src/cli/context.rs"
---

# `sn context`

## Modules

- `src/cli/context.rs` → sn context [scope|updateset] (instance-side session context: the application scope + update set the caller's tracked writes are captured under; read is one `GET …/concoursepicker/current`, the platform's own resolution, reported as-is; writes go through the concoursepicker PUT plus a direct apps.current_app preference write — the picker's application PUT alone never moves the scope over REST — and verify by re-read)

**Endpoints** (`context` (undocumented)): `GET /api/now/ui/concoursepicker/current` (the read); `PUT /api/now/ui/concoursepicker/{application,updateset}` (writes)

the UI pickers' endpoints. The application PUT takes `{"app_id"}` (its sibling takes `{"sysId"}` — they do not share a body shape) and **never writes `apps.current_app` over REST** — it only stages the per-scope update-set preferences — so `context scope` writes the preference row directly after it. `sys_update_set.state` stores `"in progress"` **with a space**; `state=in_progress` silently matches nothing.

**One authoritative source: `GET …/current`** (decided 2026-11-04, issue #118). The read used to be a GraphQL query over the raw preference rows that re-derived the platform's reconciliation in code and reported `source`/`preference_stale`; it checked scope but not state, so it reported Complete sets as current. It now reports what `current` returns and nothing else — no `source`, no stale flag. Accepted cost: `current` persists its resolution (rewrites stale preferences, and can create a scope's `Default` set), the same side effect every UI page load and tracked write has; it never changes where writes land. The picker labels a set `<name> [<scope name>]`; only that exact suffix is stripped so `name` matches what `context updateset` takes. `sn` holds no cookies, so the verifying re-read after a PUT is a fresh session that sees only persisted state. Admin-only surface (Abey, 2026-11-04 — no itil testing needed): a non-admin gets `currentUpdateSet: {}` (measured as itil), which is `Error::Instance`.

**A Complete or Ignored preferred set is not where writes land** (measured 2026-11-04, dev457896, issue #118). With `sys_update_set` and `updateSetForScopeglobal` both naming a set whose `state` was then changed to `complete` (or `ignore`), the preferences stay pointing at it until the next tracked write. That write is captured into the scope's existing `is_default=true` In-progress set (no new set was generated), and the platform rewrites both preferences to that set. So the scope-memory row can be stale for the same reason as the active one, and the honest answer is the fallback.

**How the UI picker drives it** (measured 2026-11-04, dev457896, headed browser + in-page fetch; HAR in `~/Projects/sn-devops/docs/picker-118/`):
- Opening the picker is one `GET …/concoursepicker/current` → `{currentApplication{name,scopeName,sysId}, currentUpdateSet{name,sysId}, currentDomain}`; the 183-app list is `GET …/application` → `{current, list[]}`; the update-set list is `GET …/updateset` → `{current, updateSet[]}`, and `updateSet[]` holds **only In-progress sets** of the current scope.
- Picking an app: `PUT …/application {"app_id"}`, then the UI immediately `GET`s `application` and `updateset`. Switching into a scope with no default set creates `Default [<scope>]` (is_default, In progress) and the toast "Your current update set has been changed to …".
- Picking an update set: one `PUT …/updateset {"name":"<name> [<scope>]","sysId"}`, no follow-up reads.
- **The scope switch lives in the session first.** Even in a UI session, `PUT application` alone leaves `apps.current_app` unchanged; the session's `GET application`/`current` report the new scope while the preference row still names the old one, and the row catches up only on a page load. A fresh REST session (what `sn` is) reads the row — hence `context scope` writing it directly.
- **`GET …/current` heals too, not only `GET …/updateset`.** With both preferences naming a Complete set, a single `GET current` rewrote them to the scope's default set. Any page load fires it, so in the UI a completed preferred set self-heals on the next navigation; over REST nothing heals it until a tracked write (see above).
