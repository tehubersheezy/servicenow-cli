//! `sn script run` — execute server-side JavaScript ("Scripts - Background")
//! and return what it printed.
//!
//! There is no REST endpoint for this. It is the classic-UI `sys.scripts.do`
//! processor, driven the way its form drives it: a UI session (cookies + the
//! session's CSRF token, both minted by one authenticated `GET` — see
//! [`Client::ui_session`]) and a form-encoded POST. The profile's own auth stays
//! attached, so basic, OAuth and API-key profiles all work unchanged; the script
//! runs as the profile's user.
//!
//! Everything below was measured against a live instance (Australia):
//!
//! - **The answer is HTML, and every script verdict is inside a `200`.** The
//!   body is `[H:MM:SS.mmm] <HTML>…Script completed in scope <scope>: script<HR/>
//!   …sys_script_execution_history.do?sys_id=<id>…<PRE>entry<BR/>entry<BR/></PRE>`.
//!   Entries are HTML-escaped (a printed `<BR/>` arrives as `&lt;BR/&gt;`), so
//!   splitting on the literal tag is exact; one entry may span lines.
//! - **"Completed" is printed for failures too.** A compilation or runtime error
//!   is an entry beginning `Script compilation error:` / `Script execution
//!   error:`, followed by detail entries (evaluator text, Java stack).
//! - **Output is identified by prefix, and the prefix is the scope.** `gs.print`,
//!   `gs.info`, `gs.warn`, `gs.error`, `gs.debug` and a source-less `gs.log` all
//!   arrive as `*** Script: …` in global and `<scope>: …` in an app scope
//!   (`gs.print` is refused in a scope). Everything else in the block is the
//!   platform talking — slow-business-rule notices, notification-provider
//!   chatter, `gs.log(msg, source)`'s `source: msg` — and is reported
//!   separately as `messages` rather than mixed into `output`.
//! - **A scope the instance does not recognise is silently ignored** — an
//!   unknown sys_id, or a scope *name* where the form wants a sys_id, runs the
//!   script in global and says so only in the "completed in scope" line. So
//!   `--scope` is resolved to a sys_id before anything runs, and the reported
//!   scope is checked against it afterwards.
//! - **Refusals come in three shapes.** A user without the right role: HTTP
//!   403 with an empty body (basic), or `302 → /logout_redirect.do` (OAuth) —
//!   hence a client that does not follow redirects. A missing CSRF token, or a
//!   store-application scope: a `200` whose whole body is `not authorized`.
//!   No session cookie: a `200` with an empty body, and the script did not run.
//!
//! The issue that proposed this command suggested wrapping the script in
//! printed sentinels. Structural parsing makes them unnecessary, and they would
//! cost: `gs.print` is refused in app scopes, a compile error suppresses the
//! opening sentinel anyway, and any wrapping line shifts the line numbers the
//! instance reports in errors.

use crate::cli::GlobalFlags;
use crate::cli::OutputMode;
use crate::cli::context::resolve_scope;
use crate::cli::kernel::{confirm_destructive, connect_without_redirects, write_response};
use crate::client::{Client, UiResponse};
use crate::error::{Error, Result};
use clap::Subcommand;
use serde_json::{Map, Value, json};
use std::io::Read;

/// The processor behind System Definition › Scripts - Background.
const SCRIPTS_PATH: &str = "/sys.scripts.do";

/// The default, and the one scope that needs no lookup: its sys_id is literally
/// `global`.
const GLOBAL: &str = "global";

#[derive(Subcommand, Debug)]
pub enum ScriptSub {
    /// Run server-side JavaScript as the profile's user and return what it
    /// printed. Arbitrary code execution: gated behind --yes.
    #[command(long_about = RUN_LONG_ABOUT)]
    Run(ScriptRunArgs),
}

const RUN_LONG_ABOUT: &str = "\
Run server-side JavaScript as the profile's user (System Definition > Scripts - Background) \
and return what it printed.

This executes arbitrary code with that user's full privileges — normally admin — and it is \
not undoable unless --rollback recorded it. It is gated: pass --yes (required whenever stdin \
is not a terminal).

Output is one JSON object: `output` holds each line the script logged (gs.info, gs.warn, \
gs.error, gs.debug, gs.log, and gs.print in global), `messages` anything else the platform \
printed while it ran, `error` a compilation or runtime error (then `ok` is false and the exit \
code is 2), plus `scope`, `elapsed_ms`, `history_id` (its sys_script_execution_history row) \
and `rollback_context`. Print JSON.stringify(x) to hand back structured data.

