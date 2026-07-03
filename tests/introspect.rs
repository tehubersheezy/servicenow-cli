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

#[test]
fn introspect_reports_flags_and_options_accurately() {
    let out = Command::cargo_bin("sn")
        .unwrap()
        .args(["introspect"])
        .assert()
        .success();
    let v: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    let table = find_sub(&v, "table");
    let list = find_sub(table, "list");
    let args = list["args"].as_array().unwrap();

    // Boolean flags must not claim to take a value (an agent following
    // `takes_value: true` would emit `--all true`, which clap rejects).
    let all = find_arg(args, "all");
    assert_eq!(all["takes_value"], false, "--all is a flag: {all}");
    assert!(
        all["possible_values"].as_array().unwrap().is_empty(),
        "flags must not advertise true/false values: {all}"
    );

    // Value-taking options still report takes_value, aliases, and defaults.
    let setlimit = find_arg(args, "setlimit");
    assert_eq!(setlimit["takes_value"], true);
    assert_eq!(setlimit["default_values"][0], "1000");
    assert!(setlimit["aliases"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a == "limit"));

    // Positionals are marked so agents don't render them as --flags.
    let table_arg = find_arg(args, "table");
    assert_eq!(table_arg["positional"], true);
    assert!(table_arg["long"].is_null());
}

fn find_sub<'a>(v: &'a Value, name: &str) -> &'a Value {
    v["subcommands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == name)
        .unwrap_or_else(|| panic!("missing subcommand {name}"))
}

fn find_arg<'a>(args: &'a [Value], name: &str) -> &'a Value {
    args.iter()
        .find(|a| a["name"] == name)
        .unwrap_or_else(|| panic!("missing arg {name}"))
}
