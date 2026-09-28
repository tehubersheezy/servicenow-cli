//! Keyset `--all` pagination (issue #44).
//!
//! `FakeTable` is a stateful stand-in for the Table API that evaluates the
//! subset of encoded-query syntax a keyset walk emits, with ServiceNow's real
//! precedence: `^NQ` splits the query into OR'd segments and every other term
//! ANDs into the segment it sits in — so a cursor appended once constrains only
//! the last segment, the bug a naive keyset would ship. Row ACLs are applied
//! after the page is cut, and under `sysparm_no_count=true` `X-Total-Count` is
//! the number of rows scanned for the page; both as measured live.

mod common;

use common::{ProfileSpec, sn_cmd, write_profiles};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, Request, Respond, ResponseTemplate};

#[derive(Clone)]
struct Row {
    sys_id: String,
    grp: &'static str,
}

#[derive(Default)]
struct TableState {
    rows: Vec<Row>,
    hidden: Vec<String>,
    requests: usize,
    /// Delete this sys_id once the first request has been answered.
    delete_after_first: Option<String>,
    /// Answer this (1-based) request with a 503.
    fail_request: Option<usize>,
    /// Ignore `sys_id>` terms, as an instance that silently dropped them would.
    drop_cursor_terms: bool,
}

#[derive(Clone)]
struct FakeTable(Arc<Mutex<TableState>>);

impl FakeTable {
    fn new(rows: &[(&str, &'static str)]) -> Self {
        let mut rows: Vec<Row> = rows
            .iter()
            .map(|(id, grp)| Row {
                sys_id: (*id).into(),
                grp,
            })
            .collect();
        rows.sort_by(|a, b| a.sys_id.cmp(&b.sys_id));
        Self(Arc::new(Mutex::new(TableState {
            rows,
            ..Default::default()
        })))
    }

    /// Rows `01`..`NN`, all in group `a`.
    fn numbered(n: u32) -> Self {
        let ids = ids(1..=n);
        let rows: Vec<(&str, &'static str)> = ids.iter().map(|i| (i.as_str(), "a")).collect();
        Self::new(&rows)
    }

    fn with(self, f: impl FnOnce(&mut TableState)) -> Self {
        f(&mut self.0.lock().unwrap());
        self
    }
}

fn term_matches(term: &str, row: &Row, drop_cursor: bool) -> bool {
    if term.starts_with("ORDERBY") {
        return true;
    }
    if let Some(v) = term.strip_prefix("sys_id>") {
        return drop_cursor || row.sys_id.as_str() > v;
    }
    if let Some(v) = term.strip_prefix("grp=") {
        return row.grp == v;
    }
    panic!("FakeTable cannot evaluate term {term:?}");
}

impl Respond for FakeTable {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let mut st = self.0.lock().unwrap();
        st.requests += 1;
        let n = st.requests;
        if st.fail_request == Some(n) {
            return ResponseTemplate::new(503)
                .set_body_json(json!({"error": {"message": "unavailable"}}));
        }
        let param = |k: &str| {
            req.url
                .query_pairs()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.into_owned())
        };
        let query = param("sysparm_query").unwrap_or_default();
        let limit: usize = param("sysparm_limit").map_or(10_000, |v| v.parse().unwrap());
        let offset: usize = param("sysparm_offset").map_or(0, |v| v.parse().unwrap());
        let no_count = param("sysparm_no_count").as_deref() == Some("true");
        let fields = param("sysparm_fields");

        let segments: Vec<Vec<&str>> = query
            .split("^NQ")
            .map(|s| s.split('^').filter(|t| !t.is_empty()).collect())
            .collect();
        let matching: Vec<&Row> = st
            .rows
            .iter()
            .filter(|r| {
                segments
                    .iter()
                    .any(|seg| seg.iter().all(|t| term_matches(t, r, st.drop_cursor_terms)))
            })
            .collect();
        let scanned: Vec<&Row> = matching.iter().skip(offset).take(limit).copied().collect();
        let want = |f: &str| {
            fields
                .as_deref()
                .is_none_or(|fs| fs.split(',').any(|x| x == f))
        };
        let visible: Vec<Value> = scanned
            .iter()
            .filter(|r| !st.hidden.contains(&r.sys_id))
            .map(|r| {
                let mut rec = serde_json::Map::new();
                if want("sys_id") {
                    rec.insert("sys_id".into(), json!(r.sys_id));
                }
                if want("grp") {
                    rec.insert("grp".into(), json!(r.grp));
                }
                Value::Object(rec)
            })
            .collect();
        let mut resp = ResponseTemplate::new(200).set_body_json(json!({ "result": visible }));
        if no_count {
            resp = resp.insert_header("X-Total-Count", scanned.len().to_string().as_str());
        } else {
            resp = resp.insert_header("X-Total-Count", matching.len().to_string().as_str());
            if offset + limit < matching.len() {
                let kept: Vec<(String, String)> = req
                    .url
                    .query_pairs()
                    .filter(|(k, _)| k != "sysparm_offset")
                    .map(|(k, v)| (k.into_owned(), v.into_owned()))
                    .collect();
                // `req.url` has lost the port; the Host header still has it.
                let host = req.headers.get("host").unwrap().to_str().unwrap();
                let mut next: reqwest::Url =
                    format!("http://{host}{}", req.url.path()).parse().unwrap();
                next.query_pairs_mut()
                    .clear()
                    .extend_pairs(kept)
                    .append_pair("sysparm_offset", &(offset + limit).to_string());
                resp = resp.insert_header("Link", format!("<{next}>;rel=\"next\"").as_str());
            }
        }
        if n == 1
            && let Some(id) = st.delete_after_first.take()
        {
            st.rows.retain(|r| r.sys_id != id);
        }
        resp
    }
}

async fn mount(table: &FakeTable) -> wiremock::MockServer {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(table.clone())
        .mount(&server)
        .await;
    server
}

struct Run {
    code: i32,
    ids: Vec<String>,
    stdout: String,
    stderr: String,
}

/// `sn table list incident --all <args>` against `server`.
async fn run(server: &wiremock::MockServer, args: &[&str]) -> Run {
    let uri = server.uri();
    let args: Vec<String> = args.iter().map(ToString::to_string).collect();
    tokio::task::spawn_blocking(move || {
        let tmp = write_profiles(
            "test",
            &[ProfileSpec {
                name: "test",
                instance: &uri,
                username: "u",
                password: "p",
            }],
        );
        let out = sn_cmd(tmp.path())
            .args(["table", "list", "incident", "--all"])
            .args(&args)
            // A regression here is an endless page loop; fail, don't hang.
            .timeout(Duration::from_secs(20))
            .output()
            .unwrap();
        let stdout = String::from_utf8(out.stdout).unwrap();
        let ids = stdout
            .lines()
            .map(|l| {
                let v: Value = serde_json::from_str(l).unwrap();
                v["sys_id"].as_str().unwrap_or("").to_string()
            })
            .collect();
        Run {
            code: out.status.code().unwrap_or(-1),
            ids,
            stdout,
            stderr: String::from_utf8(out.stderr).unwrap(),
        }
    })
    .await
    .unwrap()
}

async fn queries(server: &wiremock::MockServer) -> Vec<Vec<(String, String)>> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| {
            r.url
                .query_pairs()
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect()
        })
        .collect()
}

