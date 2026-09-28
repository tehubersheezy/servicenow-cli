//! `sn script run` drives the classic-UI `sys.scripts.do` processor: mint a UI
//! session (cookies + CSRF token) from `GET /api/now/sg/impersonation/session`,
//! then POST the script form inside it. The response shapes mocked here are
//! the ones measured on a live instance (see `src/cli/script.rs`).

mod common;

use common::{ProfileSpec, sn_cmd, write_oauth_profile, write_profiles};
use serde_json::{Value, json};
use sn::config::now_unix;
use wiremock::matchers::{body_string_contains, header, header_regex, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The session's CSRF token. Distinctive so a leak into any log is greppable.
const TOKEN: &str = "CSRFTOKEN0xDEADBEEF";
const SESSION_ID: &str = "SESSIONCOOKIE0xFEEDFACE";

fn profile(uri: &str) -> tempfile::TempDir {
    write_profiles(
        "test",
        &[ProfileSpec {
            name: "test",
            instance: uri,
            username: "u",
            password: "p",
        }],
    )
}

fn page(scope: &str, pre: &str) -> String {
    format!(
        "[0:00:00.951] <HTML><BODY>Script completed in scope {scope}: script<HR/>Script \
         execution history <A target='blank' HREF='sys_script_execution_history.do?\
         sys_id=48169e6a2fe303107efd1d707fa4e3f4'>available here</A><HR/><PRE>{pre}</PRE>\
         <HR/></BODY></HTML>"
    )
}

async fn mount_session(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/api/now/sg/impersonation/session"))
        .respond_with(
            ResponseTemplate::new(200)
                .append_header(
                    "Set-Cookie",
                    format!("JSESSIONID={SESSION_ID}; Path=/; HttpOnly"),
                )
                .append_header("Set-Cookie", "glide_user_route=node1; Path=/")
                .set_body_json(json!({
                    "CurrentUser": "admin",
                    "OriginalUser": "admin",
                    "SessionToken": TOKEN,
                })),
        )
        .expect(1)
        .mount(server)
        .await;
}

/// The POST only matches when it carries the session: cookie, token header and
/// token field. A request missing any of them falls through to a 404.
fn scripts_post() -> wiremock::MockBuilder {
    Mock::given(method("POST"))
        .and(path("/sys.scripts.do"))
        .and(header_regex("cookie", SESSION_ID))
        .and(header("x-usertoken", TOKEN))
        .and(body_string_contains(format!("sysparm_ck={TOKEN}")))
        .and(body_string_contains("runscript=Run+script"))
}

fn stdout_json(out: &assert_cmd::assert::Assert) -> Value {
    let s = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    serde_json::from_str(s.trim()).unwrap_or_else(|e| panic!("stdout not JSON ({e}): {s}"))
}

fn stderr_json(out: &assert_cmd::assert::Assert) -> Value {
    let s = String::from_utf8(out.get_output().stderr.clone()).unwrap();
    serde_json::from_str(s.trim()).unwrap_or_else(|e| panic!("stderr not JSON ({e}): {s}"))
}

#[tokio::test(flavor = "current_thread")]
async fn runs_a_script_and_returns_its_output() {
    let server = MockServer::start().await;
    mount_session(&server).await;
    scripts_post()
        .and(body_string_contains("sys_scope=global"))
        .and(body_string_contains("script=gs.info"))
        .respond_with(ResponseTemplate::new(200).set_body_string(page(
            "global",
            "*** Script: hello<BR/>*** Script: {&quot;n&quot;:3}<BR/>Slow business rule 'X', \
             time was: 0:00:00.789<BR/>",
        )))
        .expect(1)
        .mount(&server)
        .await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&uri);
        let out = sn_cmd(tmp.path())
            .args(["--compact", "script", "run", "gs.info('hello')", "--yes"])
            .assert()
            .success();
        let v = stdout_json(&out);
        assert_eq!(v["ok"], true);
        assert_eq!(v["scope"], "global");
        assert_eq!(v["output"], json!(["hello", "{\"n\":3}"]));
        assert_eq!(v["messages"].as_array().unwrap().len(), 1);
        assert_eq!(v["error"], Value::Null);
        assert_eq!(v["elapsed_ms"], 951);
        assert_eq!(v["history_id"], "48169e6a2fe303107efd1d707fa4e3f4");
        assert_eq!(v["rollback_context"], Value::Null);
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn script_is_read_from_stdin() {
    let server = MockServer::start().await;
    mount_session(&server).await;
    scripts_post()
        .and(body_string_contains("script=gs.info%28%27from+stdin%27%29"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(page("global", "*** Script: from stdin<BR/>")),
        )
        .expect(1)
        .mount(&server)
        .await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&uri);
        let out = sn_cmd(tmp.path())
            .args(["--compact", "script", "run", "@-", "--yes"])
            .write_stdin("gs.info('from stdin')")
            .assert()
            .success();
        assert_eq!(stdout_json(&out)["output"], json!(["from stdin"]));
    })
    .await
    .unwrap();
}

