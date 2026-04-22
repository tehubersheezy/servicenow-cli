mod common;

use serde_json::json;
use wiremock::matchers::{basic_auth, method, path};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test(flavor = "current_thread")]
async fn get_success_returns_parsed_json() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(basic_auth("admin", "pw"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"result": [{"sys_id": "a"}]})),
        )
        .mount(&server)
        .await;

    let profile = common::mock_profile(&server.uri());
    let body = tokio::task::spawn_blocking(move || {
        let client = sn::client::Client::builder().build(&profile).unwrap();
        client.get("/api/now/table/incident", &[])
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(body["result"][0]["sys_id"], "a");
}

#[tokio::test(flavor = "current_thread")]
async fn http_404_maps_to_api_error() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident/none"))
        .respond_with(
            ResponseTemplate::new(404)
                .insert_header("X-Transaction-ID", "tx-abc")
                .set_body_json(
                    json!({"error": {"message": "No Record found", "detail": "ACL restrictions"}}),
                ),
        )
        .mount(&server)
        .await;

    let profile = common::mock_profile(&server.uri());
    let err = tokio::task::spawn_blocking(move || {
        let client = sn::client::Client::builder().build(&profile).unwrap();
        client.get("/api/now/table/incident/none", &[])
    })
    .await
    .unwrap()
    .unwrap_err();
    match err {
        sn::error::Error::Api {
            status,
            message,
            transaction_id,
            sn_error,
            ..
        } => {
            assert_eq!(status, 404);
            assert_eq!(message, "No Record found");
            assert_eq!(transaction_id.as_deref(), Some("tx-abc"));
            assert!(sn_error.is_some());
        }
        other => panic!("expected Api error, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn http_401_maps_to_auth_error() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/x"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(json!({"error": {"message": "Unauthorized"}})),
        )
        .mount(&server)
        .await;
    let profile = common::mock_profile(&server.uri());
    let err = tokio::task::spawn_blocking(move || {
        let client = sn::client::Client::builder().build(&profile).unwrap();
        client.get("/api/now/table/x", &[])
    })
    .await
    .unwrap()
    .unwrap_err();
    assert!(matches!(err, sn::error::Error::Auth { status: 401, .. }));
}
