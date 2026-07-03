//! End-to-end test of the OAuth authorization-code (SSO) flow, with the
//! browser-open step injected so no real browser is launched. A fake "browser"
//! thread delivers the redirect to the loopback server; a wiremock token
//! endpoint completes the exchange. Off Windows to match the repo's caution
//! around live socket tests.
#![cfg(not(target_os = "windows"))]

mod common;

use serde_json::json;
use sn::config::{ResolvedOauth, ResolvedProfile};
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test(flavor = "current_thread")]
async fn full_authorization_code_flow_exchanges_code_for_tokens() {
    let server = wiremock::MockServer::start().await;
    // The token endpoint must receive grant_type=authorization_code, our code,
    // and — proving PKCE is wired through — a code_verifier.
    Mock::given(method("POST"))
        .and(path("/oauth_token.do"))
        .and(body_string_contains("grant_type=authorization_code"))
        .and(body_string_contains("code=THECODE"))
        .and(body_string_contains("code_verifier="))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "AC_AT",
            "refresh_token": "AC_RT",
            "expires_in": 1800,
            "token_type": "Bearer"
        })))
        .mount(&server)
        .await;

    let port = common::free_port();
    let redirect_uri = format!("http://localhost:{port}/callback");

    // Build an authorization_code profile pointed at the mock instance.
    let base = common::mock_oauth_profile(&server.uri(), "unused");
    let o = base.oauth.clone().unwrap();
    let profile = ResolvedProfile {
        oauth: Some(ResolvedOauth {
            redirect_uri: redirect_uri.clone(),
            tokens: None,
            ..o
        }),
        ..base
    };

    let tokens = tokio::task::spawn_blocking(move || {
        sn::oauth::login_authorization_code_with(&profile, None, move |url| {
            // Stand in for the browser+IdP: echo the `state` back to the loopback
            // with an authorization code, on a thread so this returns immediately
            // and `run_loopback` can start listening.
            let parsed = reqwest::Url::parse(url).expect("authorize url parses");
            let state = parsed
                .query_pairs()
                .find(|(k, _)| k == "state")
                .map(|(_, v)| v.into_owned())
                .expect("authorize url carries state");
            std::thread::spawn(move || {
                common::send_loopback_request(
                    port,
                    &format!("/callback?code=THECODE&state={state}"),
                );
            });
            Ok(())
        })
    })
    .await
    .unwrap()
    .unwrap();

    assert_eq!(tokens.access_token, "AC_AT");
    assert_eq!(tokens.refresh_token.as_deref(), Some("AC_RT"));
    assert!(tokens.expires_at.is_some());
}