The script runs synchronously inside one transaction: a long one is bounded by the \
instance's transaction quota, and by --timeout (default 30s) on this side. A client timeout \
does not stop the script — it keeps running, and its result lands in \
sys_script_execution_history.

EXAMPLES:
  sn script run 'gs.info(new GlideRecord(\"incident\").getRowCount())' --yes
  sn script run @cleanup.js --rollback --yes
  echo 'gs.info(gs.getUserName())' | sn script run @- --yes
  sn script run @job.js --scope x_acme_app --timeout 300 --yes";

#[derive(clap::Args, Debug)]
pub struct ScriptRunArgs {
    /// The script: inline JavaScript, @file (path), or @- (stdin).
    pub script: String,
    /// Application scope to run in: scope name (x_acme_app), display name, or
    /// sys_id. Resolved before the script runs, because the instance silently
    /// runs an unrecognised scope in global. Store application scopes are
    /// refused by the instance.
    #[arg(long, default_value = GLOBAL, value_name = "SCOPE")]
    pub scope: String,
    /// Record the run for rollback and report the rollback context
    /// (sys_rollback_context) its writes can be undone from. Slows the run.
    #[arg(long)]
    pub rollback: bool,
    /// Run without asking. Required when stdin is not a terminal.
    #[arg(long, short = 'y')]
    pub yes: bool,
}

pub fn run(global: &GlobalFlags, args: ScriptRunArgs) -> Result<()> {
    let scope_arg = args.scope.trim().to_string();
    if scope_arg.is_empty() {
        return Err(Error::Usage("--scope cannot be empty".into()));
    }
    // The name is interpolated into an encoded query; `^` would splice terms.
    if scope_arg.contains('^') {
        return Err(Error::Usage(format!(
            "--scope '{scope_arg}' contains '^', which no scope name does"
        )));
    }
    // Gate on argv alone, before the profile or the network (see CLAUDE.md).
    confirm_destructive(
        args.yes,
        "run",
        &format!("a background script in scope {scope_arg}"),
    )?;

    let script = read_script(&args.script)?;
    if script.trim().is_empty() {
        return Err(Error::Usage("the script is empty".into()));
    }

    let client = connect_without_redirects(global)?;

    // (scope name the response must report, sys_id the form takes)
    let (scope_name, scope_id) = if scope_arg.eq_ignore_ascii_case(GLOBAL) {
        (GLOBAL.to_string(), GLOBAL.to_string())
    } else {
        let row = resolve_scope(&client, &scope_arg)?;
        (row.scope, row.sys_id)
    };

    let session = client.ui_session().map_err(|e| match e {
        Error::Api { status: 404, .. } => Error::Api {
            status: 404,
            message: format!(
                "this instance has no {}, which `sn script run` needs for its session \
                 token",
                crate::client::UI_SESSION_PATH
            ),
            detail: None,
            transaction_id: None,
            sn_error: None,
        },
        other => other,
    })?;

    let mut form = vec![
        ("script".to_string(), script),
        ("runscript".to_string(), "Run script".to_string()),
        // The form's "cancel after 4 hours" box: keeps the run under the
        // instance's transaction quota, as the UI does by default.
        ("quota_managed_transaction".to_string(), "on".to_string()),
        ("sys_scope".to_string(), scope_id.clone()),
    ];
    if args.rollback {
        form.push(("record_for_rollback".to_string(), "on".to_string()));
    }

    let resp = client
        .post_ui_form(SCRIPTS_PATH, &session, &form)
        .map_err(|e| match e {
            Error::Transport(m) => Error::Transport(format!(
                "{m}; if the request reached the instance the script may still be running \
                 there — its result will be in sys_script_execution_history (raise \
                 --timeout for long scripts)"
            )),
            other => other,
        })?;

    let parsed = judge(&resp, &scope_arg)?;

    let rollback_context = match (&parsed.history_id, args.rollback) {
        (Some(id), true) => rollback_context(&client, id)?,
        _ => None,
    };

    let mut out = parsed.to_json(rollback_context);
    if let (OutputMode::Raw, Value::Object(m)) = (global.output, &mut out) {
        m.insert("response".into(), Value::String(resp.body.clone()));
    }

    // Emit first either way: output printed before an error is still output,
    // and a caller that wants it should not have to re-run the script.
    write_response(global, &out)?;

    if let Some(err) = &parsed.error {
        return Err(Error::Api {
            status: resp.status,
            message: format!("script {} error: {}", err.kind, err.message),
            detail: err.line.map(|l| format!("line {l}")),
            transaction_id: resp.transaction_id.clone(),
            sn_error: Some(err.to_json()),
        });
    }
    if parsed.scope != scope_name {
        return Err(Error::Instance {
            message: format!(
                "asked to run in scope {scope_name} but the instance reports it ran in scope {}",
                parsed.scope
            ),
            detail: None,
        });
    }
    Ok(())
}

