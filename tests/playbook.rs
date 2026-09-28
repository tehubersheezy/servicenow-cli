//! `sn playbook` — the Playbook Experience GraphQL API (`snPlaybookExp`) end
//! to end: documents and variables on the wire, the union/typed-error mapping,
//! the `null`-means-unreadable list answer, the not-installed error, and the
//! argv guards. Response bodies are the shapes measured live on dev421992.

mod common;

use serde_json::{Value, json};
use wiremock::matchers::{body_partial_json, body_string_contains, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const REC: &str = "3f4bd2ea2f2703107efd1d707fa4e3fe";
const CTX_TYPE: &str = "snPlaybookExp_playbook_PlaybookContext";
const ERR_TYPE: &str = "snPlaybookExp_playbook_TriggerPlaybookError";

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

fn stdout_json(out: &assert_cmd::assert::Assert) -> Value {
    serde_json::from_slice(&out.get_output().stdout).unwrap()
}

fn stderr_json(out: &assert_cmd::assert::Assert) -> Value {
    serde_json::from_slice(&out.get_output().stderr).unwrap()
}

fn run(server_uri: String, args: Vec<&'static str>, code: i32) -> (Value, Value) {
    let tmp = profile(&server_uri);
    let out = common::sn_cmd(tmp.path())
        .args(["--compact"])
        .args(&args)
        .assert()
        .code(code);
    let stdout = if out.get_output().stdout.is_empty() {
        Value::Null
    } else {
        stdout_json(&out)
    };
    let stderr = if out.get_output().stderr.is_empty() {
        Value::Null
    } else {
        stderr_json(&out)
    };
    (stdout, stderr)
}

#[tokio::test(flavor = "current_thread")]
async fn list_sends_variables_and_emits_the_executions() {
    let server = MockServer::start().await;
    let execution = json!({
        "can_read": true, "cancellation_reason": "",
        "playbook_id": "3ee4733177b73110c2123a91fa5a99f6",
        "scoped_name": "sn_vsc.task_steps_5",
        "state": {"displayValue": "In Progress", "value": "IN_PROGRESS"},
        "sys_id": "90052af6588f461c96f389a43a9bcddc", "title": "Task steps"
    });
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .and(body_string_contains("getPlaybooksForParentRecord"))
        .and(body_partial_json(
            json!({"variables": {"table": "incident", "record": REC}}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {"snPlaybookExp": {"playbook": {"getPlaybooksForParentRecord": [execution]}}}
        })))
        .expect(1)
        .mount(&server)
        .await;
    let uri = server.uri();
    let (out, _) =
        tokio::task::spawn_blocking(move || run(uri, vec!["playbook", "list", "incident", REC], 0))
            .await
            .unwrap();
    assert_eq!(out, json!([execution]));
}

#[tokio::test(flavor = "current_thread")]
async fn list_resolves_a_number_reference_first() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_query", "number=INC0010052"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"result": [{"sys_id": REC}]})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .and(body_partial_json(json!({"variables": {"record": REC}})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {"snPlaybookExp": {"playbook": {"getPlaybooksForParentRecord": []}}}
        })))
        .expect(1)
        .mount(&server)
        .await;
    let uri = server.uri();
    let (out, _) = tokio::task::spawn_blocking(move || {
        run(uri, vec!["playbook", "list", "incident:INC0010052"], 0)
    })
    .await
    .unwrap();
    assert_eq!(out, json!([]));
}

#[tokio::test(flavor = "current_thread")]
async fn list_null_is_exit_2_hedged_with_no_status() {
    // The resolver reads the parent through GlideRecordSecure and answers
    // null for a record it cannot see — missing and unreadable are one shape.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {"snPlaybookExp": {"playbook": {"getPlaybooksForParentRecord": null}}}
        })))
        .mount(&server)
        .await;
    let uri = server.uri();
    let (out, err) =
        tokio::task::spawn_blocking(move || run(uri, vec!["playbook", "list", "incident", REC], 2))
            .await
            .unwrap();
    assert_eq!(out, Value::Null, "nothing on stdout");
    let msg = err["error"]["message"].as_str().unwrap();
    assert!(msg.contains("not readable by this profile"), "{msg}");
    assert!(err["error"].get("status_code").is_none(), "{err}");
}

