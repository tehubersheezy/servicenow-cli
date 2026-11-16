//! `sn appmanager` against a mocked instance. Response shapes are the ones
//! measured live (see the module doc of `src/cli/app_manager.rs`).

mod common;

use serde_json::{Value, json};
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const APPS: &str = "/api/sn_appclient/appmanager/apps";
const PLUGINS: &str = "/api/sn_appclient/appmanager/plugins";
const FLOW_ID: &str = "801cef500b312200c438ee6537673a04";

struct Ran {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Ran {
    fn json(&self) -> Value {
        serde_json::from_str(&self.stdout).unwrap_or_else(|e| panic!("{e}: {}", self.stdout))
    }

    fn error(&self) -> Value {
        let line = self.stderr.lines().last().unwrap_or_default();
        serde_json::from_str::<Value>(line).unwrap_or_else(|e| panic!("{e}: {}", self.stderr))["error"]
            .clone()
    }
}

/// Run `sn appmanager <args>` (or any argv) against the mock, off the async
/// runtime: the binary's client is blocking.
async fn sn(server: &MockServer, args: &[&str]) -> Ran {
    let uri = server.uri();
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    tokio::task::spawn_blocking(move || {
        let tmp = common::write_profiles(
            "test",
            &[common::ProfileSpec {
                name: "test",
                instance: &uri,
                username: "u",
                password: "p",
            }],
        );
        let out = common::sn_cmd(tmp.path()).args(&args).output().unwrap();
        Ran {
            code: out.status.code().unwrap(),
            stdout: String::from_utf8(out.stdout).unwrap(),
            stderr: String::from_utf8(out.stderr).unwrap(),
        }
    })
    .await
    .unwrap()
}

fn app(name: &str, scope: &str, sys_id: &str) -> Value {
    json!({
        "sys_id": sys_id, "name": name, "scope": scope, "version": "30.0.7",
        "assigned_version": "30.0.7", "latest_version": "30.0.7", "update_available": "0",
        "vendor": "ServiceNow", "short_description": format!("{name} description"),
        "logo": "", "indicators": null, "lob": null, "products": null, "customization": null
    })
}

fn plugin(name: &str, id: &str, active: &str) -> Value {
    json!({
        "active": active, "app_id": id, "app_name": name, "scope": "global", "sys_id": id,
        "description": format!("{name} description"), "entitled": "not_applicable",
        "for_fee": "0", "indicators": "[]", "lineage_name": "Incident Management"
    })
}

async fn mount_apps(server: &MockServer, tab: &str, rows: Value) {
    Mock::given(method("POST"))
        .and(path(APPS))
        .and(query_param("tab_context", tab))
        .and(body_json(json!({})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": {"apps": rows}})))
        .mount(server)
        .await;
}

async fn mount_plugins(server: &MockServer, tab: &str, rows: Value) {
    Mock::given(method("POST"))
        .and(path(PLUGINS))
        .and(query_param("tab_context", tab))
        .and(body_json(json!({})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": rows})))
        .mount(server)
        .await;
}

async fn mount_plugin_detail(server: &MockServer, id: &str, plugin: Value) {
    Mock::given(method("GET"))
        .and(path(format!("/api/sn_appclient/appmanager/plugin/{id}")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"result": {"plugin": plugin}})),
        )
        .mount(server)
        .await;
}

async fn mount_app_detail(server: &MockServer, sys_id: &str, info: Value) {
    Mock::given(method("GET"))
        .and(path(format!("/api/sn_appclient/appmanager/app/{sys_id}")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                json!({"result": {"app_info_on_instance": info, "time_taken": 12.0}}),
            ),
        )
        .mount(server)
        .await;
}

fn flow_detail() -> Value {
    json!({
        "sys_id": FLOW_ID, "scope": "sn_flow_designer", "name": "Flow Designer - Designer",
        "version": "30.0.7", "latest_version": "30.0.7", "isInstalled": true,
        "isInstalledAndUpdateAvailable": false, "vendor": "ServiceNow",
        "short_description": "debugger", "sys_code": "com.glide.hub.designer",
        "install_date": "Sep 23, 2026", "demo_data": "no_demo_data",
        "dependencies": "com.glide.hub.model:sys,sn_workflow_studio:30.0.4", "indicators": []
    })
}

/// The query strings of every request the mock received.
async fn queries(server: &MockServer) -> Vec<String> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| r.url.query().unwrap_or_default().to_string())
        .collect()
}

#[tokio::test(flavor = "current_thread")]
async fn list_merges_both_kinds_into_one_row_shape_sorted_by_name() {
    let server = MockServer::start().await;
    mount_apps(
        &server,
        "installed",
        json!([app("Zeta", "sn_zeta", "a".repeat(32).as_str()), {
            "sys_id": FLOW_ID, "name": "flow Designer", "scope": "sn_flow_designer",
            "version": "30.0.6", "latest_version": "30.0.7", "update_available": "1",
            "vendor": null, "short_description": ""
        }]),
    )
    .await;
    mount_plugins(
        &server,
        "installed",
        json!([plugin("Incident", "com.snc.incident", "1")]),
    )
    .await;

    let ran = sn(&server, &["appmanager", "list"]).await;
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert_eq!(
        ran.json(),
        json!([
            {"kind": "app", "id": "sn_flow_designer", "name": "flow Designer",
             "scope": "sn_flow_designer", "version": "30.0.6", "latest_version": "30.0.7",
             "installed": true, "update_available": true, "vendor": null, "description": null,
             "sys_id": FLOW_ID},
            {"kind": "plugin", "id": "com.snc.incident", "name": "Incident", "scope": "global",
             "version": null, "latest_version": null, "installed": true,
             "update_available": false, "vendor": null,
             "description": "Incident description", "sys_id": null},
            {"kind": "app", "id": "sn_zeta", "name": "Zeta", "scope": "sn_zeta",
             "version": "30.0.7", "latest_version": "30.0.7", "installed": true,
             "update_available": false, "vendor": "ServiceNow",
             "description": "Zeta description", "sys_id": "a".repeat(32)}
        ])
    );
    assert!(ran.stderr.is_empty(), "{}", ran.stderr);
    // Whole lists only: the instance pages and searches these routes wrongly.
    assert_eq!(queries(&server).await, ["tab_context=installed"; 2]);
}

#[tokio::test(flavor = "current_thread")]
async fn search_filters_here_and_is_never_sent() {
    let server = MockServer::start().await;
    mount_apps(
        &server,
        "installed",
        json!([
            app("Flow Designer", "sn_flow_designer", FLOW_ID),
            app("Zeta", "sn_zeta", "a".repeat(32).as_str())
        ]),
    )
    .await;
    mount_plugins(
        &server,
        "installed",
        json!([
            plugin("Incident", "com.snc.incident", "1"),
            plugin("Workflow", "com.glide.workflow", "1")
        ]),
    )
    .await;

    let ran = sn(&server, &["appmanager", "search", "FLOW"]).await;
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let ids: Vec<String> = ran
        .json()
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids, ["sn_flow_designer", "com.glide.workflow"]);
    assert_eq!(queries(&server).await, ["tab_context=installed"; 2]);
}

