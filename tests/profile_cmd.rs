mod common;

use common::ProfileSpec;
use serde_json::Value;

fn stdout_json(cmd: &mut assert_cmd::Command) -> (Value, String) {
    let output = cmd.assert().success().get_output().stdout.clone();
    let text = String::from_utf8(output).unwrap();
    let v: Value = serde_json::from_str(&text).unwrap();
    (v, text)
}

#[test]
fn profile_list_emits_name_instance_auth_and_default_marker() {
    let tmp = common::write_profiles(
        "beta",
        &[
            ProfileSpec {
                name: "alpha",
                instance: "alpha.example.com",
                username: "au",
                password: "alpha-pw",
            },
            ProfileSpec {
                name: "beta",
                instance: "beta.example.com",
                username: "bu",
                password: "beta-pw",
            },
        ],
    );

    let (v, text) = stdout_json(common::sn_cmd(tmp.path()).args(["profile", "list"]));
    let arr = v.as_array().expect("list emits a JSON array");
    assert_eq!(arr.len(), 2);

    let alpha = arr
        .iter()
        .find(|p| p["name"] == "alpha")
        .expect("alpha listed");
    assert_eq!(alpha["instance"], "alpha.example.com");
    assert_eq!(alpha["auth"], "basic");
    assert_eq!(alpha["default"], false);

    let beta = arr
        .iter()
        .find(|p| p["name"] == "beta")
        .expect("beta listed");
    assert_eq!(beta["instance"], "beta.example.com");
    assert_eq!(beta["auth"], "basic");
    assert_eq!(beta["default"], true);

    // Secrets never appear in list output.
    assert!(!text.contains("alpha-pw"), "password leaked:\n{text}");
    assert!(!text.contains("beta-pw"), "password leaked:\n{text}");
}

#[test]
fn profile_list_reports_oauth_auth_method() {
    let expires = sn::config::now_unix() as i64 + 3600;
    let tmp = common::write_oauth_profile("sso", "sso.example.com", "client-abc", expires);

    let (v, text) = stdout_json(common::sn_cmd(tmp.path()).args(["profile", "list"]));
    let arr = v.as_array().expect("list emits a JSON array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["name"], "sso");
    assert_eq!(arr[0]["auth"], "oauth");
    assert_eq!(arr[0]["default"], true);

    assert!(!text.contains("shh"), "client secret leaked:\n{text}");
    assert!(!text.contains("VALID_AT"), "access token leaked:\n{text}");
}

#[test]
fn profile_show_basic_emits_username_but_never_password() {
    let tmp = common::write_profiles(
        "dev",
        &[ProfileSpec {
            name: "dev",
            instance: "dev.example.com",
            username: "admin",
            password: "s3cret-pw",
        }],
    );

    let (v, text) = stdout_json(common::sn_cmd(tmp.path()).args(["profile", "show", "dev"]));
    assert_eq!(v["name"], "dev");
    assert_eq!(v["instance"], "dev.example.com");
    assert_eq!(v["auth"], "basic");
    assert_eq!(v["username"], "admin");

    assert!(!text.contains("s3cret-pw"), "password leaked:\n{text}");
    assert!(
        v.get("password").is_none(),
        "password field present:\n{text}"
    );
}

#[test]
fn profile_show_without_name_resolves_default_profile() {
    let tmp = common::write_profiles(
        "dev",
        &[ProfileSpec {
            name: "dev",
            instance: "dev.example.com",
            username: "admin",
            password: "pw",
        }],
    );

    let (v, _) = stdout_json(common::sn_cmd(tmp.path()).args(["profile", "show"]));
    assert_eq!(v["name"], "dev");
}

#[test]
fn profile_show_oauth_emits_client_config_and_token_state_but_no_secrets() {
    let expires = sn::config::now_unix() as i64 + 3600;
    let tmp = common::write_oauth_profile("sso", "sso.example.com", "client-abc", expires);

    let (v, text) = stdout_json(common::sn_cmd(tmp.path()).args(["profile", "show", "sso"]));
    assert_eq!(v["name"], "sso");
    assert_eq!(v["instance"], "sso.example.com");
    assert_eq!(v["auth"], "oauth");
    assert_eq!(v["client_id"], "client-abc");
    assert_eq!(v["grant"], "authorization_code");
    assert_eq!(v["redirect_uri"], "http://localhost:8400/callback");
    assert_eq!(v["pkce"], true);
    assert_eq!(v["loggedIn"], true);
    assert_eq!(v["hasRefreshToken"], true);
    assert_eq!(v["expiresAt"], expires);

    // Secret values seeded by write_oauth_profile must never surface.
    assert!(!text.contains("shh"), "client secret leaked:\n{text}");
    assert!(!text.contains("VALID_AT"), "access token leaked:\n{text}");
    assert!(
        v.get("client_secret").is_none() && v.get("access_token").is_none(),
        "secret field present:\n{text}"
    );
    assert!(
        v.get("refresh_token").is_none(),
        "refresh token value present:\n{text}"
    );
}

#[test]
fn profile_show_unknown_name_errors() {
    let tmp = common::write_profiles(
        "dev",
        &[ProfileSpec {
            name: "dev",
            instance: "dev.example.com",
            username: "admin",
            password: "pw",
        }],
    );

    common::sn_cmd(tmp.path())
        .args(["profile", "show", "nope"])
        .assert()
        .failure()
        .code(1);
}

/// Seed one basic profile carrying every proxy/TLS field, plus proxy
/// credentials that must never be echoed.
fn write_tls_profile() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let mut cfg = sn::config::Config {
        default_profile: Some("tls".into()),
        ..Default::default()
    };
    cfg.profiles.insert(
        "tls".into(),
        sn::config::ProfileConfig {
            instance: "tls.example.com".into(),
            proxy: Some("http://pu:proxy-url-secret@proxy.corp:8080".into()),
            no_proxy: Some("localhost,127.0.0.1".into()),
            insecure: true,
            ca_cert: Some("/etc/ssl/custom-ca.pem".into()),
            proxy_ca_cert: Some("/etc/ssl/proxy-ca.pem".into()),
            ..Default::default()
        },
    );
    let mut creds = sn::config::Credentials::default();
    creds.profiles.insert(
        "tls".into(),
        sn::config::ProfileCredentials {
            username: "u".into(),
            password: "pw".into(),
            proxy_username: Some("proxy-user".into()),
            proxy_password: Some("proxy-cred-secret".into()),
            ..Default::default()
        },
    );
    sn::config::save_config_to(&tmp.path().join("config.toml"), &cfg).unwrap();
    sn::config::save_credentials_to(&tmp.path().join("credentials.toml"), &creds).unwrap();
    tmp
}

