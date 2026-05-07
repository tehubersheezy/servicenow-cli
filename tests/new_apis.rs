use assert_cmd::Command;
use serde_json::json;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

// ── change management ────────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn change_list_normal() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/sn_chg_rest/change/normal"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [{"number": "CHG001", "type": "normal"}]
        })))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let out = Command::cargo_bin("sn")
            .unwrap()
            .args([
                "--instance-override",
                &server_uri,
                "--username",
                "u",
                "--password",
                "p",
                "--compact",
                "change",
                "list",
                "--type",
                "normal",
            ])
            .assert()
            .success();
        let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        assert!(v.is_array());
        assert_eq!(v[0]["number"], "CHG001");
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn change_create_normal() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/sn_chg_rest/change/normal"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "result": {"sys_id": "chg001", "number": "CHG001"}
        })))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let out = Command::cargo_bin("sn")
            .unwrap()
            .args([
                "--instance-override",
                &server_uri,
                "--username",
                "u",
                "--password",
                "p",
                "--compact",
                "change",
                "create",
                "--type",
                "normal",
                "--field",
                "short_description=Test change",
            ])
            .assert()
            .success();
        let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        assert_eq!(v["number"], "CHG001");
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn change_task_list() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/sn_chg_rest/change/chg001/task"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [{"number": "CTASK001"}]
        })))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let out = Command::cargo_bin("sn")
            .unwrap()
            .args([
                "--instance-override",
                &server_uri,
                "--username",
                "u",
                "--password",
                "p",
                "--compact",
                "change",
                "task",
                "list",
                "chg001",
            ])
            .assert()
            .success();
        let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        assert_eq!(v[0]["number"], "CTASK001");
    })
    .await
    .unwrap();
}

// ── attachment ───────────────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn attachment_list_with_query() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/attachment"))
        .and(query_param("sysparm_query", "table_name=incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [{"sys_id": "att001", "file_name": "log.txt"}]
        })))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let out = Command::cargo_bin("sn")
            .unwrap()
            .args([
                "--instance-override",
                &server_uri,
                "--username",
                "u",
                "--password",
                "p",
                "--compact",
                "attachment",
                "list",
                "--query",
                "table_name=incident",
            ])
            .assert()
            .success();
        let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        assert_eq!(v[0]["file_name"], "log.txt");
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn attachment_get_metadata() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/attachment/att001"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": {"sys_id": "att001", "file_name": "log.txt", "size_bytes": "1024"}
        })))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let out = Command::cargo_bin("sn")
            .unwrap()
            .args([
                "--instance-override",
                &server_uri,
                "--username",
                "u",
                "--password",
                "p",
                "--compact",
                "attachment",
                "get",
                "att001",
            ])
            .assert()
            .success();
        let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        assert_eq!(v["file_name"], "log.txt");
    })
    .await
    .unwrap();
}

// ── cmdb ─────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn cmdb_list_servers() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/cmdb/instance/cmdb_ci_server"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [{"sys_id": "ci001", "name": "web-server-01"}]
        })))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let out = Command::cargo_bin("sn")
            .unwrap()
            .args([
                "--instance-override",
                &server_uri,
                "--username",
                "u",
                "--password",
                "p",
                "--compact",
                "cmdb",
                "list",
                "cmdb_ci_server",
            ])
            .assert()
            .success();
        let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        assert_eq!(v[0]["name"], "web-server-01");
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn cmdb_meta() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/cmdb/meta/cmdb_ci_server"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": {"name": "cmdb_ci_server", "label": "Server"}
        })))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let out = Command::cargo_bin("sn")
            .unwrap()
            .args([
                "--instance-override",
                &server_uri,
                "--username",
                "u",
                "--password",
                "p",
                "--compact",
                "cmdb",
                "meta",
                "cmdb_ci_server",
            ])
            .assert()
            .success();
        let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        assert_eq!(v["label"], "Server");
    })
    .await
    .unwrap();
}

