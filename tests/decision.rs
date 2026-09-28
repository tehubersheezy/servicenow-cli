//! `sn decision` against a mocked instance. Response shapes are the ones
//! measured live (see the module doc of `src/cli/decision.rs`).

mod common;

use serde_json::{Value, json};
use wiremock::matchers::{body_partial_json, body_string_contains, method, path, query_param};
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

const DT: &str = "253ffb31ff3a221009d9ffffffffff4e";
const ROW: &str = "635df3c4ff232210c419ffffffffff25";
const DEFAULT_ROW: &str = "675df3c4ff232210c419ffffffffff2b";

fn header_row() -> Value {
    json!({
        "sys_id": DT,
        "name": "Deployment Migration to ReleaseOps",
        "description": "",
        "answer_table": "sys_decision_multi_result",
        "active": "true",
        "sys_scope.scope": "sn_deploy_pipeline",
    })
}

fn decision(sys_id: &str, label: &str, condition: &str, value: &str) -> Value {
    json!({
        "sys_id": sys_id, "label": label, "order": 100, "active": true,
        "condition": condition,
        "answer": {
            "value": "b7c2c802ff7a221009d9ffffffffff56",
            "displayValue": format!("Decision Table Multiple Result: Use ReleaseOps: {value}"),
            "answerElementValues": [{"name": "use_releaseops", "value": value, "displayValue": value}]
        }
    })
}

fn input(element: &str, kind: &str, mandatory: bool, reference: &str) -> Value {
    json!({
        "sys_id": {"value": format!("{element}_id")}, "element": {"value": element},
        "label": {"value": element}, "internal_type": {"value": kind},
        "mandatory": {"value": mandatory}, "active": {"value": true},
        "order": {"value": 100}, "reference": {"value": reference}, "choices": []
    })
}

fn definition(inputs: Vec<Value>) -> Value {
    json!({"data": {
        "snDtableDesigner": {
            "decisionInput": {"getDecisionInputsByDecisionTable": inputs},
            "decisionCondition": {"getDecisionConditionsByDecisionTable": []},
            "decision": {
                "rows": [decision(ROW, "rule", "releaseops_plugin_is_installed=true^EQ", "true")],
                "fallback": [decision(DEFAULT_ROW, "default result", "", "false")]
            }
        },
        "snDecisionTable": {"answerElement": {"getAnswerElementsOfDecisionTable": [{
            "sys_id": {"value": "e1"}, "element": {"value": "use_releaseops"},
            "label": {"value": "Use ReleaseOps"}, "internal_type": {"value": "boolean"},
            "order": {"value": 100}
        }]}}
    }})
}

