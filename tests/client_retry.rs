mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test(flavor = "current_thread")]
async fn retries_on_503_then_succeeds() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/x"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(2)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/x"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .mount(&server)
        .await;
    let profile = common::mock_profile(&server.uri());
    let v = tokio::task::spawn_blocking(move || {
        let client = sn::client::Client::builder()
            .retry(sn::client::RetryPolicy {
                enabled: true,
                max_attempts: 3,
                initial_backoff: std::time::Duration::from_millis(1),
            })
            .build(&profile)
            .unwrap();
        client.get("/x", &[])
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(v["ok"], true);
}

#[tokio::test(flavor = "current_thread")]
async fn disabled_retry_returns_first_failure() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/x"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let profile = common::mock_profile(&server.uri());
    let err = tokio::task::spawn_blocking(move || {
        let client = sn::client::Client::builder()
            .retry(sn::client::RetryPolicy {
                enabled: false,
                max_attempts: 3,
                initial_backoff: std::time::Duration::from_millis(1),
            })
            .build(&profile)
            .unwrap();
        client.get("/x", &[])
    })
    .await
    .unwrap()
    .unwrap_err();
    assert!(matches!(err, sn::error::Error::Api { status: 503, .. }));
}

#[tokio::test(flavor = "current_thread")]
async fn does_not_retry_4xx_except_429() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/x"))
        .respond_with(ResponseTemplate::new(400))
        .expect(1)
        .mount(&server)
        .await;
    let profile = common::mock_profile(&server.uri());
    let _ = tokio::task::spawn_blocking(move || {
        let client = sn::client::Client::builder()
            .retry(sn::client::RetryPolicy {
                enabled: true,
                max_attempts: 3,
                initial_backoff: std::time::Duration::from_millis(1),
            })
            .build(&profile)
            .unwrap();
        client.get("/x", &[])
    })
    .await
    .unwrap()
    .unwrap_err();
    // wiremock asserts `expect(1)` on drop
}
