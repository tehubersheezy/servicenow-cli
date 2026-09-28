//! End-to-end tests for the two headless auth types added for issue #77:
//!
//! * the OAuth `jwt_bearer` grant (`--auth oauth --grant jwt_bearer`), whose
//!   token request carries a JWT sn signs with a local key — asserted here down
//!   to the claims and the signature, against the public half of the key;
//! * external bearer tokens (`--auth token`), static or produced by a
//!   `token_command`, including the cache-only-with-a-stated-expiry contract.
//!
//! Driven through the compiled binary; each test gets its own `SN_CONFIG_DIR`.

mod common;

use base64::Engine;
use common::sn_cmd;
use ring::rand::SystemRandom;
use ring::signature::{self, EcdsaKeyPair, KeyPair};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use wiremock::matchers::{header, method, path as wm_path};
use wiremock::{Mock, ResponseTemplate};

const B64URL: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::URL_SAFE_NO_PAD;

fn load_config(dir: &Path) -> sn::config::Config {
    sn::config::load_config_from(&dir.join("config.toml")).unwrap()
}

fn load_creds(dir: &Path) -> sn::config::Credentials {
    sn::config::load_credentials_from(&dir.join("credentials.toml")).unwrap()
}

fn stderr_text(out: &assert_cmd::assert::Assert) -> String {
    String::from_utf8(out.get_output().stderr.clone()).unwrap()
}

/// A fresh P-256 key: the PKCS#8 PEM written to `dir/key.pem`, plus its public
/// key bytes for verifying what sn signs.
fn write_ec_key(dir: &Path) -> (PathBuf, Vec<u8>) {
    let rng = SystemRandom::new();
    let pkcs8 =
        EcdsaKeyPair::generate_pkcs8(&signature::ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
    let kp = EcdsaKeyPair::from_pkcs8(
        &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
        pkcs8.as_ref(),
        &rng,
    )
    .unwrap();
    let b64 = base64::engine::general_purpose::STANDARD.encode(pkcs8.as_ref());
    let lines: Vec<&str> = b64
        .as_bytes()
        .chunks(64)
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect();
    let pem = format!(
        "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
        lines.join("\n")
    );
    let path = dir.join("key.pem");
    std::fs::write(&path, pem).unwrap();
    (path, kp.public_key().as_ref().to_vec())
}

fn form(body: &[u8]) -> std::collections::HashMap<String, String> {
    let body = std::str::from_utf8(body).unwrap_or("");
    reqwest::Url::parse(&format!("http://x/?{body}"))
        .unwrap()
        .query_pairs()
        .into_owned()
        .collect()
}

/// Matches a `/oauth_token.do` request that is a well-formed jwt-bearer grant:
/// the RFC 7523 grant type, the client credentials, and an assertion whose
/// header and claims are what ServiceNow's JWT endpoint checks and whose
/// ES256 signature verifies under `public_key`.
struct JwtBearerGrant {
    public_key: Vec<u8>,
}

impl wiremock::Match for JwtBearerGrant {
    fn matches(&self, request: &wiremock::Request) -> bool {
        let f = form(&request.body);
        if f.get("grant_type").map(String::as_str)
            != Some("urn:ietf:params:oauth:grant-type:jwt-bearer")
            || f.get("client_id").map(String::as_str) != Some("cid")
            || f.get("client_secret").map(String::as_str) != Some("shh")
        {
            return false;
        }
        let Some(assertion) = f.get("assertion") else {
            return false;
        };
        let parts: Vec<&str> = assertion.split('.').collect();
        if parts.len() != 3 {
            return false;
        }
        let decode = |p: &str| -> Value {
            serde_json::from_slice(&B64URL.decode(p).unwrap_or_default()).unwrap_or(Value::Null)
        };
        let (h, c) = (decode(parts[0]), decode(parts[1]));
        let now = sn::config::now_unix();
        let claims_ok = h["alg"] == "ES256"
            && h["kid"] == "k1"
            && c["iss"] == "cid"
            && c["aud"] == "cid"
            && c["sub"] == "svc.user"
            && c["jti"].as_str().is_some_and(|j| !j.is_empty())
            && c["exp"].as_u64().is_some_and(|e| e > now && e <= now + 600);
        let Ok(sig) = B64URL.decode(parts[2]) else {
            return false;
        };
        claims_ok
            && signature::UnparsedPublicKey::new(
                &signature::ECDSA_P256_SHA256_FIXED,
                &self.public_key,
            )
            .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &sig)
            .is_ok()
    }
}