#[tokio::test(flavor = "current_thread")]
async fn kind_and_tab_flags_choose_the_requests() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(APPS))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    mount_plugins(
        &server,
        "available_for_you",
        json!([plugin("Agile", "com.snc.sdlc.agile.2.0", "0")]),
    )
    .await;

    let ran = sn(
        &server,
        &["appmanager", "list", "--available", "--kind", "plugin"],
    )
    .await;
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert_eq!(ran.json()[0]["installed"], false);
    assert_eq!(queries(&server).await, ["tab_context=available_for_you"]);

    let server = MockServer::start().await;
    mount_apps(&server, "updates", json!([])).await;
    let ran = sn(&server, &["appmanager", "list", "--updates", "-k", "app"]).await;
    assert_eq!((ran.code, ran.json()), (0, json!([])));
}

#[tokio::test(flavor = "current_thread")]
async fn limit_and_offset_cut_after_the_sort_and_say_so() {
    let server = MockServer::start().await;
    mount_apps(&server, "installed", json!([])).await;
    mount_plugins(
        &server,
        "installed",
        json!([
            plugin("C", "c", "1"),
            plugin("a", "a", "1"),
            plugin("B", "b", "1")
        ]),
    )
    .await;

    let ran = sn(
        &server,
        &["appmanager", "list", "--limit", "1", "--offset", "1"],
    )
    .await;
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert_eq!(ran.json()[0]["id"], "b");
    assert_eq!(ran.json().as_array().unwrap().len(), 1);
    assert!(ran.stderr.contains("showing 1 of 3 rows"), "{}", ran.stderr);
}

