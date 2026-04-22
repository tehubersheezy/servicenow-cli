mod common;

use assert_cmd::Command;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test(flavor = "current_thread")]
async fn delete_with_yes_succeeds() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/api/now/table/incident/abc"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut cmd = Command::cargo_bin("sn").unwrap();
        let out = cmd
            .env("SN_INSTANCE", &server_uri)
            .env("SN_USERNAME", "u")
            .env("SN_PASSWORD", "p")
            .args(["table", "delete", "incident", "abc", "--yes"])
            .assert()
            .success();
        let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        assert_eq!(stdout.trim(), "");
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn delete_without_yes_in_non_tty_errors() {
    tokio::task::spawn_blocking(move || {
        let mut cmd = Command::cargo_bin("sn").unwrap();
        cmd.env("SN_INSTANCE", "http://127.0.0.1:1")
            .env("SN_USERNAME", "u")
            .env("SN_PASSWORD", "p")
            .args(["table", "delete", "incident", "abc"])
            .assert()
            .code(1);
    })
    .await
    .unwrap();
}
