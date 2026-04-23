mod common;

use serde_json::json;
use wiremock::matchers::{basic_auth, method, path};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test(flavor = "current_thread")]
async fn insecure_flag_disables_cert_verification() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(basic_auth("admin", "pw"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"result": []})),
        )
        .mount(&server)
        .await;

    let mut profile = common::mock_profile(&server.uri());
    profile.insecure = true;
    let result = tokio::task::spawn_blocking(move || {
        sn::client::Client::builder()
            .insecure(true)
            .build(&profile)
            .unwrap()
            .get("/api/now/table/incident", &[])
    })
    .await
    .unwrap();
    assert!(result.is_ok());
}

#[tokio::test(flavor = "current_thread")]
async fn invalid_proxy_url_returns_config_error() {
    let profile = common::mock_profile("http://localhost:1");
    let err = tokio::task::spawn_blocking(move || {
        sn::client::Client::builder()
            .proxy(Some("://bad-url".into()))
            .build(&profile)
            .err()
            .expect("expected an error")
    })
    .await
    .unwrap();
    assert!(matches!(err, sn::error::Error::Config(_)));
}

#[tokio::test(flavor = "current_thread")]
async fn missing_ca_cert_file_returns_config_error() {
    let profile = common::mock_profile("http://localhost:1");
    let err = tokio::task::spawn_blocking(move || {
        sn::client::Client::builder()
            .ca_cert(Some("/nonexistent/cert.pem".into()))
            .build(&profile)
            .err()
            .expect("expected an error")
    })
    .await
    .unwrap();
    assert!(matches!(err, sn::error::Error::Config(_)));
    let msg = format!("{err}");
    assert!(msg.contains("cert.pem"), "error should mention the file path: {msg}");
}

#[tokio::test(flavor = "current_thread")]
async fn proxy_auth_builder_does_not_error() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"result": []})),
        )
        .mount(&server)
        .await;

    let profile = common::mock_profile(&server.uri());
    let result = tokio::task::spawn_blocking(move || {
        sn::client::Client::builder()
            .proxy_auth(Some("puser".into()), Some("ppass".into()))
            .build(&profile)
            .unwrap()
            .get("/api/now/table/incident", &[])
    })
    .await
    .unwrap();
    assert!(result.is_ok());
}