#[tokio::test(flavor = "current_thread")]
async fn argv_mistakes_never_reach_the_instance() {
    let server = MockServer::start().await;
    for args in [
        &["appmanager", "list", "--updates", "--available"][..],
        &["appmanager", "list", "--output", "raw"],
        &[
            "appmanager",
            "list",
            "--output",
            "raw",
            "--kind",
            "app",
            "--limit",
            "5",
        ],
        &["appmanager", "search", " "],
        &["appmanager", "show", "activate"],
        &["appmanager", "show", "Rollback"],
        &["appmanager", "show", "a/b"],
        &["appmanager", "show", ".."],
        &["appmanager", "show", FLOW_ID, "--kind", "plugin"],
    ] {
        let ran = sn(&server, args).await;
        assert_eq!(ran.code, 1, "{args:?}: {}", ran.stderr);
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn raw_with_a_kind_keeps_the_envelope() {
    let server = MockServer::start().await;
    mount_plugins(
        &server,
        "installed",
        json!([plugin("Incident", "com.snc.incident", "1")]),
    )
    .await;

    let ran = sn(
        &server,
        &[
            "appmanager",
            "search",
            "nothing",
            "--kind",
            "plugin",
            "--output",
            "raw",
        ],
    )
    .await;
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert_eq!(ran.json()["result"][0]["app_id"], "com.snc.incident");
}

#[tokio::test(flavor = "current_thread")]
async fn an_answer_that_is_not_a_list_is_an_instance_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(APPS))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": {}})))
        .mount(&server)
        .await;

    let ran = sn(&server, &["appmanager", "list"]).await;
    assert_eq!(ran.code, 2, "{}", ran.stderr);
    let err = ran.error();
    assert!(
        err["message"].as_str().unwrap().contains("result.apps"),
        "{err}"
    );
    assert!(err.get("status_code").is_none(), "{err}");
}

#[tokio::test(flavor = "current_thread")]
async fn show_reads_a_plugin_by_id() {
    let server = MockServer::start().await;
    mount_apps(
        &server,
        "installed",
        json!([app("Zeta", "sn_zeta", "a".repeat(32).as_str())]),
    )
    .await;
    mount_apps(&server, "available_for_you", json!([])).await;
    mount_plugin_detail(
        &server,
        "com.snc.incident",
        json!({
            "id": "com.snc.incident", "app_id": "com.snc.incident", "name": "Incident",
            "scope": "global", "active": "1", "isInstalled": true, "version": "1.0.0",
            "has_demo_data": "1", "can_rollback": false, "description": "Base incident",
            "lineage_name": "Incident Management", "indicators": []
        }),
    )
    .await;

    let ran = sn(&server, &["appmanager", "show", "com.snc.incident"]).await;
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert_eq!(
        ran.json(),
        json!({
            "kind": "plugin", "id": "com.snc.incident", "name": "Incident", "scope": "global",
            "version": "1.0.0", "latest_version": null, "installed": true,
            "update_available": false, "vendor": null, "description": "Base incident",
            "sys_id": null, "product": "Incident Management", "has_demo_data": true,
            "can_rollback": false, "indicators": []
        })
    );
}

