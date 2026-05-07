mod common;

use assert_cmd::Command;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test(flavor = "current_thread")]
async fn schema_tables_filter() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/doc/table/schema"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": [
            {"label": "Incident", "value": "incident", "reference": false},
            {"label": "Incident Task", "value": "incident_task", "reference": false},
            {"label": "User", "value": "sys_user", "reference": true}
        ]})))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let out = Command::cargo_bin("sn")
            .unwrap()
            .args([
                "--instance-override",
                &server_uri,
                "--username",
                "u",
                "--password",
                "p",
                "--compact",
                "schema",
                "tables",
                "--filter",
                "incident",
            ])
            .assert()
            .success();
        let s = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        assert!(s.contains("\"incident\""));
        assert!(s.contains("\"incident_task\""));
        assert!(!s.contains("sys_user"));
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn schema_columns_writable_filter() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET")).and(path("/api/now/ui/meta/incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": {"columns": {
            "short_description": {"label": "Short description", "type": "string", "mandatory": true, "read_only": false},
            "sys_id": {"label": "Sys ID", "type": "GUID", "mandatory": false, "read_only": true},
            "state": {"label": "State", "type": "integer", "mandatory": true, "read_only": false, "choices": [{"value": "1", "label": "New"}]}
        }}})))
        .mount(&server).await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let out = Command::cargo_bin("sn")
            .unwrap()
            .args([
                "--instance-override",
                &server_uri,
                "--username",
                "u",
                "--password",
                "p",
                "--compact",
                "schema",
                "columns",
                "incident",
                "--writable",
            ])
            .assert()
            .success();
        let s = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        assert!(s.contains("short_description"));
        assert!(s.contains("state"));
        assert!(!s.contains("sys_id"));
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn schema_choices_for_field() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET")).and(path("/api/now/ui/meta/incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": {"columns": {
            "state": {"label": "State", "type": "integer",
                      "choices": [{"value": "1", "label": "New"}, {"value": "2", "label": "In Progress"}]}
        }}})))
        .mount(&server).await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let out = Command::cargo_bin("sn")
            .unwrap()
            .args([
                "--instance-override",
                &server_uri,
                "--username",
                "u",
                "--password",
                "p",
                "--compact",
                "schema",
                "choices",
                "incident",
                "state",
            ])
            .assert()
            .success();
        let s = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        assert!(s.contains("\"New\""));
        assert!(s.contains("\"In Progress\""));
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn schema_choices_missing_field_is_usage_error() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/ui/meta/incident"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"result": {"columns": {"state": {"choices": []}}}})),
        )
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        Command::cargo_bin("sn")
            .unwrap()
            .args([
                "--instance-override",
                &server_uri,
                "--username",
                "u",
                "--password",
                "p",
                "schema",
                "choices",
                "incident",
                "bogus_field",
            ])
            .assert()
            .code(1);
    })
    .await
    .unwrap();
}