#[tokio::test(flavor = "current_thread")]
async fn missing_schema_names_the_application() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "errors": [{
                "errorType": "ValidationError",
                "message": "Validation error (FieldUndefined@[snPlaybookExp]) : Field 'snPlaybookExp' in type 'QueryType' is undefined",
                "validationErrorType": "FieldUndefined"
            }]
        })))
        .mount(&server)
        .await;
    let uri = server.uri();
    let (_, err) =
        tokio::task::spawn_blocking(move || run(uri, vec!["playbook", "list", "incident", REC], 2))
            .await
            .unwrap();
    assert!(
        err["error"]["message"]
            .as_str()
            .unwrap()
            .contains("no Playbook Experience GraphQL API"),
        "{err}"
    );
    assert!(
        err["error"]["detail"]
            .as_str()
            .unwrap()
            .contains("sn_playbook_exp"),
        "{err}"
    );
    assert_eq!(err["error"]["status_code"], 200);
}

#[tokio::test(flavor = "current_thread")]
async fn trigger_selects_namespaced_union_members_and_reports_the_start() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .and(body_string_contains(format!("... on {CTX_TYPE}")))
        .and(body_string_contains(format!("... on {ERR_TYPE}")))
        .and(body_partial_json(json!({"variables": {
            "scopedName": "sn_vsc.task_steps_5", "table": "incident",
            "record": REC, "onlyIfNone": false
        }})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {"snPlaybookExp": {"playbook": {"triggerPlaybook": {
                "__typename": CTX_TYPE, "sys_id": "55e1613a25984b9585444f66668f5fe0",
                "parent_table": "incident", "parent_record": REC
            }}}}
        })))
        .expect(1)
        .mount(&server)
        .await;
    let uri = server.uri();
    let (out, _) = tokio::task::spawn_blocking(move || {
        run(
            uri,
            vec![
                "playbook",
                "trigger",
                "incident",
                REC,
                "--scoped-name",
                "sn_vsc.task_steps_5",
            ],
            0,
        )
    })
    .await
    .unwrap();
    assert_eq!(
        out,
        json!({"triggered": true, "sys_id": "55e1613a25984b9585444f66668f5fe0",
               "parent_table": "incident", "parent_record": REC})
    );
}

#[tokio::test(flavor = "current_thread")]
async fn only_if_none_skip_is_triggered_false_exit_0() {
    // Measured: a skip answers the context member with a null sys_id.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .and(body_partial_json(
            json!({"variables": {"onlyIfNone": true}}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {"snPlaybookExp": {"playbook": {"triggerPlaybook": {
                "__typename": CTX_TYPE, "sys_id": null,
                "parent_table": "incident", "parent_record": REC
            }}}}
        })))
        .expect(1)
        .mount(&server)
        .await;
    let uri = server.uri();
    let (out, _) = tokio::task::spawn_blocking(move || {
        run(
            uri,
            vec![
                "playbook",
                "trigger",
                "incident",
                REC,
                "--scoped-name",
                "sn_vsc.task_steps_4",
                "--only-if-none",
            ],
            0,
        )
    })
    .await
    .unwrap();
    assert_eq!(out["triggered"], false);
    assert_eq!(out["sys_id"], Value::Null);
}

