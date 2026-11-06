//! `sn xml`: the export reaches the unload processor only for a query the
//! instance can evaluate, and the import uploads the file as `sys_upload.do` reads it.

mod common;

use common::{ProfileSpec, sn_cmd, write_profiles};
use serde_json::{Value, json};
use wiremock::matchers::{body_string_contains, header_exists, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const UNLOAD: &str =
    "<unload><incident action=\"INSERT_OR_UPDATE\"><sys_id>a</sys_id></incident></unload>\n";

/// Run `sn <args>` off the async runtime; returns (exit code, stdout, stderr).
async fn run(uri: String, args: Vec<String>, stdin: Option<&'static str>) -> (i32, String, String) {
    tokio::task::spawn_blocking(move || {
        let tmp = write_profiles(
            "test",
            &[ProfileSpec {
                name: "test",
                instance: &uri,
                username: "u",
                password: "p",
            }],
        );
        let mut cmd = sn_cmd(tmp.path());
        cmd.args(&args);
        cmd.write_stdin(stdin.unwrap_or(""));
        let out = cmd.output().unwrap();
        (
            out.status.code().unwrap(),
            String::from_utf8(out.stdout).unwrap(),
            String::from_utf8(out.stderr).unwrap(),
        )
    })
    .await
    .unwrap()
}

fn argv(args: &[&str]) -> Vec<String> {
    args.iter().map(ToString::to_string).collect()
}

async fn meta(server: &MockServer, table: &str, columns: Value) {
    Mock::given(method("GET"))
        .and(path(format!("/api/now/ui/meta/{table}")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"result": {"columns": columns}})),
        )
        .mount(server)
        .await;
}

async fn unload_never_called(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/incident.do"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(server)
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn export_writes_the_unload_document_untouched() {
    let server = MockServer::start().await;
    meta(&server, "incident", json!({"active": {}})).await;
    Mock::given(method("GET"))
        .and(path("/incident.do"))
        .and(query_param("UNL", ""))
        .and(query_param("sysparm_query", "active=true"))
        .and(query_param("sysparm_record_count", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_string(UNLOAD))
        .expect(1)
        .mount(&server)
        .await;

    let args = argv(&["xml", "export", "incident", "active=true", "--limit", "5"]);
    let (code, stdout, stderr) = run(server.uri(), args, None).await;
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, UNLOAD);
}

#[tokio::test(flavor = "current_thread")]
async fn export_refuses_a_column_the_instance_would_drop() {
    let server = MockServer::start().await;
    meta(&server, "incident", json!({"active": {}})).await;
    unload_never_called(&server).await;

    let args = argv(&["xml", "export", "incident", "active=true^zzz=1"]);
    let (code, stdout, stderr) = run(server.uri(), args, None).await;
    assert_eq!(code, 1, "{stderr}");
    assert!(stdout.is_empty());
    assert!(stderr.contains("zzz"), "{stderr}");
}

#[tokio::test(flavor = "current_thread")]
async fn export_walks_a_dot_walk_through_its_reference() {
    let server = MockServer::start().await;
    meta(
        &server,
        "incident",
        json!({"caller_id": {"reference": "sys_user"}}),
    )
    .await;
    meta(&server, "sys_user", json!({"name": {}})).await;
    unload_never_called(&server).await;

    let args = argv(&["xml", "export", "incident", "caller_id.zzz=1"]);
    let (code, _, stderr) = run(server.uri(), args, None).await;
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("sys_user has no column 'zzz'"), "{stderr}");
}

#[tokio::test(flavor = "current_thread")]
async fn export_refuses_an_unknown_table() {
    let server = MockServer::start().await;
    meta(&server, "incident", json!({})).await;
    unload_never_called(&server).await;

    let args = argv(&["xml", "export", "incident", "active=true"]);
    let (code, _, stderr) = run(server.uri(), args, None).await;
    assert_eq!(code, 1, "{stderr}");
}

