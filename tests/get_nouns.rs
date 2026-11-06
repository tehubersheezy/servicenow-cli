//! `sn get <noun>` and `sn set`: the noun/reference split, and the canaries
//! each noun carries against a query term the instance silently drops.

mod common;

use common::{ProfileSpec, sn_cmd, write_profiles};
use serde_json::{Value, json};
use wiremock::matchers::{body_string_contains, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SET: &str = "37d4d5b03da7031074a2476226988e43";

fn profile_for(uri: &str) -> tempfile::TempDir {
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

/// Run `sn <args>` off the async runtime; returns (exit code, stdout, stderr).
async fn run(uri: String, args: Vec<&'static str>) -> (i32, String, Value) {
    tokio::task::spawn_blocking(move || {
        let tmp = profile_for(&uri);
        let out = sn_cmd(tmp.path())
            .arg("--compact")
            .args(&args)
            .output()
            .unwrap();
        let stderr = String::from_utf8(out.stderr).unwrap();
        let err = serde_json::from_str(stderr.trim()).unwrap_or(Value::Null);
        (
            out.status.code().unwrap(),
            String::from_utf8(out.stdout).unwrap(),
            err,
        )
    })
    .await
    .unwrap()
}

async fn mount_current(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/api/now/ui/concoursepicker/current"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": {
            "currentApplication": {"name": "Global", "scopeName": "global", "sysId": "global"},
            "currentUpdateSet": {"name": "Default [Global]", "sysId": SET},
        }})))
        .mount(server)
        .await;
}

#[test]
fn a_mistyped_noun_lists_the_nouns_offline() {
    let tmp = profile_for("http://127.0.0.1:9");
    let out = sn_cmd(tmp.path()).args(["get", "scop"]).assert().code(1);
    let stderr = String::from_utf8(out.get_output().stderr.clone()).unwrap();
    assert!(stderr.contains("updateset"), "{stderr}");
    assert!(stderr.contains("not a record number"), "{stderr}");
}

#[tokio::test(flavor = "current_thread")]
async fn updateset_reports_count_and_changes() {
    let server = MockServer::start().await;
    mount_current(&server).await;
    Mock::given(method("GET"))
        .and(path(format!("/api/now/table/sys_update_set/{SET}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": {
            "sys_id": {"value": SET, "display_value": SET},
            "name": {"value": "Default", "display_value": "Default"},
            "state": {"value": "in progress", "display_value": "In progress"},
            "application": {"value": "global", "display_value": "Global"},
        }})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/stats/sys_update_xml"))
        .and(query_param("sysparm_group_by", "update_set"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": [
            {"groupby_fields": [{"field": "update_set", "value": SET}], "stats": {"count": "3"}},
        ]})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_update_xml"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": [
            {"sys_id": "x1", "type": "Business Rule", "target_name": "BR", "action": "INSERT_OR_UPDATE",
             "update_set": {"value": SET}},
        ]})))
        .mount(&server)
        .await;

    let (code, stdout, _) = run(server.uri(), vec!["get", "updateset", "-l", "1"]).await;
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["name"], "Default");
    assert_eq!(v["current"], true);
    assert_eq!(v["change_count"], 3);
    assert_eq!(v["changes"][0]["target"], "BR");
}

#[tokio::test(flavor = "current_thread")]
async fn updateset_change_from_another_set_is_a_dropped_term() {
    let server = MockServer::start().await;
    mount_current(&server).await;
    Mock::given(method("GET"))
        .and(path(format!("/api/now/table/sys_update_set/{SET}")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"result": {"sys_id": {"value": SET}}})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/stats/sys_update_xml"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": []})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_update_xml"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": [
            {"sys_id": "x1", "update_set": {"value": "someone_elses_set"}},
        ]})))
        .mount(&server)
        .await;

    let (code, stdout, err) = run(server.uri(), vec!["get", "updateset"]).await;
    assert_eq!(code, 2, "{err}");
    assert!(stdout.is_empty());
    assert!(err["error"].get("status_code").is_none(), "{err}");
}

#[tokio::test(flavor = "current_thread")]
async fn property_without_a_row_falls_back_to_the_runtime() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_properties"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": []})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .and(body_string_contains("getSysPropertyValue"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {
            "snDecisionTable": {"sysProperties": {"v": "Brazil"}}}})))
        .mount(&server)
        .await;

    let (code, stdout, _) = run(server.uri(), vec!["get", "property", "glide.buildname"]).await;
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(
        v,
        json!({"name": "glide.buildname", "value": "Brazil", "source": "runtime"})
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_shared_script_name_is_listed_not_picked() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {
        "GlideRecord_Query": {"sys_script": {"_results": [
            {"sys_id": {"value": "a"}, "name": {"value": "Set Parent"}},
            {"sys_id": {"value": "b"}, "name": {"value": "Set Parent"}},
        ]}}}})))
        .mount(&server)
        .await;

    let (code, _, err) = run(
        server.uri(),
        vec!["get", "script", "Set Parent", "--type", "br"],
    )
    .await;
    assert_eq!(code, 1);
    let msg = err["error"]["message"].as_str().unwrap();
    assert!(
        msg.contains("sys_script:a") && msg.contains("sys_script:b"),
        "{msg}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn set_updateset_to_the_current_set_skips_the_picker() {
    let server = MockServer::start().await;
    mount_current(&server).await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_update_set"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": [
            {"sys_id": SET, "name": "Default", "application": {"value": "global"}},
        ]})))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/api/now/ui/concoursepicker/updateset"))
        .respond_with(ResponseTemplate::new(400))
        .expect(0)
        .mount(&server)
        .await;

    let (code, stdout, err) = run(server.uri(), vec!["set", "updateset", "Default"]).await;
    assert_eq!(code, 0, "{err}");
    let v: Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["update_set"]["sys_id"], SET);
}

#[tokio::test(flavor = "current_thread")]
async fn logs_older_than_the_window_are_a_dropped_term() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/syslog"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": [
            {"sys_created_on": "2020-01-01 00:00:00", "level": "2", "message": "old"},
        ]})))
        .mount(&server)
        .await;

    let (code, stdout, _) = run(server.uri(), vec!["get", "logs"]).await;
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
}