/// Resolve the inline / `@file` / `@-` script argument to its text.
fn read_script(raw: &str) -> Result<String> {
    if raw == "@-" {
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .map_err(|e| Error::Usage(format!("read script from stdin: {e}")))?;
        Ok(s)
    } else if let Some(path) = raw.strip_prefix('@') {
        std::fs::read_to_string(path).map_err(|e| Error::Usage(format!("read {path}: {e}")))
    } else {
        Ok(raw.to_string())
    }
}

/// Turn the processor's answer into a parsed run, or into the error its shape
/// means. Pure: every branch here is a measured response shape.
fn judge(resp: &UiResponse, scope_arg: &str) -> Result<Run> {
    if (300..400).contains(&resp.status) {
        let to = resp.location.as_deref().unwrap_or("(no Location)");
        return Err(Error::Auth {
            status: resp.status,
            message: format!(
                "the instance refused to run background scripts for this user (redirected \
                 to {to}); sys.scripts.do requires the admin role"
            ),
            transaction_id: resp.transaction_id.clone(),
        });
    }
    if resp.status == 401 || resp.status == 403 {
        return Err(Error::Auth {
            status: resp.status,
            message: format!(
                "HTTP {}: the instance refused to run background scripts for this user; \
                 sys.scripts.do requires the admin role",
                resp.status
            ),
            transaction_id: resp.transaction_id.clone(),
        });
    }
    if !(200..300).contains(&resp.status) {
        return Err(Error::Api {
            status: resp.status,
            message: format!("sys.scripts.do answered HTTP {}", resp.status),
            detail: truncated(&resp.body),
            transaction_id: resp.transaction_id.clone(),
            sn_error: None,
        });
    }
    let body = resp.body.trim();
    if body.eq_ignore_ascii_case("not authorized") {
        let message = if scope_arg.eq_ignore_ascii_case(GLOBAL) {
            "sys.scripts.do answered \"not authorized\": the instance rejected the session's \
             CSRF token, and the script did not run"
                .to_string()
        } else {
            format!(
                "sys.scripts.do answered \"not authorized\" for scope {scope_arg}: the instance \
                 refuses background scripts in this scope (store application scopes refuse \
                 them even for admin), and the script did not run"
            )
        };
        return Err(Error::Api {
            status: resp.status,
            message,
            detail: None,
            transaction_id: resp.transaction_id.clone(),
            sn_error: None,
        });
    }
    if body.is_empty() {
        return Err(Error::Instance {
            message: "sys.scripts.do returned an empty response: the instance did not accept \
                      the session, and the script did not run"
                .into(),
            detail: None,
        });
    }
    parse_run(&resp.body).ok_or_else(|| Error::Instance {
        message: "sys.scripts.do answered with a page this command does not recognise; \
                  whether the script ran is unknown — check sys_script_execution_history"
            .into(),
        detail: truncated(&resp.body),
    })
}

fn truncated(body: &str) -> Option<String> {
    let t = body.trim();
    if t.is_empty() {
        return None;
    }
    let mut s: String = t.chars().take(500).collect();
    if t.chars().count() > 500 {
        s.push('…');
    }
    Some(s)
}

/// Read the rollback context a `--rollback` run recorded off its history row.
/// Empty when the script wrote nothing — there is then nothing to roll back,
/// and the instance creates no context (measured).
fn rollback_context(client: &Client, history_id: &str) -> Result<Option<String>> {
    let resp = client.get(
        &format!("/api/now/table/sys_script_execution_history/{history_id}"),
        &[
            ("sysparm_fields".into(), "rollback_context".into()),
            ("sysparm_display_value".into(), "false".into()),
            ("sysparm_exclude_reference_link".into(), "true".into()),
        ],
    )?;
    Ok(resp
        .pointer("/result/rollback_context")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string))
}

/// One parsed run of `sys.scripts.do`.
#[derive(Debug, PartialEq)]
struct Run {
    scope: String,
    elapsed_ms: Option<u64>,
    history_id: Option<String>,
    output: Vec<String>,
    messages: Vec<String>,
    error: Option<ScriptError>,
}