#[tokio::test(flavor = "current_thread")]
async fn show_resolves_a_scope_through_installed_then_available() {
    let server = MockServer::start().await;
    mount_apps(
        &server,
        "installed",
        json!([app("Zeta", "sn_zeta", "a".repeat(32).as_str())]),
    )
    .await;
    mount_apps(
        &server,
        "available_for_you",
        json!([app("Flow Designer", "sn_flow_designer", FLOW_ID)]),
    )
    .await;
    mount_plugin_detail(&server, "sn_flow_designer", json!({})).await;
    mount_app_detail(&server, FLOW_ID, flow_detail()).await;

    let ran = sn(&server, &["appmanager", "show", "sn_flow_designer"]).await;
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let out = ran.json();
    assert_eq!(out["kind"], "app");
    assert_eq!(out["sys_id"], FLOW_ID);
    assert_eq!(out["plugin_id"], "com.glide.hub.designer");
    assert_eq!(
        out["dependencies"],
        json!([{"id": "com.glide.hub.model", "version": "sys"},
               {"id": "sn_workflow_studio", "version": "30.0.4"}])
    );
}

#[tokio::test(flavor = "current_thread")]
async fn show_reads_an_app_by_sys_id_and_raw_keeps_its_envelope() {
    let server = MockServer::start().await;
    mount_app_detail(&server, FLOW_ID, flow_detail()).await;

    let ran = sn(&server, &["appmanager", "show", &FLOW_ID.to_uppercase()]).await;
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert_eq!(ran.json()["id"], "sn_flow_designer");
    assert_eq!(server.received_requests().await.unwrap().len(), 1);

    let ran = sn(&server, &["appmanager", "show", FLOW_ID, "--output", "raw"]).await;
    assert_eq!(ran.json()["result"]["time_taken"], 12.0);
}

#[tokio::test(flavor = "current_thread")]
async fn an_id_nothing_answers_to_is_exit_2_without_a_status() {
    let server = MockServer::start().await;
    mount_apps(&server, "installed", json!([])).await;
    mount_apps(&server, "available_for_you", json!([])).await;
    mount_plugin_detail(&server, "com.nope.fake", json!({})).await;

    let ran = sn(&server, &["appmanager", "show", "com.nope.fake"]).await;
    assert_eq!(ran.code, 2, "{}", ran.stderr);
    let err = ran.error();
    assert!(
        err["message"].as_str().unwrap().contains("com.nope.fake"),
        "{err}"
    );
    assert!(err.get("status_code").is_none(), "{err}");
}

#[tokio::test(flavor = "current_thread")]
async fn a_different_record_than_asked_for_is_an_instance_error() {
    let server = MockServer::start().await;
    mount_plugin_detail(
        &server,
        "com.snc.incident",
        json!({"id": "com.snc.incident.other", "isInstalled": true}),
    )
    .await;
    let ran = sn(
        &server,
        &["appmanager", "show", "com.snc.incident", "-k", "plugin"],
    )
    .await;
    assert_eq!(ran.code, 2, "{}", ran.stderr);
    assert!(
        ran.error()["message"]
            .as_str()
            .unwrap()
            .contains("different record")
    );

    let mut other = flow_detail();
    other["sys_id"] = json!("b".repeat(32));
    mount_app_detail(&server, FLOW_ID, other).await;
    let ran = sn(&server, &["appmanager", "show", FLOW_ID]).await;
    assert_eq!(ran.code, 2, "{}", ran.stderr);
    assert!(ran.error().get("status_code").is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn an_id_on_both_sides_is_the_callers_to_settle() {
    let server = MockServer::start().await;
    mount_apps(
        &server,
        "installed",
        json!([app("Twin", "sn_twin", FLOW_ID)]),
    )
    .await;
    mount_plugin_detail(
        &server,
        "sn_twin",
        json!({"id": "sn_twin", "isInstalled": true}),
    )
    .await;

    let ran = sn(&server, &["appmanager", "show", "sn_twin"]).await;
    assert_eq!(ran.code, 1, "{}", ran.stderr);
    assert!(ran.error()["message"].as_str().unwrap().contains("--kind"));

    let ran = sn(
        &server,
        &["appmanager", "show", "sn_twin", "--kind", "plugin"],
    )
    .await;
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert_eq!(ran.json()["kind"], "plugin");
}

#[tokio::test(flavor = "current_thread")]
async fn a_403_names_the_roles_and_stays_exit_4() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(APPS))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(json!({"error": {"message": "User Not Authorized"}})),
        )
        .mount(&server)
        .await;

    let ran = sn(&server, &["appmanager", "list"]).await;
    assert_eq!(ran.code, 4, "{}", ran.stderr);
    let err = ran.error();
    assert!(
        err["message"]
            .as_str()
            .unwrap()
            .contains("sn_appclient.app_client_user"),
        "{err}"
    );
    assert_eq!(err["status_code"], 403);
}

