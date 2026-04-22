mod common;

use assert_cmd::Command;
use serde_json::json;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test(flavor = "current_thread")]
async fn update_sends_patch_with_only_named_fields() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/api/now/table/incident/abc"))
        .and(body_partial_json(json!({"state": 2})))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"result": {"sys_id": "abc", "state": "2"}})),
        )
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut cmd = Command::cargo_bin("sn").unwrap();
        cmd.env("SN_INSTANCE", &server_uri)
            .env("SN_USERNAME", "u")
            .env("SN_PASSWORD", "p")
            .args([
                "--compact",
                "table",
                "update",
                "incident",
                "abc",
                "--field",
                "state=2",
            ])
            .assert()
            .success();
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn replace_sends_put_with_full_body() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/api/now/table/incident/abc"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"result": {"sys_id": "abc"}})),
        )
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut cmd = Command::cargo_bin("sn").unwrap();
        cmd.env("SN_INSTANCE", &server_uri)
            .env("SN_USERNAME", "u")
            .env("SN_PASSWORD", "p")
            .args([
                "--compact",
                "table",
                "replace",
                "incident",
                "abc",
                "--data",
                r#"{"number":"INC1"}"#,
            ])
            .assert()
            .success();
    })
    .await
    .unwrap();
}
