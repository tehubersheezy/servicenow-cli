//! `sn get instance` — which release and build the instance runs, and which
//! node answered.
//!
//! Measured on a Brazil PDI: `glide.buildname`/`glide.buildtag` have **no
//! `sys_properties` row** (a Table API read finds nothing), yet
//! `gs.getProperty` answers them — so the values come from the GraphQL
//! `getSysPropertyValue` getter, one document for all of them. That getter is
//! admin-only: a non-admin gets `null` for each, reported as `null` rather
//! than as an error, since the instance URL is still an answer. The getter
//! folds unset into the default, so a sentinel default reads back as `null`.

use crate::cli::GlobalFlags;
use crate::cli::graphql::{errors_to_api_error, execute, graphql_errors};
use crate::cli::kernel::{connect, write_response};
use crate::error::Result;
use serde_json::{Map, Value, json};

const UNSET: &str = "\u{1}sn-unset\u{1}";

/// Output key → property.
const PROPS: [(&str, &str); 6] = [
    ("release", "glide.buildname"),
    ("build_tag", "glide.buildtag"),
    ("build_date", "glide.builddate"),
    ("war", "glide.war"),
    ("node", "glide.cluster.node_name"),
    ("instance_name", "instance_name"),
];

pub fn run(global: &GlobalFlags) -> Result<()> {
    let client = connect(global)?;
    let fields: String = PROPS
        .iter()
        .map(|(k, p)| {
            format!(" {k}: getSysPropertyValue(sys_property_name: \"{p}\", default_value: $d)")
        })
        .collect();
    let resp = execute(
        &client,
        &format!("query ($d: String) {{ snDecisionTable {{ sysProperties {{{fields} }} }} }}"),
        Some(json!({ "d": UNSET })),
        None,
    )?;
    let errors = graphql_errors(&resp);
    if !errors.is_empty() {
        return Err(errors_to_api_error(errors));
    }
    let props = resp
        .pointer("/data/snDecisionTable/sysProperties")
        .cloned()
        .unwrap_or(Value::Null);
    write_response(global, &instance_json(client.base_url(), &props))
}

fn instance_json(url: &str, props: &Value) -> Value {
    let get = |k: &str| {
        props
            .get(k)
            .and_then(Value::as_str)
            .filter(|v| *v != UNSET && !v.is_empty())
            .map(str::to_string)
    };
    let mut out = Map::new();
    out.insert("url".into(), Value::from(url));
    for (k, _) in PROPS {
        out.insert(k.into(), get(k).map_or(Value::Null, Value::from));
    }
    let patch = get("war").and_then(|w| patch_of(&w));
    out.insert("patch".into(), patch.map_or(Value::Null, Value::from));
    Value::Object(out)
}

/// The patch level out of the war name:
/// `glide-brazil-08-25-2026__patch0-08-26-2026_…zip` → `patch0`.
fn patch_of(war: &str) -> Option<String> {
    let rest = war.split_once("__")?.1;
    let p = rest.split('-').next()?;
    p.starts_with("patch").then(|| p.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measured_brazil_values() {
        let props = json!({
            "release": "Brazil",
            "build_tag": "glide-brazil-08-25-2026__patch0-08-26-2026",
            "build_date": "09-22-2026_1636",
            "war": "glide-brazil-08-25-2026__patch0-08-26-2026_09-22-2026_1636.zip",
            "node": "dev457896001",
            "instance_name": UNSET,
        });
        let out = instance_json("https://x", &props);
        assert_eq!(out["release"], "Brazil");
        assert_eq!(out["patch"], "patch0");
        assert_eq!(out["instance_name"], Value::Null);
    }

    #[test]
    fn non_admin_nulls_stay_null() {
        let out = instance_json("https://x", &json!({"release": null}));
        assert_eq!(out["release"], Value::Null);
        assert_eq!(out["patch"], Value::Null);
        assert_eq!(out["url"], "https://x");
    }

    #[test]
    fn patch_parsing() {
        assert_eq!(patch_of("glide-x__patch3-01").as_deref(), Some("patch3"));
        assert_eq!(patch_of("glide-x-no-patch.zip"), None);
    }
}