#[derive(Debug, PartialEq)]
struct ScriptError {
    /// `compilation` or `execution`.
    kind: &'static str,
    /// The instance's `Error Description`.
    message: String,
    line: Option<u64>,
    /// Every entry from the error on, joined: evaluator text, Java stack.
    detail: String,
}

impl ScriptError {
    fn to_json(&self) -> Value {
        json!({
            "type": self.kind,
            "message": self.message,
            "line": self.line,
            "detail": self.detail,
        })
    }
}

impl Run {
    fn to_json(&self, rollback_context: Option<String>) -> Value {
        let mut m = Map::new();
        m.insert("ok".into(), Value::Bool(self.error.is_none()));
        m.insert("scope".into(), Value::String(self.scope.clone()));
        m.insert("output".into(), json!(self.output));
        m.insert("messages".into(), json!(self.messages));
        m.insert(
            "error".into(),
            self.error
                .as_ref()
                .map_or(Value::Null, ScriptError::to_json),
        );
        m.insert("elapsed_ms".into(), json!(self.elapsed_ms));
        m.insert("history_id".into(), json!(self.history_id));
        m.insert("rollback_context".into(), json!(rollback_context));
        Value::Object(m)
    }
}

const COMPLETED: &str = "Script completed in scope ";
const HISTORY: &str = "sys_script_execution_history.do?sys_id=";

/// Parse a `sys.scripts.do` result page. `None` when the page is not one —
/// no "completed in scope" line, or no `<PRE>` block.
fn parse_run(body: &str) -> Option<Run> {
    let scope = {
        let at = body.find(COMPLETED)? + COMPLETED.len();
        let rest = &body[at..];
        rest[..rest.find(':')?].trim().to_string()
    };
    let elapsed_ms = body
        .trim_start()
        .strip_prefix('[')
        .and_then(|r| r.split_once(']'))
        .and_then(|(t, _)| parse_elapsed(t));
    let history_id = body.find(HISTORY).map(|at| {
        body[at + HISTORY.len()..]
            .chars()
            .take_while(char::is_ascii_hexdigit)
            .collect::<String>()
    });
    let history_id = history_id.filter(|s| !s.is_empty());

    let pre_start = body.find("<PRE>")? + "<PRE>".len();
    let pre_end = body.rfind("</PRE>")?;
    let pre = body.get(pre_start..pre_end)?;

    let prefix = if scope == GLOBAL {
        "*** Script: ".to_string()
    } else {
        format!("{scope}: ")
    };

    let mut output = Vec::new();
    let mut messages = Vec::new();
    let mut error: Option<ScriptError> = None;
    let mut error_lines: Vec<String> = Vec::new();
    for raw in pre.split("<BR/>") {
        if raw.is_empty() {
            continue;
        }
        let entry = unescape_html(raw);
        if error.is_some() {
            // Execution stops at an error; what follows is its detail.
            error_lines.push(entry);
            continue;
        }
        if let Some(kind) = error_kind(&entry) {
            error = Some(ScriptError {
                kind,
                message: error_description(&entry),
                line: None,
                detail: String::new(),
            });
            error_lines.push(entry);
        } else if let Some(line) = entry.strip_prefix(prefix.as_str()) {
            output.push(line.to_string());
        } else {
            messages.push(entry);
        }
    }
    if let Some(e) = error.as_mut() {
        e.detail = error_lines.join("\n");
        e.line = error_line(&e.detail);
    }
    Some(Run {
        scope,
        elapsed_ms,
        history_id,
        output,
        messages,
        error,
    })
}

fn error_kind(entry: &str) -> Option<&'static str> {
    if entry.starts_with("Script compilation error:") {
        Some("compilation")
    } else if entry.starts_with("Script execution error:") {
        Some("execution")
    } else {
        None
    }
}

/// `…, Error Description: <this>, Script ES Level: …` — falling back to the
/// entry's first line when the instance words it differently.
fn error_description(entry: &str) -> String {
    const KEY: &str = "Error Description: ";
    const END: &str = ", Script ES Level:";
    if let Some(at) = entry.find(KEY) {
        let rest = &entry[at + KEY.len()..];
        let end = rest.find(END).unwrap_or(rest.len());
        return rest[..end].trim().to_string();
    }
    entry.lines().next().unwrap_or(entry).trim().to_string()
}

