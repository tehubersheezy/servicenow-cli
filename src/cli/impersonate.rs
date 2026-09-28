//! `sn impersonate <USER> -- <COMMAND>...` — run one sn command as another user.
//!
//! Impersonation is state on a server-side *session*, and a session is exactly
//! what the rest of this CLI never holds: every other command authenticates
//! each request on its own, and a per-request credential (Basic or bearer)
//! re-establishes the credential holder's identity on every call, wiping any
//! impersonation. So this command mints one session with the profile's
//! credential, drops that credential, and runs the wrapped command on the
//! session's cookies alone ([`Auth::Session`](crate::client::Auth::Session)).
//!
//! The session is private to this process — its cookies live in memory and
//! are never written anywhere — so the caller's own sessions (a browser tab,
//! a parallel `sn` invocation) are never the impersonated one. It is ended on
//! every exit path this process gets to run: success, a failing command, an
//! early error, a panic, and Ctrl-C (SIGINT). Ending means reverting to the
//! original user *and* logging the session out, then re-reading it to confirm
//! it is gone. A SIGKILL leaves nothing to run, and even then the session is
//! unreachable (nobody else has its cookies) and lapses at the instance's idle
//! timeout.
//!
//! Measured on a live instance, and load-bearing:
//! - `POST /api/now/ui/impersonate/{sys_id}` answers **201 even when nothing
//!   happened** — a caller without the impersonator role gets a 201 and an
//!   unchanged session. The session is re-read after the POST and the command
//!   refuses to run unless it names the target, and `CanImpersonate: false` is
//!   refused before the POST is ever sent.
//! - The POST did not require `X-UserToken`, and the token did not rotate
//!   across a hop. The token is still sent, and re-read after every hop, so
//!   neither observation is relied on.

use crate::cli::auth::non_empty;
use crate::cli::kernel::{SessionScope, build_client, build_profile};
use crate::cli::{Cli, Command, GlobalFlags, OutputMode};
use crate::client::{Client, SESSION_COOKIE, SessionJar};
use crate::error::{Error, NO_HTTP_STATUS, Result};
use crate::observability::{self, log_note};
use serde_json::{Value, json};
use std::ffi::OsString;
use std::sync::{Arc, Mutex};

/// The documented (Mobile Impersonation API) read side: who the session is,
/// who it started as, whether it may impersonate, and its CSRF token.
const SESSION_PATH: &str = "/api/now/sg/impersonation/session";
/// Undocumented. `POST …/{sys_id}` switches the session to that user; posting
/// the original user's sys_id switches it back.
const IMPERSONATE_PATH: &str = "/api/now/ui/impersonate";
/// Ends the session outright (302 to `logout_success.do`; the cookies answer
/// 401 afterwards — measured).
const LOGOUT_PATH: &str = "/logout.do";

#[derive(clap::Args, Debug)]
pub struct ImpersonateArgs {
    /// The user to act as: a `user_name` (`abel.tuter`) or a `sys_user` sys_id.
    /// The profile's user needs the admin or impersonator role.
    pub user: String,
    /// The sn command to run as that user, after `--` (`-- table list incident`;
    /// a leading `sn` is accepted). Connection options (`--profile`, `--proxy`,
    /// `--timeout`, TLS) go before `--` — the session is opened before the
    /// command runs; output options work on either side. Its stdout, stderr and
    /// exit code are the command's own. The impersonation ends when it
    /// finishes, fails, or is interrupted.
    #[arg(last = true, required = true, value_name = "COMMAND")]
    pub command: Vec<String>,
}