fn get<'a>(q: &'a [(String, String)], key: &str) -> Option<&'a str> {
    q.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

fn ids(range: std::ops::RangeInclusive<u32>) -> Vec<String> {
    range.map(|i| format!("{i:02}")).collect()
}

#[tokio::test(flavor = "current_thread")]
async fn keyset_is_the_default_and_skips_the_count() {
    let server = mount(&FakeTable::numbered(7)).await;
    let r = run(&server, &["--setlimit", "3"]).await;
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(r.ids, ids(1..=7));

    let qs = queries(&server).await;
    // 3 + 3 + 1 rows. The third page scanned fewer rows than the limit, so it
    // is the last: no trailing probe for an empty page (on a large table with
    // a selective filter that probe walks the primary key to the end).
    assert_eq!(qs.len(), 3, "{qs:?}");
    for q in &qs {
        assert_eq!(get(q, "sysparm_no_count"), Some("true"));
        assert_eq!(get(q, "sysparm_suppress_pagination_header"), Some("true"));
        assert_eq!(get(q, "sysparm_offset"), None);
    }
    assert_eq!(get(&qs[0], "sysparm_query"), Some("ORDERBYsys_id"));
    assert_eq!(
        get(&qs[1], "sysparm_query"),
        Some("sys_id>03^ORDERBYsys_id")
    );
    assert_eq!(
        get(&qs[2], "sysparm_query"),
        Some("sys_id>06^ORDERBYsys_id")
    );
}

