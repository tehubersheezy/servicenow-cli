#![cfg(target_os = "linux")] // directories respects XDG_CONFIG_HOME only on Linux

mod common;

use assert_cmd::Command;
use predicates::str::contains;
use serde_json::json;
use serial_test::serial;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use wiremock::matchers::{basic_auth, method, path as path_matcher};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Description of a profile to be written into config.toml + credentials.toml.
struct ProfileSpec<'a> {
    name: &'a str,
    instance: &'a str,
    username: &'a str,
    password: &'a str,
}

/// Write a `config.toml` and `credentials.toml` under `<tmp>/sn/` so that
/// `XDG_CONFIG_HOME=<tmp>` lets the binary discover them. Returns the temp dir
/// handle (drop = cleanup).
fn write_profiles(default_profile: Option<&str>, profiles: &[ProfileSpec<'_>]) -> TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let sn_dir: PathBuf = tmp.path().join("sn");
    fs::create_dir_all(&sn_dir).unwrap();

    let mut cfg_profiles: BTreeMap<String, sn::config::ProfileConfig> = BTreeMap::new();
    let mut cred_profiles: BTreeMap<String, sn::config::ProfileCredentials> = BTreeMap::new();

    for p in profiles {
        cfg_profiles.insert(
            p.name.to_string(),
            sn::config::ProfileConfig {
                instance: p.instance.to_string(),
                ..Default::default()
            },
        );
        cred_profiles.insert(
            p.name.to_string(),
            sn::config::ProfileCredentials {
                username: p.username.to_string(),
                password: p.password.to_string(),
                ..Default::default()
            },
        );
    }

    let cfg = sn::config::Config {
        default_profile: default_profile.map(ToString::to_string),
        profiles: cfg_profiles,
    };
    let cr = sn::config::Credentials {
        profiles: cred_profiles,
    };

    sn::config::save_config_to(&sn_dir.join("config.toml"), &cfg).unwrap();
    sn::config::save_credentials_to(&sn_dir.join("credentials.toml"), &cr).unwrap();

    tmp
}

/// Build a command for `sn` rooted at the given temp config dir, with proxy
/// env vars cleared so a CI host's HTTP_PROXY can't redirect requests.
fn sn_cmd(xdg_home: &Path) -> Command {
    let mut cmd = Command::cargo_bin("sn").unwrap();
    cmd.env("XDG_CONFIG_HOME", xdg_home)
        .env_remove("SN_PROXY")
        .env_remove("HTTP_PROXY")
        .env_remove("HTTPS_PROXY")
        .env_remove("http_proxy")
        .env_remove("https_proxy")
        .env_remove("SN_NO_PROXY")
        .env_remove("SN_INSECURE")
        .env_remove("SN_CA_CERT")
        .env_remove("SN_PROXY_CA_CERT");
    cmd
}

/// Spawn a wiremock server that expects `n_calls` to `GET /api/now/table/sys_user`
/// with the given basic-auth pair.
async fn mount_auth_test_mock(server: &MockServer, user: &str, pass: &str, n_calls: u64) {
    Mock::given(method("GET"))
        .and(path_matcher("/api/now/table/sys_user"))
        .and(basic_auth(user, pass))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"result": [{"user_name": "ok"}]})),
        )
        .expect(n_calls)
        .mount(server)
        .await;
}

#[tokio::test(flavor = "current_thread")]
#[serial]
async fn profile_flag_selects_correct_instance() {
    let server_a = MockServer::start().await;
    let server_b = MockServer::start().await;

    // Server A expects exactly one call; Server B must receive none.
    mount_auth_test_mock(&server_a, "ua", "pa", 1).await;
    mount_auth_test_mock(&server_b, "ub", "pb", 0).await;

    let uri_a = server_a.uri();
    let uri_b = server_b.uri();

    let tmp = write_profiles(
        None,
        &[
            ProfileSpec {
                name: "profile_a",
                instance: &uri_a,
                username: "ua",
                password: "pa",
            },
            ProfileSpec {
                name: "profile_b",
                instance: &uri_b,
                username: "ub",
                password: "pb",
            },
        ],
    );
    let xdg = tmp.path().to_path_buf();

    tokio::task::spawn_blocking(move || {
        sn_cmd(&xdg)
            .args(["--profile", "profile_a", "auth", "test"])
            .assert()
            .success();
    })
    .await
    .unwrap();

    // wiremock verifies expectations on drop, but make it explicit.
    drop(server_a);
    drop(server_b);
}

