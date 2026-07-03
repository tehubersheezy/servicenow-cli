#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

/// Reserve and immediately release a free localhost TCP port, returning its
/// number. The port is unbound on return so a loopback server can claim it.
pub fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

/// Connect to a loopback HTTP server on `127.0.0.1:port` (retrying until it
/// binds), send one raw `GET <target>` request, and drain the response. Used to
/// simulate the browser's OAuth redirect callback against `run_loopback`.
pub fn send_loopback_request(port: u16, target: &str) {
    let mut stream = None;
    for _ in 0..100 {
        if let Ok(s) = TcpStream::connect(("127.0.0.1", port)) {
            stream = Some(s);
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut stream = stream.expect("connect to loopback server");
    let req = format!("GET {target} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).unwrap();
    stream.flush().unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
}

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
