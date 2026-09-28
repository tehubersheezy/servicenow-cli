//! `sn flow list/get/versions` against a mocked instance. Response shapes are
//! the ones measured live on dev421992 (issue #48): `processflow` wraps its
//! answer as `{"result": {"data", "errorCode", "errorMessage", ...}}` and
//! reports a missing flow as a 404 whose body is that same envelope.

mod common;

use serde_json::{Value, json};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const FLOW: &str = "7d124cc7b7e8f2107df5c0cd2e11a9f6";

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

fn envelope(data: Value) -> Value {
    json!({"result": {
        "data": data,
        "errorCode": 0,
        "errorMessage": "",
        "integrationsPluginActive": false
    }})
}

fn model() -> Value {
    json!({
        "id": FLOW,
        "name": "AIAM - Create impacted asset tasks for change",
        "internalName": "aiam__create",
        "type": "flow",
        "status": "published",
        "scopeName": "sn_ai_asset_mgmt",
        "triggerInstances": [{"name": "Created or Updated", "type": "record_create_or_update",
            "triggerType": "Record", "inputs": [{"name": "table", "value": "change_request"}]}],
        "flowLogicInstances": [{"order": "1", "name": "If: x", "parent": "", "uiUniqueIdentifier": "u1", "id": "l1"}],
        "actionInstances": [{"order": "2", "name": "Update Record", "internalName": "update_record",
            "parent": "u1", "uiUniqueIdentifier": "u2", "id": "a2"}],
        "subFlowInstances": [],
        "inputs": [],
        "outputs": []
    })
}

/// Run `sn` with `args` against a profile pointing at `server_uri`, returning
/// (exit code, stdout, stderr).
fn run(server_uri: &str, args: &[&str]) -> (i32, String, String) {
    let tmp = profile(server_uri);
    let out = common::sn_cmd(tmp.path())
        .args(args)
        .output()
        .expect("run sn");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

fn args(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

async fn run_async(server: &MockServer, a: &[&str]) -> (i32, String, String) {
    let uri = server.uri();
    let a = args(a);
    tokio::task::spawn_blocking(move || {
        let refs: Vec<&str> = a.iter().map(String::as_str).collect();
        run(&uri, &refs)
    })
    .await
    .unwrap()
}

fn err_json(stderr: &str) -> Value {
    serde_json::from_str(stderr.trim()).unwrap_or_else(|_| panic!("stderr not JSON: {stderr}"))
}

#[tokio::test(flavor = "current_thread")]
async fn list_filters_sorts_and_pins_raw_values() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_hub_flow"))
        .and(query_param(
            "sysparm_query",
            "sys_scope.scope=global^type=subflow^active=true^nameLIKEmail^ORDERBYname",
        ))
        .and(query_param("sysparm_display_value", "false"))
        .and(query_param("sysparm_exclude_reference_link", "true"))
        .and(query_param("sysparm_limit", "5"))
        .and(query_param(
            "sysparm_fields",
            "sys_id,name,internal_name,type,status,active,sys_scope.scope,sys_updated_on,sys_updated_by",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": [
            {"sys_id": "cb9e", "internal_name": "send_email", "sys_scope.scope": "global"}
        ]})))
        .expect(1)
        .mount(&server)
        .await;
    let (code, out, err) = run_async(
        &server,
        &[
            "--compact",
            "flow",
            "list",
            "--scope",
            "global",
            "--type",
            "subflow",
            "--active",
            "-q",
            "nameLIKEmail",
            "--limit",
            "5",
        ],
    )
    .await;
    assert_eq!(code, 0, "stderr: {err}");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v[0]["internal_name"], "send_email");
}

#[tokio::test(flavor = "current_thread")]
async fn get_by_sys_id_emits_the_model_and_raw_keeps_the_envelope() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/api/now/processflow/flow/{FLOW}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(envelope(model())))
        .expect(2)
        .mount(&server)
        .await;
    let (code, out, err) = run_async(&server, &["--compact", "flow", "get", FLOW]).await;
    assert_eq!(code, 0, "stderr: {err}");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["id"], FLOW);
    assert!(
        v.get("errorCode").is_none(),
        "default output is result.data"
    );

    let (code, out, _) = run_async(
        &server,
        &["--compact", "--output", "raw", "flow", "get", FLOW],
    )
    .await;
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["result"]["data"]["id"], FLOW);
    assert_eq!(v["result"]["errorCode"], 0);
}

#[tokio::test(flavor = "current_thread")]
async fn get_by_scoped_name_resolves_then_outlines() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_hub_flow"))
        .and(query_param(
            "sysparm_query",
            "internal_name=aiam__create^sys_scope.scope=sn_ai_asset_mgmt",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": [
            {"sys_id": FLOW, "internal_name": "aiam__create", "sys_scope.scope": "sn_ai_asset_mgmt"}
        ]})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/api/now/processflow/flow/{FLOW}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(envelope(model())))
        .expect(1)
        .mount(&server)
        .await;
    let (code, out, err) = run_async(
        &server,
        &[
            "--compact",
            "flow",
            "get",
            "sn_ai_asset_mgmt.aiam__create",
            "--outline",
        ],
    )
    .await;
    assert_eq!(code, 0, "stderr: {err}");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["internal_name"], "aiam__create");
    assert_eq!(v["triggers"][0]["inputs"]["table"], "change_request");
    assert_eq!(v["steps"][1]["depth"], 1);
    assert_eq!(v["steps"][1]["parent"], 1);
    assert_eq!(v["steps"][1]["kind"], "action");
}