pub fn run(
    global: &GlobalFlags,
    args: ImpersonateArgs,
    dispatch: fn(Cli) -> Result<()>,
) -> Result<()> {
    // Everything argv can get wrong is refused before a session exists.
    let Some(inner) = prepare_inner(global, &args.command)? else {
        return Ok(()); // the wrapped command's --help/--version was printed
    };
    let target_query = user_query(&args.user)?;

    let profile = build_profile(global)?;
    // Mint: one request under the profile's own credential, which both opens
    // the session and reads its CSRF token. After this the credential is
    // never sent again.
    let minted = build_client(&profile, global.timeout)?;
    let (v, cookies) = minted.get_secret_with_cookies(SESSION_PATH)?;
    drop(minted);
    let session = read_session(&v)?;
    if !cookies.iter().any(|(n, _)| n == SESSION_COOKIE) {
        return Err(Error::Instance {
            message: format!(
                "the instance opened no session ({SESSION_COOKIE} cookie) for profile '{}', \
                 and impersonation can only live on one",
                profile.name
            ),
            detail: None,
        });
    }
    let jar = Arc::new(SessionJar::new(cookies, session.token.clone()));
    let _scope = SessionScope::enter(Arc::clone(&jar));

    // From here on the session exists, so every exit path must end it.
    let hop = Arc::new(Hop {
        client: build_client(&profile, global.timeout)?,
        state: Mutex::new(HopState::default()),
    });
    let guard = EndOnDrop(Arc::clone(&hop));
    install_sigint(Arc::clone(&hop));

    if session.can_impersonate == Some(false) {
        return Err(Error::Auth {
            status: NO_HTTP_STATUS,
            message: format!(
                "'{}' may not impersonate other users: the instance reports CanImpersonate: \
                 false for this session (the admin or impersonator role is required)",
                session.current
            ),
            transaction_id: None,
        });
    }

    let original = resolve_user(
        &hop.client,
        &format!("user_name={}", session.current),
        &session.current,
    )?;
    let target = resolve_user(&hop.client, &target_query, args.user.trim())?;

    hop.arm(original.clone());
    hop.client.post(
        &format!("{IMPERSONATE_PATH}/{}", target.sys_id),
        &[],
        &json!({}),
    )?;

    // The POST's 201 proves nothing (it is also what a refused hop returns),
    // so the session itself has to say it is now the target.
    let after = read_session(&hop.client.get_secret(SESSION_PATH)?)?;
    jar.set_token(after.token.clone());
    let took = after.current.eq_ignore_ascii_case(&target.user_name)
        && after
            .original
            .as_deref()
            .is_some_and(|o| o.eq_ignore_ascii_case(&original.user_name));
    if !took {
        return Err(Error::Instance {
            message: format!(
                "impersonation of '{}' did not take effect: the instance accepted the request \
                 but the session is still '{}'",
                target.user_name, after.current
            ),
            detail: Some(
                "the instance answers the impersonate call with success even when it refuses it"
                    .into(),
            ),
        });
    }
    log_note(&format!(
        "impersonating {} (session opened as {})",
        target.user_name, original.user_name
    ));

    let outcome = dispatch(inner);
    // Ended here rather than by the guard so the result can be reported; the
    // guard's own `end()` then finds nothing left to do.
    let ended = hop.end();
    drop(guard);
    match (outcome, ended) {
        (Ok(()), ended) => ended,
        (Err(e), Ok(())) => Err(e),
        (Err(e), Err(end_err)) => {
            // The command's own failure is the one the caller branches on;
            // the cleanup failure still has to be said out loud.
            eprintln!("sn: warning: {}", end_err);
            Err(e)
        }
    }
}

/// Parse the wrapped argv into a command that inherits this invocation's
/// connection, and refuse the ones that cannot meaningfully run as someone
/// else. `Ok(None)` means clap printed `--help`/`--version` for it.
fn prepare_inner(outer: &GlobalFlags, argv: &[String]) -> Result<Option<Cli>> {
    let rest = match argv.first().map(String::as_str) {
        Some("sn") => &argv[1..],
        _ => argv,
    };
    if rest.is_empty() {
        return Err(Error::Usage(
            "no command to run after `--` (e.g. `sn impersonate abel.tuter -- table list incident`)"
                .into(),
        ));
    }
    let mut full: Vec<OsString> = vec![OsString::from("sn")];
    full.extend(rest.iter().map(OsString::from));
    let mut cli = match crate::cli::parse_from(full) {
        Ok(cli) => cli,
        Err(e) => {
            use clap::error::ErrorKind;
            if matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) {
                let _ = e.print();
                return Ok(None);
            }
            return Err(Error::Usage(format!(
                "the command after `--` is invalid: {}",
                e.render().to_string().trim_end()
            )));
        }
    };

    if let Some(why) = refusal(&cli.command) {
        return Err(Error::Usage(format!(
            "cannot run this under `sn impersonate`: {why}"
        )));
    }

    let g = &cli.global;
    let connection_flags = [
        ("--profile", g.profile.is_some()),
        ("--proxy", g.proxy.is_some()),
        ("--no-proxy", g.no_proxy),
        ("--proxy-ca-cert", g.proxy_ca_cert.is_some()),
        ("--insecure", g.insecure),
        ("--ca-cert", g.ca_cert.is_some()),
        ("--timeout", g.timeout.is_some()),
    ];
    if let Some((flag, _)) = connection_flags.iter().find(|(_, set)| *set) {
        return Err(Error::Usage(format!(
            "{flag} must go before `--`: the impersonated session is opened before the command \
             runs (`sn impersonate <USER> {flag} … -- <COMMAND>`)"
        )));
    }

    // Connection: exactly the outer invocation's, so the command resolves the
    // same profile the session was opened on. Presentation: the command's own
    // choice, else whatever was given before `--`.
    let inner = &mut cli.global;
    inner.profile = outer.profile.clone();
    inner.proxy = outer.proxy.clone();
    inner.no_proxy = outer.no_proxy;
    inner.proxy_ca_cert = outer.proxy_ca_cert.clone();
    inner.insecure = outer.insecure;
    inner.ca_cert = outer.ca_cert.clone();
    inner.timeout = outer.timeout;
    if inner.output == OutputMode::Default {
        inner.output = outer.output;
    }
    if !inner.pretty && !inner.compact {
        inner.pretty = outer.pretty;
        inner.compact = outer.compact;
    }
    inner.verbose = inner.verbose.max(outer.verbose);
    observability::set_level(inner.verbose);
    Ok(Some(cli))
}

