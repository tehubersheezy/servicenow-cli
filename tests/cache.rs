//! `sn cache` (the offline schema index) and the dynamic completion that reads
//! it. The instance is mocked with the aggregate shapes measured live: every
//! request is `GET /api/now/stats/{table}` answering
//! `{"result": [{"groupby_fields": [{field, value}…], "stats": {"count"}}]}`.

mod common;

use serde_json::{Value, json};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TERMS: &str = "elementISNOTEMPTY^nameNOT LIKEvar__m_";

fn group(fields: &[(&str, &str)], count: u64) -> Value {
    json!({
        "groupby_fields": fields.iter().map(|(f, v)| json!({"field": f, "value": v})).collect::<Vec<_>>(),
        "stats": {"count": count.to_string()}
    })
}

async fn mount_tables(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/api/now/stats/sys_db_object"))
        .and(query_param("sysparm_group_by", "name,super_class.name"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": [
            group(&[("name", "cmdb_ci"), ("super_class.name", "")], 1),
            group(&[("name", "cmdb_ci_server"), ("super_class.name", "cmdb_ci")], 1),
            group(&[("name", "incident"), ("super_class.name", "task")], 1),
            group(&[("name", "task"), ("super_class.name", "")], 1),
        ]})))
        .mount(server)
        .await;
}

async fn mount_plan(server: &MockServer, plan: &[(&str, u64)]) {
    let rows: Vec<Value> = plan
        .iter()
        .map(|(n, c)| group(&[("name", n)], *c))
        .collect();
    Mock::given(method("GET"))
        .and(path("/api/now/stats/sys_dictionary"))
        .and(query_param("sysparm_group_by", "name"))
        .and(query_param("sysparm_query", TERMS))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "result": rows })))
        .mount(server)
        .await;
}

async fn mount_shard(server: &MockServer, query: &str, response: ResponseTemplate) {
    Mock::given(method("GET"))
        .and(path("/api/now/stats/sys_dictionary"))
        .and(query_param("sysparm_group_by", "name,element"))
        .and(query_param("sysparm_query", query))
        .respond_with(response)
        .expect(1)
        .mount(server)
        .await;
}

fn columns(pairs: &[(&str, &str)]) -> ResponseTemplate {
    let rows: Vec<Value> = pairs
        .iter()
        .map(|(n, e)| group(&[("element", e), ("name", n)], 1))
        .collect();
    ResponseTemplate::new(200).set_body_json(json!({ "result": rows }))
}

fn stdout_json(out: &assert_cmd::assert::Assert) -> Value {
    serde_json::from_slice(&out.get_output().stdout).unwrap()
}

fn cache_file(dir: &std::path::Path, server_uri: &str) -> std::path::PathBuf {
    sn::schema_cache::cache_path_in(dir, server_uri)
}

#[tokio::test(flavor = "current_thread")]
async fn refresh_builds_an_index_that_answers_offline() {
    let server = MockServer::start().await;
    mount_tables(&server).await;
    mount_plan(
        &server,
        &[
            ("cmdb_ci", 1),
            ("cmdb_ci_server", 1),
            ("incident", 2),
            ("task", 2),
        ],
    )
    .await;
    mount_shard(
        &server,
        TERMS,
        columns(&[
            ("cmdb_ci", "name"),
            ("cmdb_ci_server", "os"),
            ("incident", "caller_id"),
            ("incident", "severity"),
            ("task", "number"),
            ("task", "short_description"),
            // Not in sys_db_object (a var__m_ table that slipped past the
            // NOT LIKE term): dropped, not indexed.
            ("var__m_x", "stray"),
        ]),
    )
    .await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile_for(&uri);
        let out = common::sn_cmd(tmp.path())
            .args(["--compact", "cache", "refresh"])
            .assert()
            .success();
        let report = stdout_json(&out);
        assert_eq!(report["tables"], 4);
        assert_eq!(report["columns"], 6);
        assert_eq!(report["columns_indexed"], true);
        assert_eq!(report["requests"], 3);

        let file = cache_file(tmp.path(), &uri);
        assert!(file.exists(), "{file:?}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&file).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        // Everything below is offline: the mock would 404 anything unmounted.
        let out = common::sn_cmd(tmp.path())
            .args(["--compact", "cache", "columns", "incident"])
            .assert()
            .success();
        assert_eq!(
            stdout_json(&out),
            json!(["caller_id", "number", "severity", "short_description"])
        );
        let out = common::sn_cmd(tmp.path())
            .args(["--compact", "cache", "tables", "cmdb"])
            .assert()
            .success();
        assert_eq!(stdout_json(&out), json!(["cmdb_ci", "cmdb_ci_server"]));
        let out = common::sn_cmd(tmp.path())
            .args(["--compact", "cache", "status"])
            .assert()
            .success();
        assert_eq!(stdout_json(&out)["exists"], true);
        common::sn_cmd(tmp.path())
            .args(["cache", "columns", "incidnet"])
            .assert()
            .code(1);
    })
    .await
    .unwrap();
}