/// A script error is reported inside a 200: stdout still gets the run (with
/// the output printed before the error), stderr the envelope with the real
/// status, and the exit code is 2.
#[tokio::test(flavor = "current_thread")]
async fn script_error_exits_2_with_output_kept() {
    let server = MockServer::start().await;
    mount_session(&server).await;
    scripts_post()
        .respond_with(ResponseTemplate::new(200).set_body_string(page(
            "global",
            "*** Script: before<BR/>Script execution error: Script Identifier: \
             null.null.script, Error Description: Error: boom (null.null.script; line 2), \
             Script ES Level: 0<BR/>Root cause of JavaScriptException<BR/>",
        )))
        .mount(&server)
        .await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&uri);
        let out = sn_cmd(tmp.path())
            .args(["--compact", "script", "run", "x", "--yes"])
            .assert()
            .code(2);
        let v = stdout_json(&out);
        assert_eq!(v["ok"], false);
        assert_eq!(v["output"], json!(["before"]));
        assert_eq!(v["error"]["type"], "execution");
        assert_eq!(
            v["error"]["message"],
            "Error: boom (null.null.script; line 2)"
        );
        assert_eq!(v["error"]["line"], 2);
        let e = stderr_json(&out);
        assert_eq!(e["error"]["status_code"], 200);
        assert_eq!(e["error"]["sn_error"]["type"], "execution");
        assert!(
            e["error"]["message"]
                .as_str()
                .unwrap()
                .starts_with("script execution error: Error: boom"),
            "{e}"
        );
    })
    .await
    .unwrap();
}

/// Arbitrary code execution is gated like every destructive command, before
/// any network call (the instance is a closed port).
#[test]
fn refuses_without_yes_on_a_non_tty_stdin() {
    let tmp = profile("http://127.0.0.1:1");
    let out = sn_cmd(tmp.path())
        .args(["script", "run", "gs.info(1)", "--scope", "x_acme_app"])
        .assert()
        .code(1);
    assert_eq!(
        stderr_json(&out)["error"]["message"],
        "run a background script in scope x_acme_app requires --yes when stdin is not a terminal"
    );
}

/// A user the processor will not serve gets `302 → /logout_redirect.do` under
/// OAuth. The redirect must not be followed (that turns the verdict into a 200
/// login page), and the refusal is an auth error carrying the real status.
#[tokio::test(flavor = "current_thread")]
async fn redirect_to_logout_is_an_auth_error_and_is_not_followed() {
    let server = MockServer::start().await;
    mount_session(&server).await;
    Mock::given(method("POST"))
        .and(path("/sys.scripts.do"))
        .and(header("authorization", "Bearer VALID_AT"))
        .respond_with(ResponseTemplate::new(302).append_header("Location", "/logout_redirect.do"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/logout_redirect.do"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>login</html>"))
        .expect(0)
        .mount(&server)
        .await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = write_oauth_profile("otest", &uri, "cid", now_unix() as i64 + 3600);
        let out = sn_cmd(tmp.path())
            .args(["script", "run", "gs.info(1)", "--yes"])
            .assert()
            .code(4);
        let e = stderr_json(&out);
        assert_eq!(e["error"]["status_code"], 302);
        assert!(
            e["error"]["message"]
                .as_str()
                .unwrap()
                .contains("/logout_redirect.do"),
            "{e}"
        );
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn not_authorized_body_is_an_in_band_refusal() {
    let server = MockServer::start().await;
    mount_session(&server).await;
    scripts_post()
        .respond_with(ResponseTemplate::new(200).set_body_string("not authorized"))
        .mount(&server)
        .await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&uri);
        let out = sn_cmd(tmp.path())
            .args(["script", "run", "gs.info(1)", "--yes"])
            .assert()
            .code(2);
        let e = stderr_json(&out);
        assert_eq!(e["error"]["status_code"], 200);
        assert!(
            e["error"]["message"]
                .as_str()
                .unwrap()
                .contains("did not run"),
            "{e}"
        );
        assert!(out.get_output().stdout.is_empty());
    })
    .await
    .unwrap();
}

