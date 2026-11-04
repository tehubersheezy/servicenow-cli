//! `sn context` — the instance-side session context: which application scope
//! and update set the caller's tracked writes are captured under.
//!
//! This is per-user **server-side** state, not CLI config. When an API user
//! writes to a tracked table (`sys_script`, `sys_properties`, …), the artifact
//! is scoped and captured under whatever application scope and update set that
//! account's session context names — invisibly, unless something reads it.
//! The state lives in `sys_user_preference` rows: `apps.current_app` (the
//! scope's sys_id, literal `global` for Global), `sys_update_set` (the active
//! set), and one `updateSetForScope<scopeSysId>` per scope the user has
//! touched.
//!
//! The UI drives this through undocumented endpoints under
//! `/api/now/ui/concoursepicker/` ("Concourse" is the UI frame's internal
//! codename). Two findings shape this module (measured on PDIs, 2026-08-17
//! and 2026-11-04):
//!
//! - **`GET …/current` is the one authoritative answer.** The platform
//!   resolves the context itself — a preferred set that is Complete, Ignored
//!   or in another scope is replaced by the one a tracked write would really
//!   use — so `sn` reports what it says rather than re-deriving it from the
//!   preference rows. The resolution is persisted: the read rewrites stale
//!   preferences to the answer (and can create a scope's `Default` set), the
//!   same thing every UI page load and every tracked write does. It never
//!   changes where writes land.
//! - **The application PUT is a decoy over REST.** `PUT …/application
//!   {"app_id": X}` answers success, stages the per-scope update-set
//!   preferences — and moves only the *session's* scope, never
//!   `apps.current_app`. `sn` keeps no cookies, so every request is a fresh
//!   session that reads the row; `scope` therefore writes it directly (after
//!   the PUT, so the platform's own staging still runs), and the verifying
//!   re-read sees only what was persisted.
//!
//! Admin surface: for an account that cannot select update sets the picker
//! answers with an empty `currentUpdateSet`, which is reported as an error
//! rather than as a context.

use crate::cli::kernel::{connect, write_response};
use crate::cli::{GlobalFlags, auth};
use crate::client::Client;
use crate::error::{Error, Result};
use clap::Subcommand;
use serde_json::{Map, Value, json};

const PICKER_CURRENT: &str = "/api/now/ui/concoursepicker/current";
const PICKER_APPLICATION: &str = "/api/now/ui/concoursepicker/application";
const PICKER_UPDATESET: &str = "/api/now/ui/concoursepicker/updateset";
const PREF_TABLE: &str = "/api/now/table/sys_user_preference";
const CURRENT_APP_PREF: &str = "apps.current_app";

#[derive(Subcommand, Debug)]
pub enum ContextSub {
    /// Switch the current application scope (by scope name, display name, or
    /// sys_id) and stage that scope's update set; verified by re-read.
    Scope(ContextTargetArgs),
    /// Switch the current update set (by name or sys_id; must be in progress
    /// and in the current scope); verified by re-read.
    Updateset(ContextTargetArgs),
}

#[derive(clap::Args, Debug)]
pub struct ContextTargetArgs {
    /// The scope or update set to switch to: name or sys_id.
    pub target: String,
}

/// One application scope.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ScopeRow {
    pub(crate) sys_id: String,
    pub(crate) name: String,
    pub(crate) scope: String,
}

/// One update set, with the scope it belongs to (selection side).
#[derive(Debug, Clone, PartialEq)]
struct UpdateSetRow {
    sys_id: String,
    name: String,
    application: String,
}

/// The current update set as the platform reports it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CurrentUpdateSet {
    pub(crate) sys_id: String,
    pub(crate) name: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ContextState {
    pub(crate) scope: ScopeRow,
    pub(crate) update_set: CurrentUpdateSet,
}

pub fn show(global: &GlobalFlags) -> Result<()> {
    let client = connect(global)?;
    let state = read_context(&client)?;
    write_response(global, &context_json(&state, None))
}

pub fn set_scope(global: &GlobalFlags, args: ContextTargetArgs) -> Result<()> {
    let target = validate_target(&args.target)?;
    let client = connect(global)?;
    let previous = read_context(&client)?;
    let scope = resolve_scope(&client, target)?;

    // The platform's own staging half first: the picker PUT creates the
    // per-scope update-set preferences the way the UI would. Its "success"
    // proves nothing about the scope itself (see module docs) — the
    // preference write below and the verifying re-read are what count.
    client.put(PICKER_APPLICATION, &[], &json!({ "app_id": scope.sys_id }))?;
    upsert_current_app_pref(&client, &scope.sys_id)?;

    let state = read_context(&client)?;
    if state.scope.sys_id != scope.sys_id {
        return Err(Error::Api {
            status: 200,
            message: format!(
                "scope switch did not persist: asked for {} ({}), instance reports {} ({})",
                scope.name, scope.sys_id, state.scope.name, state.scope.sys_id
            ),
            detail: None,
            transaction_id: None,
            sn_error: None,
        });
    }
    write_response(global, &context_json(&state, Some(&previous)))
}

