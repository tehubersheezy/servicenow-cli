//! `sn flow runs/debug/steps/logs/why-not` end to end against a mocked
//! instance: the encoded queries on the wire, flow resolution and its canary,
//! the reporting-OFF explanation, and the why-not verdicts.

mod common;

use serde_json::{Value, json};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn profile(server_uri: &str) -> tempfile::TempDir {
    common::write_profiles(
        "test",
        &[common::ProfileSpec {
            name: "test",
            instance: server_uri,
            username: "u",
            password: "p",
        }],
    )
}

const FLOW: &str = "09cb5ab493b302100a004c5284891829";
const TRIGGER: &str = "d1cf8f24933012100a004c528489184c";
const CTX: &str = "3a595ae29d270310023aa9843c4d4ee0";
const REC: &str = "d6c3c43e2fce4f507efd1d707fa4e35b";

fn result(v: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({ "result": v }))
}

fn count(n: u64) -> ResponseTemplate {
    result(json!({"stats": {"count": n.to_string()}}))
}

fn flow_row(name: &str, internal: &str) -> Value {
    json!({
        "sys_id": FLOW, "name": name, "internal_name": internal, "type": "flow",
        "active": "true", "status": "published", "remote_trigger_id": TRIGGER,
        "sys_scope": "global", "sys_class_name": "sys_hub_flow"
    })
}

async fn mount_table(server: &MockServer, table: &str, query: &str, rows: Value) {
    Mock::given(method("GET"))
        .and(path(format!("/api/now/table/{table}")))
        .and(query_param("sysparm_query", query))
        .respond_with(result(rows))
        .mount(server)
        .await;
}

fn run(server_uri: String, args: Vec<&'static str>) -> (i32, Value, String) {
    let tmp = profile(&server_uri);
    let out = common::sn_cmd(tmp.path())
        .arg("--compact")
        .args(args)
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    let v = serde_json::from_str(&stdout).unwrap_or(Value::Null);
    (out.status.code().unwrap(), v, stderr)
}

#[tokio::test(flavor = "current_thread")]
async fn runs_resolves_a_name_and_filters_contexts() {
    let server = MockServer::start().await;
    mount_table(
        &server,
        "sys_hub_flow",
        "name=Delegate Roles in Group^ORinternal_name=Delegate Roles in Group",
        json!([flow_row(
            "Delegate Roles in Group",
            "delegate_roles_in_group"
        )]),
    )
    .await;
    mount_table(
        &server,
        "sys_flow_context",
        &format!("flow={FLOW}^state=ERROR^ORerror_stateISNOTEMPTY^ORDERBYDESCsys_created_on"),
        json!([{"sys_id": CTX, "state": "ERROR"}]),
    )
    .await;
    let uri = server.uri();
    let (code, v, err) = tokio::task::spawn_blocking(move || {
        run(
            uri,
            vec!["flow", "runs", "Delegate Roles in Group", "--errors"],
        )
    })
    .await
    .unwrap();
    assert_eq!(code, 0, "{err}");
    assert_eq!(v[0]["sys_id"], CTX);
}

#[tokio::test(flavor = "current_thread")]
async fn runs_by_record_resolves_the_number_and_needs_no_flow() {
    let server = MockServer::start().await;
    mount_table(
        &server,
        "change_request",
        "number=CHG0030421",
        json!([{"sys_id": REC}]),
    )
    .await;
    mount_table(
        &server,
        "sys_flow_context",
        &format!("source_record={REC}^ORDERBYDESCsys_created_on"),
        json!([{"sys_id": CTX, "source_record": REC}]),
    )
    .await;
    let uri = server.uri();
    let (code, v, err) = tokio::task::spawn_blocking(move || {
        run(
            uri,
            vec!["flow", "runs", "--record", "change_request:CHG0030421"],
        )
    })
    .await
    .unwrap();
    assert_eq!(code, 0, "{err}");
    assert_eq!(v[0]["source_record"], REC);
}