/// A full last page cannot know it is last; one more page that scans nothing
/// ends the walk.
#[tokio::test(flavor = "current_thread")]
async fn keyset_ends_on_an_empty_scan_after_a_full_page() {
    let server = mount(&FakeTable::numbered(6)).await;
    let r = run(&server, &["--setlimit", "3"]).await;
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(r.ids, ids(1..=6));
    assert_eq!(queries(&server).await.len(), 3);
}

/// Without `X-Total-Count` there is no scanned count to read, so a short page
/// might only be ACL-filtered; the walk continues until a page is empty.
#[tokio::test(flavor = "current_thread")]
async fn keyset_without_the_count_header_ends_on_an_empty_page() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_query", "ORDERBYsys_id"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"result": [{"sys_id": "01"}]})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_query", "sys_id>01^ORDERBYsys_id"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"result": [{"sys_id": "02"}]})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": []})))
        .mount(&server)
        .await;
    let r = run(&server, &["--setlimit", "5"]).await;
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(r.ids, ["01", "02"]);
    assert_eq!(queries(&server).await.len(), 3);
}

/// The acceptance scenario: a row already emitted is deleted between pages.
/// Offset shifts every later row down one and skips one; keyset does not.
#[tokio::test(flavor = "current_thread")]
async fn keyset_survives_a_delete_mid_walk_where_offset_skips_a_row() {
    let mk = || FakeTable::numbered(8).with(|s| s.delete_after_first = Some("02".into()));

    let offset = mount(&mk()).await;
    let r = run(&offset, &["--setlimit", "3", "--paginate", "offset"]).await;
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(
        !r.ids.contains(&"04".to_string()),
        "offset was expected to skip 04 (the demonstration): {:?}",
        r.ids
    );

    let keyset = mount(&mk()).await;
    let r = run(&keyset, &["--setlimit", "3"]).await;
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(r.ids, ids(1..=8), "every row, each exactly once");
}

/// `^NQ` regression. A cursor appended once binds only the last segment, so
/// the first segment (larger than a page here) would come back from the top
/// on every page. The walk must distribute the cursor and finish.
#[tokio::test(flavor = "current_thread")]
async fn keyset_distributes_the_cursor_into_every_nq_segment() {
    let table = FakeTable::new(&[
        ("01", "a"),
        ("02", "b"),
        ("03", "a"),
        ("04", "a"),
        ("05", "c"),
        ("06", "b"),
        ("07", "a"),
        ("08", "b"),
    ]);
    let server = mount(&table).await;
    let r = run(&server, &["-q", "grp=a^NQgrp=b", "--setlimit", "2"]).await;
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(r.ids, ["01", "02", "03", "04", "06", "07", "08"]);

    let qs = queries(&server).await;
    assert_eq!(
        get(&qs[1], "sysparm_query"),
        Some("grp=a^sys_id>02^NQgrp=b^sys_id>02^ORDERBYsys_id")
    );
}

/// `^OR` binds tighter than the trailing AND (measured), so the cursor is
/// appended once, not rewritten into the group.
#[tokio::test(flavor = "current_thread")]
async fn keyset_appends_the_cursor_once_to_an_or_group() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_query", "grp=a^ORgrp=b^ORDERBYsys_id"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("X-Total-Count", "2")
                .set_body_json(json!({"result": [{"sys_id": "01"}, {"sys_id": "02"}]})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("X-Total-Count", "0")
                .set_body_json(json!({"result": []})),
        )
        .mount(&server)
        .await;
    let r = run(&server, &["-q", "grp=a^ORgrp=b", "--setlimit", "2"]).await;
    assert_eq!(r.code, 0, "{}", r.stderr);
    let qs = queries(&server).await;
    assert_eq!(
        get(&qs[1], "sysparm_query"),
        Some("grp=a^ORgrp=b^sys_id>02^ORDERBYsys_id")
    );
}

/// Row ACLs cut a page after the database fetched it: a short page is not the
/// end, and a page can be entirely hidden. The walk steps over a hidden page
/// with `sysparm_offset` inside the cursor-bounded set and keeps going.
#[tokio::test(flavor = "current_thread")]
async fn keyset_walks_past_pages_hidden_by_row_acls() {
    let table = FakeTable::numbered(10).with(|s| {
        s.hidden = ["02", "04", "05", "06", "07"].map(String::from).to_vec();
    });
    let server = mount(&table).await;
    let r = run(&server, &["--setlimit", "3"]).await;
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(r.ids, ["01", "03", "08", "09", "10"]);
    let qs = queries(&server).await;
    assert!(
        qs.iter().any(|q| get(q, "sysparm_offset") == Some("3")
            && get(q, "sysparm_query") == Some("sys_id>03^ORDERBYsys_id")),
        "a fully hidden page should be stepped over by offset: {qs:?}"
    );
}