pub fn set_updateset(global: &GlobalFlags, args: ContextTargetArgs) -> Result<()> {
    let target = validate_target(&args.target)?;
    let client = connect(global)?;
    let previous = read_context(&client)?;
    let set = resolve_update_set(&client, target, &previous.scope)?;
    // Already current: nothing to switch. The picker refuses this case with a
    // 400 "Cannot set update set as current" (measured 2026-11-04,
    // dev457896), which would report a no-op as a failure.
    if previous.update_set.sys_id == set.sys_id {
        return write_response(global, &context_json(&previous, Some(&previous)));
    }

    let resp = client.put(PICKER_UPDATESET, &[], &json!({ "sysId": set.sys_id }))?;
    if resp.pointer("/result/success").and_then(Value::as_bool) != Some(true) {
        return Err(Error::Instance {
            message: "the update-set picker did not acknowledge the change".into(),
            detail: Some(format!("response: {resp}")),
        });
    }

    let state = read_context(&client)?;
    if state.update_set.sys_id != set.sys_id {
        return Err(Error::Api {
            status: 200,
            message: format!(
                "update set selection did not persist: asked for {} ({}), instance reports {} \
                 ({})",
                set.name, set.sys_id, state.update_set.name, state.update_set.sys_id
            ),
            detail: None,
            transaction_id: None,
            sn_error: None,
        });
    }
    write_response(global, &context_json(&state, Some(&previous)))
}

/// Ask the platform for the resolved context: one `GET …/current`.
pub(crate) fn read_context(client: &Client) -> Result<ContextState> {
    parse_current(&client.get(PICKER_CURRENT, &[])?)
}

/// Parse the picker's `current` answer. Both halves are required: a missing
/// application is schema drift, and an empty `currentUpdateSet` is what an
/// account that cannot select update sets gets — neither is a context.
fn parse_current(resp: &Value) -> Result<ContextState> {
    let app = resp.pointer("/result/currentApplication");
    let scope = app.and_then(|a| {
        Some(ScopeRow {
            sys_id: str_field(a, "sysId")?,
            name: str_field(a, "name")?,
            scope: str_field(a, "scopeName")?,
        })
    });
    let Some(scope) = scope else {
        return Err(Error::Instance {
            message: "the application picker answered without a current application".into(),
            detail: Some(format!("response: {resp}")),
        });
    };

    let set = resp.pointer("/result/currentUpdateSet").and_then(|u| {
        Some(CurrentUpdateSet {
            sys_id: str_field(u, "sysId")?,
            name: strip_scope_suffix(&str_field(u, "name")?, &scope.name),
        })
    });
    let Some(update_set) = set else {
        return Err(Error::Instance {
            message: "the instance names no current update set for this account".into(),
            detail: Some(
                "the picker resolves one for every account that can select update sets \
                 (admin or delegated developer); an empty answer means this one cannot"
                    .into(),
            ),
        });
    };

    Ok(ContextState { scope, update_set })
}

/// The picker labels a set `<name> [<scope name>]`; report the record's own
/// name, which is what `sn context updateset` matches on. Only that exact
/// suffix is removed.
fn strip_scope_suffix(label: &str, scope_name: &str) -> String {
    label
        .strip_suffix(&format!(" [{scope_name}]"))
        .unwrap_or(label)
        .to_string()
}

fn context_json(state: &ContextState, previous: Option<&ContextState>) -> Value {
    let mut out = Map::new();
    out.insert("scope".into(), scope_json(&state.scope));
    out.insert("update_set".into(), update_set_json(&state.update_set));
    if let Some(prev) = previous {
        out.insert(
            "previous".into(),
            json!({
                "scope": scope_json(&prev.scope),
                "update_set": update_set_json(&prev.update_set),
            }),
        );
    }
    Value::Object(out)
}

fn scope_json(scope: &ScopeRow) -> Value {
    json!({ "sys_id": scope.sys_id, "name": scope.name, "scope": scope.scope })
}

fn update_set_json(set: &CurrentUpdateSet) -> Value {
    json!({ "sys_id": set.sys_id, "name": set.name })
}

