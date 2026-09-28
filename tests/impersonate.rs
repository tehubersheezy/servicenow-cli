//! `sn impersonate <USER> -- <COMMAND>`: the wrapped command must run on the
//! impersonated session (cookie only, never the profile's credential), and the
//! impersonation must end on every exit path — success, a failing command, a
//! refused hop, and Ctrl-C.
//!
//! One stateful responder plays the instance: it mints a session for a
//! credentialed request, tracks who that session currently is, honors (or, to
//! reproduce the live instance's silent refusal, ignores) impersonate POSTs,
//! and kills the session at `/logout.do`.

mod common;

use common::{ProfileSpec, sn_cmd, write_profiles};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use wiremock::matchers::any;
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

const REAL: &str = "real_user";
const REAL_ID: &str = "218517762f7903107efd1d707fa4e3de";
const ABEL: &str = "abel.tuter";
const ABEL_ID: &str = "62826bf03710200044e0bfc8bcbe5df1";
const SESSION_ID: &str = "SESSIONCOOKIE123";
/// The session's CSRF token. Greppable: it must never reach stdout or stderr.
const TOKEN: &str = "LEAKYSESSIONTOKEN0xC5RF";

#[derive(Default)]
struct State {
    minted: bool,
    current: Option<&'static str>,
    logged_out: bool,
    /// Requests after the mint that carried `Authorization` — each one would
    /// have re-authenticated as the profile's user and wiped the hop.
    credentialed_after_mint: u32,
    events: Vec<String>,
}

#[derive(Clone)]
struct Instance {
    state: Arc<Mutex<State>>,
    can_impersonate: bool,
    /// Answer impersonate POSTs 201 without switching (what the live
    /// instance does for a caller without the role).
    ignore_hops: bool,
    incident_delay: Option<Duration>,
    incident_status: u16,
}

impl Instance {
    fn new() -> Self {
        Instance {
            state: Arc::default(),
            can_impersonate: true,
            ignore_hops: false,
            incident_delay: None,
            incident_status: 200,
        }
    }

    fn events(&self) -> Vec<String> {
        self.state.lock().unwrap().events.clone()
    }
}

fn header<'a>(req: &'a Request, name: &str) -> Option<&'a str> {
    req.headers.get(name).and_then(|v| v.to_str().ok())
}

fn unauthenticated() -> ResponseTemplate {
    ResponseTemplate::new(401).set_body_json(
        json!({"error": {"message": "User is not authenticated"}, "status": "failure"}),
    )
}

impl Respond for Instance {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let mut s = self.state.lock().unwrap();
        let path = req.url.path().to_string();
        let credentialed = header(req, "authorization").is_some();
        let has_session =
            header(req, "cookie").is_some_and(|c| c.contains(&format!("JSESSIONID={SESSION_ID}")));

        // Mint: the one request that may carry the profile's credential.
        if credentialed && path == "/api/now/sg/impersonation/session" {
            s.minted = true;
            s.logged_out = false;
            s.current = Some(REAL);
            s.events.push("mint".into());
            return ResponseTemplate::new(200)
                .append_header(
                    "Set-Cookie",
                    format!("JSESSIONID={SESSION_ID}; Path=/; HttpOnly"),
                )
                .append_header("Set-Cookie", "glide_user_route=node1; Path=/")
                .append_header("Set-Cookie", "glide_user=; Max-Age=0; Path=/")
                .set_body_json(json!({
                    "CurrentUser": REAL, "OriginalUser": REAL,
                    "CanImpersonate": self.can_impersonate, "admin": true,
                    "InactivityTimeout": 90, "SessionToken": TOKEN,
                }));
        }
        if credentialed {
            s.credentialed_after_mint += 1;
        }
        if !has_session || s.logged_out {
            s.events.push(format!("401 {path}"));
            return unauthenticated();
        }
        let current = s.current.unwrap_or(REAL);