#[tokio::test(flavor = "current_thread")]
async fn a_dropped_lookup_term_is_not_mistaken_for_the_flow() {
    let server = MockServer::start().await;
    // The instance ignored the name filter: rows that do not carry the name.
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_hub_flow"))
        .respond_with(result(json!([flow_row("Some Other Flow", "other")])))
        .mount(&server)
        .await;
    let uri = server.uri();
    let (code, _, err) =
        tokio::task::spawn_blocking(move || run(uri, vec!["flow", "runs", "my_flow"]))
            .await
            .unwrap();
    assert_eq!(code, 2);
    assert!(err.contains("dropped the lookup term"), "{err}");
}

#[tokio::test(flavor = "current_thread")]
async fn an_ambiguous_flow_name_lists_the_candidates() {
    let server = MockServer::start().await;
    let mut other = flow_row("send_email", "send_email");
    other["sys_id"] = json!("ffffffffffffffffffffffffffffffff");
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_hub_flow"))
        .respond_with(result(json!([flow_row("Send Email", "send_email"), other])))
        .mount(&server)
        .await;
    let uri = server.uri();
    let (code, _, err) =
        tokio::task::spawn_blocking(move || run(uri, vec!["flow", "runs", "send_email"]))
            .await
            .unwrap();
    assert_eq!(code, 1);
    assert!(err.contains("ambiguous"), "{err}");
    assert!(err.contains("ffffffffffffffffffffffffffffffff"), "{err}");
}

#[test]
fn runs_argv_errors_never_reach_the_network() {
    // An unroutable instance: any request would fail with exit 3, not 1.
    let tmp = profile("http://127.0.0.1:9");
    for args in [
        vec!["flow", "runs"],
        vec!["flow", "runs", "x", "--since", "10w"],
        vec!["flow", "runs", "a^b"],
        vec!["flow", "runs", "--record", "not-a-ref"],
    ] {
        common::sn_cmd(tmp.path()).args(&args).assert().code(1);
    }
}

fn context(reporting: &str, state: &str) -> Value {
    json!([{"sys_id": CTX, "name": "Scheduled Document Collaboration", "state": state,
            "reporting": reporting, "flow": FLOW}])
}

