mod common;

use serde_json::json;
use sn::config::{OAuthGrant, ResolvedOauth, ResolvedProfile, TokenSet};
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, ResponseTemplate};

/// A `ResolvedProfile` wired for the client-credentials grant against `uri`.
fn client_credentials_profile(uri: &str) -> ResolvedProfile {
    let base = common::mock_oauth_profile(uri, "unused");
    ResolvedProfile {
        oauth: Some(ResolvedOauth {
            client_id: "cid".into(),
            client_secret: Some("shh".into()),
            redirect_uri: sn::config::default_redirect_uri(),
            auth_path: "/oauth_auth.do".into(),
            token_path: "/oauth_token.do".into(),
            grant: OAuthGrant::ClientCredentials,
            pkce: false,
            tokens: None,
        }),
        ..base
    }
}

#[tokio::test(flavor = "current_thread")]
async fn bearer_token_is_attached_to_requests() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(header("authorization", "Bearer AT123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": []})))
        .mount(&server)
        .await;

    let profile = common::mock_oauth_profile(&server.uri(), "AT123");
    let body = tokio::task::spawn_blocking(move || {
        let client = sn::client::Client::builder()
            .auth(sn::client::Auth::Bearer {
                token: "AT123".into(),
            })
            .build(&profile)
            .unwrap();
        client.get("/api/now/table/incident", &[])
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(body["result"], json!([]));
}

#[tokio::test(flavor = "current_thread")]
async fn client_credentials_grant_mints_a_token() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth_token.do"))
        .and(body_string_contains("grant_type=client_credentials"))
        .and(body_string_contains("client_secret=shh"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "CCAT",
            "expires_in": 1800,
            "token_type": "Bearer"
        })))
        .mount(&server)
        .await;

    let profile = client_credentials_profile(&server.uri());
    let tokens = tokio::task::spawn_blocking(move || {
        let client = sn::oauth::build_token_client(&profile, None).unwrap();
        let o = profile.oauth.as_ref().unwrap();
        sn::oauth::client_credentials(&client, o)
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(tokens.access_token, "CCAT");
    assert!(tokens.expires_at.is_some());
}

#[tokio::test(flavor = "current_thread")]
async fn refresh_preserves_old_refresh_token_when_response_omits_one() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth_token.do"))
        .and(body_string_contains("grant_type=refresh_token"))
        .and(body_string_contains("refresh_token=OLDRT"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "NEWAT",
            "expires_in": 1800
        })))
        .mount(&server)
        .await;

    let profile = client_credentials_profile(&server.uri());
    let tokens = tokio::task::spawn_blocking(move || {
        let client = sn::oauth::build_token_client(&profile, None).unwrap();
        let o = profile.oauth.as_ref().unwrap();
        sn::oauth::refresh(&client, o, "OLDRT")
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(tokens.access_token, "NEWAT");
    assert_eq!(tokens.refresh_token.as_deref(), Some("OLDRT"));
}

#[tokio::test(flavor = "current_thread")]
async fn token_endpoint_error_surfaces() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth_token.do"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({"error": "invalid_grant"})))
        .mount(&server)
        .await;

    let profile = client_credentials_profile(&server.uri());
    let err = tokio::task::spawn_blocking(move || {
        let client = sn::oauth::build_token_client(&profile, None).unwrap();
        let o = profile.oauth.as_ref().unwrap();
        sn::oauth::client_credentials(&client, o)
    })
    .await
    .unwrap()
    .unwrap_err();
    assert!(matches!(err, sn::error::Error::Auth { status: 401, .. }));
}

#[test]
fn token_set_expiry_logic() {
    let now = sn::config::now_unix();
    // Expires in 10 min — not expired even with a 60s skew.
    let fresh = TokenSet {
        access_token: "x".into(),
        refresh_token: None,
        expires_at: Some(now + 600),
        token_type: None,
    };
    assert!(!fresh.is_expired(60));
    // Expires in 30s — within a 60s skew window, treat as expired.
    let stale = TokenSet {
        expires_at: Some(now + 30),
        ..fresh.clone()
    };
    assert!(stale.is_expired(60));
    // Unknown expiry — never proactively refreshed.
    let unknown = TokenSet {
        expires_at: None,
        ..fresh
    };
    assert!(!unknown.is_expired(60));
}