        match (req.method.as_str(), path.as_str()) {
            ("GET", "/api/now/sg/impersonation/session") => {
                s.events.push(format!("session {current}"));
                ResponseTemplate::new(200).set_body_json(json!({
                    "CurrentUser": current, "OriginalUser": REAL,
                    "CanImpersonate": self.can_impersonate, "admin": current == REAL,
                    "InactivityTimeout": 90, "SessionToken": TOKEN,
                }))
            }
            ("GET", "/api/now/table/sys_user") => {
                let q = req
                    .url
                    .query_pairs()
                    .find(|(k, _)| k == "sysparm_query")
                    .map(|(_, v)| v.into_owned())
                    .unwrap_or_default();
                s.events.push(format!("lookup {q}"));
                let rows = match q.as_str() {
                    "user_name=real_user" => json!([{"sys_id": REAL_ID, "user_name": REAL}]),
                    "user_name=abel.tuter" | "sys_id=62826bf03710200044e0bfc8bcbe5df1" => {
                        json!([{"sys_id": ABEL_ID, "user_name": ABEL}])
                    }
                    "user_name=everyone" => json!([
                        {"sys_id": REAL_ID, "user_name": REAL},
                        {"sys_id": ABEL_ID, "user_name": ABEL},
                    ]),
                    _ => json!([]),
                };
                ResponseTemplate::new(200).set_body_json(json!({"result": rows}))
            }
            ("POST", p) if p.starts_with("/api/now/ui/impersonate/") => {
                assert_eq!(
                    header(req, "x-usertoken"),
                    Some(TOKEN),
                    "hop sent without CSRF token"
                );
                let id = p.rsplit('/').next().unwrap();
                let to = if id == ABEL_ID { ABEL } else { REAL };
                s.events.push(format!("impersonate {to}"));
                if !self.ignore_hops && (self.can_impersonate || to == REAL) {
                    s.current = Some(to);
                }
                ResponseTemplate::new(201)
                    .set_body_json(json!({"result": {"user": "null", "impersonatedUser": id}}))
            }
            ("GET", "/logout.do") => {
                s.logged_out = true;
                s.events.push("logout".into());
                ResponseTemplate::new(200).set_body_string("<html>logged out</html>")
            }
            ("GET", "/api/now/table/incident") => {
                s.events.push(format!("incident as {current}"));
                let mut t = ResponseTemplate::new(self.incident_status).set_body_json(
                    if self.incident_status == 200 {
                        json!({"result": [{"number": format!("INC-{current}")}]})
                    } else {
                        json!({"error": {"message": "No Record found"}, "status": "failure"})
                    },
                );
                if let Some(d) = self.incident_delay {
                    t = t.set_delay(d);
                }
                t
            }
            _ => ResponseTemplate::new(404),
        }
    }
}

async fn serve(instance: &Instance) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(instance.clone())
        .mount(&server)
        .await;
    server
}

fn profile(uri: &str) -> tempfile::TempDir {
    write_profiles(
        "p1",
        &[ProfileSpec {
            name: "p1",
            instance: uri,
            username: REAL,
            password: "pw",
        }],
    )
}