#[tokio::test(flavor = "current_thread")]
#[serial]
async fn default_profile_used_when_no_flag() {
    let server_dev = MockServer::start().await;
    let server_prod = MockServer::start().await;

    // No flag -> default_profile = "prod" -> server_prod is hit.
    mount_auth_test_mock(&server_dev, "dev-u", "dev-p", 0).await;
    mount_auth_test_mock(&server_prod, "prod-u", "prod-p", 1).await;

    let uri_dev = server_dev.uri();
    let uri_prod = server_prod.uri();

    let tmp = write_profiles(
        Some("prod"),
        &[
            ProfileSpec {
                name: "dev",
                instance: &uri_dev,
                username: "dev-u",
                password: "dev-p",
            },
            ProfileSpec {
                name: "prod",
                instance: &uri_prod,
                username: "prod-u",
                password: "prod-p",
            },
        ],
    );
    let xdg = tmp.path().to_path_buf();

    tokio::task::spawn_blocking(move || {
        sn_cmd(&xdg).args(["auth", "test"]).assert().success();
    })
    .await
    .unwrap();

    drop(server_dev);
    drop(server_prod);
}

#[tokio::test(flavor = "current_thread")]
#[serial]
async fn profile_flag_overrides_default_profile() {
    let server_dev = MockServer::start().await;
    let server_prod = MockServer::start().await;

    // default_profile points at prod, but --profile dev wins.
    mount_auth_test_mock(&server_dev, "dev-u", "dev-p", 1).await;
    mount_auth_test_mock(&server_prod, "prod-u", "prod-p", 0).await;

    let uri_dev = server_dev.uri();
    let uri_prod = server_prod.uri();

    let tmp = write_profiles(
        Some("prod"),
        &[
            ProfileSpec {
                name: "dev",
                instance: &uri_dev,
                username: "dev-u",
                password: "dev-p",
            },
            ProfileSpec {
                name: "prod",
                instance: &uri_prod,
                username: "prod-u",
                password: "prod-p",
            },
        ],
    );
    let xdg = tmp.path().to_path_buf();

    tokio::task::spawn_blocking(move || {
        sn_cmd(&xdg)
            .args(["--profile", "dev", "auth", "test"])
            .assert()
            .success();
    })
    .await
    .unwrap();

    drop(server_dev);
    drop(server_prod);
}

#[tokio::test(flavor = "current_thread")]
#[serial]
async fn instance_override_supersedes_profile_instance() {
    let server_a = MockServer::start().await;
    let server_b = MockServer::start().await;

    // Profile points at A; --instance-override sends traffic to B, but the
    // basic auth pair must still be the profile's (ua/pa) — this proves the
    // override only replaces the URL and not the credentials.
    mount_auth_test_mock(&server_a, "ua", "pa", 0).await;
    mount_auth_test_mock(&server_b, "ua", "pa", 1).await;

    let uri_a = server_a.uri();
    let uri_b = server_b.uri();

    let tmp = write_profiles(
        None,
        &[ProfileSpec {
            name: "p",
            instance: &uri_a,
            username: "ua",
            password: "pa",
        }],
    );
    let xdg = tmp.path().to_path_buf();

    tokio::task::spawn_blocking(move || {
        sn_cmd(&xdg)
            .args([
                "--profile",
                "p",
                "--instance-override",
                &uri_b,
                "auth",
                "test",
            ])
            .assert()
            .success();
    })
    .await
    .unwrap();

    drop(server_a);
    drop(server_b);
}

