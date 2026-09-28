mod common;

use serde_json::{Value, json};
use wiremock::matchers::{method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SEARCH: &str = "/api/sn_codesearch/code_search/search";
const TABLES: &str = "/api/sn_codesearch/code_search/tables";
const GROUP: &str = "sn_codesearch.Default Search Group";

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

fn stdout_json(out: &assert_cmd::assert::Assert) -> Value {
    let s = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    serde_json::from_str(&s).unwrap_or_else(|e| panic!("stdout was not JSON ({e}): {s}"))
}

fn stderr(out: &assert_cmd::assert::Assert) -> String {
    String::from_utf8(out.get_output().stderr.clone()).unwrap()
}

/// One hit as the instance serializes it — every number a float, the
/// HTML-escaped duplicate of each line beside it.
fn hit(class: &str, sys_id: &str, name: &str) -> Value {
    json!({
        "className": class,
        "sysId": sys_id,
        "name": name,
        "modified": 1446757340000u64,
        "tableLabel": class,
        "matches": [{
            "field": "script",
            "fieldLabel": "Script",
            "count": 1.0,
            "lineMatches": [
                {"line": 4.0, "context": "var gr;", "escaped": "var gr;"},
                {"line": 5.0, "context": "gr = new GlideRecord('x');", "escaped": "gr = new GlideRecord(&#x27;x&#x27;);"}
            ]
        }]
    })
}

fn table_result(table: &str, hits: Vec<Value>) -> Value {
    json!({"recordType": table, "tableLabel": table, "hits": hits})
}

async fn mock_tables(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path(TABLES))
        .and(query_param("search_group", GROUP))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": [
            {"name": "sys_script", "label": "Business Rule"},
            {"name": "sys_script_include", "label": "Script Include"}
        ]})))
        .mount(server)
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_group_search_covers_every_scope_and_flattens_to_one_row_per_field() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(SEARCH))
        .and(query_param("term", "GlideRecord"))
        .and(query_param("search_all_scopes", "true"))
        .and(query_param("search_group", GROUP))
        .and(query_param("limit", "500"))
        .and(query_param_is_missing("current_app"))
        .and(query_param_is_missing("table"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": [
            table_result("sys_script", vec![hit("sys_script", "b1", "My rule")]),
            table_result("sys_ui_action", vec![]),
            table_result("sys_script_include", vec![hit("sys_script_include", "s1", "MyUtil")])
        ]})))
        .expect(1)
        .mount(&server)
        .await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&uri);
        let out = common::sn_cmd(tmp.path())
            .args(["--compact", "codesearch", "GlideRecord"])
            .assert()
            .success();
        let rows = stdout_json(&out);
        assert_eq!(rows.as_array().unwrap().len(), 2);
        assert_eq!(
            rows[0],
            json!({
                "table": "sys_script", "sys_id": "b1", "name": "My rule",
                "field": "script", "count": 1,
                "lines": [
                    {"line": 4, "text": "var gr;", "match": false},
                    {"line": 5, "text": "gr = new GlideRecord('x');", "match": true}
                ]
            })
        );
        assert_eq!(rows[1]["table"], "sys_script_include");
        assert_eq!(
            stderr(&out),
            "",
            "a result well inside the limit warns nothing"
        );
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn a_table_search_is_validated_then_sent_with_the_table() {
    let server = MockServer::start().await;
    mock_tables(&server).await;
    Mock::given(method("GET"))
        .and(path(SEARCH))
        .and(query_param("table", "sys_script_include"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result":
            table_result("sys_script_include", vec![hit("sys_script_include", "s1", "MyUtil")])
        })))
        .expect(1)
        .mount(&server)
        .await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&uri);
        let out = common::sn_cmd(tmp.path())
            .args([
                "--compact",
                "codesearch",
                "GlideRecord",
                "--table",
                "sys_script_include",
            ])
            .assert()
            .success();
        assert_eq!(stdout_json(&out)[0]["sys_id"], "s1");
    })
    .await
    .unwrap();
}

/// The instance ignores a table outside the search group and searches the
/// whole group instead, so the CLI must refuse before the search is sent.
#[tokio::test(flavor = "current_thread")]
async fn a_table_outside_the_search_group_is_refused_before_searching() {
    let server = MockServer::start().await;
    mock_tables(&server).await;
    Mock::given(method("GET"))
        .and(path(SEARCH))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": []})))
        .expect(0)
        .mount(&server)
        .await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&uri);
        let out = common::sn_cmd(tmp.path())
            .args(["codesearch", "x", "--table", "incident"])
            .assert()
            .code(1);
        let err = stderr(&out);
        assert!(err.contains("'incident'"), "{err}");
        assert!(err.contains("sys_script, sys_script_include"), "{err}");
        assert!(out.get_output().stdout.is_empty());
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn a_caret_in_the_term_is_refused_without_a_request() {
    let server = MockServer::start().await;
    let uri = server.uri();
    let received = tokio::task::spawn_blocking(move || {
        let tmp = profile(&uri);
        let out = common::sn_cmd(tmp.path())
            .args(["codesearch", "active=true^ORactive=false"])
            .assert()
            .code(1);
        assert!(stderr(&out).contains('^'));
    });
    received.await.unwrap();
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn a_missing_plugin_is_named_rather_than_reported_as_a_bare_400() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(SEARCH))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": {"message": "Requested URI does not represent any resource", "detail": null},
            "status": "failure"
        })))
        .mount(&server)
        .await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&uri);
        let out = common::sn_cmd(tmp.path())
            .args(["codesearch", "x"])
            .assert()
            .code(2);
        let err: Value = serde_json::from_str(stderr(&out).trim()).unwrap();
        let message = err["error"]["message"].as_str().unwrap();
        assert!(message.contains("sn_codesearch"), "{message}");
        assert!(message.contains("not installed"), "{message}");
        assert_eq!(err["error"]["status_code"], 400);
    })
    .await
    .unwrap();
}