fn run(uri: &str, args: &[&str]) -> (i32, String, String) {
    let tmp = profile(uri);
    let out = sn_cmd(tmp.path()).args(args).output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// The impersonation was undone and the session killed, in that order, after
/// everything else.
fn assert_ended(events: &[String]) {
    let tail: Vec<&str> = events
        .iter()
        .rev()
        .take(3)
        .rev()
        .map(String::as_str)
        .collect();
    assert_eq!(
        tail,
        [
            "impersonate real_user",
            "logout",
            "401 /api/now/sg/impersonation/session"
        ],
        "session must be reverted, logged out, then confirmed gone: {events:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn wrapped_command_runs_as_the_target_on_the_session_alone_then_ends_it() {
    let inst = Instance::new();
    let server = serve(&inst).await;
    let (code, stdout, stderr) = run(
        &server.uri(),
        &[
            "impersonate",
            ABEL,
            "-ddd",
            "--",
            "sn",
            "table",
            "list",
            "incident",
        ],
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    // stdout is exactly the wrapped command's output — nothing added.
    let v: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v, json!([{"number": "INC-abel.tuter"}]));

    let events = inst.events();
    assert_eq!(
        &events[..6],
        [
            "mint",
            "lookup user_name=real_user",
            "lookup user_name=abel.tuter",
            "impersonate abel.tuter",
            "session abel.tuter",
            "incident as abel.tuter",
        ],
        "{events:?}"
    );
    assert_ended(&events);
    assert_eq!(
        inst.state.lock().unwrap().credentialed_after_mint,
        0,
        "the profile's credential must not ride along after the mint"
    );
    // Even at -ddd, the CSRF token and the session cookie stay out of the logs.
    assert!(
        !stdout.contains(TOKEN) && !stderr.contains(TOKEN),
        "{stderr}"
    );
    assert!(!stderr.contains(SESSION_ID), "{stderr}");
}

#[tokio::test(flavor = "current_thread")]
async fn a_sys_id_names_the_target_too() {
    let inst = Instance::new();
    let server = serve(&inst).await;
    let (code, _, stderr) = run(
        &server.uri(),
        &["impersonate", ABEL_ID, "--", "table", "list", "incident"],
    );
    assert_eq!(code, 0, "{stderr}");
    assert!(inst.events().contains(&format!("lookup sys_id={ABEL_ID}")));
}

#[tokio::test(flavor = "current_thread")]
async fn a_failing_command_keeps_its_exit_code_and_still_ends_the_session() {
    let mut inst = Instance::new();
    inst.incident_status = 404;
    let server = serve(&inst).await;
    let (code, stdout, stderr) = run(
        &server.uri(),
        &["impersonate", ABEL, "--", "table", "list", "incident"],
    );
    assert_eq!(code, 2, "the wrapped command's own exit code: {stderr}");
    assert!(stdout.is_empty());
    let err: Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(err["error"]["status_code"], 404);
    assert_ended(&inst.events());
}

#[tokio::test(flavor = "current_thread")]
async fn cannot_impersonate_is_refused_before_the_hop_and_the_session_is_still_ended() {
    let mut inst = Instance::new();
    inst.can_impersonate = false;
    let server = serve(&inst).await;
    let (code, stdout, stderr) = run(
        &server.uri(),
        &["impersonate", ABEL, "--", "table", "list", "incident"],
    );
    assert_eq!(code, 4, "{stderr}");
    assert!(stdout.is_empty());
    let err: Value = serde_json::from_str(stderr.trim()).unwrap();
    assert!(
        err["error"]["message"]
            .as_str()
            .unwrap()
            .contains("CanImpersonate")
    );
    // Reported inside a 200: no invented HTTP status.
    assert!(err["error"].get("status_code").is_none(), "{err}");
    let events = inst.events();
    assert!(
        !events.iter().any(|e| e.starts_with("impersonate")),
        "{events:?}"
    );
    assert!(
        !events.iter().any(|e| e.starts_with("incident")),
        "{events:?}"
    );
    assert!(events.contains(&"logout".to_string()), "{events:?}");
}

#[tokio::test(flavor = "current_thread")]
async fn a_hop_the_instance_silently_ignored_never_runs_the_command() {
    // Live behavior: the impersonate POST answers 201 even when it did nothing.
    let mut inst = Instance::new();
    inst.ignore_hops = true;
    let server = serve(&inst).await;
    let (code, stdout, stderr) = run(
        &server.uri(),
        &["impersonate", ABEL, "--", "table", "list", "incident"],
    );
    assert_eq!(code, 2, "{stderr}");
    assert!(stdout.is_empty());
    assert!(stderr.contains("did not take effect"), "{stderr}");
    let events = inst.events();
    assert!(
        !events.iter().any(|e| e.starts_with("incident")),
        "{events:?}"
    );
    assert_ended(&events);
}

#[tokio::test(flavor = "current_thread")]
async fn unknown_and_ambiguous_users_fail_without_a_hop() {
    let inst = Instance::new();
    let server = serve(&inst).await;
    let (code, _, stderr) = run(&server.uri(), &["impersonate", "nobody", "--", "ping"]);
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("no user 'nobody'"), "{stderr}");

    let (code, _, stderr) = run(&server.uri(), &["impersonate", "everyone", "--", "ping"]);
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("more than one"), "{stderr}");
    assert!(!inst.events().iter().any(|e| e.starts_with("impersonate")));
}