/// The script line an error names: `(null.null.script; line 3)` in the
/// description, or the evaluator's `Line(3)`.
fn error_line(detail: &str) -> Option<u64> {
    let digits = |s: &str| -> Option<u64> {
        let n: String = s.chars().take_while(char::is_ascii_digit).collect();
        n.parse().ok()
    };
    if let Some(at) = detail.find("; line ") {
        return digits(&detail[at + "; line ".len()..]);
    }
    detail
        .find("Line(")
        .and_then(|at| digits(&detail[at + "Line(".len()..]))
}

/// `H:MM:SS.mmm` → milliseconds.
fn parse_elapsed(t: &str) -> Option<u64> {
    let mut parts = t.trim().split(':');
    let h: u64 = parts.next()?.parse().ok()?;
    let m: u64 = parts.next()?.parse().ok()?;
    let (s, ms) = parts.next()?.split_once('.')?;
    if parts.next().is_some() {
        return None;
    }
    let s: u64 = s.parse().ok()?;
    let ms: u64 = ms.parse().ok()?;
    Some(((h * 60 + m) * 60 + s) * 1000 + ms)
}

/// Undo the processor's HTML escaping: the five named entities it (and HTML)
/// use, plus numeric references. Anything unrecognised is left as written.
fn unescape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        let decoded = tail.find(';').filter(|&end| end <= 10).and_then(|end| {
            let name = &tail[1..end];
            let ch = match name {
                "quot" => Some('"'),
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "apos" => Some('\''),
                _ => name
                    .strip_prefix("#x")
                    .or_else(|| name.strip_prefix("#X"))
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .or_else(|| name.strip_prefix('#').and_then(|d| d.parse().ok()))
                    .and_then(char::from_u32),
            }?;
            Some((ch, end + 1))
        });
        match decoded {
            Some((ch, len)) => {
                out.push(ch);
                rest = &tail[len..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(scope: &str, pre: &str) -> String {
        format!(
            "[0:00:01.234] <HTML><BODY>Script completed in scope {scope}: script<HR/>Script \
             execution history <A target='blank' HREF='sys_script_execution_history.do?\
             sys_id=48169e6a2fe303107efd1d707fa4e3f4'>available here</A><HR/><PRE>{pre}</PRE>\
             <HR/></BODY></HTML>"
        )
    }

    fn resp(status: u16, body: &str) -> UiResponse {
        UiResponse {
            status,
            location: None,
            transaction_id: None,
            body: body.into(),
        }
    }

    #[test]
    fn global_output_is_split_unescaped_and_classified() {
        // Verbatim shape from the live instance, including a platform message.
        let body = page(
            "global",
            "*** Script: hello<BR/>*** Script: {&quot;a&quot;:1,&quot;b&quot;:\
             &quot;&lt;x&amp;y&gt;&quot;}<BR/>Slow business rule 'X' on y:&lt;span&gt; \
             Created &lt;/span&gt;, time was: 0:00:00.789<BR/>mysrc: m<BR/>",
        );
        let run = parse_run(&body).unwrap();
        assert_eq!(run.scope, "global");
        assert_eq!(run.elapsed_ms, Some(1234));
        assert_eq!(
            run.history_id.as_deref(),
            Some("48169e6a2fe303107efd1d707fa4e3f4")
        );
        assert_eq!(run.output, vec!["hello", r#"{"a":1,"b":"<x&y>"}"#]);
        assert_eq!(run.messages.len(), 2);
        assert!(run.messages[0].starts_with("Slow business rule 'X' on y:<span>"));
        assert_eq!(run.messages[1], "mysrc: m");
        assert!(run.error.is_none());
    }

    #[test]
    fn a_printed_br_tag_does_not_split_an_entry() {
        let run = parse_run(&page("global", "*** Script: &lt;BR/&gt;tag<BR/>")).unwrap();
        assert_eq!(run.output, vec!["<BR/>tag"]);
    }

    #[test]
    fn multi_line_print_stays_one_entry() {
        let run = parse_run(&page("global", "*** Script: line1\nline2<BR/>")).unwrap();
        assert_eq!(run.output, vec!["line1\nline2"]);
    }

    #[test]
    fn empty_pre_is_a_successful_silent_run() {
        let run = parse_run(&page("global", "")).unwrap();
        assert!(run.output.is_empty() && run.messages.is_empty() && run.error.is_none());
    }

    #[test]
    fn scoped_output_uses_the_scope_prefix() {
        let run = parse_run(&page(
            "x_912401_abeytest",
            "x_912401_abeytest: scope=x_912401_abeytest<BR/>x_912401_abeytest: d \
             (sys.scripts extended logging)<BR/>*** Script: not ours<BR/>",
        ))
        .unwrap();
        assert_eq!(run.scope, "x_912401_abeytest");
        assert_eq!(
            run.output,
            vec![
                "scope=x_912401_abeytest",
                "d (sys.scripts extended logging)"
            ]
        );
        assert_eq!(run.messages, vec!["*** Script: not ours"]);
    }

    #[test]
    fn compilation_error_is_parsed_with_its_line() {
        let run = parse_run(&page(
            "global",
            "Script compilation error: Script Identifier: null.null.script, Error \
             Description: syntax error (null.null.script; line 1), Script ES Level: 0, \
             Interpreted Mode: true<BR/>Javascript compiler exception: syntax error \
             (null.null.script; line 1) in:\ngs.print(&quot;a&quot; +;\n Stack trace:\n<BR/>",
        ))
        .unwrap();
        let e = run.error.unwrap();
        assert_eq!(e.kind, "compilation");
        assert_eq!(e.message, "syntax error (null.null.script; line 1)");
        assert_eq!(e.line, Some(1));
        assert!(e.detail.contains("gs.print(\"a\" +;"), "{}", e.detail);
    }

    #[test]
    fn execution_error_keeps_prior_output_and_takes_the_rest_as_detail() {
        let run = parse_run(&page(
            "global",
            "*** Script: before<BR/>Script execution error: Script Identifier: \
             null.null.script, Error Description: Cannot read property &quot;x&quot; from \
             null, Script ES Level: 0<BR/>Evaluator: com.glide.script.RhinoEcmaError: \
             Cannot read property &quot;x&quot; from null\n   script : Line(3) column(0)\n<BR/>\
             Background message, type:error, message: boom<BR/>",
        ))
        .unwrap();
        assert_eq!(run.output, vec!["before"]);
        assert!(run.messages.is_empty(), "{:?}", run.messages);
        let e = run.error.unwrap();
        assert_eq!(e.kind, "execution");
        assert_eq!(e.message, "Cannot read property \"x\" from null");
        assert_eq!(e.line, Some(3));
        assert!(e.detail.contains("Background message"), "{}", e.detail);
    }

    #[test]
    fn a_page_without_the_completed_line_is_unrecognised() {
        assert!(parse_run("<html><body>Log in</body></html>").is_none());
        assert!(parse_run("Script completed in scope global: script, no pre").is_none());
    }

    #[test]
    fn elapsed_parses_hours_minutes_seconds() {
        assert_eq!(parse_elapsed("0:00:00.951"), Some(951));
        assert_eq!(parse_elapsed("1:02:03.004"), Some(3_723_004));
        assert_eq!(parse_elapsed("garbage"), None);
    }

    #[test]
    fn unescape_handles_named_numeric_and_stray_ampersands() {
        assert_eq!(
            unescape_html("&lt;a href=&quot;x&quot;&gt; &amp;amp; &#39;q&#x27; & done &bogus;"),
            "<a href=\"x\"> &amp; 'q' & done &bogus;"
        );
    }

    #[test]
    fn not_authorized_is_an_in_band_refusal_with_the_real_status() {
        let err = judge(&resp(200, "not authorized"), "global").unwrap_err();
        match err {
            Error::Api {
                status, message, ..
            } => {
                assert_eq!(status, 200);
                assert!(message.contains("CSRF"), "{message}");
            }
            other => panic!("unexpected {other:?}"),
        }
        let err = judge(&resp(200, "not authorized"), "sn_chg_rest").unwrap_err();
        assert!(err.to_string().contains("scope sn_chg_rest"), "{err}");
    }

    #[test]
    fn empty_200_means_the_script_did_not_run() {
        let err = judge(&resp(200, ""), "global").unwrap_err();
        assert!(matches!(err, Error::Instance { .. }), "{err:?}");
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn redirect_and_403_are_auth_verdicts_with_their_real_status() {
        let mut r = resp(302, "");
        r.location = Some("/logout_redirect.do".into());
        let err = judge(&r, "global").unwrap_err();
        assert!(matches!(err, Error::Auth { status: 302, .. }), "{err:?}");
        assert!(err.to_string().contains("/logout_redirect.do"));
        let err = judge(&resp(403, ""), "global").unwrap_err();
        assert!(matches!(err, Error::Auth { status: 403, .. }), "{err:?}");
        assert_eq!(err.exit_code(), 4);
    }
}
