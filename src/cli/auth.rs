use crate::cli::table::{build_client, build_profile};
use crate::cli::GlobalFlags;
use crate::error::Result;
use serde_json::json;
use std::io::Write;

pub fn test(global: &GlobalFlags) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let v = client.get(
        "/api/now/table/sys_user",
        &[("sysparm_limit".into(), "1".into())],
    )?;
    let user = v["result"]
        .get(0)
        .and_then(|r| r.get("user_name"))
        .and_then(|x| x.as_str())
        .unwrap_or(&profile.username);
    let msg = json!({
        "ok": true,
        "instance": profile.instance,
        "username": user,
        "profile": profile.name,
    });
    writeln!(std::io::stderr(), "{msg}").ok();
    Ok(())
}