#[tokio::test(flavor = "current_thread")]
async fn jwt_bearer_add_signs_a_verifiable_assertion_and_remints_when_stale() {
    let tmp = tempfile::tempdir().unwrap();
    let (key_path, public_key) = write_ec_key(tmp.path());
    let server = wiremock::MockServer::start().await;
    // Minted once by `profile add`'s verification and once more after the
    // cached token is expired by hand: the JWT grant has no refresh token, so
    // a stale token must be re-minted from a fresh assertion.
    Mock::given(method("POST"))
        .and(wm_path("/oauth_token.do"))
        .and(JwtBearerGrant { public_key })
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "JWTAT",
            "token_type": "Bearer",
            "expires_in": 1799
        })))
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(wm_path("/api/now/ui/user/current_user"))
        .and(header("authorization", "Bearer JWTAT"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"result": {"user_name": "svc.user"}})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(wm_path("/api/now/table/incident"))
        .and(header("authorization", "Bearer JWTAT"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": []})))
        .expect(1)
        .mount(&server)
        .await;

    let dir = tmp.path().to_path_buf();
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let out = sn_cmd(&dir)
            .args([
                "profile",
                "add",
                "jwt",
                "--instance",
                &uri,
                "--auth",
                "oauth",
                "--grant",
                "jwt_bearer",
                "--client-id",
                "cid",
                "--client-secret-stdin",
                "--jwt-key-file",
                key_path.to_str().unwrap(),
                "--jwt-subject",
                "svc.user",
                "--jwt-kid",
                "k1",
                "--set-default",
            ])
            .write_stdin("shh\n")
            .assert()
            .success();
        let v: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
        assert_eq!(v["grant"], "jwt_bearer");
        assert_eq!(v["verified"], true);
        assert_eq!(v["user"], "svc.user");

        let o = load_config(&dir).profiles["jwt"].oauth.clone().unwrap();
        assert_eq!(o.grant, sn::config::OAuthGrant::JwtBearer);
        assert!(Path::new(o.jwt_key_file.as_deref().unwrap()).is_absolute());
        assert_eq!(o.jwt_subject.as_deref(), Some("svc.user"));
        assert!(o.redirect_uri.is_none());
        // The key stays in its file; sn stores the path only.
        let cfg_text = std::fs::read_to_string(dir.join("config.toml")).unwrap();
        let cred_text = std::fs::read_to_string(dir.join("credentials.toml")).unwrap();
        assert!(!cfg_text.contains("PRIVATE KEY") && !cred_text.contains("PRIVATE KEY"));

        // Age the cached token past expiry; the next call must mint again.
        let mut creds = load_creds(&dir);
        creds.profiles.get_mut("jwt").unwrap().oauth_tokens = Some(sn::config::TokenSet {
            access_token: "STALE".into(),
            refresh_token: None,
            expires_at: Some(1),
            token_type: None,
        });
        sn::config::save_credentials_to(&dir.join("credentials.toml"), &creds).unwrap();
        sn_cmd(&dir)
            .args(["table", "list", "incident", "--setlimit", "1"])
            .assert()
            .success();

        let out = sn_cmd(&dir).args(["profile", "status"]).assert().success();
        let v: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
        assert_eq!(v["grant"], "jwt_bearer");
        assert_eq!(v["jwtSubject"], "svc.user");
    })
    .await
    .unwrap();
}

#[test]
fn jwt_bearer_with_an_unreadable_key_fails_before_writing_anything() {
    let tmp = tempfile::tempdir().unwrap();
    let out = sn_cmd(tmp.path())
        .args([
            "profile",
            "add",
            "jwt",
            "--instance",
            "https://example.invalid",
            "--auth",
            "oauth",
            "--grant",
            "jwt_bearer",
            "--client-id",
            "cid",
            "--jwt-key-file",
            "/definitely/not/here.pem",
            "--jwt-subject",
            "u",
            "--no-verify",
        ])
        .assert()
        .failure()
        .code(1);
    assert!(
        stderr_text(&out).contains("JWT key file"),
        "{}",
        stderr_text(&out)
    );
    assert!(!tmp.path().join("config.toml").exists());
}

