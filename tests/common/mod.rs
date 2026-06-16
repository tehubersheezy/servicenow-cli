#![allow(dead_code)]

pub fn mock_profile(instance: &str) -> sn::config::ResolvedProfile {
    sn::config::ResolvedProfile {
        name: "test".into(),
        instance: instance.to_string(),
        username: "admin".into(),
        password: "pw".into(),
        proxy: None,
        no_proxy: None,
        insecure: false,
        ca_cert: None,
        proxy_ca_cert: None,
        proxy_username: None,
        proxy_password: None,
        auth_method: sn::config::AuthMethod::Basic,
        oauth: None,
    }
}

/// A `ResolvedProfile` that authenticates with a bearer token (OAuth), for
/// exercising the bearer code path without a live token endpoint.
pub fn mock_oauth_profile(instance: &str, access_token: &str) -> sn::config::ResolvedProfile {
    sn::config::ResolvedProfile {
        name: "oauth-test".into(),
        instance: instance.to_string(),
        username: String::new(),
        password: String::new(),
        proxy: None,
        no_proxy: None,
        insecure: false,
        ca_cert: None,
        proxy_ca_cert: None,
        proxy_username: None,
        proxy_password: None,
        auth_method: sn::config::AuthMethod::Oauth,
        oauth: Some(sn::config::ResolvedOauth {
            client_id: "cid".into(),
            client_secret: None,
            redirect_uri: sn::config::default_redirect_uri(),
            scope: None,
            auth_path: "/oauth_auth.do".into(),
            token_path: "/oauth_token.do".into(),
            grant: sn::config::OAuthGrant::AuthorizationCode,
            pkce: true,
            tokens: Some(sn::config::TokenSet {
                access_token: access_token.to_string(),
                refresh_token: None,
                expires_at: None,
                token_type: Some("Bearer".into()),
            }),
        }),
    }
}
