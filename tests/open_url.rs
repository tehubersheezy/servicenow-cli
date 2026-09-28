//! `sn open --print-url` must emit an ABSOLUTE url.
//!
//! Profiles persist the bare host (`sn init` / `sn profile add` normalize
//! `dev123` to `dev123.service-now.com`, with no scheme), so a command that
//! interpolates `profile.instance` straight into a URL string emits
//! `acme.service-now.com/nav_to.do?...` — which is not a URL any browser will
//! open. That shipped, unnoticed, because nothing exercised `sn open`.
//!
//! No network: `--print-url` short-circuits before the browser call.

mod common;

use common::{ProfileSpec, sn_cmd, write_profiles};

fn print_url(instance: &str) -> String {
    let tmp = write_profiles(
        "t",
        &[ProfileSpec {
            name: "t",
            instance,
            username: "u",
            password: "p",
        }],
    );
    let out = sn_cmd(tmp.path())
        .args(["open", "incident", "abc123", "--print-url"])
        .assert()
        .success();
    String::from_utf8(out.get_output().stdout.clone())
        .unwrap()
        .trim()
        .to_string()
}

#[test]
fn bare_host_gets_a_scheme() {
    // The shape every profile created the documented way actually has.
    assert_eq!(
        print_url("acme.service-now.com"),
        "https://acme.service-now.com/nav_to.do?uri=%2Fincident.do%3Fsys_id%3Dabc123"
    );
}

#[test]
fn explicit_https_is_left_alone() {
    assert_eq!(
        print_url("https://acme.service-now.com"),
        "https://acme.service-now.com/nav_to.do?uri=%2Fincident.do%3Fsys_id%3Dabc123"
    );
}

#[test]
fn explicit_http_is_not_upgraded() {
    // A local/dev instance on plain http must not be silently rewritten to https.
    assert_eq!(
        print_url("http://localhost:8080"),
        "http://localhost:8080/nav_to.do?uri=%2Fincident.do%3Fsys_id%3Dabc123"
    );
}

#[test]
fn trailing_slash_does_not_double_up() {
    assert_eq!(
        print_url("https://acme.service-now.com/"),
        "https://acme.service-now.com/nav_to.do?uri=%2Fincident.do%3Fsys_id%3Dabc123"
    );
}

// ------------------------------------------------------------- list view ---

fn open_args(args: &[&str]) -> assert_cmd::assert::Assert {
    let tmp = write_profiles(
        "t",
        &[ProfileSpec {
            name: "t",
            instance: "acme.service-now.com",
            username: "u",
            password: "p",
        }],
    );
    let mut all = vec!["open"];
    all.extend_from_slice(args);
    sn_cmd(tmp.path()).args(all).assert()
}

fn stdout_of(a: assert_cmd::assert::Assert) -> String {
    String::from_utf8(a.success().get_output().stdout.clone())
        .unwrap()
        .trim()
        .to_string()
}

#[test]
fn bare_table_opens_the_list_view() {
    assert_eq!(
        stdout_of(open_args(&["incident", "--print-url"])),
        "https://acme.service-now.com/nav_to.do?uri=%2Fincident_list.do"
    );
}

#[test]
fn query_is_double_encoded_inside_uri() {
    // `nav_to.do` decodes `uri=` once; the list page decodes its own
    // `sysparm_query=` once more — so the query's `=`/`^`/space arrive as %25xx.
    assert_eq!(
        stdout_of(open_args(&[
            "incident",
            "-q",
            "active=true^short_descriptionLIKEmail server",
            "--print-url",
        ])),
        "https://acme.service-now.com/nav_to.do?uri=%2Fincident_list.do%3Fsysparm_query%3D\
         active%253Dtrue%255Eshort_descriptionLIKEmail%2520server"
    );
}

#[test]
fn sysparm_query_alias_is_accepted() {
    assert!(
        stdout_of(open_args(&[
            "incident",
            "--sysparm-query",
            "active=true",
            "--print-url"
        ]))
        .ends_with("sysparm_query%3Dactive%253Dtrue")
    );
}

#[test]
fn query_with_a_sys_id_is_a_usage_error() {
    open_args(&["incident", "abc123", "-q", "active=true", "--print-url"]).code(1);
}

#[test]
fn query_with_a_record_ref_is_a_usage_error() {
    let out = open_args(&["incident:INC0010001", "-q", "active=true", "--print-url"]).code(1);
    let stderr = String::from_utf8(out.get_output().stderr.clone()).unwrap();
    assert!(stderr.contains("--query filters a list view"), "{stderr}");
}

#[test]
fn invalid_list_table_is_a_usage_error() {
    open_args(&["Incident", "--print-url"]).code(1);
}