fn profile_for(uri: &str) -> tempfile::TempDir {
    common::write_profiles(
        "test",
        &[common::ProfileSpec {
            name: "test",
            instance: uri,
            username: "u",
            password: "p",
        }],
    )
}

#[tokio::test(flavor = "current_thread")]
async fn a_timed_out_shard_is_split_and_retried() {
    let server = MockServer::start().await;
    mount_tables(&server).await;
    // 4,000 + 4,000 fit one 10,000-row shard; the third starts another.
    mount_plan(
        &server,
        &[("cmdb_ci", 4000), ("incident", 4000), ("task", 4000)],
    )
    .await;
    mount_shard(
        &server,
        &format!("{TERMS}^name<task"),
        ResponseTemplate::new(504).set_body_string("<html>504 Gateway Time-out</html>"),
    )
    .await;
    mount_shard(
        &server,
        &format!("{TERMS}^name<incident"),
        columns(&[("cmdb_ci", "name")]),
    )
    .await;
    mount_shard(
        &server,
        &format!("{TERMS}^name>=incident^name<task"),
        columns(&[("incident", "caller_id")]),
    )
    .await;
    mount_shard(
        &server,
        &format!("{TERMS}^name>=task"),
        columns(&[("task", "number")]),
    )
    .await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile_for(&uri);
        let out = common::sn_cmd(tmp.path())
            .args(["--compact", "cache", "refresh"])
            .assert()
            .success();
        let report = stdout_json(&out);
        assert_eq!(report["columns"], 3);
        // tables + plan + two shards + the failed shard's two halves.
        assert_eq!(report["requests"], 6);
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn a_shard_whose_range_was_dropped_is_refused() {
    let server = MockServer::start().await;
    mount_tables(&server).await;
    mount_plan(&server, &[("incident", 6000), ("task", 6000)]).await;
    // The instance ignored `^name<task` and answered with task's rows too —
    // more foreign rows than the shard planned in total.
    let mut unfiltered: Vec<(String, String)> = (0..6001)
        .map(|i| ("task".to_string(), format!("c{i}")))
        .collect();
    unfiltered.push(("incident".into(), "caller_id".into()));
    let rows: Vec<Value> = unfiltered
        .iter()
        .map(|(n, e)| group(&[("name", n), ("element", e)], 1))
        .collect();
    mount_shard(
        &server,
        &format!("{TERMS}^name<task"),
        ResponseTemplate::new(200).set_body_json(json!({ "result": rows })),
    )
    .await;
    Mock::given(method("GET"))
        .and(path("/api/now/stats/sys_dictionary"))
        .and(query_param("sysparm_query", format!("{TERMS}^name>=task")))
        .respond_with(columns(&[("task", "number")]))
        .mount(&server)
        .await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile_for(&uri);
        let out = common::sn_cmd(tmp.path())
            .args(["cache", "refresh"])
            .assert()
            .code(2);
        let err = String::from_utf8_lossy(&out.get_output().stderr).to_string();
        assert!(err.contains("did not apply the range terms"), "{err}");
        assert!(!cache_file(tmp.path(), &uri).exists());
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn a_profile_without_dictionary_read_gets_a_tables_only_index() {
    let server = MockServer::start().await;
    mount_tables(&server).await;
    Mock::given(method("GET"))
        .and(path("/api/now/stats/sys_dictionary"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "error": {"message": "Insufficient rights to query records", "detail": null},
            "status": "failure"
        })))
        .mount(&server)
        .await;
    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile_for(&uri);
        let out = common::sn_cmd(tmp.path())
            .args(["--compact", "cache", "refresh"])
            .assert()
            .success();
        let report = stdout_json(&out);
        assert_eq!(report["columns_indexed"], false);
        assert_eq!(report["tables"], 4);
        let err = String::from_utf8_lossy(&out.get_output().stderr).to_string();
        assert!(err.contains("indexed table names only"), "{err}");

        let out = common::sn_cmd(tmp.path())
            .args(["--compact", "cache", "tables", "inc"])
            .assert()
            .success();
        assert_eq!(stdout_json(&out), json!(["incident"]));
        let out = common::sn_cmd(tmp.path())
            .args(["cache", "columns", "incident"])
            .assert()
            .code(1);
        let err = String::from_utf8_lossy(&out.get_output().stderr).to_string();
        assert!(err.contains("names tables only"), "{err}");
    })
    .await
    .unwrap();
}

