mod common;

use assert_cmd::Command;
use serde_json::json;
use wiremock::matchers::{basic_auth, method, path};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test(flavor = "current_thread")]
async fn auth_test_ok() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_user"))
        .and(basic_auth("u", "p"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"result": [{"user_name": "api.user"}]})),
        )
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut cmd = Command::cargo_bin("sn").unwrap();
        cmd.args([
            "--instance-override",
            &server_uri,
            "--username",
            "u",
            "--password",
            "p",
            "auth",
            "test",
        ])
        .assert()
        .success();
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn auth_test_401_exit_4() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_user"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(json!({"error": {"message": "nope"}})),
        )
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut cmd = Command::cargo_bin("sn").unwrap();
        cmd.args([
            "--instance-override",
            &server_uri,
            "--username",
            "u",
            "--password",
            "p",
            "auth",
            "test",
        ])
        .assert()
        .code(4);
    })
    .await
    .unwrap();
}
