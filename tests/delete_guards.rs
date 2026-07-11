//! Destructive `delete` commands share one confirmation guard
//! (`cli::table::confirm_delete`): with `--yes` the request proceeds; without
//! it, a non-interactive stdin is refused (exit 1 + JSON usage envelope on
//! stderr) rather than deleting silently. assert_cmd runs the binary with a
//! non-TTY stdin, so the no-`--yes` path here is exactly the guard path.

mod common;

use common::{sn_cmd, write_profiles, ProfileSpec};
use serde_json::Value;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

fn stderr_envelope(out: &assert_cmd::assert::Assert) -> Value {
    let stderr = String::from_utf8(out.get_output().stderr.clone()).unwrap();
    serde_json::from_str(stderr.trim()).unwrap_or_else(|e| {
        panic!("stderr is not the JSON error envelope ({e}): {stderr}");
    })
}

fn one_profile(instance: &str) -> tempfile::TempDir {
    write_profiles(
        "test",
        &[ProfileSpec {
            name: "test",
            instance,
            username: "u",
            password: "p",
        }],
    )
}

/// Every guarded delete must refuse a non-TTY stdin without `--yes`: exit 1 and
/// a usage envelope naming the `--yes` requirement, before any network call.
fn assert_guarded(args: &[&str]) {
    let tmp = one_profile("http://127.0.0.1:1");
    let mut cmd = sn_cmd(tmp.path());
    let out = cmd.args(args).assert().code(1);
    let v = stderr_envelope(&out);
    let msg = v["error"]["message"].as_str().unwrap();
    assert!(
        msg.contains("requires --yes"),
        "unexpected message for {args:?}: {msg}"
    );
    drop(tmp);
}

#[test]
fn change_delete_without_yes_is_guarded() {
    assert_guarded(&["change", "delete", "chg001"]);
}

#[test]
fn change_task_delete_without_yes_is_guarded() {
    assert_guarded(&["change", "task", "delete", "chg001", "task001"]);
}

#[test]
fn attachment_delete_without_yes_is_guarded() {
    assert_guarded(&["attachment", "delete", "att001"]);
}

#[test]
fn cmdb_relation_delete_without_yes_is_guarded() {
    assert_guarded(&[
        "cmdb",
        "relation",
        "delete",
        "cmdb_ci_server",
        "ci001",
        "rel001",
    ]);
}

#[tokio::test(flavor = "current_thread")]
async fn change_delete_with_yes_proceeds() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/api/sn_chg_rest/change/chg001"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = one_profile(&uri);
        let mut cmd = sn_cmd(tmp.path());
        let out = cmd
            .args(["change", "delete", "chg001", "--yes"])
            .assert()
            .success();
        let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        assert_eq!(stdout.trim(), "");
        drop(tmp);
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn change_task_delete_with_yes_proceeds() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/api/sn_chg_rest/change/chg001/task/task001"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = one_profile(&uri);
        let mut cmd = sn_cmd(tmp.path());
        cmd.args(["change", "task", "delete", "chg001", "task001", "--yes"])
            .assert()
            .success();
        drop(tmp);
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn attachment_delete_with_yes_proceeds() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/api/now/attachment/att001"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = one_profile(&uri);
        let mut cmd = sn_cmd(tmp.path());
        cmd.args(["attachment", "delete", "att001", "-y"])
            .assert()
            .success();
        drop(tmp);
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn cmdb_relation_delete_with_yes_proceeds() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path(
            "/api/now/cmdb/instance/cmdb_ci_server/ci001/relation/rel001",
        ))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = one_profile(&uri);
        let mut cmd = sn_cmd(tmp.path());
        cmd.args([
            "cmdb",
            "relation",
            "delete",
            "cmdb_ci_server",
            "ci001",
            "rel001",
            "--yes",
        ])
        .assert()
        .success();
        drop(tmp);
    })
    .await
    .unwrap();
}