/// ServiceNow silently drops query terms it cannot evaluate. A dropped cursor
/// would return the same page forever; the walk must fail instead.
#[tokio::test(flavor = "current_thread")]
async fn keyset_fails_instead_of_looping_when_the_cursor_is_dropped() {
    let table = FakeTable::numbered(5).with(|s| s.drop_cursor_terms = true);
    let server = mount(&table).await;
    let r = run(&server, &["--setlimit", "2"]).await;
    assert_eq!(r.code, 2, "stdout: {} stderr: {}", r.stdout, r.stderr);
    assert!(
        r.stderr.contains("ignored the keyset cursor"),
        "{}",
        r.stderr
    );
    // Resuming would only trip the same canary, so no token is offered.
    assert!(!r.stderr.contains("resume_from"), "{}", r.stderr);
    assert_eq!(r.ids, ["01", "02"]);
}

/// Past the first page, a record without a sys_id is a malformed page (seen
/// once live under heavy load), not a property of the table: it fails with the
/// token to resume from the last good record, and writes none of that page.
#[tokio::test(flavor = "current_thread")]
async fn a_mid_walk_record_without_sys_id_fails_resumably() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_query", "ORDERBYsys_id"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("X-Total-Count", "2")
                .set_body_json(json!({"result": [{"sys_id": "01"}, {"sys_id": "02"}]})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("X-Total-Count", "2")
                .set_body_json(json!({"result": [{"sys_id": "03"}, {"n": 4}]})),
        )
        .mount(&server)
        .await;
    let r = run(&server, &["--setlimit", "2"]).await;
    assert_eq!(r.code, 3, "{}", r.stderr);
    assert_eq!(r.ids, ["01", "02"]);
    let env: Value = serde_json::from_str(r.stderr.trim()).unwrap();
    assert_eq!(env["error"]["resume_from"], "02");
}

/// A table with no seekable sys_id (a database view answers with a synthetic
/// `__ENC__…` id, measured) restarts in offset mode before writing anything.
#[tokio::test(flavor = "current_thread")]
async fn keyset_falls_back_to_offset_when_records_have_no_plain_sys_id() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_no_count", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [{"sys_id": "__ENC__YWJj=-ZGVm=", "n": 1}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": [{"sys_id": "__ENC__YWJj=-ZGVm=", "n": 1}, {"sys_id": "__ENC__eHl6=", "n": 2}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let r = run(&server, &[]).await;
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(r.stdout.lines().count(), 2, "{}", r.stdout);
}

/// An interrupted JSONL walk names the token; resuming from it finishes the
/// export with no gap and no overlap.
#[tokio::test(flavor = "current_thread")]
async fn interrupted_walk_names_the_token_and_resume_finishes_it() {
    let table = FakeTable::numbered(7).with(|s| s.fail_request = Some(2));
    let server = mount(&table).await;
    let first = run(&server, &["--setlimit", "3"]).await;
    assert_eq!(first.code, 2, "{}", first.stderr);
    assert_eq!(first.ids, ids(1..=3));
    let env: Value = serde_json::from_str(first.stderr.trim()).unwrap();
    assert_eq!(env["error"]["resume_from"], "03");
    assert_eq!(env["error"]["status_code"], 503);

    let second = run(&server, &["--setlimit", "3", "--resume-from", "03"]).await;
    assert_eq!(second.code, 0, "{}", second.stderr);
    let mut all = first.ids.clone();
    all.extend(second.ids);
    assert_eq!(all, ids(1..=7));
}

/// `--array` writes nothing until the end, so a failure there is not
/// resumable and must not claim to be.
#[tokio::test(flavor = "current_thread")]
async fn array_failure_carries_no_resume_token() {
    let table = FakeTable::numbered(7).with(|s| s.fail_request = Some(2));
    let server = mount(&table).await;
    let r = run(&server, &["--setlimit", "3", "--array"]).await;
    assert_eq!(r.code, 2, "{}", r.stderr);
    assert!(r.stdout.is_empty(), "{}", r.stdout);
    assert!(!r.stderr.contains("resume_from"), "{}", r.stderr);
}