#[tokio::test(flavor = "current_thread")]
async fn export_does_not_follow_a_refusal_to_a_login_page() {
    let server = MockServer::start().await;
    meta(&server, "incident", json!({"active": {}})).await;
    Mock::given(method("GET"))
        .and(path("/incident.do"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", "/login.do"))
        .mount(&server)
        .await;

    let args = argv(&["xml", "export", "incident", "active=true"]);
    let (code, stdout, stderr) = run(server.uri(), args, None).await;
    assert_ne!(code, 0, "{stderr}");
    assert!(stdout.is_empty(), "{stdout}");
}

#[tokio::test(flavor = "current_thread")]
async fn export_refuses_output_before_the_network() {
    let args = argv(&["xml", "export", "incident", "a=1", "--output", "table"]);
    let (code, _, _) = run("http://127.0.0.1:9".into(), args, None).await;
    assert_eq!(code, 1);
}

async fn session(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/api/now/sg/impersonation/session"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Set-Cookie", "JSESSIONID=s; Path=/")
                .set_body_json(json!({"SessionToken": "tok"})),
        )
        .mount(server)
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn import_uploads_stdin_as_the_form_expects() {
    let server = MockServer::start().await;
    session(&server).await;
    Mock::given(method("POST"))
        .and(path("/sys_upload.do"))
        .and(header_exists("X-UserToken"))
        .and(body_string_contains("name=\"sysparm_ck\""))
        .and(body_string_contains("name=\"sysparm_target\""))
        .and(body_string_contains(
            "name=\"attachFile\"; filename=\"unload.xml\"",
        ))
        .and(body_string_contains("<sys_id>a</sys_id>"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let args = argv(&["xml", "import", "-", "--yes"]);
    let (code, stdout, stderr) = run(server.uri(), args, Some(UNLOAD)).await;
    assert_eq!(code, 0, "{stderr}");
    let v: Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v, json!({"uploaded": "unload.xml", "bytes": UNLOAD.len()}));
}

#[tokio::test(flavor = "current_thread")]
async fn import_needs_yes_without_a_terminal() {
    let args = argv(&["xml", "import", "-"]);
    let (code, _, stderr) = run("http://127.0.0.1:9".into(), args, Some(UNLOAD)).await;
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("--yes"), "{stderr}");
}

#[tokio::test(flavor = "current_thread")]
async fn import_reports_a_refused_upload() {
    let server = MockServer::start().await;
    session(&server).await;
    Mock::given(method("POST"))
        .and(path("/sys_upload.do"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;

    let args = argv(&["xml", "import", "-", "--yes"]);
    let (code, _, stderr) = run(server.uri(), args, Some(UNLOAD)).await;
    assert_eq!(code, 4, "{stderr}");
}

/// `.do?UNL` answers 401 to a bearer token, so the export must arrive on the
/// session's cookies alone, after the session has loaded a page.
#[tokio::test(flavor = "current_thread")]
async fn a_bearer_profile_exports_on_cookies_alone_after_a_page_load() {
    let server = MockServer::start().await;
    meta(&server, "incident", json!({"active": {}})).await;
    session(&server).await;
    Mock::given(method("GET"))
        .and(path("/home.do"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/incident.do"))
        .respond_with(ResponseTemplate::new(200).set_body_string(UNLOAD))
        .expect(1)
        .mount(&server)
        .await;

    let uri = server.uri();
    let (code, stdout, stderr) = tokio::task::spawn_blocking(move || {
        let tmp = tempfile::tempdir().unwrap();
        sn_cmd(tmp.path())
            .args([
                "profile", "add", "tok", "--auth", "token", "--token", "secret",
            ])
            .args(["--instance", &uri, "--no-verify"])
            .assert()
            .success();
        let out = sn_cmd(tmp.path())
            .args([
                "--profile",
                "tok",
                "xml",
                "export",
                "incident",
                "active=true",
            ])
            .output()
            .unwrap();
        (
            out.status.code().unwrap(),
            String::from_utf8(out.stdout).unwrap(),
            String::from_utf8(out.stderr).unwrap(),
        )
    })
    .await
    .unwrap();
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, UNLOAD);

    let requests = server.received_requests().await.unwrap();
    let position = |p: &str| requests.iter().position(|r| r.url.path() == p).unwrap();
    assert!(position("/home.do") < position("/incident.do"));
    let unload = &requests[position("/incident.do")];
    assert!(unload.headers.get("authorization").is_none());
    let cookie = unload.headers.get("cookie").unwrap().to_str().unwrap();
    assert!(cookie.contains("JSESSIONID=s"), "{cookie}");
}