// ── import set ───────────────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn import_create_record() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/now/import/u_staging_table"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "result": [{"sys_id": "imp001", "status": "inserted"}]
        })))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let out = Command::cargo_bin("sn")
            .unwrap()
            .args([
                "--instance-override",
                &server_uri,
                "--username",
                "u",
                "--password",
                "p",
                "--compact",
                "import",
                "create",
                "u_staging_table",
                "--field",
                "u_name=test",
            ])
            .assert()
            .success();
        let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        assert_eq!(v[0]["status"], "inserted");
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn import_get_record() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/import/u_staging_table/imp001"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": {"sys_id": "imp001", "u_name": "test"}
        })))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let out = Command::cargo_bin("sn")
            .unwrap()
            .args([
                "--instance-override",
                &server_uri,
                "--username",
                "u",
                "--password",
                "p",
                "--compact",
                "import",
                "get",
                "u_staging_table",
                "imp001",
            ])
            .assert()
            .success();
        let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        assert_eq!(v["u_name"], "test");
    })
    .await
    .unwrap();
}

// ── service catalog ──────────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn catalog_list() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/sn_sc/servicecatalog/catalogs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [{"sys_id": "cat001", "title": "Service Catalog"}]
        })))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let out = Command::cargo_bin("sn")
            .unwrap()
            .args([
                "--instance-override",
                &server_uri,
                "--username",
                "u",
                "--password",
                "p",
                "--compact",
                "catalog",
                "list",
            ])
            .assert()
            .success();
        let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        assert_eq!(v[0]["title"], "Service Catalog");
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn catalog_items_with_text_search() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/sn_sc/servicecatalog/items"))
        .and(query_param("sysparm_text", "laptop"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [{"sys_id": "item001", "name": "Laptop Request"}]
        })))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let out = Command::cargo_bin("sn")
            .unwrap()
            .args([
                "--instance-override",
                &server_uri,
                "--username",
                "u",
                "--password",
                "p",
                "--compact",
                "catalog",
                "items",
                "--text",
                "laptop",
            ])
            .assert()
            .success();
        let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        assert_eq!(v[0]["name"], "Laptop Request");
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn catalog_order_item() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/sn_sc/servicecatalog/items/item001/order_now"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": {"request_number": "REQ001", "request_id": "req001"}
        })))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let out = Command::cargo_bin("sn")
            .unwrap()
            .args([
                "--instance-override",
                &server_uri,
                "--username",
                "u",
                "--password",
                "p",
                "--compact",
                "catalog",
                "order",
                "item001",
            ])
            .assert()
            .success();
        let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        assert_eq!(v["request_number"], "REQ001");
    })
    .await
    .unwrap();
}

// ── identify & reconcile ─────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn identify_create_update() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/now/identifyreconcile"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": {"items": [{"sysId": "ci001", "className": "cmdb_ci_server"}]}
        })))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let out = Command::cargo_bin("sn")
            .unwrap()
            .args([
                "--instance-override",
                &server_uri,
                "--username",
                "u",
                "--password",
                "p",
                "--compact",
                "identify",
                "create-update",
                "--data",
                r#"{"items":[{"className":"cmdb_ci_server","values":{"name":"web01"}}]}"#,
            ])
            .assert()
            .success();
        let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        assert_eq!(v["items"][0]["className"], "cmdb_ci_server");
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn identify_query() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/now/identifyreconcile/query"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": {"items": [{"sysId": "ci001", "className": "cmdb_ci_server"}]}
        })))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let out = Command::cargo_bin("sn")
            .unwrap()
            .args([
                "--instance-override",
                &server_uri,
                "--username",
                "u",
                "--password",
                "p",
                "--compact",
                "identify",
                "query",
                "--data",
                r#"{"items":[{"className":"cmdb_ci_server","values":{"name":"web01"}}]}"#,
            ])
            .assert()
            .success();
        let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        assert_eq!(v["items"][0]["sysId"], "ci001");
    })
    .await
    .unwrap();
}