#[test]
fn jwt_flags_without_the_jwt_grant_are_refused_by_name() {
    let tmp = tempfile::tempdir().unwrap();
    let out = sn_cmd(tmp.path())
        .args([
            "profile",
            "add",
            "p",
            "--instance",
            "https://example.invalid",
            "--auth",
            "oauth",
            "--client-id",
            "cid",
            "--jwt-key-file",
            "k.pem",
            "--no-verify",
        ])
        .assert()
        .failure()
        .code(1);
    assert!(
        stderr_text(&out).contains("--grant jwt_bearer"),
        "{}",
        stderr_text(&out)
    );
}

#[test]
fn jwt_bearer_without_a_subject_names_the_flag() {
    let tmp = tempfile::tempdir().unwrap();
    let (key_path, _) = write_ec_key(tmp.path());
    let out = sn_cmd(tmp.path())
        .args([
            "profile",
            "add",
            "jwt",
            "--instance",
            "https://example.invalid",
            "--auth",
            "oauth",
            "--grant",
            "jwt_bearer",
            "--client-id",
            "cid",
            "--jwt-key-file",
            key_path.to_str().unwrap(),
            "--no-verify",
        ])
        .assert()
        .failure()
        .code(1);
    assert!(
        stderr_text(&out).contains("--jwt-subject"),
        "{}",
        stderr_text(&out)
    );
}

// ---------------------------------------------------------------------------
// auth = "token"
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn static_token_from_stdin_is_sent_verified_and_never_shown() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path("/api/now/ui/user/current_user"))
        .and(header("authorization", "Bearer EXT.TOKEN-1"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"result": {"user_name": "ext.user"}})),
        )
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_path_buf();
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let out = sn_cmd(&dir)
            .args([
                "profile",
                "add",
                "ext",
                "--instance",
                &uri,
                "--auth",
                "token",
                "--token-stdin",
            ])
            .write_stdin("EXT.TOKEN-1\n")
            .assert()
            .success();
        let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        assert!(!stdout.contains("EXT.TOKEN-1"), "token leaked: {stdout}");
        let v: Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(v["auth"], "token");
        assert_eq!(v["tokenSource"], "static");
        assert_eq!(v["user"], "ext.user");
        assert_eq!(
            load_creds(&dir).profiles["ext"].token.as_deref(),
            Some("EXT.TOKEN-1")
        );

        let out = sn_cmd(&dir)
            .args(["profile", "show", "ext"])
            .assert()
            .success();
        let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        assert!(!stdout.contains("EXT.TOKEN-1"), "token leaked: {stdout}");
        let v: Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(v["tokenSource"], "static");
        assert_eq!(v["hasToken"], true);

        // A static token has nothing to re-run.
        sn_cmd(&dir)
            .args(["--profile", "ext", "profile", "refresh"])
            .assert()
            .failure()
            .code(1);
    })
    .await
    .unwrap();
}

#[test]
fn token_auth_without_a_source_names_the_flags() {
    let tmp = tempfile::tempdir().unwrap();
    let out = sn_cmd(tmp.path())
        .args([
            "profile",
            "add",
            "ext",
            "--instance",
            "https://example.invalid",
            "--auth",
            "token",
            "--no-verify",
        ])
        .assert()
        .failure()
        .code(1);
    let e = stderr_text(&out);
    assert!(
        e.contains("--token-command") && e.contains("--token-stdin"),
        "{e}"
    );
}

#[test]
fn token_flags_under_another_auth_are_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let out = sn_cmd(tmp.path())
        .args([
            "profile",
            "add",
            "p",
            "--instance",
            "https://example.invalid",
            "--auth",
            "apikey",
            "--api-key",
            "k",
            "--token-command",
            "echo t",
            "--no-verify",
        ])
        .assert()
        .failure()
        .code(1);
    assert!(stderr_text(&out).contains("--auth token"));
}

/// The command side of `auth = "token"` runs through `sh -c`, so these are
/// Unix-only; the parsing and caching rules they exercise are platform-neutral
/// and also unit-tested in `external_token`.
#[cfg(unix)]
mod command {
    use super::*;

    /// A token command that appends a line to `runs` each time it executes, so
    /// a test can count invocations, then prints `output`.
    fn counting_command(runs: &Path, output: &str) -> String {
        format!("echo run >> '{}'; printf '%s' '{}'", runs.display(), output)
    }

