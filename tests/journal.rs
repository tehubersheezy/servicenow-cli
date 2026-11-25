mod common;

use serde_json::json;
use wiremock::matchers::{body_string_contains, method, path};
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

const SYS_ID: &str = "47a91e3c2f8acf107efd1d707fa4e387";

fn stream_response(col: &str, stream: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "data": {"GlideRecord_Query": {"incident": {"_results": [
            {col: {"displayValue": stream}}
        ]}}}
    }))
}

#[tokio::test(flavor = "current_thread")]
async fn record_source_parses_the_rendered_stream() {
    let server = MockServer::start().await;
    let stream = "2026-08-11 10:40:20 - Abey Ahmad (Work notes)\nchecked the router\n\n\
                  2026-08-10 07:40:58 - Abey Ahmad (Comments)\nuser called back\n\n";
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .and(body_string_contains("comments_and_work_notes"))
        .respond_with(stream_response("comments_and_work_notes", stream))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&server_uri);
        let out = common::sn_cmd(tmp.path())
            .args(["--compact", "journal", "incident", SYS_ID])
            .assert()
            .success();
        let s = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        let entries = v.as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["element"], "work_notes");
        assert_eq!(entries[0]["author"], "Abey Ahmad");
        assert_eq!(entries[0]["text"], "checked the router");
        assert_eq!(entries[1]["element"], "comments");
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn comments_filter_queries_only_that_column() {
    let server = MockServer::start().await;
    // Mounted first: reject any request for the combined or work_notes column.
    for wrong in ["comments_and_work_notes", "work_notes"] {
        Mock::given(method("POST"))
            .and(path("/api/now/graphql"))
            .and(body_string_contains(wrong))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
    }
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .respond_with(stream_response(
            "comments",
            "2026-08-10 07:40:58 - Abey Ahmad (Comments)\nuser called back\n\n",
        ))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&server_uri);
        let out = common::sn_cmd(tmp.path())
            .args(["--compact", "journal", "incident", SYS_ID, "--comments"])
            .assert()
            .success();
        let s = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        assert!(s.contains("user called back"), "stdout was: {s}");
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn missing_combined_column_falls_back_to_single_columns() {
    let server = MockServer::start().await;
    // The combined column does not exist on this table: FieldUndefined.
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .and(body_string_contains("comments_and_work_notes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": null,
            "errors": [{
                "message": "Validation error (FieldUndefined@[x]) : Field 'comments_and_work_notes' in type 'GlideRecord_TableResultsType_incident' is undefined",
                "errorType": "ValidationError"
            }]
        })))
        .mount(&server)
        .await;
    // The both-columns retry succeeds.
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .and(body_string_contains("work_notes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {"GlideRecord_Query": {"incident": {"_results": [{
                "comments": {"displayValue": "2026-08-10 07:00:00 - A B (Comments)\nolder comment\n\n"},
                "work_notes": {"displayValue": "2026-08-11 09:00:00 - A B (Work notes)\nnewer note\n\n"}
            }]}}}
        })))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&server_uri);
        let out = common::sn_cmd(tmp.path())
            .args(["--compact", "journal", "incident", SYS_ID])
            .assert()
            .success();
        let s = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        let entries = v.as_array().unwrap();
        assert_eq!(entries.len(), 2);
        // Merged across the two streams and re-sorted newest first.
        assert_eq!(entries[0]["text"], "newer note");
        assert_eq!(entries[1]["text"], "older comment");
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn missing_record_is_a_404_style_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {"GlideRecord_Query": {"incident": {"_results": []}}}
        })))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&server_uri);
        let out = common::sn_cmd(tmp.path())
            .args(["journal", "incident", SYS_ID])
            .assert()
            .code(2);
        let err = String::from_utf8(out.get_output().stderr.clone()).unwrap();
        assert!(err.contains(SYS_ID), "stderr was: {err}");
        assert!(err.contains("404"), "stderr was: {err}");
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn table_source_returns_exact_rows() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .and(body_string_contains("sys_journal_field"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {"GlideRecord_Query": {"sys_journal_field": {
                "_rowCount": 2,
                "_results": [
                    {"element": {"value": "work_notes"}, "value": {"value": "newer note"},
                     "sys_created_on": {"value": "2026-08-11 17:40:20"},
                     "sys_created_by": {"value": "abeyahmad"}},
                    {"element": {"value": "comments"}, "value": {"value": "older comment"},
                     "sys_created_on": {"value": "2026-08-10 14:40:58"},
                     "sys_created_by": {"value": "abeyahmad"}}
                ]
            }}}
        })))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&server_uri);
        let out = common::sn_cmd(tmp.path())
            .args([
                "--compact",
                "journal",
                "incident",
                SYS_ID,
                "--source",
                "table",
            ])
            .assert()
            .success();
        let s = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        let entries = v.as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["element"], "work_notes");
        assert_eq!(entries[0]["author"], "abeyahmad");
        assert_eq!(entries[0]["created_on"], "2026-08-11 17:40:20");
        assert!(entries[0].get("label").is_none());
    })
    .await
    .unwrap();
}