fn assert_tls_fields(v: &Value, text: &str) {
    assert_eq!(v["insecure"], true, "{text}");
    assert_eq!(v["proxy"], "http://pu:***@proxy.corp:8080", "{text}");
    assert_eq!(v["no_proxy"], "localhost,127.0.0.1", "{text}");
    assert_eq!(v["ca_cert"], "/etc/ssl/custom-ca.pem", "{text}");
    assert_eq!(v["proxy_ca_cert"], "/etc/ssl/proxy-ca.pem", "{text}");
    assert!(
        !text.contains("proxy-url-secret"),
        "proxy URL password leaked:\n{text}"
    );
    assert!(
        !text.contains("proxy-cred-secret"),
        "proxy password leaked:\n{text}"
    );
    assert!(
        !text.contains("proxy-user"),
        "proxy credentials read:\n{text}"
    );
}

#[test]
fn profile_show_and_list_report_persisted_proxy_and_tls_settings() {
    // Issue #97: `insecure = true` was persisted and honored but appeared in
    // neither `show` nor `list`, so it looked like `--insecure` never stuck.
    let tmp = write_tls_profile();

    let (v, text) = stdout_json(common::sn_cmd(tmp.path()).args(["profile", "show", "tls"]));
    assert_tls_fields(&v, &text);

    let (v, text) = stdout_json(common::sn_cmd(tmp.path()).args(["profile", "list"]));
    let arr = v.as_array().expect("list emits a JSON array");
    assert_eq!(arr.len(), 1);
    assert_tls_fields(&arr[0], &text);
}

#[test]
fn profile_show_and_list_state_insecure_false_and_omit_unset_fields() {
    let tmp = common::write_profiles(
        "dev",
        &[ProfileSpec {
            name: "dev",
            instance: "dev.example.com",
            username: "admin",
            password: "pw",
        }],
    );

    let (show, _) = stdout_json(common::sn_cmd(tmp.path()).args(["profile", "show", "dev"]));
    let (list, _) = stdout_json(common::sn_cmd(tmp.path()).args(["profile", "list"]));
    for v in [&show, &list[0]] {
        // `insecure` is stated either way: its absence was the bug.
        assert_eq!(v["insecure"], false);
        for key in ["proxy", "no_proxy", "ca_cert", "proxy_ca_cert"] {
            assert!(v.get(key).is_none(), "unset {key} emitted: {v}");
        }
    }
}
