use assert_cmd::Command;
use serde_json::json;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

// ── progress ─────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn progress_get_unwraps_result() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/sn_cicd/progress/prog123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": {
                "links": {},
                "percentComplete": 100,
                "status": "2",
                "status_detail": "Completed",
                "status_label": "Complete"
            }
        })))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut cmd = Command::cargo_bin("sn").unwrap();
        let out = cmd
            .args([
                "--instance-override",
                &server_uri,
                "--username",
                "u",
                "--password",
                "p",
                "--compact",
                "progress",
                "prog123",
            ])
            .assert()
            .success();
        let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        assert_eq!(v["status_label"], "Complete");
        assert!(!stdout.contains("\"result\""), "should unwrap result");
    })
    .await
    .unwrap();
}

// ── app install ───────────────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn app_install_posts_with_scope_query_param() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/sn_cicd/app_repo/install"))
        .and(query_param("scope", "x_acme_myapp"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": {"links": {}, "status": "0", "status_label": "Pending"}
        })))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut cmd = Command::cargo_bin("sn").unwrap();
        let out = cmd
            .args([
                "--instance-override",
                &server_uri,
                "--username",
                "u",
                "--password",
                "p",
                "--compact",
                "app",
                "install",
                "--scope",
                "x_acme_myapp",
            ])
            .assert()
            .success();
        let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        assert_eq!(v["status_label"], "Pending");
    })
    .await
    .unwrap();
}

// ── update-set create ────────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn update_set_create_posts_with_name_param() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/sn_cicd/update_set/create"))
        .and(query_param("name", "My Update Set"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": {"sys_id": "us001", "name": "My Update Set"}
        })))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut cmd = Command::cargo_bin("sn").unwrap();
        let out = cmd
            .args([
                "--instance-override",
                &server_uri,
                "--username",
                "u",
                "--password",
                "p",
                "--compact",
                "updateset",
                "create",
                "--name",
                "My Update Set",
            ])
            .assert()
            .success();
        let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        assert_eq!(v["sys_id"], "us001");
    })
    .await
    .unwrap();
}

// ── atf run ───────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn atf_run_posts_with_suite_name_param() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/sn_cicd/testsuite/run"))
        .and(query_param("test_suite_name", "MySuite"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": {
                "links": {},
                "status": "0",
                "status_label": "Pending",
                "test_suite_name": "MySuite"
            }
        })))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut cmd = Command::cargo_bin("sn").unwrap();
        let out = cmd
            .args([
                "--instance-override",
                &server_uri,
                "--username",
                "u",
                "--password",
                "p",
                "--compact",
                "atf",
                "run",
                "--suite-name",
                "MySuite",
            ])
            .assert()
            .success();
        let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        assert_eq!(v["test_suite_name"], "MySuite");
    })
    .await
    .unwrap();
}

// ── aggregate ────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn aggregate_count_incident() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/stats/incident"))
        .and(query_param("sysparm_count", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": {
                "stats": {
                    "count": "42"
                }
            }
        })))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut cmd = Command::cargo_bin("sn").unwrap();
        let out = cmd
            .args([
                "--instance-override",
                &server_uri,
                "--username",
                "u",
                "--password",
                "p",
                "--compact",
                "aggregate",
                "incident",
                "--count",
            ])
            .assert()
            .success();
        let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        assert_eq!(v["stats"]["count"], "42");
    })
    .await
    .unwrap();
}

// ── scores list ───────────────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn scores_list_per_page() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/pa/scorecards"))
        .and(query_param("sysparm_per_page", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": []
        })))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut cmd = Command::cargo_bin("sn").unwrap();
        let out = cmd
            .args([
                "--instance-override",
                &server_uri,
                "--username",
                "u",
                "--password",
                "p",
                "--compact",
                "scores",
                "list",
                "--per-page",
                "5",
            ])
            .assert()
            .success();
        let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        assert!(v.is_array());
    })
    .await
    .unwrap();
}

// ── atf results ───────────────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn atf_results_get_unwraps_result() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/sn_cicd/testsuite/results/res456"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": {
                "id": "res456",
                "status": "success",
                "tests_total": 10,
                "tests_passed": 10
            }
        })))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut cmd = Command::cargo_bin("sn").unwrap();
        let out = cmd
            .args([
                "--instance-override",
                &server_uri,
                "--username",
                "u",
                "--password",
                "p",
                "--compact",
                "atf",
                "results",
                "res456",
            ])
            .assert()
            .success();
        let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        assert_eq!(v["status"], "success");
        assert_eq!(v["tests_passed"], 10);
        assert!(!stdout.contains("\"result\""), "should unwrap result");
    })
    .await
    .unwrap();
}

// ── --wait flag ───────────────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn app_install_wait_polls_until_complete() {
    let server = wiremock::MockServer::start().await;

    // Initial install POST returns progress link
    Mock::given(method("POST"))
        .and(path("/api/sn_cicd/app_repo/install"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": {
                "links": {
                    "progress": {
                        "id": "prog123"
                    }
                },
                "status": "0",
                "status_label": "Pending"
            }
        })))
        .mount(&server)
        .await;

    // Progress poll: already complete
    Mock::given(method("GET"))
        .and(path("/api/sn_cicd/progress/prog123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": {
                "status": "2",
                "status_label": "Succeeded",
                "percent_complete": "100",
                "status_message": "Install complete"
            }
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
                "app",
                "install",
                "--scope",
                "x_test",
                "--wait",
            ])
            .assert()
            .success();
        let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        assert!(
            v["status_label"] == "Succeeded" || v["status_message"] == "Install complete",
            "expected final progress result, got: {stdout}"
        );
    })
    .await
    .unwrap();
}
