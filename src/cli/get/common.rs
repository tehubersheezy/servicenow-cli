//! Small helpers the noun readers share.

use crate::cli::auth;
use crate::client::Client;
use crate::error::{Error, NO_HTTP_STATUS, Result};
use serde_json::Value;

/// A platform identifier (table, field, plugin id): letters, digits, `_`,
/// `.`, `-`. It rides in a URL path or an encoded query, so anything else
/// is refused before the network.
pub(super) fn identifier<'a>(raw: &'a str, what: &str) -> Result<&'a str> {
    let s = raw.trim();
    let ok = !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
    if ok {
        Ok(s)
    } else {
        Err(Error::Usage(format!(
            "'{raw}' is not a {what} (letters, digits, '_', '.', '-')"
        )))
    }
}

/// A free-text value bound for an encoded query: a `^` would splice in terms
/// of the caller's own, so it is refused rather than misqueried.
pub(super) fn query_value<'a>(raw: &'a str, what: &str) -> Result<&'a str> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(Error::Usage(format!("{what} must not be empty")));
    }
    if s.contains('^') || s.chars().any(char::is_control) {
        return Err(Error::Usage(format!(
            "{what} '{raw}' must not contain '^' (it would be parsed as an encoded-query \
             separator) or control characters"
        )));
    }
    Ok(s)
}

/// Page size for the walks a noun makes over a whole membership table.
pub(super) const PAGE: &str = "500";

pub(super) fn pairs(p: &[(&str, &str)]) -> Vec<(String, String)> {
    p.iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// The `result` array of a Table API list response, by move.
pub(super) fn rows(resp: Value) -> Vec<Value> {
    match crate::cli::kernel::take_field(resp, "result") {
        Some(Value::Array(rows)) => rows,
        _ => Vec::new(),
    }
}

/// A string field under `sysparm_display_value=false`, or the `value` half
/// of a `{value, display_value}` / `{value, link}` pair. Empty is `None`.
pub(super) fn value(row: &Value, field: &str) -> Option<String> {
    let v = row.get(field)?;
    let s = match v {
        Value::Object(o) => o.get("value").and_then(Value::as_str),
        other => other.as_str(),
    }?;
    (!s.is_empty()).then(|| s.to_string())
}

/// The `display_value` half of a `sysparm_display_value=all` pair (or the
/// bare string under `true`). Empty is `None`.
pub(super) fn display(row: &Value, field: &str) -> Option<String> {
    let v = row.get(field)?;
    let s = match v {
        Value::Object(o) => o.get("display_value").and_then(Value::as_str),
        other => other.as_str(),
    }?;
    (!s.is_empty()).then(|| s.to_string())
}

pub(super) fn opt(v: Option<String>) -> Value {
    v.map_or(Value::Null, Value::from)
}

/// `"true"` / `true` → true; anything else false.
pub(super) fn truthy(row: &Value, field: &str) -> bool {
    match row.get(field) {
        Some(Value::Bool(b)) => *b,
        Some(v) => value(&serde_json::json!({ "f": v }), "f").as_deref() == Some("true"),
        None => false,
    }
}

/// The operation succeeded and found nothing. Row ACLs make "absent" and
/// "not readable" the same bytes, so the message hedges.
pub(super) fn not_found(what: String) -> Error {
    Error::Api {
        status: NO_HTTP_STATUS,
        message: format!("{what} (or not readable by this profile)"),
        detail: None,
        transaction_id: None,
        sn_error: None,
    }
}

/// A returned row that does not satisfy the query asked: the instance
/// dropped a term and these rows are unfiltered.
pub(super) fn dropped(term: &str, table: &str) -> Error {
    Error::Instance {
        message: format!(
            "the instance dropped the {term} query term on {table}, so the rows returned are \
             arbitrary"
        ),
        detail: None,
    }
}

/// The caller, as the instance names them: user_name and sys_id. Fails
/// closed — a caller the instance cannot name is an error, never a guess.
pub(super) fn caller(client: &Client) -> Result<(String, String)> {
    let id = auth::identify(client)?.ok_or_else(unnamed)?;
    let sys_id = id.sys_id.ok_or_else(unnamed)?;
    Ok((id.user_name, sys_id))
}

fn unnamed() -> Error {
    Error::Instance {
        message: "the instance did not name the calling user".into(),
        detail: None,
    }
}

/// The groups `user` belongs to, as `(sys_id, name)`, each row checked
/// against the `user=` term (a dropped term returns everyone's memberships).
pub(super) fn groups_of(client: &Client, user: &str) -> Result<Vec<(String, String)>> {
    let q = pairs(&[
        ("sysparm_query", &format!("user={user}")),
        ("sysparm_fields", "user,group,group.name"),
        ("sysparm_display_value", "false"),
        ("sysparm_limit", PAGE),
    ]);
    let mut out = Vec::new();
    for row in client.paginate("/api/now/table/sys_user_grmember", &q, None) {
        let row = row?;
        if value(&row, "user").as_deref() != Some(user) {
            return Err(dropped("user=", "sys_user_grmember"));
        }
        if let Some(g) = value(&row, "group") {
            let name = value(&row, "group.name").unwrap_or_default();
            if !out.iter().any(|(id, _)| *id == g) {
                out.push((g, name));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn value_and_display_take_every_shape() {
        let row = json!({
            "a": "x",
            "b": {"value": "v", "display_value": "D"},
            "c": {"value": "r", "link": "https://x"},
            "e": "",
        });
        assert_eq!(value(&row, "a").as_deref(), Some("x"));
        assert_eq!(value(&row, "b").as_deref(), Some("v"));
        assert_eq!(display(&row, "b").as_deref(), Some("D"));
        assert_eq!(value(&row, "c").as_deref(), Some("r"));
        assert_eq!(value(&row, "e"), None);
        assert_eq!(value(&row, "missing"), None);
    }

    #[test]
    fn truthy_reads_strings_bools_and_pairs() {
        let row = json!({"a": "true", "b": true, "c": {"value": "true"}, "d": "false"});
        assert!(truthy(&row, "a") && truthy(&row, "b") && truthy(&row, "c"));
        assert!(!truthy(&row, "d") && !truthy(&row, "z"));
    }

    #[test]
    fn query_values_refuse_carets() {
        assert!(query_value("ok value", "x").is_ok());
        assert!(query_value("a^b", "x").is_err());
        assert!(query_value("  ", "x").is_err());
    }
}
