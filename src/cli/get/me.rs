//! `sn get me` — the caller: their user record, roles and groups.
//!
//! The instance names the caller (`auth::identify`: `ui/user/current_user`
//! first, which carries no roles), then the record is read by that sys_id.
//! Roles come from `sys_user_has_role` and groups from `sys_user_grmember`,
//! both itil-readable for the caller's own rows (measured, Brazil PDI); each
//! row is checked against the `user=` term, because a dropped term would
//! hand back everyone's grants as the caller's.

use super::common::{PAGE, caller, display, dropped, groups_of, opt, pairs, truthy, value};
use crate::cli::GlobalFlags;
use crate::cli::kernel::{connect, take_field, write_response};
use crate::client::Client;
use crate::error::Result;
use serde_json::{Value, json};
use std::collections::BTreeSet;

const USER_FIELDS: &str = "sys_id,user_name,name,email,title,department,manager,location,\
                           time_zone,active,last_login_time";

pub fn run(global: &GlobalFlags) -> Result<()> {
    let client = connect(global)?;
    let (_, sys_id) = caller(&client)?;
    let resp = client.get(
        &format!("/api/now/table/sys_user/{sys_id}"),
        &pairs(&[
            ("sysparm_fields", USER_FIELDS),
            ("sysparm_display_value", "all"),
        ]),
    )?;
    let user = take_field(resp, "result").unwrap_or(Value::Null);
    let (roles, direct) = roles_of(&client, &sys_id)?;
    let groups: Vec<Value> = groups_of(&client, &sys_id)?
        .into_iter()
        .map(|(id, name)| json!({ "sys_id": id, "name": name }))
        .collect();

    let out = json!({
        "sys_id": sys_id,
        "user_name": opt(value(&user, "user_name")),
        "name": opt(display(&user, "name")),
        "email": opt(value(&user, "email")),
        "title": opt(value(&user, "title")),
        "department": opt(display(&user, "department")),
        "manager": opt(display(&user, "manager")),
        "location": opt(display(&user, "location")),
        "time_zone": opt(value(&user, "time_zone")),
        "active": truthy(&user, "active"),
        "last_login": opt(value(&user, "last_login_time")),
        "admin": roles.contains("admin"),
        "roles": roles,
        "direct_roles": direct,
        "groups": groups,
    });
    write_response(global, &out)
}

/// Every active role the user holds (deduplicated — the table has one row per
/// grant path), and the subset granted directly rather than inherited.
fn roles_of(client: &Client, user: &str) -> Result<(BTreeSet<String>, BTreeSet<String>)> {
    let q = pairs(&[
        ("sysparm_query", &format!("user={user}^state=active")),
        ("sysparm_fields", "user,role.name,inherited"),
        ("sysparm_display_value", "false"),
        ("sysparm_limit", PAGE),
    ]);
    let mut all = BTreeSet::new();
    let mut direct = BTreeSet::new();
    for row in client.paginate("/api/now/table/sys_user_has_role", &q, None) {
        let row = row?;
        if value(&row, "user").as_deref() != Some(user) {
            return Err(dropped("user=", "sys_user_has_role"));
        }
        let Some(name) = value(&row, "role.name") else {
            continue;
        };
        if !truthy(&row, "inherited") {
            direct.insert(name.clone());
        }
        all.insert(name);
    }
    Ok((all, direct))
}