#[tokio::test(flavor = "current_thread")]
async fn typed_error_member_is_exit_2_with_error_type_in_sn_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {"snPlaybookExp": {"playbook": {"triggerPlaybook": {
                "__typename": ERR_TYPE,
                "errorType": "TRIGGER_PLAYBOOK_FAILED",
                "message": "com.snc.pd.validator.PDValidationException: Process Definition 'sn_nope.none' is missing or inactive",
                "parent_table": "incident", "parent_record": REC,
                "process_definition_id": null, "input": null
            }}}}
        })))
        .mount(&server)
        .await;
    let uri = server.uri();
    let (out, err) = tokio::task::spawn_blocking(move || {
        run(
            uri,
            vec![
                "playbook",
                "trigger",
                "incident",
                REC,
                "--scoped-name",
                "sn_nope.none",
            ],
            2,
        )
    })
    .await
    .unwrap();
    assert_eq!(out, Value::Null, "nothing on stdout");
    assert_eq!(
        err["error"]["sn_error"]["errorType"],
        "TRIGGER_PLAYBOOK_FAILED"
    );
    assert!(
        err["error"]["sn_error"].get("input").is_none(),
        "unset fields dropped: {err}"
    );
    assert_eq!(err["error"]["status_code"], 200);
    assert!(
        err["error"]["message"]
            .as_str()
            .unwrap()
            .contains("missing or inactive")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn launch_double_encodes_inputs_and_reports_the_start() {
    let server = MockServer::start().await;
    // `a&b` → encodeURIComponent twice → `a%2526b`: the resolver's
    // whole-string decode leaves `a%26b`, its per-part decode restores `a&b`.
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .and(body_string_contains("launchPlaybook(processDefinitionId"))
        .and(body_partial_json(json!({"variables": {
            "definition": "3ee4733177b73110c2123a91fa5a99f6",
            "record": REC,
            "input": "note=a%2526b&who=me"
        }})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {"snPlaybookExp": {"playbook": {"launchPlaybook": {
                "__typename": CTX_TYPE, "sys_id": "aaaa1111bbbb2222cccc3333dddd4444",
                "parent_table": "incident", "parent_record": REC
            }}}}
        })))
        .expect(1)
        .mount(&server)
        .await;
    let uri = server.uri();
    let (out, _) = tokio::task::spawn_blocking(move || {
        run(
            uri,
            vec![
                "playbook",
                "launch",
                "3ee4733177b73110c2123a91fa5a99f6",
                "--record",
                REC,
                "--input",
                "note=a&b",
                "--input",
                "who=me",
            ],
            0,
        )
    })
    .await
    .unwrap();
    assert_eq!(out["launched"], true);
    assert_eq!(out["sys_id"], "aaaa1111bbbb2222cccc3333dddd4444");
}

#[tokio::test(flavor = "current_thread")]
async fn launch_resolver_exception_names_the_parent_record_input() {
    // Measured: every definition on the reference instance lacks a
    // parent_record input, and launchPlaybook then throws in its resolver.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {"snPlaybookExp": {"playbook": {"launchPlaybook": null}}},
            "errors": [{
                "errorType": "DataFetchingException",
                "locations": [{"column": 7, "line": 4}],
                "message": "Error occurred while executing the resolver",
                "path": ["snPlaybookExp", "playbook", "launchPlaybook"]
            }]
        })))
        .mount(&server)
        .await;
    let uri = server.uri();
    let (_, err) = tokio::task::spawn_blocking(move || {
        run(
            uri,
            vec!["playbook", "launch", "3ee4733177b73110c2123a91fa5a99f6"],
            2,
        )
    })
    .await
    .unwrap();
    assert!(
        err["error"]["detail"]
            .as_str()
            .unwrap()
            .contains("parent_record"),
        "{err}"
    );
    assert_eq!(
        err["error"]["sn_error"][0]["errorType"],
        "DataFetchingException"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn output_raw_keeps_the_graphql_envelope() {
    let server = MockServer::start().await;
    let body = json!({
        "data": {"snPlaybookExp": {"playbook": {"getPlaybooksForParentRecord": []}}}
    });
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body.clone()))
        .mount(&server)
        .await;
    let uri = server.uri();
    let (out, _) = tokio::task::spawn_blocking(move || {
        run(
            uri,
            vec!["--output", "raw", "playbook", "list", "incident", REC],
            0,
        )
    })
    .await
    .unwrap();
    assert_eq!(out, body);
}

#[tokio::test(flavor = "current_thread")]
async fn argv_guards_fail_before_the_network() {
    // No mock is mounted: a request reaching the server would 404 and exit
    // 2, so exit 1 proves the guard fired first.
    let server = MockServer::start().await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&uri);
        for args in [
            // A reference and a second positional.
            vec!["playbook", "list", "incident:INC0010052", REC],
            // No sys_id and no reference.
            vec!["playbook", "list", "incident"],
            // --scoped-name is required.
            vec!["playbook", "trigger", "incident", REC],
            vec!["playbook", "trigger", "incident", REC, "--scoped-name", " "],
            // Malformed input pair.
            vec![
                "playbook",
                "launch",
                "3ee4733177b73110c2123a91fa5a99f6",
                "--input",
                "novalue",
            ],
            // A definition id that is not an id.
            vec!["playbook", "launch", "not an id"],
        ] {
            common::sn_cmd(tmp.path()).args(&args).assert().code(1);
        }
    })
    .await
    .unwrap();
    assert!(server.received_requests().await.unwrap().is_empty());
}
