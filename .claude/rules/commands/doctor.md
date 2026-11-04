---
paths:
  - "src/cli/doctor.rs"
  - "tests/doctor.rs"
---

# `sn doctor`

## Modules

- `src/cli/doctor.rs` → sn doctor [--need-role/--need-plugin/--need-property] (one-round-trip preflight over scripted GraphQL: identity, admin flag, capabilities, and pass/fail/unavailable checks; only `pass` passes, and any other outcome writes the report *then* exits 2 with NO_HTTP_STATUS. A root the schema lacks is dropped and the document re-sent, never absorbed silently)

**Endpoints** (`doctor` (undocumented)): `POST /api/now/graphql` — `now.sessionUser.getMatchingRoles`, `snWorkflowStudio.workflowStudio.isPluginInstalled`, `snDecisionTable.sysProperties.getSysPropertyValue`, `GlideRecord_Query`

scripted namespaces owned by product scopes, measured as admin and as `itil` (dev421992, 2026-09-28). **`getMatchingRoles` fails open under admin** — it is `roleNames.filter(gs.hasRole)`, and `gs.hasRole` is true for *any* name under `admin` (`nonexistent_xyz` included; elevated `security_admin` excepted) — so a role passes only when `sys_user_role` `_rowCount` for `name=<role>` is 1, and a pass under admin carries `basis: "admin"`. `_rowCount` counts before row ACLs (`itil` cannot read the `admin` role row but counts it), and a count above 1 is a dropped filter (a bad term returned all 981 roles) → `unavailable`. Role names ride in an encoded query, so they are charset-validated before connect. `isPluginInstalled` is truthful (fakes false, no prefix match, case-sensitive) and the only route — `sys_plugins` is 403 even to admin, `v_plugin` times out server-side; `workflowStudio` is `null` for non-admins; canary `com.glide.graphql` must answer true. `getSysPropertyValue` returns its default for unset **and** empty properties (default is a sentinel; they read as one state), `null` for password-type properties, and `null` for every read by a non-admin while the namespace itself resolves — so null is "unreadable", never "unset". Introspection (`__type`) resets the connection on the PDI; argument types were read off `VariableTypeMismatch` errors instead (`roleNames: [String]!`, `plugin_name`/`sys_property_name: String!`)
