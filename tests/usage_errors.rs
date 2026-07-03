//! Usage errors must honor the machine contract: exit 1 (clap's default of 2
//! is reserved for API errors) and a JSON error envelope on non-TTY stderr.

use assert_cmd::Command;
use serde_json::Value;

fn stderr_envelope(out: &assert_cmd::assert::Assert) -> Value {
    let stderr = String::from_utf8(out.get_output().stderr.clone()).unwrap();
    serde_json::from_str(stderr.trim()).unwrap_or_else(|e| {
        panic!("stderr is not the JSON error envelope ({e}): {stderr}");
    })
}

#[test]
fn unknown_flag_exits_1_with_json_envelope() {
    let out = Command::cargo_bin("sn")
        .unwrap()
        .args(["table", "list", "incident", "--bogus-flag"])
        .assert()
        .code(1);
    let v = stderr_envelope(&out);
    let msg = v["error"]["message"].as_str().unwrap();
    assert!(msg.contains("--bogus-flag"), "message was: {msg}");
}

#[test]
fn missing_required_arg_exits_1_with_json_envelope() {
    let out = Command::cargo_bin("sn")
        .unwrap()
        .args(["table", "list"])
        .assert()
        .code(1);
    let v = stderr_envelope(&out);
    assert!(v["error"]["message"].is_string());
}

#[test]
fn missing_subcommand_exits_1() {
    Command::cargo_bin("sn").unwrap().assert().code(1);
}

#[test]
fn help_exits_0() {
    Command::cargo_bin("sn")
        .unwrap()
        .args(["--help"])
        .assert()
        .success();
}

#[test]
fn version_flag_is_capital_v() {
    let out = Command::cargo_bin("sn")
        .unwrap()
        .args(["-V"])
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.starts_with("sn "), "expected version, got: {stdout}");
}

#[test]
fn lowercase_v_is_verbose_not_version() {
    // `sn -v ping` must attempt the command (failing on missing config with a
    // JSON envelope), not print the version and exit 0.
    let out = Command::cargo_bin("sn")
        .unwrap()
        .env("HOME", "/nonexistent-sn-test-home")
        .env("XDG_CONFIG_HOME", "/nonexistent-sn-test-home/.config")
        .args(["-v", "ping"])
        .assert()
        .code(1);
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(
        !stdout.contains("sn 0."),
        "-v printed the version instead of running the command"
    );
    stderr_envelope(&out);
}
