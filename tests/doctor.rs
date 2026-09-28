//! `sn doctor` end to end: the one-document wire shape, the exit-code contract
//! for a failed preflight, graceful loss of a namespace, and the argv guards.

mod common;

use serde_json::{Value, json};
use wiremock::matchers::{body_partial_json, body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn profile(server_uri: &str) -> tempfile::TempDir {
    common::write_profiles(
        "test",
        &[common::ProfileSpec {
            name: "test",
            instance: server_uri,
            username: "u",
            password: "p",
        }],
    )
}

fn me() -> Value {
    json!({"_rowCount": 1, "_results": [
        {"sys_id": {"value": "u1"}, "user_name": {"value": "beth"}}
    ]})
}

#[tokio::test(flavor = "current_thread")]
async fn all_checks_pass_in_one_round_trip() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        // Role names ride as variables, never spliced into the document.
        .and(body_partial_json(json!({"variables": {
            "roles": ["itil"],
            "role0": "name=itil",
            "plugin0": "com.snc.incident",
            "property0": "glide.servlet.uri",
        }})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {
            "now": {"sessionUser": {"admin": [], "held": ["itil"]}},
            "GlideRecord_Query": {"me": me(), "role0": {"_rowCount": 1}},
            "snWorkflowStudio": {"workflowStudio": {"canary": true, "plugin0": true}},
            "snDecisionTable": {"sysProperties": {
                "build": "glide-brazil", "property0": "https://x/"
            }},
        }})))
        .expect(1)
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&server_uri);
        let out = common::sn_cmd(tmp.path())
            .args([
                "--compact",
                "doctor",
                "--need-role",
                "itil",
                "--need-plugin",
                "com.snc.incident",
                "--need-property",
                "glide.servlet.uri=https://x/",
            ])
            .assert()
            .success();
        let v: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["admin"], false);
        assert_eq!(v["user"]["user_name"], "beth");
        assert_eq!(v["build_tag"], "glide-brazil");
        assert_eq!(v["checks"].as_array().unwrap().len(), 3);
        assert!(out.get_output().stderr.is_empty());
    })
    .await
    .unwrap();
}

/// The live finding behind the design: an admin session's role check echoes
/// back any name. The report still lands on stdout, the exit is 2, and no
/// HTTP status is invented for a failure that every response 200'd around.
#[tokio::test(flavor = "current_thread")]
async fn admin_echo_of_a_fake_role_fails_with_exit_2() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {
            "now": {"sessionUser": {"admin": ["admin"], "held": ["not_a_role_xyzzy"]}},
            "GlideRecord_Query": {"me": me(), "role0": {"_rowCount": 0}},
            "snWorkflowStudio": {"workflowStudio": {"canary": true}},
            "snDecisionTable": {"sysProperties": {"build": "glide-brazil"}},
        }})))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&server_uri);
        let out = common::sn_cmd(tmp.path())
            .args(["--compact", "doctor", "--need-role", "not_a_role_xyzzy"])
            .assert()
            .code(2);
        let v: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
        assert_eq!(v["ok"], false);
        assert_eq!(v["admin"], true);
        assert_eq!(v["checks"][0]["status"], "fail");
        assert_eq!(v["checks"][0]["exists"], false);
        let err: Value = serde_json::from_slice(&out.get_output().stderr).unwrap();
        assert!(
            err["error"]["message"]
                .as_str()
                .unwrap()
                .contains("role 'not_a_role_xyzzy' (fail)"),
            "{err}"
        );
        assert!(err["error"].get("status_code").is_none(), "{err}");
    })
    .await
    .unwrap();
}

/// A namespace the schema lacks fails validation for the whole document; the
/// command drops it, asks again, and reports its checks as unavailable.
#[tokio::test(flavor = "current_thread")]
async fn missing_namespace_degrades_to_unavailable() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .and(body_string_contains("snDecisionTable"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "errors": [{
                "errorType": "ValidationError",
                "message": "Validation error (FieldUndefined@[snDecisionTable]) : Field 'snDecisionTable' in type 'QueryType' is undefined",
            }],
        })))
        .with_priority(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {
            "now": {"sessionUser": {"admin": []}},
            "GlideRecord_Query": {"me": me()},
            "snWorkflowStudio": {"workflowStudio": {"canary": true}},
        }})))
        .expect(1)
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&server_uri);
        let out = common::sn_cmd(tmp.path())
            .args([
                "--compact",
                "doctor",
                "--need-property",
                "glide.servlet.uri",
            ])
            .assert()
            .code(2);
        let v: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
        assert_eq!(v["checks"][0]["status"], "unavailable", "{v}");
        let reason = v["capabilities"]["properties"]["reason"].as_str().unwrap();
        assert!(reason.contains("is undefined"), "{reason}");
        assert_eq!(v["capabilities"]["plugins"]["available"], true);
    })
    .await
    .unwrap();
}

/// An error no root can own is a real failure: exit 2 with the GraphQL
/// envelope, not a report of unavailable checks.
#[tokio::test(flavor = "current_thread")]
async fn unattributable_graphql_error_is_a_real_failure() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "errors": [{"message": "Invalid Syntax : offending token '}'"}],
        })))
        .expect(1)
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&server_uri);
        let out = common::sn_cmd(tmp.path())
            .args(["--compact", "doctor"])
            .assert()
            .code(2);
        assert!(out.get_output().stdout.is_empty());
        let err: Value = serde_json::from_slice(&out.get_output().stderr).unwrap();
        assert_eq!(err["error"]["status_code"], 200, "{err}");
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn a_name_that_would_splice_a_query_term_never_reaches_the_network() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&server_uri);
        for argv in [
            ["doctor", "--need-role", "itil^ORname=admin"],
            ["doctor", "--need-role", "itil,,admin"],
            ["doctor", "--need-property", "glide.x="],
        ] {
            let out = common::sn_cmd(tmp.path()).args(argv).assert().code(1);
            let err: Value = serde_json::from_slice(&out.get_output().stderr).unwrap();
            assert!(err["error"]["message"].is_string(), "{err}");
        }
    })
    .await
    .unwrap();
}
