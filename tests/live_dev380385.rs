// Live end-to-end tests against the dev380385 ServiceNow instance.
//
// These tests use the developer's on-disk `dev380385` profile (created via
// `sn init`) and exercise the real ServiceNow REST API. They verify that
// profile switching works against a real instance, and that the env-var
// contract holds end-to-end (not just at the unit/mock layer).
//
// Tagged `#[ignore]` so they don't run by default. Run them with:
//
//     cargo test --release --test live_dev380385 -- --ignored --test-threads=1
//
// Required setup:
//   - `sn init --profile dev380385` against https://dev380385.service-now.com
//   - The instance must be awake and reachable from this host
//
// Each test sets `--timeout 60` because dev instances sleep when idle and
// the wake-up handshake on the first request can take 20-30s.

#![cfg(not(target_os = "windows"))] // assert_cmd works on Windows but the user's instance is exercised from Unix

use assert_cmd::Command;
use serde_json::Value;

const PROFILE: &str = "dev380385";
const EXPECTED_INSTANCE: &str = "https://dev380385.service-now.com";

/// Build a `Command` invocation of `sn` that:
///   - bumps timeout to 60s (handles dev-instance wake from idle)
///   - clears every removed-design env var so a developer's stale shell can't
///     mask a real env-var leak. If the env vars *did* leak silently, this
///     would hide the bug; we want them gone.
fn sn_cmd() -> Command {
    let mut cmd = Command::cargo_bin("sn").unwrap();
    cmd.env_remove("SN_INSTANCE")
        .env_remove("SN_INSTANCE_URL")
        .env_remove("SN_USERNAME")
        .env_remove("SN_PASSWORD")
        .env_remove("SN_PROFILE")
        .env_remove("SN_TIMEOUT");
    cmd
}

/// Skip if the `dev380385` profile is not configured on this host. Returns
/// `true` if tests should run, `false` if they should bail.
fn profile_exists() -> bool {
    let out = Command::cargo_bin("sn")
        .unwrap()
        .args(["profile", "show", PROFILE])
        .output()
        .expect("spawn sn");
    out.status.success()
}

/// Macro to short-circuit a test with a SKIP message when the profile isn't
/// present. Stdlib `eprintln!` lands in `cargo test`'s captured output and
/// becomes visible with `--nocapture`.
macro_rules! require_profile {
    () => {
        if !profile_exists() {
            eprintln!(
                "SKIP: profile '{}' not configured on this host (run `sn init --profile {}`)",
                PROFILE, PROFILE
            );
            return;
        }
    };
}

// =============================================================================
// Baseline reachability — these prove the real instance is reachable and
// authentication succeeds with the configured profile.
// =============================================================================

#[test]
#[ignore]
fn live_auth_test_succeeds() {
    require_profile!();
    sn_cmd()
        .args(["--profile", PROFILE, "--timeout", "60", "auth", "test"])
        .assert()
        .success();
}

#[test]
#[ignore]
fn live_ping_returns_correct_instance() {
    require_profile!();
    let out = sn_cmd()
        .args(["--profile", PROFILE, "--timeout", "60", "ping"])
        .output()
        .expect("spawn sn");
    assert!(out.status.success(), "ping failed: {:?}", out);
    let v: Value = serde_json::from_slice(&out.stdout).expect("ping stdout is JSON");
    assert_eq!(v["ok"], Value::Bool(true), "ping ok flag: {v}");
    assert_eq!(v["profile"], Value::String(PROFILE.into()));
    assert_eq!(v["instance"], Value::String(EXPECTED_INSTANCE.into()));
    assert!(
        v["latency_ms"].is_number(),
        "latency_ms should be numeric: {v}"
    );
}

#[test]
#[ignore]
fn live_user_me_returns_authed_user() {
    require_profile!();
    let out = sn_cmd()
        .args(["--profile", PROFILE, "--timeout", "60", "user", "me"])
        .output()
        .expect("spawn sn");
    assert!(out.status.success(), "user me failed: {:?}", out);
    let v: Value = serde_json::from_slice(&out.stdout).expect("user me stdout is JSON");
    let user_name = v["user_name"].as_str().expect("user_name field present");
    assert!(
        !user_name.is_empty(),
        "authed user_name should not be empty"
    );
    // Sanity: the authed user should have a sys_id.
    assert!(
        v["sys_id"].as_str().is_some_and(|s| !s.is_empty()),
        "sys_id should be present: {v}"
    );
}

