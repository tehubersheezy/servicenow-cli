mod common;

use common::{sn_cmd, write_profiles, ProfileSpec};
use serde_json::json;
use wiremock::matchers::{basic_auth, method, path};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test(flavor = "current_thread")]
async fn ping_ok() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_user"))
        .and(basic_auth("u", "p"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"result": [{"user_name": "api.user"}]})),
        )
        .expect(1)
        .mount(&server)
        .await;
    // ping's best-effort sys_properties probe 404s harmlessly (unmatched).
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = write_profiles(
            "p1",
            &[ProfileSpec {
                name: "p1",
                instance: &server_uri,
                username: "u",
                password: "p",
            }],
        );
        let assert = sn_cmd(tmp.path()).arg("ping").assert().success();
        let out: serde_json::Value =
            serde_json::from_slice(&assert.get_output().stdout).expect("ping emits JSON on stdout");
        assert_eq!(out["ok"], json!(true));
        assert_eq!(out["profile"], json!("p1"));
        assert_eq!(out["username"], json!("u"));
        assert!(out["latency_ms"].is_u64());
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn ping_401_exit_4() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_user"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(json!({"error": {"message": "nope"}})),
        )
        .expect(1)
        .mount(&server)
        .await;
    // The 401 aborts ping before its sys_properties probe.
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_properties"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": []})))
        .expect(0)
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = write_profiles(
            "p1",
            &[ProfileSpec {
                name: "p1",
                instance: &server_uri,
                username: "u",
                password: "p",
            }],
        );
        sn_cmd(tmp.path()).arg("ping").assert().code(4);
    })
    .await
    .unwrap();
}