#[tokio::test(flavor = "current_thread")]
async fn a_missing_route_names_the_application_manager() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(PLUGINS))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": {"message": "Requested URI does not represent any resource", "detail": null},
            "status": "failure"
        })))
        .mount(&server)
        .await;

    let ran = sn(&server, &["appmanager", "list", "--kind", "plugin"]).await;
    assert_eq!(ran.code, 2, "{}", ran.stderr);
    let err = ran.error();
    assert!(
        err["message"]
            .as_str()
            .unwrap()
            .contains("Application Manager API is not available"),
        "{err}"
    );
    assert_eq!(err["status_code"], 400);
}

#[tokio::test(flavor = "current_thread")]
async fn get_plugin_points_at_its_replacement_without_a_request() {
    let server = MockServer::start().await;
    let ran = sn(&server, &["get", "plugin", "com.snc.incident", "sn_x"]).await;
    assert_eq!(ran.code, 1, "{}", ran.stderr);
    let message = ran.error()["message"].as_str().unwrap().to_string();
    assert!(message.contains("sn appmanager show"), "{message}");
    assert!(message.contains("--need-plugin"), "{message}");
    assert!(server.received_requests().await.unwrap().is_empty());
}

const ACTIVATE: &str = "/api/sn_appclient/appmanager/plugin/activate";
const TRACKER: &str = "fb9f725683689f9066c3e3e0deaad3c1";

fn inactive_plugin(id: &str) -> Value {
    json!({"id": id, "app_id": id, "name": "Pad", "scope": "sn_pad", "active": "0",
           "isInstalled": false, "indicators": []})
}

fn pending() -> Value {
    json!({"result": {
        "error": "", "status": "0", "status_label": "Pending", "status_message": "",
        "percent_complete": 0, "trackerId": TRACKER,
        "links": {"progress": {"id": TRACKER, "url": "x"}}
    }})
}

#[tokio::test(flavor = "current_thread")]
async fn install_activates_a_resolved_plugin_and_waits_for_it() {
    let server = MockServer::start().await;
    mount_plugin_detail(&server, "com.sn_pad", inactive_plugin("com.sn_pad")).await;
    Mock::given(method("GET"))
        .and(path(ACTIVATE))
        .and(query_param("plugin_id", "com.sn_pad"))
        .and(query_param("load_demo_data", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(pending()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/api/sn_cicd/progress/{TRACKER}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": {
            "status": "2", "status_label": "Successful", "percent_complete": 100,
            "status_message": "Plugin activated/upgraded successfuly"
        }})))
        .expect(1)
        .mount(&server)
        .await;

    let ran = sn(
        &server,
        &[
            "appmanager",
            "install",
            "com.sn_pad",
            "--demo-data",
            "--yes",
            "--wait",
        ],
    )
    .await;
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert_eq!(ran.json()["status_label"], "Successful");
}

#[tokio::test(flavor = "current_thread")]
async fn install_without_wait_returns_the_progress_link() {
    let server = MockServer::start().await;
    mount_plugin_detail(&server, "com.sn_pad", inactive_plugin("com.sn_pad")).await;
    Mock::given(method("GET"))
        .and(path(ACTIVATE))
        .and(query_param("load_demo_data", "false"))
        .respond_with(ResponseTemplate::new(200).set_body_json(pending()))
        .mount(&server)
        .await;

    let ran = sn(&server, &["appmanager", "install", "com.sn_pad", "-y"]).await;
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert_eq!(ran.json()["links"]["progress"]["id"], TRACKER);
}