/// `--scope` is resolved to a sys_id first (the form ignores names and runs
/// them in global), and `--rollback` reports the context from the history row.
#[tokio::test(flavor = "current_thread")]
async fn scope_is_resolved_and_rollback_context_reported() {
    let server = MockServer::start().await;
    mount_session(&server).await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_scope"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [{"sys_id": "a52b2ebce7424d3c9260d39e499ca1d4", "name": "Acme", "scope": "x_acme_app"}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    scripts_post()
        .and(body_string_contains(
            "sys_scope=a52b2ebce7424d3c9260d39e499ca1d4",
        ))
        .and(body_string_contains("record_for_rollback=on"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(page("x_acme_app", "x_acme_app: hi<BR/>*** Script: x<BR/>")),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(
            "/api/now/table/sys_script_execution_history/48169e6a2fe303107efd1d707fa4e3f4",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": {"rollback_context": "adc85eee2fe303107efd1d707fa4e3cf"}
        })))
        .expect(1)
        .mount(&server)
        .await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&uri);
        let out = sn_cmd(tmp.path())
            .args([
                "--compact",
                "script",
                "run",
                "gs.info('hi')",
                "--scope",
                "x_acme_app",
                "--rollback",
                "--yes",
            ])
            .assert()
            .success();
        let v = stdout_json(&out);
        assert_eq!(v["scope"], "x_acme_app");
        assert_eq!(v["output"], json!(["hi"]));
        assert_eq!(v["messages"], json!(["*** Script: x"]));
        assert_eq!(v["rollback_context"], "adc85eee2fe303107efd1d707fa4e3cf");
    })
    .await
    .unwrap();
}

/// If the instance reports a different scope than the one resolved, the run
/// happened somewhere the caller did not ask for — that must not exit 0.
#[tokio::test(flavor = "current_thread")]
async fn scope_mismatch_is_an_error() {
    let server = MockServer::start().await;
    mount_session(&server).await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_scope"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [{"sys_id": "a52b2ebce7424d3c9260d39e499ca1d4", "name": "Acme", "scope": "x_acme_app"}]
        })))
        .mount(&server)
        .await;
    scripts_post()
        .respond_with(
            ResponseTemplate::new(200).set_body_string(page("global", "*** Script: hi<BR/>")),
        )
        .mount(&server)
        .await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&uri);
        let out = sn_cmd(tmp.path())
            .args(["script", "run", "x", "--scope", "x_acme_app", "--yes"])
            .assert()
            .code(2);
        let e = stderr_json(&out);
        assert!(e["error"].get("status_code").is_none(), "{e}");
        assert!(
            e["error"]["message"]
                .as_str()
                .unwrap()
                .contains("ran in scope global"),
            "{e}"
        );
    })
    .await
    .unwrap();
}

/// `-ddd` prints bodies verbatim, and both the session response and the form
/// carry the CSRF token; neither it nor the session cookie may reach stderr.
#[tokio::test(flavor = "current_thread")]
async fn ddd_never_logs_the_csrf_token_or_session_cookie() {
    let server = MockServer::start().await;
    mount_session(&server).await;
    scripts_post()
        .respond_with(
            ResponseTemplate::new(200).set_body_string(page("global", "*** Script: ok<BR/>")),
        )
        .mount(&server)
        .await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&uri);
        let out = sn_cmd(tmp.path())
            .args(["-ddd", "script", "run", "gs.info('ok')", "--yes"])
            .assert()
            .success();
        let stderr = String::from_utf8(out.get_output().stderr.clone()).unwrap();
        assert!(!stderr.contains(TOKEN), "CSRF token leaked:\n{stderr}");
        assert!(
            !stderr.contains(SESSION_ID),
            "session cookie leaked:\n{stderr}"
        );
        // The exchange was logged — redaction, not a silenced log.
        assert!(stderr.contains("<session response: redacted>"), "{stderr}");
        assert!(stderr.contains("sysparm_ck"), "{stderr}");
        assert!(stderr.contains("POST"), "{stderr}");
    })
    .await
    .unwrap();
}