async fn mock_scoped_search(server: &MockServer, scope: &str) {
    Mock::given(method("GET"))
        .and(path(SEARCH))
        .and(query_param("search_all_scopes", "false"))
        .and(query_param("current_app", scope))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": [
            table_result("sys_script", vec![])
        ]})))
        .mount(server)
        .await;
}

/// An unknown scope matches nothing under HTTP 200 — the same bytes as a real
/// scope with no matching code — so an empty scoped result asks `sys_scope`.
#[tokio::test(flavor = "current_thread")]
async fn an_empty_result_for_an_unknown_scope_is_a_usage_error() {
    let server = MockServer::start().await;
    mock_scoped_search(&server, "x_typo").await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_scope"))
        .and(query_param("sysparm_query", "scope=x_typo^ORsys_id=x_typo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": []})))
        .expect(1)
        .mount(&server)
        .await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&uri);
        let out = common::sn_cmd(tmp.path())
            .args(["codesearch", "x", "--scope", "x_typo"])
            .assert()
            .code(1);
        assert!(stderr(&out).contains("no application scope 'x_typo'"));
        assert!(out.get_output().stdout.is_empty());
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn an_empty_result_for_a_real_scope_is_an_empty_array() {
    let server = MockServer::start().await;
    mock_scoped_search(&server, "global").await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_scope"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"result": [{"sys_id": "global"}]})),
        )
        .mount(&server)
        .await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&uri);
        let out = common::sn_cmd(tmp.path())
            .args(["--compact", "codesearch", "x", "--scope", "global"])
            .assert()
            .success();
        assert_eq!(stdout_json(&out), json!([]));
        assert_eq!(stderr(&out), "");
    })
    .await
    .unwrap();
}

/// `sys_scope` is admin-readable only. A caller who cannot verify the scope
/// still gets the search it ran, with the doubt stated on stderr.
#[tokio::test(flavor = "current_thread")]
async fn an_unverifiable_scope_leaves_the_empty_result_with_a_warning() {
    let server = MockServer::start().await;
    mock_scoped_search(&server, "x_app").await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_scope"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "error": {"message": "Insufficient rights to query records", "detail": null},
            "status": "failure"
        })))
        .mount(&server)
        .await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&uri);
        let out = common::sn_cmd(tmp.path())
            .args(["--compact", "codesearch", "x", "--scope", "x_app"])
            .assert()
            .success();
        assert_eq!(stdout_json(&out), json!([]));
        assert!(
            stderr(&out).contains("could not be verified"),
            "{}",
            stderr(&out)
        );
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn a_result_that_used_up_the_limit_warns_and_names_the_unsearched_tables() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(SEARCH))
        .and(query_param("limit", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": [
            table_result("sys_script", vec![hit("sys_script", "b1", "r1"), hit("sys_script", "b2", "r2")]),
            table_result("sys_script_include", vec![])
        ]})))
        .mount(&server)
        .await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&uri);
        let out = common::sn_cmd(tmp.path())
            .args(["--compact", "codesearch", "x", "--limit", "2"])
            .assert()
            .success();
        assert_eq!(stdout_json(&out).as_array().unwrap().len(), 2);
        let err = stderr(&out);
        assert!(
            err.starts_with("sn: warning: results may be incomplete"),
            "{err}"
        );
        assert!(err.contains("sys_script (2 of 2)"), "{err}");
        assert!(err.contains("not searched"), "{err}");
        assert!(err.contains("(sys_script_include)"), "{err}");
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn raw_output_keeps_the_instance_response() {
    let server = MockServer::start().await;
    let body = json!({"result": [
        table_result("sys_script", vec![hit("sys_script", "b1", "r1")])
    ]});
    Mock::given(method("GET"))
        .and(path(SEARCH))
        .respond_with(ResponseTemplate::new(200).set_body_json(body.clone()))
        .mount(&server)
        .await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&uri);
        let out = common::sn_cmd(tmp.path())
            .args(["--compact", "--output", "raw", "codesearch", "x"])
            .assert()
            .success();
        assert_eq!(stdout_json(&out), body);
    })
    .await
    .unwrap();
}

#[test]
fn a_zero_limit_is_a_usage_error() {
    let tmp = profile("http://127.0.0.1:9");
    common::sn_cmd(tmp.path())
        .args(["codesearch", "x", "--limit", "0"])
        .assert()
        .code(1);
}
