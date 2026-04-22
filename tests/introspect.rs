use assert_cmd::Command;
use serde_json::Value;

#[test]
fn introspect_lists_all_subcommands() {
    let out = Command::cargo_bin("sn")
        .unwrap()
        .args(["introspect"])
        .assert()
        .success();
    let v: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    let names: Vec<String> = v["subcommands"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|s| s["name"].as_str().map(String::from))
        .collect();
    for expected in ["init", "auth", "profile", "table", "schema", "introspect"] {
        assert!(
            names.iter().any(|n| n == expected),
            "missing subcommand {expected}"
        );
    }
}
