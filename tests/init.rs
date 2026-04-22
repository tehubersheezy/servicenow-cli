#![cfg(target_os = "linux")] // directories respects XDG_CONFIG_HOME only on Linux

mod common;

use assert_cmd::Command;
use predicates::str::contains;
use serde_json::json;
use wiremock::matchers::{basic_auth, method, path};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test(flavor = "current_thread")]
async fn init_writes_files_and_verifies_creds() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_user"))
        .and(basic_auth("u", "p"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": []})))
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let tmp_path = tmp.path().to_path_buf();
    let server_uri = server.uri();

    tokio::task::spawn_blocking(move || {
        let mut cmd = Command::cargo_bin("sn").unwrap();
        cmd.env("XDG_CONFIG_HOME", &tmp_path)
            .args([
                "init",
                "--profile",
                "t",
                "--instance",
                &server_uri,
                "--username",
                "u",
                "--password",
                "p",
            ])
            .assert()
            .success()
            .stderr(contains("saved and verified"));

        assert!(tmp_path.join("sn/config.toml").exists());
        assert!(tmp_path.join("sn/credentials.toml").exists());
    })
    .await
    .unwrap();
}