/// Why a command cannot run impersonated, or `None` when it can.
fn refusal(cmd: &Command) -> Option<&'static str> {
    Some(match cmd {
        Command::Impersonate(_) => "impersonations do not nest",
        Command::Init(_) | Command::Profile { .. } => {
            "it manages local profiles, and a profile is not a user to impersonate"
        }
        Command::Completion(_) | Command::Introspect => "it never contacts the instance",
        Command::Watch(_) => "`sn watch` opens its own websocket session, not this one",
        Command::Open(_) => "it opens your browser, whose session is not the impersonated one",
        Command::Script { .. } => {
            "`sn script run` mints its own UI session for sys.scripts.do, not this one"
        }
        _ => return None,
    })
}

/// The `sysparm_query` term that finds `user` in `sys_user`: by sys_id when it
/// is one, else by `user_name`.
fn user_query(user: &str) -> Result<String> {
    let u = user.trim();
    if u.is_empty() {
        return Err(Error::Usage("the user to impersonate is empty".into()));
    }
    // `^` would splice extra terms into the encoded query.
    if u.contains('^') || u.chars().any(char::is_control) {
        return Err(Error::Usage(format!(
            "'{u}' is not a user_name or sys_id (it contains `^` or a control character)"
        )));
    }
    Ok(if is_sys_id(u) {
        format!("sys_id={u}")
    } else {
        format!("user_name={u}")
    })
}