#[tokio::test(flavor = "current_thread")]
#[serial]
async fn username_password_overrides_apply_per_field() {
    let server = MockServer::start().await;

    // Profile creds are u1/p1, but CLI overrides to u2/p2 — server only
    // accepts the override pair.
    mount_auth_test_mock(&server, "u2", "p2", 1).await;

    let uri = server.uri();

    let tmp = write_profiles(
        None,
        &[ProfileSpec {
            name: "p",
            instance: &uri,
            username: "u1",
            password: "p1",
        }],
    );
    let xdg = tmp.path().to_path_buf();

    tokio::task::spawn_blocking(move || {
        sn_cmd(&xdg)
            .args([
                "--profile",
                "p",
                "--username",
                "u2",
                "--password",
                "p2",
                "auth",
                "test",
            ])
            .assert()
            .success();
    })
    .await
    .unwrap();

    drop(server);
}

#[tokio::test(flavor = "current_thread")]
#[serial]
async fn unknown_profile_errors_clearly() {
    // Only `dev` is configured; ask for a non-existent profile.
    let server = MockServer::start().await;
    let uri = server.uri();

    let tmp = write_profiles(
        None,
        &[ProfileSpec {
            name: "dev",
            instance: &uri,
            username: "u",
            password: "p",
        }],
    );
    let xdg = tmp.path().to_path_buf();

    tokio::task::spawn_blocking(move || {
        sn_cmd(&xdg)
            .args(["--profile", "nonexistent", "auth", "test"])
            .assert()
            .code(1)
            .stderr(contains("nonexistent"));
    })
    .await
    .unwrap();

    drop(server);
}

#[tokio::test(flavor = "current_thread")]
#[serial]
async fn missing_default_profile_falls_back_to_literal_default() {
    // Config has no default_profile field, but a profile literally named
    // "default" exists. With no --profile flag the resolver must fall back
    // to "default".
    let server = MockServer::start().await;
    mount_auth_test_mock(&server, "u", "p", 1).await;

    let uri = server.uri();

    let tmp = write_profiles(
        None,
        &[ProfileSpec {
            name: "default",
            instance: &uri,
            username: "u",
            password: "p",
        }],
    );
    let xdg = tmp.path().to_path_buf();

    tokio::task::spawn_blocking(move || {
        sn_cmd(&xdg).args(["auth", "test"]).assert().success();
    })
    .await
    .unwrap();

    drop(server);
}

#[tokio::test(flavor = "current_thread")]
#[serial]
async fn multiple_profiles_isolation() {
    let server_a = MockServer::start().await;
    let server_b = MockServer::start().await;
    let server_c = MockServer::start().await;

    // Each server should be hit exactly once when called with its profile,
    // and zero times when other profiles are addressed.
    mount_auth_test_mock(&server_a, "ua", "pa", 1).await;
    mount_auth_test_mock(&server_b, "ub", "pb", 1).await;
    mount_auth_test_mock(&server_c, "uc", "pc", 1).await;

    let uri_a = server_a.uri();
    let uri_b = server_b.uri();
    let uri_c = server_c.uri();

    let tmp = write_profiles(
        None,
        &[
            ProfileSpec {
                name: "a",
                instance: &uri_a,
                username: "ua",
                password: "pa",
            },
            ProfileSpec {
                name: "b",
                instance: &uri_b,
                username: "ub",
                password: "pb",
            },
            ProfileSpec {
                name: "c",
                instance: &uri_c,
                username: "uc",
                password: "pc",
            },
        ],
    );
    let xdg = tmp.path().to_path_buf();

    tokio::task::spawn_blocking(move || {
        for prof in ["a", "b", "c"] {
            sn_cmd(&xdg)
                .args(["--profile", prof, "auth", "test"])
                .assert()
                .success();
        }
    })
    .await
    .unwrap();

    drop(server_a);
    drop(server_b);
    drop(server_c);
}