/// The cursor needs sys_id in the response; when `--fields` left it out it is
/// requested anyway and removed from each record again.
#[tokio::test(flavor = "current_thread")]
async fn keyset_requests_sys_id_but_outputs_only_the_fields_asked_for() {
    let server = mount(&FakeTable::numbered(4)).await;
    let r = run(&server, &["-f", "grp", "--setlimit", "3"]).await;
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(r.stdout.lines().count(), 4);
    for line in r.stdout.lines() {
        assert_eq!(line, r#"{"grp":"a"}"#);
    }
    let qs = queries(&server).await;
    assert_eq!(get(&qs[0], "sysparm_fields"), Some("grp,sys_id"));
}

/// A query with its own sort cannot be keyset-walked; it pages by Link, which
/// needs the count — so no `sysparm_no_count` there.
#[tokio::test(flavor = "current_thread")]
async fn orderby_query_falls_back_to_link_pagination() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"result": [{"sys_id": "01"}]})),
        )
        .mount(&server)
        .await;
    let r = run(&server, &["-q", "active=true^ORDERBYnumber"]).await;
    assert_eq!(r.code, 0, "{}", r.stderr);
    let qs = queries(&server).await;
    assert_eq!(qs.len(), 1);
    assert_eq!(
        get(&qs[0], "sysparm_query"),
        Some("active=true^ORDERBYnumber")
    );
    assert_eq!(get(&qs[0], "sysparm_no_count"), None);
}

/// A 200 whose body has no `result` array is not an empty last page: ending the
/// walk there would be a silently short export. It fails, resumably.
#[tokio::test(flavor = "current_thread")]
async fn a_page_without_a_result_array_fails_instead_of_ending_the_walk() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_query", "ORDERBYsys_id"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("X-Total-Count", "2")
                .set_body_json(json!({"result": [{"sys_id": "01"}, {"sys_id": "02"}]})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&server)
        .await;
    let r = run(&server, &["--setlimit", "2"]).await;
    assert_eq!(r.code, 3, "{}", r.stderr);
    assert_eq!(r.ids, ["01", "02"]);
    let env: Value = serde_json::from_str(r.stderr.trim()).unwrap();
    assert_eq!(env["error"]["resume_from"], "02");
}

/// An offset walk keeps going past a page whose rows were all hidden by row
/// ACLs, as long as the Link header says there is a next page.
#[tokio::test(flavor = "current_thread")]
async fn offset_walk_continues_past_an_empty_page_with_a_next_link() {
    let table = FakeTable::numbered(9).with(|s| {
        s.hidden = ["04", "05", "06"].map(String::from).to_vec();
    });
    let server = mount(&table).await;
    let r = run(&server, &["--setlimit", "3", "--paginate", "offset"]).await;
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(r.ids, ["01", "02", "03", "07", "08", "09"]);
}

/// Argv-only conflicts are exit 1 before any request.
#[test]
fn keyset_usage_errors_before_connecting() {
    let tmp = write_profiles(
        "test",
        &[ProfileSpec {
            name: "test",
            instance: "http://127.0.0.1:1",
            username: "u",
            password: "p",
        }],
    );
    for (args, needle) in [
        (
            vec!["-q", "ORDERBYnumber", "--paginate", "keyset"],
            "ORDERBY",
        ),
        (vec!["--paginate", "offset", "--no-count"], "--no-count"),
        (
            vec!["--paginate", "offset", "--suppress-pagination-header"],
            "--suppress-pagination-header",
        ),
        (
            vec!["--paginate", "offset", "--resume-from", "abc"],
            "--resume-from",
        ),
        (
            vec!["-q", "ORDERBYnumber", "--resume-from", "abc"],
            "ORDERBY",
        ),
        (vec!["--resume-from", "abc^NQactive=true"], "not a sys_id"),
    ] {
        let out = sn_cmd(tmp.path())
            .args(["table", "list", "incident", "--all"])
            .args(&args)
            .assert()
            .code(1);
        let stderr = String::from_utf8(out.get_output().stderr.clone()).unwrap();
        assert!(stderr.contains(needle), "{args:?}: {stderr}");
    }
    // Both flags mean nothing without --all.
    for flag in [["--resume-from", "abc"], ["--paginate", "keyset"]] {
        sn_cmd(tmp.path())
            .args(["table", "list", "incident"])
            .args(flag)
            .assert()
            .code(1);
    }
}