fn is_sys_id(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

#[derive(Clone, Debug)]
struct User {
    sys_id: String,
    user_name: String,
}

/// One `sys_user` row for `query`, or an error that says why not. Two rows
/// means the filter did not apply (user_name and sys_id are both unique), and
/// the rows are strangers — never pick one.
fn resolve_user(client: &Client, query: &str, shown: &str) -> Result<User> {
    let v = client.get(
        "/api/now/table/sys_user",
        &[
            ("sysparm_query".into(), query.into()),
            ("sysparm_fields".into(), "sys_id,user_name".into()),
            ("sysparm_limit".into(), "2".into()),
        ],
    )?;
    let rows = v
        .get("result")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    match rows.as_slice() {
        [] => Err(Error::Api {
            status: NO_HTTP_STATUS,
            message: format!("no user '{shown}' in sys_user (or not readable by this profile)"),
            detail: None,
            transaction_id: None,
            sn_error: None,
        }),
        [row] => {
            let sys_id = non_empty(row.get("sys_id")).filter(|s| is_sys_id(s));
            let user_name = non_empty(row.get("user_name"));
            match (sys_id, user_name) {
                (Some(sys_id), Some(user_name)) => Ok(User { sys_id, user_name }),
                _ => Err(Error::Instance {
                    message: format!(
                        "the sys_user row for '{shown}' has no usable sys_id/user_name"
                    ),
                    detail: None,
                }),
            }
        }
        _ => Err(Error::Instance {
            message: format!("looking up '{shown}' matched more than one sys_user row"),
            detail: Some(format!(
                "`{query}` should match at most one row; the instance did not apply it"
            )),
        }),
    }
}

/// What `sg/impersonation/session` reports. The token is kept only to be
/// handed to the [`SessionJar`]; it is never printed or logged.
struct SessionInfo {
    current: String,
    original: Option<String>,
    can_impersonate: Option<bool>,
    token: Option<String>,
}

fn read_session(v: &Value) -> Result<SessionInfo> {
    // A bare object, not a `result` envelope (see ping.rs).
    let Some(current) = non_empty(v.get("CurrentUser")) else {
        return Err(Error::Instance {
            message: "the impersonation session endpoint did not name the session's user".into(),
            detail: None,
        });
    };
    Ok(SessionInfo {
        current,
        original: non_empty(v.get("OriginalUser")),
        can_impersonate: v.get("CanImpersonate").and_then(Value::as_bool),
        token: non_empty(v.get("SessionToken")),
    })
}

/// The session's lifecycle, shared between the main thread and the SIGINT
/// handler so whichever gets there first ends it and the other finds it done.
struct Hop {
    client: Client,
    state: Mutex<HopState>,
}

#[derive(Default)]
struct HopState {
    /// Set just before the impersonate POST: from then on ending the session
    /// includes switching it back.
    revert_to: Option<User>,
    ended: bool,
}

impl Hop {
    fn lock(&self) -> std::sync::MutexGuard<'_, HopState> {
        match self.state.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn arm(&self, original: User) {
        self.lock().revert_to = Some(original);
    }

    /// Revert, log out, and confirm the session is gone. Idempotent; the lock
    /// is held throughout so a concurrent caller waits for the outcome rather
    /// than racing it.
    fn end(&self) -> Result<()> {
        let mut state = self.lock();
        if state.ended {
            return Ok(());
        }
        state.ended = true;

        let reverted = match &state.revert_to {
            Some(original) => self
                .client
                .post(
                    &format!("{IMPERSONATE_PATH}/{}", original.sys_id),
                    &[],
                    &json!({}),
                )
                .map(|_| ()),
            None => Ok(()),
        };
        let logged_out = self.client.get_discard(LOGOUT_PATH).map(|_| ());

        match self.client.get_secret(SESSION_PATH) {
            // The cookies no longer authenticate: the session is gone.
            Err(Error::Auth { .. }) => {
                log_note("impersonated session ended");
                Ok(())
            }
            Ok(v) => {
                let current = non_empty(v.get("CurrentUser"));
                let back = match &state.revert_to {
                    None => true,
                    Some(original) => current
                        .as_deref()
                        .is_some_and(|c| c.eq_ignore_ascii_case(&original.user_name)),
                };
                if back {
                    // Logout did not take, but the session is the caller's own
                    // again and nobody else holds it.
                    log_note("session reverted to the original user; logout did not end it");
                    Ok(())
                } else {
                    Err(Error::Instance {
                        message: format!(
                            "could not end the impersonated session (it is still '{}')",
                            current.unwrap_or_default()
                        ),
                        detail: Some(
                            "only this process held its cookies; it lapses at the instance's \
                             idle timeout"
                                .into(),
                        ),
                    })
                }
            }
            Err(e) => Err(reverted.err().or(logged_out.err()).unwrap_or(e)),
        }
    }
}

/// Ends the session on any exit the normal path does not reach: an early `?`
/// return, or a panic unwinding through [`run`].
struct EndOnDrop(Arc<Hop>);

impl Drop for EndOnDrop {
    fn drop(&mut self) {
        if let Err(e) = self.0.end() {
            eprintln!("sn: warning: {e}");
        }
    }
}

/// Ctrl-C ends the session before the process goes, then exits 130 (128 +
/// SIGINT) — an interrupted command is not a success. The handler is
/// process-wide and installable once, so a wrapped command that wants its own
/// (`attachment download`'s staging cleanup) loses it; ending the impersonation
/// is the one that must not be lost.
fn install_sigint(hop: Arc<Hop>) {
    let installed = ctrlc::set_handler(move || {
        if let Err(e) = hop.end() {
            eprintln!("sn: warning: {e}");
        }
        std::process::exit(130);
    });
    if let Err(e) = installed {
        log_note(&format!(
            "could not install the Ctrl-C handler ({e}); an interrupt will not end the \
             impersonated session, which then lapses at the instance's idle timeout"
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_query_picks_sys_id_or_user_name() {
        assert_eq!(user_query("abel.tuter").unwrap(), "user_name=abel.tuter");
        assert_eq!(
            user_query(" 62826bf03710200044e0bfc8bcbe5df1 ").unwrap(),
            "sys_id=62826bf03710200044e0bfc8bcbe5df1"
        );
    }

    #[test]
    fn user_query_refuses_encoded_query_injection() {
        assert!(user_query("abel^ORuser_name=admin").is_err());
        assert!(user_query("abel\n").is_ok()); // trimmed
        assert!(user_query("ab\u{0}el").is_err());
        assert!(user_query("  ").is_err());
    }

    #[test]
    fn session_token_is_read_but_session_without_user_is_refused() {
        let v = json!({"CurrentUser": "a", "OriginalUser": "b", "CanImpersonate": true, "SessionToken": "t"});
        let s = read_session(&v).unwrap();
        assert_eq!(s.current, "a");
        assert_eq!(s.original.as_deref(), Some("b"));
        assert_eq!(s.can_impersonate, Some(true));
        assert_eq!(s.token.as_deref(), Some("t"));
        assert!(read_session(&json!({"result": {"CurrentUser": "a"}})).is_err());
    }
}