/// `scope <TARGET>` resolution: one Table API read matching scope name,
/// display name, or sys_id, all exact.
pub(crate) fn resolve_scope(client: &Client, target: &str) -> Result<ScopeRow> {
    let resp = client.get(
        "/api/now/table/sys_scope",
        &query_pairs(&[
            (
                "sysparm_query",
                &format!("scope={target}^ORname={target}^ORsys_id={target}"),
            ),
            ("sysparm_fields", "sys_id,name,scope"),
            ("sysparm_display_value", "false"),
            ("sysparm_limit", "3"),
        ]),
    )?;
    let rows: Vec<ScopeRow> = table_rows(&resp)
        .iter()
        .filter_map(|r| {
            Some(ScopeRow {
                sys_id: str_field(r, "sys_id")?,
                name: str_field(r, "name")?,
                scope: str_field(r, "scope")?,
            })
        })
        .collect();
    match rows.len() {
        0 => Err(Error::Usage(format!(
            "no application scope matching '{target}' (by scope name, display name, or sys_id)"
        ))),
        1 => Ok(rows.into_iter().next().expect("len checked")),
        _ => Err(Error::Usage(format!(
            "'{target}' matches more than one scope: {}; use the sys_id",
            rows.iter()
                .map(|r| format!("{} ({})", r.scope, r.sys_id))
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// The stored choice value for a selectable set. **The space is real**: the
/// column's value is `in progress`, not `in_progress`, and the wrong spelling
/// silently matches nothing (measured — every real set then "failed" the
/// diagnostic re-query's explanation instead).
pub(crate) const STATE_IN_PROGRESS: &str = "in progress";

/// `updateset <TARGET>` resolution, restricted to in-progress sets in the
/// current scope — the same population the UI picker offers. The OR group
/// leads and the AND terms trail because a trailing AND applies to the whole
/// `^OR` group. A miss re-queries without the restriction so the error can
/// say *why* (wrong scope, completed) instead of "not found".
fn resolve_update_set(client: &Client, target: &str, scope: &ScopeRow) -> Result<UpdateSetRow> {
    let rows = update_set_query(
        client,
        &format!(
            "sys_id={target}^ORname={target}^application={}^state={STATE_IN_PROGRESS}",
            scope.sys_id
        ),
    )?;
    match rows.len() {
        0 => {
            let near = update_set_query(client, &format!("sys_id={target}^ORname={target}"))?;
            match near.first() {
                Some(row) if row.application != scope.sys_id => Err(Error::Usage(format!(
                    "update set '{}' ({}) belongs to another scope; switch first with \
                     `sn context scope`, or pick one in {}",
                    row.name, row.sys_id, scope.name
                ))),
                Some(row) => Err(Error::Usage(format!(
                    "update set '{}' ({}) is not in progress; only in-progress sets can be \
                     selected",
                    row.name, row.sys_id
                ))),
                None => Err(Error::Usage(format!(
                    "no update set matching '{target}' (by name or sys_id)"
                ))),
            }
        }
        1 => Ok(rows.into_iter().next().expect("len checked")),
        _ => Err(Error::Usage(format!(
            "'{target}' matches more than one update set: {}; use the sys_id",
            rows.iter()
                .map(|r| format!("{} ({})", r.name, r.sys_id))
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

fn update_set_query(client: &Client, query: &str) -> Result<Vec<UpdateSetRow>> {
    let resp = client.get(
        "/api/now/table/sys_update_set",
        &query_pairs(&[
            ("sysparm_query", query),
            ("sysparm_fields", "sys_id,name,application,state"),
            ("sysparm_display_value", "false"),
            ("sysparm_limit", "3"),
        ]),
    )?;
    Ok(table_rows(&resp)
        .iter()
        .filter_map(|r| {
            Some(UpdateSetRow {
                sys_id: str_field(r, "sys_id")?,
                name: str_field(r, "name")?,
                application: reference_value(r.get("application")).unwrap_or_default(),
            })
        })
        .collect())
}

/// Point `apps.current_app` at a scope — the write the picker PUT refuses to
/// do over REST. The row is per user, so the caller must be nameable; update
/// the existing row or create the first one.
fn upsert_current_app_pref(client: &Client, scope_sys_id: &str) -> Result<()> {
    let user = auth::identify(client)?
        .and_then(|id| id.sys_id)
        .ok_or_else(|| Error::Instance {
            message: "the instance did not name the calling user, so their preference row \
                      cannot be located"
                .into(),
            detail: None,
        })?;
    let resp = client.get(
        PREF_TABLE,
        &query_pairs(&[
            (
                "sysparm_query",
                &format!("user={user}^name={CURRENT_APP_PREF}^ORDERBYsys_created_on"),
            ),
            ("sysparm_fields", "sys_id"),
            ("sysparm_limit", "2"),
        ]),
    )?;
    let existing = table_rows(&resp)
        .first()
        .and_then(|r| str_field(r, "sys_id"));
    match existing {
        Some(row_id) => {
            client.patch(
                &format!("{PREF_TABLE}/{row_id}"),
                &[],
                &json!({ "value": scope_sys_id }),
            )?;
        }
        None => {
            client.post(
                PREF_TABLE,
                &[],
                &json!({ "user": user, "name": CURRENT_APP_PREF, "value": scope_sys_id }),
            )?;
        }
    }
    Ok(())
}

/// A target goes into an encoded query verbatim, so a `^` in it would splice
/// extra terms into our own filter. No real scope or update-set name carries
/// one; refuse rather than misquery.
fn validate_target(target: &str) -> Result<&str> {
    let t = target.trim();
    if t.is_empty() {
        return Err(Error::Usage("target must not be empty".into()));
    }
    if t.contains('^') {
        return Err(Error::Usage(
            "target must not contain '^' (it would be parsed as an encoded-query separator)".into(),
        ));
    }
    Ok(t)
}

/// The `result` array of a Table API list response.
fn table_rows(resp: &Value) -> Vec<Value> {
    resp.get("result")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn str_field(row: &Value, field: &str) -> Option<String> {
    row.get(field)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// A Table API reference field under `sysparm_display_value=false` is
/// `{value, link}`; older shapes are a bare string. Take either.
fn reference_value(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Object(o) => o
            .get("value")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        _ => None,
    }
}

fn query_pairs(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn current(app: Value, set: Value) -> Value {
        json!({ "result": {
            "currentApplication": app,
            "currentUpdateSet": set,
            "currentDomain": { "value": "global" },
        }})
    }

    fn global_app() -> Value {
        json!({ "name": "Global", "scopeName": "global", "sysId": "global" })
    }

    #[test]
    fn reports_what_the_platform_resolved() {
        let resp = current(
            json!({ "name": "Access Analyzer", "scopeName": "sn_access_analyzer", "sysId": "aa" }),
            json!({ "name": "Default [Access Analyzer]", "sysId": "d1" }),
        );
        let out = context_json(&parse_current(&resp).unwrap(), None);
        assert_eq!(
            out,
            json!({
                "scope": { "sys_id": "aa", "name": "Access Analyzer", "scope": "sn_access_analyzer" },
                "update_set": { "sys_id": "d1", "name": "Default" },
            })
        );
    }

    #[test]
    fn only_the_exact_scope_suffix_is_stripped() {
        assert_eq!(strip_scope_suffix("Default [Global]", "Global"), "Default");
        assert_eq!(
            strip_scope_suffix("Fixes [Other]", "Global"),
            "Fixes [Other]"
        );
        assert_eq!(strip_scope_suffix("Plain", "Global"), "Plain");
    }

    #[test]
    fn empty_update_set_is_an_error_not_a_context() {
        // What a non-admin gets: the application, and `currentUpdateSet: {}`.
        let err = parse_current(&current(global_app(), json!({}))).unwrap_err();
        assert!(matches!(err, Error::Instance { .. }));
        assert!(err.to_string().contains("no current update set"));
    }

    #[test]
    fn missing_application_is_an_error() {
        let resp = json!({ "result": { "currentUpdateSet": { "name": "Default", "sysId": "d" } } });
        assert!(matches!(
            parse_current(&resp).unwrap_err(),
            Error::Instance { .. }
        ));
    }

    #[test]
    fn previous_context_rides_along() {
        let prev = parse_current(&current(
            global_app(),
            json!({ "name": "Default [Global]", "sysId": "d" }),
        ))
        .unwrap();
        let state = parse_current(&current(
            json!({ "name": "WCC", "scopeName": "x_wcc", "sysId": "wcc" }),
            json!({ "name": "Default [WCC]", "sysId": "w" }),
        ))
        .unwrap();
        let out = context_json(&state, Some(&prev));
        assert_eq!(out["previous"]["scope"]["sys_id"], "global");
        assert_eq!(out["previous"]["update_set"]["name"], "Default");
        assert_eq!(out["update_set"]["sys_id"], "w");
    }

    #[test]
    fn target_validation() {
        assert!(validate_target("  x_scope ").is_ok());
        assert!(matches!(validate_target(""), Err(Error::Usage(_))));
        assert!(matches!(validate_target("a^b"), Err(Error::Usage(_))));
    }

    #[test]
    fn reference_value_takes_both_shapes() {
        assert_eq!(
            reference_value(Some(&json!({"value": "abc", "link": "https://x"}))),
            Some("abc".into())
        );
        assert_eq!(reference_value(Some(&json!("abc"))), Some("abc".into()));
        assert_eq!(reference_value(Some(&json!(""))), None);
        assert_eq!(reference_value(None), None);
    }
}