#[tokio::test(flavor = "current_thread")]
async fn argv_mistakes_are_refused_before_any_request() {
    let inst = Instance::new();
    let server = serve(&inst).await;
    let cases: &[(&[&str], &str)] = &[
        (
            &["impersonate", ABEL, "--", "profile", "list"],
            "local profiles",
        ),
        (&["impersonate", ABEL, "--", "init"], "local profiles"),
        (
            &[
                "impersonate",
                ABEL,
                "--",
                "watch",
                "incident",
                "-q",
                "active=true",
            ],
            "websocket",
        ),
        (
            &["impersonate", ABEL, "--", "impersonate", "x", "--", "ping"],
            "do not nest",
        ),
        (
            &["impersonate", ABEL, "--", "open", "incident", ABEL_ID],
            "browser",
        ),
        (
            &[
                "impersonate",
                ABEL,
                "--",
                "table",
                "list",
                "incident",
                "--profile",
                "p1",
            ],
            "--profile must go before",
        ),
        (
            &["impersonate", ABEL, "--", "ping", "--timeout", "5"],
            "--timeout must go before",
        ),
        (
            &["impersonate", ABEL, "--", "frobnicate"],
            "after `--` is invalid",
        ),
        (&["impersonate", ABEL, "--", "sn"], "no command to run"),
        (
            &["impersonate", "a^ORuser_name=admin", "--", "ping"],
            "not a user_name",
        ),
    ];
    for (args, needle) in cases {
        let (code, stdout, stderr) = run(&server.uri(), args);
        assert_eq!(code, 1, "{args:?}: {stderr}");
        assert!(stdout.is_empty(), "{args:?}");
        assert!(
            stderr.contains(needle),
            "{args:?}: expected {needle:?} in {stderr}"
        );
    }
    // The command itself is required.
    let (code, _, _) = run(&server.uri(), &["impersonate", ABEL]);
    assert_eq!(code, 1);
    assert!(inst.events().is_empty(), "{:?}", inst.events());
}

#[tokio::test(flavor = "current_thread")]
async fn output_options_before_the_separator_reach_the_wrapped_command() {
    let inst = Instance::new();
    let server = serve(&inst).await;
    let (code, stdout, stderr) = run(
        &server.uri(),
        &[
            "impersonate",
            ABEL,
            "--output",
            "raw",
            "--",
            "table",
            "list",
            "incident",
        ],
    );
    assert_eq!(code, 0, "{stderr}");
    let v: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v["result"][0]["number"], "INC-abel.tuter", "{v}");
}

#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn ctrl_c_mid_command_ends_the_session_and_exits_130() {
    let mut inst = Instance::new();
    inst.incident_delay = Some(Duration::from_secs(20));
    let server = serve(&inst).await;
    let tmp = profile(&server.uri());
    let bin = assert_cmd::cargo::cargo_bin("sn");
    let child = std::process::Command::new(bin)
        .env("SN_CONFIG_DIR", tmp.path())
        .args(["impersonate", ABEL, "--", "table", "list", "incident"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    // Wait until the wrapped command is in flight on the impersonated session.
    let started = std::time::Instant::now();
    while !inst.events().iter().any(|e| e == "incident as abel.tuter") {
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "{:?}",
            inst.events()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let killed = std::process::Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    let out = child.wait_with_output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(130),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "the interrupt must not wait out the in-flight request"
    );
    assert_ended(&inst.events());
}