async fn mount_header_by_name(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_decision"))
        .and(query_param(
            "sysparm_query",
            "name=deployment migration to releaseops",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": [header_row()]})))
        .mount(server)
        .await;
}

async fn mount_definition(server: &MockServer, body: Value) {
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .and(body_string_contains("getDecisionsByDecisionTable"))
        .and(body_partial_json(json!({"variables": {"id": DT}})))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

fn stdout_json(out: &assert_cmd::assert::Assert) -> Value {
    serde_json::from_slice(&out.get_output().stdout).unwrap()
}

fn stderr_json(out: &assert_cmd::assert::Assert) -> Value {
    serde_json::from_slice(&out.get_output().stderr).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn list_orders_by_name_after_the_callers_query() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_decision"))
        .and(query_param("sysparm_query", "active=true^ORDERBYname"))
        .and(query_param("sysparm_limit", "100"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": [{"name": "A"}]})))
        .expect(1)
        .mount(&server)
        .await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&uri);
        let out = common::sn_cmd(tmp.path())
            .args(["decision", "list", "-q", "active=true"])
            .assert()
            .success();
        assert_eq!(stdout_json(&out), json!([{"name": "A"}]));
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn show_composes_header_inputs_and_decisions_in_evaluation_order() {
    let server = MockServer::start().await;
    mount_header_by_name(&server).await;
    mount_definition(
        &server,
        definition(vec![input(
            "releaseops_plugin_is_installed",
            "boolean",
            true,
            "",
        )]),
    )
    .await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&uri);
        let out = common::sn_cmd(tmp.path())
            .args(["decision", "show", "deployment migration to releaseops"])
            .assert()
            .success();
        let v = stdout_json(&out);
        assert_eq!(v["sys_id"], DT);
        assert_eq!(v["multi_result"], true);
        assert_eq!(v["scope"], "sn_deploy_pipeline");
        assert_eq!(v["inputs"][0]["name"], "releaseops_plugin_is_installed");
        assert_eq!(v["inputs"][0]["mandatory"], true);
        assert_eq!(v["answer_elements"][0]["name"], "use_releaseops");
        let decisions = v["decisions"].as_array().unwrap();
        assert_eq!(decisions.len(), 2);
        assert_eq!(decisions[0]["sys_id"], ROW);
        assert_eq!(decisions[0]["default"], false);
        assert_eq!(decisions[1]["sys_id"], DEFAULT_ROW);
        assert_eq!(decisions[1]["default"], true);
        assert_eq!(
            decisions[0]["answer"]["elements"]["use_releaseops"],
            json!({"value": "true", "display_value": "true"})
        );
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn show_reports_null_resolvers_as_missing_access() {
    let server = MockServer::start().await;
    mount_header_by_name(&server).await;
    mount_definition(
        &server,
        json!({"data": {
            "snDtableDesigner": {
                "decisionInput": {"getDecisionInputsByDecisionTable": null},
                "decisionCondition": {"getDecisionConditionsByDecisionTable": null},
                "decision": {"rows": null, "fallback": null}
            },
            "snDecisionTable": {"answerElement": {"getAnswerElementsOfDecisionTable": null}}
        }}),
    )
    .await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&uri);
        let out = common::sn_cmd(tmp.path())
            .args(["decision", "show", "deployment migration to releaseops"])
            .assert()
            .code(2);
        let e = stderr_json(&out);
        assert!(
            e["error"]["message"]
                .as_str()
                .unwrap()
                .contains("decision_table_reader"),
            "{e}"
        );
        assert!(out.get_output().stdout.is_empty());
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn show_names_the_missing_plugin() {
    let server = MockServer::start().await;
    mount_header_by_name(&server).await;
    mount_definition(
        &server,
        json!({"errors": [{
            "message": "Validation error (FieldUndefined@[snDtableDesigner]) : Field 'snDtableDesigner' in type 'QueryType' is undefined",
            "errorType": "ValidationError"
        }]}),
    )
    .await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&uri);
        let out = common::sn_cmd(tmp.path())
            .args(["decision", "show", "deployment migration to releaseops"])
            .assert()
            .code(2);
        let e = stderr_json(&out);
        assert!(
            e["error"]["message"]
                .as_str()
                .unwrap()
                .contains("sn_decision_table"),
            "{e}"
        );
        assert_eq!(e["error"]["status_code"], 200);
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn show_refuses_output_raw_before_the_network() {
    // No mocks: any request would 404 and fail the exit-code assertion.
    let server = MockServer::start().await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&uri);
        common::sn_cmd(tmp.path())
            .args(["--output", "raw", "decision", "show", DT])
            .assert()
            .code(1);
    })
    .await
    .unwrap();
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn run_refuses_an_unknown_input_without_evaluating() {
    let server = MockServer::start().await;
    mount_header_by_name(&server).await;
    mount_definition(
        &server,
        definition(vec![input(
            "releaseops_plugin_is_installed",
            "boolean",
            false,
            "",
        )]),
    )
    .await;
    Mock::given(method("POST"))
        .and(path(
            "/api/sn_decision_table/generic/evaluate_decision_table",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": {}})))
        .expect(0)
        .mount(&server)
        .await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&uri);
        let out = common::sn_cmd(tmp.path())
            .args([
                "decision",
                "run",
                "deployment migration to releaseops",
                "--input",
                "releaseops_plugin_installed=true",
            ])
            .assert()
            .code(1);
        let msg = stderr_json(&out)["error"]["message"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(msg.contains("releaseops_plugin_is_installed"), "{msg}");
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn run_evaluates_and_labels_the_match() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/api/now/table/sys_decision/{DT}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": header_row()})))
        .mount(&server)
        .await;
    mount_definition(
        &server,
        definition(vec![input(
            "releaseops_plugin_is_installed",
            "boolean",
            true,
            "",
        )]),
    )
    .await;
    Mock::given(method("POST"))
        .and(path(
            "/api/sn_decision_table/generic/evaluate_decision_table",
        ))
        .and(query_param("decisionTableId", DT))
        .and(body_partial_json(json!({
            "testInputs": {"releaseops_plugin_is_installed": "false"},
            "isFirstMatch": true,
            "isMultiResultTable": true,
            "answerElementNames": ["use_releaseops"]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": {
            DEFAULT_ROW: [{"name": "use_releaseops", "value": "false", "displayValue": "false"}]
        }})))
        .expect(1)
        .mount(&server)
        .await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&uri);
        let out = common::sn_cmd(tmp.path())
            .args([
                "decision",
                "run",
                DT,
                "-i",
                "releaseops_plugin_is_installed=false",
            ])
            .assert()
            .success();
        let v = stdout_json(&out);
        assert_eq!(v["sys_id"], DT);
        let m = &v["matches"][0];
        assert_eq!(m["sys_id"], DEFAULT_ROW);
        assert_eq!(m["default"], true);
        assert_eq!(m["label"], "default result");
        assert_eq!(
            m["answer"],
            json!({"elements": {"use_releaseops": {"value": "false", "display_value": "false"}}})
        );
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn run_resolves_a_reference_number_to_its_sys_id() {
    let chg = "46cb2f54a9fe198101cf6814a2754606";
    let server = MockServer::start().await;
    mount_header_by_name(&server).await;
    mount_definition(
        &server,
        definition(vec![input(
            "change_request",
            "reference",
            false,
            "change_request",
        )]),
    )
    .await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/change_request"))
        .and(query_param("sysparm_query", "number=CHG0000008"))
        .and(query_param("sysparm_limit", "2"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"result": [{"sys_id": chg}]})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(
            "/api/sn_decision_table/generic/evaluate_decision_table",
        ))
        .and(body_partial_json(
            json!({"testInputs": {"change_request": chg}}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": {}})))
        .expect(1)
        .mount(&server)
        .await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&uri);
        let out = common::sn_cmd(tmp.path())
            .args([
                "decision",
                "run",
                "deployment migration to releaseops",
                "-i",
                "change_request=CHG0000008",
            ])
            .assert()
            .success();
        let v = stdout_json(&out);
        assert_eq!(v["inputs"]["change_request"], chg);
        assert_eq!(v["resolved_from"]["change_request"], "CHG0000008");
        assert_eq!(v["matches"], json!([]));
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn an_ambiguous_name_asks_for_a_sys_id() {
    let server = MockServer::start().await;
    let mut other = header_row();
    other["sys_id"] = json!("ffffffffffffffffffffffffffffffff");
    other["sys_scope.scope"] = json!("global");
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_decision"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"result": [header_row(), other]})),
        )
        .mount(&server)
        .await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&uri);
        let out = common::sn_cmd(tmp.path())
            .args(["decision", "show", "Deployment Migration to ReleaseOps"])
            .assert()
            .code(1);
        let msg = stderr_json(&out)["error"]["message"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(msg.contains(DT) && msg.contains("ffffffff"), "{msg}");
    })
    .await
    .unwrap();
}
