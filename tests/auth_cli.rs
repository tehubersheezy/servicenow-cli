//! End-to-end CLI tests for the `sn auth` OAuth commands, driving the compiled
//! binary with `assert_cmd`. Each invocation gets its own `XDG_CONFIG_HOME` (set
//! per-process via `.env`, so no global env mutation and no `serial` needed).
//! Gated to Linux because the override only redirects the config path there —
//! this keeps the tests from ever touching the developer's real macOS config.
#![cfg(target_os = "linux")]

mod common;

use assert_cmd::Command;
use serde_json::Value;
use sn::config::now_unix;
use std::fs;
use std::path::Path;
use wiremock::matchers::{body_string_contains, header, method, path as wm_path};
use wiremock::{Mock, ResponseTemplate};

/// Seed a `cli` OAuth profile (config + credentials) under `<dir>/sn/`.
fn seed(dir: &Path, instance: &str, expires_at: i64) {
    let sn_dir = dir.join("sn");
    fs::create_dir_all(&sn_dir).unwrap();
    fs::write(
        sn_dir.join("config.toml"),
        format!(
            "default_profile = \"cli\"\n\n\
             [profiles.cli]\n\
             instance = \"{instance}\"\n\
             auth = \"oauth\"\n\n\
             [profiles.cli.oauth]\n\
             client_id = \"cid\"\n\
             redirect_uri = \"http://localhost:8400/callback\"\n\
             grant = \"authorization_code\"\n\
             pkce = true\n"
        ),
    )
    .unwrap();
    fs::write(
        sn_dir.join("credentials.toml"),
        format!(
            "[profiles.cli]\n\
             client_secret = \"shh\"\n\n\
             [profiles.cli.oauth_tokens]\n\
             access_token = \"VALID_AT\"\n\
             refresh_token = \"RT\"\n\
             expires_at = {expires_at}\n\
             token_type = \"Bearer\"\n"
        ),
    )
    .unwrap();
}

fn load_creds(dir: &Path) -> sn::config::Credentials {
    sn::config::load_credentials_from(&dir.join("sn/credentials.toml")).unwrap()
}

#[test]
fn auth_status_reports_oauth_profile() {
    let tmp = tempfile::tempdir().unwrap();
    seed(
        tmp.path(),
        "https://example.invalid",
        (now_unix() + 3600) as i64,
    );

    let out = Command::cargo_bin("sn")
        .unwrap()
        .env("XDG_CONFIG_HOME", tmp.path())
        .args(["--profile", "cli", "auth", "status"])
        .assert()
        .success();
    let v: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(v["auth"], "oauth");
    assert_eq!(v["loggedIn"], true);
    assert_eq!(v["hasRefreshToken"], true);
    assert_eq!(v["grant"], "authorization_code");
}

#[test]
fn auth_logout_clears_tokens() {
    let tmp = tempfile::tempdir().unwrap();
    seed(
        tmp.path(),
        "https://example.invalid",
        (now_unix() + 3600) as i64,
    );

    Command::cargo_bin("sn")
        .unwrap()
        .env("XDG_CONFIG_HOME", tmp.path())
        .args(["--profile", "cli", "auth", "logout"])
        .assert()
        .success();

    let creds = load_creds(tmp.path());
    assert!(creds.profiles["cli"].oauth_tokens.is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn auth_refresh_rotates_and_persists_token() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("POST"))
        .and(wm_path("/oauth_token.do"))
        .and(body_string_contains("grant_type=refresh_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "NEW_AT",
            "refresh_token": "NEW_RT",
            "expires_in": 1800,
            "token_type": "Bearer"
        })))
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    seed(tmp.path(), &server.uri(), (now_unix() + 3600) as i64);
    let dir = tmp.path().to_path_buf();

    tokio::task::spawn_blocking(move || {
        Command::cargo_bin("sn")
            .unwrap()
            .env("XDG_CONFIG_HOME", &dir)
            .args(["--profile", "cli", "--timeout", "30", "auth", "refresh"])
            .assert()
            .success();
    })
    .await
    .unwrap();

    let creds = load_creds(tmp.path());
    assert_eq!(
        creds.profiles["cli"]
            .oauth_tokens
            .as_ref()
            .unwrap()
            .access_token,
        "NEW_AT"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn expired_token_auto_refreshes_then_calls_api() {
    let server = wiremock::MockServer::start().await;
    // Stale token forces build_client to refresh first…
    Mock::given(method("POST"))
        .and(wm_path("/oauth_token.do"))
        .and(body_string_contains("grant_type=refresh_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "NEW_AT",
            "refresh_token": "NEW_RT",
            "expires_in": 1800,
            "token_type": "Bearer"
        })))
        .mount(&server)
        .await;
    // …then the API call must carry the freshly-minted bearer.
    Mock::given(method("GET"))
        .and(wm_path("/api/now/table/sys_user"))
        .and(header("authorization", "Bearer NEW_AT"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"result": [{"user_name": "alice"}]})),
        )
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    seed(tmp.path(), &server.uri(), (now_unix() as i64) - 10); // expired
    let dir = tmp.path().to_path_buf();

    let out = tokio::task::spawn_blocking(move || {
        Command::cargo_bin("sn")
            .unwrap()
            .env("XDG_CONFIG_HOME", &dir)
            .args([
                "--profile",
                "cli",
                "--timeout",
                "30",
                "table",
                "list",
                "sys_user",
                "--limit",
                "1",
            ])
            .output()
            .unwrap()
    })
    .await
    .unwrap();

    assert!(out.status.success(), "command failed: {out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("alice"),
        "expected record in stdout, got: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}
