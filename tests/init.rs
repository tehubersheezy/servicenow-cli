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

#[tokio::test(flavor = "current_thread")]
async fn init_always_claims_the_default_profile() {
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

    // An existing default that `init` must take over. This is the line between
    // the two commands: `sn init` onboards you onto a profile (and so claims the
    // default); `sn profile add` merely registers one (and never does).
    let mut cfg = sn::config::Config {
        default_profile: Some("old".into()),
        ..Default::default()
    };
    cfg.profiles.insert(
        "old".into(),
        sn::config::ProfileConfig {
            instance: "old.example.com".into(),
            ..Default::default()
        },
    );
    sn::config::save_config_to(&tmp_path.join("config.toml"), &cfg).unwrap();

    tokio::task::spawn_blocking(move || {
        common::sn_cmd(&tmp_path)
            .args([
                "init",
                "--profile",
                "fresh",
                "--instance",
                &server_uri,
                "--username",
                "u",
                "--password",
                "p",
            ])
            .assert()
            .success();

        let saved = sn::config::load_config_from(&tmp_path.join("config.toml")).unwrap();
        assert_eq!(saved.default_profile.as_deref(), Some("fresh"));
        // The profile it displaced is still there, just no longer the default.
        assert!(saved.profiles.contains_key("old"));
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn reinit_preserves_proxy_credentials() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_user"))
        .and(basic_auth("new-user", "new-pass"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": []})))
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let tmp_path = tmp.path().to_path_buf();
    let server_uri = server.uri();

    // Seed an existing profile that carries proxy credentials — fields
    // `sn init` cannot configure and must therefore never destroy.
    let mut cfg = sn::config::Config {
        default_profile: Some("t".into()),
        ..Default::default()
    };
    cfg.profiles.insert(
        "t".into(),
        sn::config::ProfileConfig {
            instance: "old.example.com".into(),
            ..Default::default()
        },
    );
    let mut creds = sn::config::Credentials::default();
    creds.profiles.insert(
        "t".into(),
        sn::config::ProfileCredentials {
            username: "old-user".into(),
            password: "old-pass".into(),
            proxy_username: Some("proxy-user".into()),
            proxy_password: Some("proxy-pass".into()),
            ..Default::default()
        },
    );
    sn::config::save_config_to(&tmp_path.join("config.toml"), &cfg).unwrap();
    sn::config::save_credentials_to(&tmp_path.join("credentials.toml"), &creds).unwrap();

    tokio::task::spawn_blocking(move || {
        common::sn_cmd(&tmp_path)
            .args([
                "init",
                "--profile",
                "t",
                "--instance",
                &server_uri,
                "--username",
                "new-user",
                "--password",
                "new-pass",
            ])
            .assert()
            .success()
            .stderr(contains("saved and verified"));

        let saved = sn::config::load_credentials_from(&tmp_path.join("credentials.toml")).unwrap();
        let cred = saved.profiles.get("t").expect("profile survives re-init");
        // The re-run overwrote what it configured...
        assert_eq!(cred.username, "new-user");
        assert_eq!(cred.password, "new-pass");
        // ...and preserved the proxy credentials it cannot configure.
        assert_eq!(cred.proxy_username.as_deref(), Some("proxy-user"));
        assert_eq!(cred.proxy_password.as_deref(), Some("proxy-pass"));

        let saved_cfg = sn::config::load_config_from(&tmp_path.join("config.toml")).unwrap();
        assert_eq!(
            saved_cfg.profiles.get("t").unwrap().instance,
            server_uri.trim_end_matches('/')
        );
    })
    .await
    .unwrap();
}
