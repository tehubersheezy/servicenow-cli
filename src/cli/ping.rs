use crate::cli::table::{build_client, build_profile, format_from_flags};
use crate::cli::GlobalFlags;
use crate::error::Result;
use crate::output::emit_value;
use serde_json::{json, Value};
use std::time::Instant;

pub fn run(global: &GlobalFlags) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;

    let started = Instant::now();
    let _ = client.get(
        "/api/now/table/sys_user",
        &[("sysparm_limit".into(), "1".into())],
    )?;
    let latency_ms = started.elapsed().as_millis() as u64;

    let (build_name, build_tag) = match client.get(
        "/api/now/table/sys_properties",
        &[
            (
                "sysparm_query".into(),
                "name=glide.buildname^ORname=glide.buildtag".into(),
            ),
            ("sysparm_fields".into(), "name,value".into()),
            ("sysparm_limit".into(), "2".into()),
        ],
    ) {
        Ok(v) => extract_build(&v),
        Err(_) => (Value::Null, Value::Null),
    };

    let out = json!({
        "ok": true,
        "profile": profile.name,
        "instance": profile.instance,
        "username": profile.username,
        "latency_ms": latency_ms,
        "build_name": build_name,
        "build_tag": build_tag,
    });

    emit_value(std::io::stdout().lock(), &out, format_from_flags(global))
        .map_err(crate::output::map_stdout_err)
}

fn extract_build(v: &Value) -> (Value, Value) {
    let mut name = Value::Null;
    let mut tag = Value::Null;
    let rows = v
        .get("result")
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default();
    for row in rows {
        let prop = row.get("name").and_then(|x| x.as_str()).unwrap_or("");
        let val = row.get("value").cloned().unwrap_or(Value::Null);
        match prop {
            "glide.buildname" => name = val,
            "glide.buildtag" => tag = val,
            _ => {}
        }
    }
    (name, tag)
}