    fn run_count(runs: &Path) -> usize {
        std::fs::read_to_string(runs)
            .map(|s| s.lines().count())
            .unwrap_or(0)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_command_token_with_an_expiry_is_cached_until_refresh() {
        let server = wiremock::MockServer::start().await;
        Mock::given(method("GET"))
            .and(header("authorization", "Bearer CMDAT"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"result": {"user_name": "cmd.user"}})),
            )
            .mount(&server)
            .await;

        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let runs = dir.join("runs");
        let cmd = counting_command(&runs, r#"{"access_token":"CMDAT","expires_in":3600}"#);
        let uri = server.uri();
        tokio::task::spawn_blocking(move || {
            let out = sn_cmd(&dir)
                .args([
                    "profile",
                    "add",
                    "cmd",
                    "--instance",
                    &uri,
                    "--auth",
                    "token",
                    "--token-command",
                    &cmd,
                    "--set-default",
                ])
                .assert()
                .success();
            let v: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
            assert_eq!(v["tokenSource"], "command");
            assert_eq!(v["user"], "cmd.user");
            assert_eq!(run_count(&runs), 1);
            let cache = load_creds(&dir).profiles["cmd"]
                .token_cache
                .clone()
                .unwrap();
            assert_eq!(cache.access_token, "CMDAT");
            assert!(cache.expires_at.is_some());
            assert!(load_creds(&dir).profiles["cmd"].token.is_none());

            // Within the stated lifetime: reused, not re-run.
            sn_cmd(&dir)
                .args(["table", "list", "incident", "--setlimit", "1"])
                .assert()
                .success();
            assert_eq!(run_count(&runs), 1);

            let out = sn_cmd(&dir).args(["profile", "status"]).assert().success();
            let v: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
            assert_eq!(v["tokenSource"], "command");
            assert_eq!(v["cached"], true);
            assert_eq!(v["expired"], false);

            // `refresh` forces a re-run; `logout` drops the cache.
            sn_cmd(&dir).args(["profile", "refresh"]).assert().success();
            assert_eq!(run_count(&runs), 2);
            sn_cmd(&dir).args(["profile", "logout"]).assert().success();
            assert!(load_creds(&dir).profiles["cmd"].token_cache.is_none());
        })
        .await
        .unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_bare_token_is_never_cached_and_the_command_sees_its_profile() {
        let server = wiremock::MockServer::start().await;
        Mock::given(method("GET"))
            .and(header("authorization", "Bearer bare-tok"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"result": {"user_name": "b"}})),
            )
            .mount(&server)
            .await;

        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let runs = dir.join("runs");
        // The token is built from $SN_PROFILE, proving the child sees it.
        let cmd = format!("echo run >> '{}'; echo \"$SN_PROFILE-tok\"", runs.display());
        let uri = server.uri();
        tokio::task::spawn_blocking(move || {
            sn_cmd(&dir)
                .args([
                    "profile",
                    "add",
                    "bare",
                    "--instance",
                    &uri,
                    "--auth",
                    "token",
                    "--token-command",
                    &cmd,
                    "--set-default",
                ])
                .assert()
                .success();
            for _ in 0..2 {
                sn_cmd(&dir)
                    .args(["table", "list", "incident", "--setlimit", "1"])
                    .assert()
                    .success();
            }
            // One run per invocation: no expiry was stated, so nothing is cached.
            assert_eq!(run_count(&runs), 3);
            assert!(load_creds(&dir).profiles["bare"].token_cache.is_none());
        })
        .await
        .unwrap();
    }

    #[test]
    fn a_failing_command_is_exit_1_quoting_stderr_but_never_stdout() {
        let tmp = tempfile::tempdir().unwrap();
        let out = sn_cmd(tmp.path())
            .args([
                "profile",
                "add",
                "bad",
                "--instance",
                "https://example.invalid",
                "--auth",
                "token",
                "--token-command",
                "printf LEAKED; echo 'please log in' >&2; exit 3",
            ])
            .assert()
            .failure()
            .code(1);
        let e = stderr_text(&out);
        assert!(e.contains("status 3") && e.contains("please log in"), "{e}");
        assert!(!e.contains("LEAKED"), "stdout leaked into the error: {e}");
        // Verification failed, so the profile was rolled back.
        assert!(!load_config(tmp.path()).profiles.contains_key("bad"));
    }
}