/// The activate route is only ever reached for a plugin that exists and is
/// not installed: the instance queues ids that name nothing.
#[tokio::test(flavor = "current_thread")]
async fn install_never_sends_an_unresolved_or_installed_plugin() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(ACTIVATE))
        .respond_with(ResponseTemplate::new(200).set_body_json(pending()))
        .expect(0)
        .mount(&server)
        .await;
    mount_plugin_detail(&server, "com.nope", json!({})).await;
    mount_plugin_detail(
        &server,
        "com.snc.incident",
        json!({"id": "com.snc.incident", "isInstalled": true, "version": "1.0.0"}),
    )
    .await;

    let ran = sn(&server, &["appmanager", "install", "com.nope", "--yes"]).await;
    assert_eq!(ran.code, 2, "{}", ran.stderr);
    assert!(ran.error().get("status_code").is_none());

    let ran = sn(
        &server,
        &["appmanager", "install", "com.snc.incident", "--yes"],
    )
    .await;
    assert_eq!(ran.code, 1, "{}", ran.stderr);
    assert!(
        ran.error()["message"]
            .as_str()
            .unwrap()
            .contains("already installed (version 1.0.0)")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn install_is_refused_before_the_network_without_yes_or_a_plugin_id() {
    let server = MockServer::start().await;
    for args in [
        &["appmanager", "install", "com.sn_pad"][..],
        &["appmanager", "install", "activate", "--yes"],
        &["appmanager", "install", FLOW_ID, "--yes"],
    ] {
        let ran = sn(&server, args).await;
        assert_eq!(ran.code, 1, "{args:?}: {}", ran.stderr);
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn a_refusal_under_200_is_exit_2_and_never_polled() {
    let server = MockServer::start().await;
    mount_plugin_detail(&server, "com.sn_pad", inactive_plugin("com.sn_pad")).await;
    Mock::given(method("GET"))
        .and(path(ACTIVATE))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": {
            "error": "Plugin ID is not valid", "status": "3", "status_detail": "",
            "status_label": "Failed", "status_message": ""
        }})))
        .mount(&server)
        .await;

    let ran = sn(
        &server,
        &["appmanager", "install", "com.sn_pad", "--yes", "--wait"],
    )
    .await;
    assert_eq!(ran.code, 2, "{}", ran.stderr);
    let err = ran.error();
    assert!(
        err["message"]
            .as_str()
            .unwrap()
            .contains("Plugin ID is not valid"),
        "{err}"
    );
    assert!(err.get("status_code").is_none(), "{err}");
    assert_eq!(err["sn_error"]["status_label"], "Failed");
}

#[tokio::test(flavor = "current_thread")]
async fn an_answer_with_no_operation_to_follow_is_not_reported_as_started() {
    let server = MockServer::start().await;
    mount_plugin_detail(&server, "com.sn_pad", inactive_plugin("com.sn_pad")).await;
    Mock::given(method("GET"))
        .and(path(ACTIVATE))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": {"status": "0"}})))
        .mount(&server)
        .await;

    let ran = sn(&server, &["appmanager", "install", "com.sn_pad", "--yes"]).await;
    assert_eq!(ran.code, 2, "{}", ran.stderr);
    assert!(
        ran.error()["message"]
            .as_str()
            .unwrap()
            .contains("no operation")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_failed_activation_is_exit_2_with_the_progress_in_sn_error() {
    let server = MockServer::start().await;
    mount_plugin_detail(&server, "com.sn_pad", inactive_plugin("com.sn_pad")).await;
    Mock::given(method("GET"))
        .and(path(ACTIVATE))
        .respond_with(ResponseTemplate::new(200).set_body_json(pending()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/api/sn_cicd/progress/{TRACKER}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": {
            "status": "3", "status_label": "Failed", "status_message": "Activation failed",
            "status_detail": "dependency missing"
        }})))
        .mount(&server)
        .await;

    let ran = sn(
        &server,
        &["appmanager", "install", "com.sn_pad", "--yes", "--wait"],
    )
    .await;
    assert_eq!(ran.code, 2, "{}", ran.stderr);
    let err = ran.error();
    assert_eq!(err["message"], "Activation failed");
    assert!(err.get("status_code").is_none(), "{err}");
}