#[test]
fn offline_verbs_without_a_cache_name_the_fix() {
    let tmp = common::write_profiles(
        "test",
        &[common::ProfileSpec {
            name: "test",
            instance: "nowhere.service-now.com",
            username: "u",
            password: "p",
        }],
    );
    let out = common::sn_cmd(tmp.path())
        .args(["--compact", "cache", "status"])
        .assert()
        .success();
    let status: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(status["exists"], false);
    assert_eq!(status["instance"], "nowhere.service-now.com");
    let out = common::sn_cmd(tmp.path())
        .args(["cache", "tables"])
        .assert()
        .code(1);
    let err = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(err.contains("sn cache refresh"), "{err}");
}

/// Seed a cache file directly and drive the binary the way a shell's dynamic
/// registration script does: `SN_COMPLETE=bash sn -- <words…>`.
fn seeded_completion_dir() -> tempfile::TempDir {
    let tmp = common::write_profiles(
        "test",
        &[common::ProfileSpec {
            name: "test",
            instance: "acme.service-now.com",
            username: "u",
            password: "p",
        }],
    );
    let mut tables = std::collections::BTreeMap::new();
    for (name, parent, cols) in [
        ("task", None, vec!["number", "short_description"]),
        ("incident", Some("task"), vec!["caller_id"]),
        ("incident_task", Some("task"), vec![]),
        ("cmdb_ci", None, vec!["name"]),
        ("cmdb_ci_linux_server", Some("cmdb_ci"), vec![]),
    ] {
        tables.insert(
            name.to_string(),
            sn::schema_cache::TableEntry {
                parent: parent.map(str::to_string),
                columns: cols.into_iter().map(str::to_string).collect(),
            },
        );
    }
    let index = sn::schema_cache::SchemaIndex {
        format: sn::schema_cache::FORMAT,
        instance: "acme.service-now.com".into(),
        built_at: 0,
        columns_indexed: true,
        tables,
    };
    let path = sn::schema_cache::cache_path_in(tmp.path(), "acme.service-now.com");
    sn::schema_cache::save(&path, &index).unwrap();
    tmp
}

fn complete(dir: &std::path::Path, words: &[&str]) -> Vec<String> {
    let out = common::sn_cmd(dir)
        .env("SN_COMPLETE", "bash")
        .env("_CLAP_IFS", "\n")
        .env("_CLAP_COMPLETE_INDEX", (words.len() - 1).to_string())
        .env("_CLAP_COMPLETE_COMP_TYPE", "9")
        .env("_CLAP_COMPLETE_SPACE", "true")
        .arg("--")
        .args(words)
        .assert()
        .success();
    String::from_utf8_lossy(&out.get_output().stdout)
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn dynamic_completion_offers_tables_and_columns_from_the_cache() {
    let tmp = seeded_completion_dir();
    let dir = tmp.path();
    assert_eq!(
        complete(dir, &["sn", "table", "list", "inc"]),
        ["incident", "incident_task"]
    );
    assert_eq!(
        complete(dir, &["sn", "table", "list", "incident", "-f", "number,sh"]),
        ["number,short_description"]
    );
    assert_eq!(
        complete(
            dir,
            &[
                "sn",
                "table",
                "update",
                "incident:INC0010001",
                "--field",
                "call"
            ]
        ),
        ["caller_id="]
    );
    assert_eq!(
        complete(dir, &["sn", "cmdb", "list", "cmdb_ci_l"]),
        ["cmdb_ci_linux_server"]
    );
    // A profile with no cache completes nothing, and says nothing.
    assert!(complete(dir, &["sn", "-p", "missing", "table", "list", "inc"]).is_empty());
}

#[test]
fn dynamic_registration_matches_the_env_entry_point() {
    let tmp = seeded_completion_dir();
    for shell in ["bash", "zsh", "fish", "powershell", "elvish"] {
        let via_env = common::sn_cmd(tmp.path())
            .env("SN_COMPLETE", shell)
            .assert()
            .success();
        let via_cmd = common::sn_cmd(tmp.path())
            .args(["completion", shell, "--dynamic"])
            .assert()
            .success();
        assert_eq!(
            via_env.get_output().stdout,
            via_cmd.get_output().stdout,
            "{shell}"
        );
        assert!(
            String::from_utf8_lossy(&via_cmd.get_output().stdout).contains("SN_COMPLETE"),
            "{shell}"
        );
    }
}