#[tokio::test(flavor = "current_thread")]
async fn steps_explain_reporting_off_instead_of_an_empty_list() {
    let server = MockServer::start().await;
    mount_table(
        &server,
        "sys_flow_context",
        &format!("sys_id={CTX}"),
        context("OFF", "COMPLETE"),
    )
    .await;
    mount_table(
        &server,
        "sys_flow_report",
        &format!("context={CTX}^ORDERBYorder"),
        json!([]),
    )
    .await;
    let uri = server.uri();
    let (code, _, err) = tokio::task::spawn_blocking(move || run(uri, vec!["flow", "steps", CTX]))
        .await
        .unwrap();
    assert_eq!(code, 2);
    assert!(err.contains("reporting level OFF"), "{err}");
    assert!(
        err.contains("com.snc.process_flow.reporting.level"),
        "{err}"
    );
    assert!(
        !err.contains("status_code"),
        "no HTTP status to report: {err}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn steps_values_parses_the_json_columns() {
    let server = MockServer::start().await;
    mount_table(
        &server,
        "sys_flow_context",
        &format!("sys_id={CTX}"),
        context("FULL", "COMPLETE"),
    )
    .await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_flow_report"))
        .and(query_param("sysparm_query", format!("context={CTX}^ORDERBYorder")))
        .and(wiremock::matchers::query_param_contains(
            "sysparm_fields",
            ",input,output",
        ))
        .respond_with(result(json!([
            {"order": "1", "state": "COMPLETE", "input": "{\"table\":\"incident\"}", "output": "not json"}
        ])))
        .mount(&server)
        .await;
    let uri = server.uri();
    let (code, v, err) =
        tokio::task::spawn_blocking(move || run(uri, vec!["flow", "steps", CTX, "--values"]))
            .await
            .unwrap();
    assert_eq!(code, 0, "{err}");
    assert_eq!(v[0]["input"]["table"], "incident");
    assert_eq!(v[0]["output"], "not json");
}

#[tokio::test(flavor = "current_thread")]
async fn logs_filter_by_level_and_name_the_level() {
    let server = MockServer::start().await;
    mount_table(
        &server,
        "sys_flow_log",
        &format!("context={CTX}^levelIN1,2^ORDERBYorder"),
        json!([{"order": "1a0e6d54e250000001", "level": "2", "message": "*** Script: boom"}]),
    )
    .await;
    let uri = server.uri();
    let (code, v, err) =
        tokio::task::spawn_blocking(move || run(uri, vec!["flow", "logs", CTX, "--level", "warn"]))
            .await
            .unwrap();
    assert_eq!(code, 0, "{err}");
    assert_eq!(v[0]["level"], "error");
}

#[tokio::test(flavor = "current_thread")]
async fn debug_joins_context_failed_step_and_log_tail() {
    let server = MockServer::start().await;
    mount_table(
        &server,
        "sys_flow_context",
        &format!("sys_id={CTX}"),
        context("FULL", "ERROR"),
    )
    .await;
    mount_table(
        &server,
        "sys_flow_report",
        &format!("context={CTX}^ORDERBYorder"),
        json!([
            {"sys_id": "s1", "order": "1", "state": "COMPLETE"},
            {"sys_id": "s2", "order": "2", "state": "ERROR"}
        ]),
    )
    .await;
    mount_table(
        &server,
        "sys_flow_report",
        "sys_id=s2",
        json!([{"sys_id": "s2", "state": "ERROR", "error": "No record found", "input": "{\"sys_id\":\"x\"}"}]),
    )
    .await;
    // Newest first on the wire, oldest first in the document.
    mount_table(
        &server,
        "sys_flow_log",
        &format!("context={CTX}^ORDERBYDESCorder"),
        json!([
            {"order": "2", "level": "2", "message": "second"},
            {"order": "1", "level": "0", "message": "first"}
        ]),
    )
    .await;
    let uri = server.uri();
    let (code, v, err) = tokio::task::spawn_blocking(move || run(uri, vec!["flow", "debug", CTX]))
        .await
        .unwrap();
    assert_eq!(code, 0, "{err}");
    assert_eq!(v["context"]["state"], "ERROR");
    assert_eq!(v["failed_step"]["error"], "No record found");
    assert_eq!(v["failed_step"]["input"]["sys_id"], "x");
    assert_eq!(v["steps"]["total"], 2);
    assert_eq!(v["logs"][0]["message"], "first");
    assert_eq!(v["logs"][1]["level"], "error");
}

#[tokio::test(flavor = "current_thread")]
async fn debug_of_an_unreadable_context_says_so() {
    let server = MockServer::start().await;
    mount_table(
        &server,
        "sys_flow_context",
        &format!("sys_id={CTX}"),
        json!([]),
    )
    .await;
    let uri = server.uri();
    let (code, _, err) = tokio::task::spawn_blocking(move || run(uri, vec!["flow", "debug", CTX]))
        .await
        .unwrap();
    assert_eq!(code, 2);
    assert!(err.contains("not readable by this profile"), "{err}");
}

/// The published trigger of "Delegate Roles in Group", as read on the PDI.
async fn mount_why_not_base(server: &MockServer, condition: &str) {
    mount_table(
        server,
        "sys_hub_flow",
        &format!("sys_id={FLOW}"),
        json!([flow_row(
            "Delegate Roles in Group",
            "delegate_roles_in_group"
        )]),
    )
    .await;
    mount_table(
        server,
        "sys_flow_context",
        &format!("flow={FLOW}^source_record={REC}^ORDERBYDESCsys_created_on"),
        json!([]),
    )
    .await;
    mount_table(
        server,
        "sys_flow_trigger",
        &format!("sys_id={TRIGGER}"),
        json!([{"sys_id": TRIGGER, "sys_class_name": "sys_flow_record_trigger", "active": "true"}]),
    )
    .await;
    mount_table(
        server,
        "sys_flow_record_trigger",
        &format!("sys_id={TRIGGER}"),
        json!([{
            "sys_id": TRIGGER, "sys_class_name": "sys_flow_record_trigger", "active": "true",
            "table": "change_request", "condition": condition,
            "on_insert": "true", "on_update": "false", "on_delete": "false",
            "run_on_extended": "false", "run_when_setting": "both", "run_when_user_setting": "any"
        }]),
    )
    .await;
    mount_table(
        server,
        "change_request",
        &format!("sys_id={REC}"),
        json!([{"sys_id": REC, "sys_class_name": "change_request"}]),
    )
    .await;
}

fn stats(server: &MockServer, query: Option<&str>, n: u64) -> Mock {
    let _ = server;
    let m = Mock::given(method("GET")).and(path("/api/now/stats/change_request"));
    match query {
        Some(q) => m.and(query_param("sysparm_query", q)),
        None => m,
    }
    .respond_with(count(n))
}

#[tokio::test(flavor = "current_thread")]
async fn why_not_names_the_failing_clause() {
    let server = MockServer::start().await;
    let cond = "short_descriptionSTARTSWITHDelegate roles to^short_descriptionENDSWITHgroup";
    mount_why_not_base(&server, cond).await;
    stats(
        &server,
        Some(&format!(
            "short_descriptionSTARTSWITHDelegate roles to^sys_id={REC}"
        )),
        0,
    )
    .mount(&server)
    .await;
    stats(
        &server,
        Some(&format!("short_descriptionENDSWITHgroup^sys_id={REC}")),
        1,
    )
    .mount(&server)
    .await;
    stats(&server, Some("short_descriptionENDSWITHgroup"), 3)
        .mount(&server)
        .await;
    // The unfiltered total, mounted last so the filtered mocks win.
    stats(&server, None, 325).mount(&server).await;

    let uri = server.uri();
    let (code, v, err) = tokio::task::spawn_blocking(move || {
        run(
            uri,
            vec![
                "flow",
                "why-not",
                FLOW,
                "--record",
                "change_request:d6c3c43e2fce4f507efd1d707fa4e35b",
            ],
        )
    })
    .await
    .unwrap();
    assert_eq!(code, 0, "{err}");
    assert_eq!(v["condition"]["matches"], false);
    let clauses = &v["condition"]["segments"][0]["clauses"];
    assert_eq!(clauses[0]["matches"], false);
    assert_eq!(clauses[1]["matches"], true);
    assert!(clauses[1].get("note").is_none(), "{clauses}");
    let blockers = v["blockers"].as_array().unwrap();
    assert_eq!(blockers.len(), 1, "{blockers:?}");
    assert!(
        blockers[0]
            .as_str()
            .unwrap()
            .contains("condition does not match")
    );
    let notes = v["notes"].to_string();
    assert!(notes.contains("fires on insert only"), "{notes}");
}

#[tokio::test(flavor = "current_thread")]
async fn why_not_flags_change_operators_and_suspect_clauses() {
    let server = MockServer::start().await;
    mount_why_not_base(&server, "stateCHANGESTO3^bogus_field=1").await;
    stats(&server, Some(&format!("bogus_field=1^sys_id={REC}")), 1)
        .mount(&server)
        .await;
    // Matching every row is the signature of a silently dropped term.
    stats(&server, Some("bogus_field=1"), 325)
        .mount(&server)
        .await;
    stats(&server, None, 325).mount(&server).await;

    let uri = server.uri();
    let (code, v, err) = tokio::task::spawn_blocking(move || {
        run(
            uri,
            vec![
                "flow",
                "why-not",
                FLOW,
                "--record",
                "change_request:d6c3c43e2fce4f507efd1d707fa4e35b",
            ],
        )
    })
    .await
    .unwrap();
    assert_eq!(code, 0, "{err}");
    assert!(v["condition"]["matches"].is_null(), "{v}");
    let clauses = &v["condition"]["segments"][0]["clauses"];
    assert!(clauses[0]["matches"].is_null());
    assert!(clauses[0]["note"].as_str().unwrap().contains("CHANGESTO"));
    assert!(
        clauses[1]["note"]
            .as_str()
            .unwrap()
            .contains("silently dropped")
    );
    assert!(v["blockers"].as_array().unwrap().is_empty(), "{v}");
}

#[test]
fn why_not_requires_a_table_reference() {
    let tmp = profile("http://127.0.0.1:9");
    common::sn_cmd(tmp.path())
        .args(["flow", "why-not", FLOW, "--record", REC])
        .assert()
        .code(1);
}