#[tokio::test(flavor = "current_thread")]
async fn an_ambiguous_name_is_a_usage_error_and_never_reads_a_model() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_hub_flow"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": [
            {"sys_id": "a1", "internal_name": "send_email", "sys_scope.scope": "sn_creatorstudio"},
            {"sys_id": "b2", "internal_name": "send_email", "sys_scope.scope": "global"}
        ]})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/processflow/flow/a1"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let (code, _, err) = run_async(&server, &["flow", "get", "send_email"]).await;
    assert_eq!(code, 1);
    let msg = err_json(&err)["error"]["message"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(msg.contains("global.send_email (b2)"), "{msg}");
}

#[tokio::test(flavor = "current_thread")]
async fn a_malformed_reference_fails_before_the_network() {
    let server = MockServer::start().await;
    let (code, _, err) = run_async(&server, &["flow", "get", "send_email^active=true"]).await;
    assert_eq!(code, 1, "stderr: {err}");
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn a_processflow_404_reports_its_own_message() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/api/now/processflow/flow/{FLOW}")))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"result": {
            "errorMessage": format!("Flow {FLOW} not found."),
            "errorCode": 0,
            "integrationsPluginActive": false
        }})))
        .mount(&server)
        .await;
    let (code, _, err) = run_async(&server, &["flow", "get", FLOW]).await;
    assert_eq!(code, 2);
    let e = err_json(&err);
    assert_eq!(e["error"]["message"], format!("Flow {FLOW} not found."));
    assert_eq!(e["error"]["status_code"], 404);
}

#[tokio::test(flavor = "current_thread")]
async fn an_in_band_error_under_200_exits_2_with_nothing_on_stdout() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/api/now/processflow/flow/{FLOW}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": {
            "data": null, "errorCode": 1, "errorMessage": "Something failed",
            "integrationsPluginActive": false
        }})))
        .mount(&server)
        .await;
    let (code, out, err) = run_async(&server, &["flow", "get", FLOW]).await;
    assert_eq!(code, 2);
    assert!(out.is_empty(), "stdout: {out}");
    let e = err_json(&err);
    assert_eq!(e["error"]["message"], "Something failed");
    assert_eq!(e["error"]["status_code"], 200);
}

#[tokio::test(flavor = "current_thread")]
async fn empty_history_for_an_unknown_sys_id_is_not_found() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/api/now/processflow/versioning/{FLOW}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(envelope(json!([]))))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/api/now/table/sys_hub_flow_base/{FLOW}")))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"error": {
            "message": "No Record found",
            "detail": "Record doesn't exist or ACL restricts the record retrieval"
        }})))
        .expect(1)
        .mount(&server)
        .await;
    let (code, out, err) = run_async(&server, &["flow", "versions", FLOW]).await;
    assert_eq!(code, 2, "stdout: {out}");
    let e = err_json(&err);
    assert!(
        e["error"]["message"]
            .as_str()
            .unwrap()
            .starts_with(&format!("no flow with sys_id {FLOW}")),
        "{e}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn empty_history_for_a_real_flow_is_an_empty_array() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/api/now/processflow/versioning/{FLOW}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(envelope(json!([]))))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/api/now/table/sys_hub_flow_base/{FLOW}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": {"sys_id": FLOW}})))
        .expect(1)
        .mount(&server)
        .await;
    let (code, out, err) = run_async(&server, &["--compact", "flow", "versions", FLOW]).await;
    assert_eq!(code, 0, "stderr: {err}");
    assert_eq!(out.trim(), "[]");
}

#[tokio::test(flavor = "current_thread")]
async fn versions_with_history_skip_the_existence_check() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/api/now/processflow/versioning/{FLOW}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(envelope(json!([
            {"id": "v1", "flowId": FLOW, "type": "update", "createdBy": "admin"}
        ]))))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/api/now/table/sys_hub_flow_base/{FLOW}")))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let (code, out, err) = run_async(&server, &["--compact", "flow", "versions", FLOW]).await;
    assert_eq!(code, 0, "stderr: {err}");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v[0]["id"], "v1");
}

#[tokio::test(flavor = "current_thread")]
async fn outline_with_raw_output_is_refused_before_the_network() {
    let server = MockServer::start().await;
    let (code, _, _) = run_async(
        &server,
        &["--output", "raw", "flow", "get", FLOW, "--outline"],
    )
    .await;
    assert_eq!(code, 1);
    assert!(server.received_requests().await.unwrap().is_empty());
}
