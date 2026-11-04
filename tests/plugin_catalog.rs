//! `hooks/catalog.ts` is the command tree the Claude Code panel parses argv
//! with: every command path, and which flags consume the next word. It is
//! generated from `sn introspect`, and this test fails when the two drift.
//! Regenerate with `UPDATE_PLUGIN_CATALOG=1 cargo test --test plugin_catalog`.

use assert_cmd::Command;
use serde_json::Value;

const PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/hooks/catalog.ts");

// The long names (aliases included) and short letters of the options that take a value.
fn value_flags(args: &Value) -> (Vec<String>, String) {
    let mut longs = Vec::new();
    let mut shorts = String::new();
    for arg in args.as_array().into_iter().flatten() {
        if arg["positional"].as_bool() == Some(true) || arg["takes_value"].as_bool() != Some(true) {
            continue;
        }
        if let Some(long) = arg["long"].as_str() {
            longs.push(long.to_string());
        }
        for alias in arg["aliases"].as_array().into_iter().flatten() {
            if let Some(alias) = alias.as_str() {
                longs.push(alias.to_string());
            }
        }
        if let Some(short) = arg["short"].as_str() {
            shorts.push_str(short);
        }
    }
    longs.sort();
    longs.dedup();
    let mut letters: Vec<char> = shorts.chars().collect();
    letters.sort_unstable();
    letters.dedup();

    (longs, letters.into_iter().collect())
}

// The names of the positional arguments, in order.
fn operands(args: &Value) -> Vec<String> {
    args.as_array()
        .into_iter()
        .flatten()
        .filter(|arg| arg["positional"].as_bool() == Some(true))
        .filter_map(|arg| arg["name"].as_str().map(String::from))
        .collect()
}

// One node per line, children indented under it, so a diff names the command
// that changed. `indent` is the indentation of the line the node opens on.
fn node(command: &Value, indent: usize) -> String {
    let (values, shorts) = value_flags(&command["args"]);
    let values = serde_json::to_string(&values).unwrap();
    let shorts = serde_json::to_string(&shorts).unwrap();
    let operands = serde_json::to_string(&operands(&command["args"])).unwrap();
    let children: Vec<(&str, &Value)> = command["subcommands"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|child| child["name"].as_str().map(|name| (name, child)))
        .collect();
    if children.is_empty() {
        return format!(
            "{{ operands: {operands}, values: {values}, shorts: {shorts}, sub: {{}} }}"
        );
    }
    let field = " ".repeat(indent + 2);
    let entry = " ".repeat(indent + 4);
    let sub: Vec<String> = children
        .into_iter()
        .map(|(name, child)| {
            let key = serde_json::to_string(name).unwrap();
            format!("{entry}{key}: {},", node(child, indent + 4))
        })
        .collect();

    format!(
        "{{\n{field}operands: {operands},\n{field}values: {values},\n{field}shorts: {shorts},\n{field}sub: {{\n{}\n{field}}},\n{}}}",
        sub.join("\n"),
        " ".repeat(indent),
    )
}

fn render(introspect: &Value) -> String {
    let (globals, global_shorts) = value_flags(&introspect["global_args"]);
    let tree = node(introspect, 0);

    format!(
        "// Generated from `sn introspect` by tests/plugin_catalog.rs. Do not edit:\n\
         // regenerate with `UPDATE_PLUGIN_CATALOG=1 cargo test --test plugin_catalog`.\n\
         \n\
         export type CatalogNode = {{\n\
         \x20 // The positional arguments' names, in order.\n\
         \x20 operands: readonly string[]\n\
         \x20 // Long options (aliases included) that consume the next word.\n\
         \x20 values: readonly string[]\n\
         \x20 // Short options that consume the next word, one letter each.\n\
         \x20 shorts: string\n\
         \x20 sub: Readonly<Record<string, CatalogNode>>\n\
         }}\n\
         \n\
         export const GLOBAL_VALUES: readonly string[] = {}\n\
         export const GLOBAL_SHORTS = {}\n\
         \n\
         export const CATALOG: CatalogNode = {}\n",
        serde_json::to_string(&globals).unwrap(),
        serde_json::to_string(&global_shorts).unwrap(),
        tree,
    )
}

#[test]
fn plugin_catalog_matches_introspect() {
    let out = Command::cargo_bin("sn")
        .unwrap()
        .arg("introspect")
        .assert()
        .success();
    let introspect: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    let expected = render(&introspect);

    if std::env::var_os("UPDATE_PLUGIN_CATALOG").is_some() {
        std::fs::write(PATH, &expected).unwrap();
        return;
    }
    // A Windows checkout may hold the file with CRLF line endings
    // (core.autocrlf); the content is what has to match.
    let actual = std::fs::read_to_string(PATH)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        actual == expected,
        "hooks/catalog.ts is out of date with `sn introspect`; regenerate it with \
         `UPDATE_PLUGIN_CATALOG=1 cargo test --test plugin_catalog`"
    );
}