#[test]
#[ignore]
fn live_table_list_sys_user_works() {
    require_profile!();
    let out = sn_cmd()
        .args([
            "--profile",
            PROFILE,
            "--timeout",
            "60",
            "table",
            "list",
            "sys_user",
            "--setlimit",
            "1",
        ])
        .output()
        .expect("spawn sn");
    assert!(out.status.success(), "table list failed: {:?}", out);
    let v: Value = serde_json::from_slice(&out.stdout).expect("table list stdout is JSON");
    let arr = v
        .as_array()
        .expect("table list returns an array (result unwrapped)");
    assert_eq!(
        arr.len(),
        1,
        "setlimit=1 should yield exactly 1 record: {v}"
    );
}

// =============================================================================
// The headline regression tests — env vars MUST NOT leak into a live request.
// If the bug the user feared were real, these would fail.
// =============================================================================

#[test]
#[ignore]
fn live_credential_env_vars_do_not_leak() {
    require_profile!();
    // Every removed-design env var set to garbage that would break resolution
    // if the binary consulted it. The on-disk profile must still be used and
    // auth against the real instance must succeed.
    sn_cmd()
        .env("SN_INSTANCE", "http://nonexistent.invalid")
        .env("SN_INSTANCE_URL", "http://also-nonexistent.invalid")
        .env("SN_USERNAME", "hacker")
        .env("SN_PASSWORD", "wrongpass")
        .env("SN_PROFILE", "totally_made_up_profile")
        .env("SN_TIMEOUT", "0")
        .args(["--profile", PROFILE, "--timeout", "60", "auth", "test"])
        .assert()
        .success();
}

#[test]
#[ignore]
fn live_user_shell_env_does_not_leak() {
    require_profile!();
    // Reproduce the shape of a developer's stale shell environment — env vars
    // that point at a different ServiceNow instance with credentials that
    // would be rejected by dev380385. The dev380385 profile must still
    // authenticate correctly, proving the env vars are ignored.
    sn_cmd()
        .env("SN_INSTANCE_URL", "https://other-instance.example.com")
        .env("SN_USERNAME", "stale-shell-user")
        .env("SN_PASSWORD", "definitely-the-wrong-password-for-dev380385")
        .args(["--profile", PROFILE, "--timeout", "60", "auth", "test"])
        .assert()
        .success();
}

// =============================================================================
// Profile-switching against a real instance.
// =============================================================================

#[test]
#[ignore]
fn live_unknown_profile_errors_clearly() {
    // Doesn't actually contact the instance — fails at profile resolution. We
    // run it as part of the live suite to verify the error path doesn't
    // accidentally fall back to env vars at runtime.
    sn_cmd()
        .args([
            "--profile",
            "no_such_profile_exists_hopefully",
            "auth",
            "test",
        ])
        .assert()
        .code(1)
        .stderr(predicates::str::contains(
            "no_such_profile_exists_hopefully",
        ));
}

#[test]
#[ignore]
fn live_instance_override_redirects_traffic() {
    require_profile!();
    // Use the dev380385 profile's credentials against the same instance via
    // --instance-override. This proves --instance-override doesn't disturb
    // the credentials fetch from the profile.
    sn_cmd()
        .args([
            "--profile",
            PROFILE,
            "--instance-override",
            EXPECTED_INSTANCE,
            "--timeout",
            "60",
            "auth",
            "test",
        ])
        .assert()
        .success();
}

// =============================================================================
// Output-mode contract — verifies JSON contract holds against a real backend.
// =============================================================================

#[test]
#[ignore]
fn live_raw_output_preserves_envelope() {
    require_profile!();
    let out = sn_cmd()
        .args([
            "--profile",
            PROFILE,
            "--timeout",
            "60",
            "--output",
            "raw",
            "table",
            "list",
            "sys_user",
            "--setlimit",
            "1",
        ])
        .output()
        .expect("spawn sn");
    assert!(out.status.success(), "table list raw failed: {:?}", out);
    let v: Value = serde_json::from_slice(&out.stdout).expect("raw stdout is JSON");
    assert!(
        v.get("result").is_some(),
        "--output raw must preserve the SN envelope: {v}"
    );
}

#[test]
#[ignore]
fn live_default_output_unwraps_envelope() {
    require_profile!();
    let out = sn_cmd()
        .args([
            "--profile",
            PROFILE,
            "--timeout",
            "60",
            "table",
            "list",
            "sys_user",
            "--setlimit",
            "1",
        ])
        .output()
        .expect("spawn sn");
    assert!(out.status.success(), "table list default failed: {:?}", out);
    let v: Value = serde_json::from_slice(&out.stdout).expect("default stdout is JSON");
    assert!(
        v.is_array(),
        "default output must unwrap the result envelope into an array: {v}"
    );
}
