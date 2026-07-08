mod common;

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
        common::sn_cmd(&tmp_path)
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

        assert!(tmp_path.join("config.toml").exists());
        assert!(tmp_path.join("credentials.toml").exists());
    })
    .await
    .unwrap();
}
