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