/// GraphQL returns `value: null` for a note whose text is the string "null";
/// the entry keeps the text from `displayValue`.
#[tokio::test(flavor = "current_thread")]
async fn table_source_keeps_a_literal_null_note() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .and(body_string_contains("value { value displayValue }"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {"GlideRecord_Query": {"sys_journal_field": {
                "_rowCount": 1,
                "_results": [
                    {"element": {"value": "comments"},
                     "value": {"value": null, "displayValue": "null"},
                     "sys_created_on": {"value": "2026-08-11 17:40:20"},
                     "sys_created_by": {"value": "abeyahmad"}}
                ]
            }}}
        })))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&server_uri);
        let out = common::sn_cmd(tmp.path())
            .args([
                "--compact",
                "journal",
                "incident",
                SYS_ID,
                "--source",
                "table",
            ])
            .assert()
            .success();
        let v: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
        assert_eq!(v[0]["text"], "null");
    })
    .await
    .unwrap();
}

/// A dot-separated time format parses, a casual header lookalike stays in the
/// note's text, and the field's label is fetched in the same document so a
/// custom journal label is recognised.
#[tokio::test(flavor = "current_thread")]
async fn record_source_header_rules() {
    let server = MockServer::start().await;
    let stream = "11-21-2026 07.55.49 AM - Abey Ahmad (Work notes)\ncalled user\n\
                  10:30 - no answer (voicemail)\nwill retry\n\n\
                  11-20-2026 06.10.02 PM - Abey Ahmad (Customer updates)\nolder\n\n";
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .and(body_string_contains(
            "comments_and_work_notes { label displayValue }",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {"GlideRecord_Query": {"incident": {"_results": [
                {"comments_and_work_notes": {"label": "Customer updates", "displayValue": stream}}
            ]}}}
        })))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&server_uri);
        let out = common::sn_cmd(tmp.path())
            .args(["--compact", "journal", "incident", SYS_ID])
            .assert()
            .success();
        let v: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
        let entries = v.as_array().unwrap();
        assert_eq!(entries.len(), 2, "{v}");
        assert_eq!(
            entries[0]["text"],
            "called user\n10:30 - no answer (voicemail)\nwill retry"
        );
        assert_eq!(entries[1]["label"], "Customer updates");
        assert_eq!(entries[1]["created_on"], "11-20-2026 06.10.02 PM");
    })
    .await
    .unwrap();
}

/// `--output table` escapes control bytes from note text instead of
/// passing them to the terminal.
#[tokio::test(flavor = "current_thread")]
async fn table_output_escapes_control_bytes() {
    let server = MockServer::start().await;
    let stream = "2026-08-11 10:40:20 - A B (Comments)\nhi\u{1b}]0;pwned\u{7}there\n\n";
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .respond_with(stream_response("comments_and_work_notes", stream))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&server_uri);
        let out = common::sn_cmd(tmp.path())
            .args(["--output", "table", "journal", "incident", SYS_ID])
            .assert()
            .success();
        let s = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        assert!(!s.contains('\u{1b}') && !s.contains('\u{7}'), "{s}");
        assert!(s.contains("\\u{1b}]0;pwned\\u{7}there"), "{s}");
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn acl_filtered_table_source_names_the_cause() {
    let server = MockServer::start().await;
    // The signature measured live as an itil user: the count leaks through
    // row ACLs, the rows do not.
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {"GlideRecord_Query": {"sys_journal_field": {
                "_rowCount": 2, "_results": []
            }}}
        })))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&server_uri);
        let out = common::sn_cmd(tmp.path())
            .args(["journal", "incident", SYS_ID, "--source", "table"])
            .assert()
            .code(2);
        let err = String::from_utf8(out.get_output().stderr.clone()).unwrap();
        assert!(err.contains("ACL"), "stderr was: {err}");
        assert!(err.contains("--source record"), "stderr was: {err}");
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn raw_emits_the_unparsed_stream() {
    let server = MockServer::start().await;
    let stream = "2026-08-10 07:40:58 - Abey Ahmad (Comments)\nuser called back\n\n";
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .respond_with(stream_response("comments_and_work_notes", stream))
        .mount(&server)
        .await;
    let server_uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let tmp = profile(&server_uri);
        let out = common::sn_cmd(tmp.path())
            .args(["--compact", "journal", "incident", SYS_ID, "--raw"])
            .assert()
            .success();
        let s = String::from_utf8(out.get_output().stdout.clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert!(v.as_str().unwrap().contains("user called back"));
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn invalid_table_name_is_a_usage_error_without_a_request() {
    // No mock server: a bad identifier must fail before any HTTP.
    let tmp = profile("https://unused.example.com");
    common::sn_cmd(tmp.path())
        .args(["journal", "incident; drop", SYS_ID])
        .assert()
        .code(1);
    common::sn_cmd(tmp.path())
        .args(["journal", "incident", "bad^sys_id"])
        .assert()
        .code(1);
}
