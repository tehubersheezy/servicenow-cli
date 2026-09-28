//! `auth = "token"`: a bearer token sn did not mint.
//!
//! Covers every enterprise setup the OAuth grants cannot model — an IdP issuing
//! ServiceNow-audience tokens directly, a secret broker, Vault, a cloud CLI. The
//! token comes from one of two places:
//!
//! * **Static** — stored verbatim in `credentials.toml` (0600), sent as-is.
//! * **`token_command`** — a command in `config.toml` that sn runs through the
//!   platform shell (`sh -c` / `cmd /C`), kubectl-exec-plugin style. Its stdout
//!   is the token: either the bare token on one line, or a JSON object carrying
//!   it (see [`parse_output`]).
//!
//! **The caching contract.** A command's token is cached in `credentials.toml`
//! only when the command *said* when it expires (`expires_at`/`expires_on`/
//! `expires_in` in its JSON). Such a token is reused until 60s before that
//! expiry, then the command runs again. A token with no stated expiry is never
//! cached: the command runs on every invocation. Guessing a lifetime would
//! either re-run a slow command needlessly or — worse — keep presenting a token
//! the issuer already revoked, and an agent can't tell that 401 from a missing
//! role. `sn profile refresh` forces a re-run; `sn profile logout` drops the
//! cache.
//!
//! **Output is a secret.** It is never logged, never placed in an error
//! message, and never passed on anyone's argv — sn reads it from the child's
//! stdout pipe and sends it only as the `Authorization` header. The child's
//! stdin is closed (an agent's invocation has nobody to answer a prompt), and
//! its stderr is captured and quoted, tail-truncated, only when it fails.
//!
//! Unlike `oauth::ensure_access_token`, a fetch does **not** hold the config
//! lock across the network call. That lock exists there because refresh-token
//! rotation makes a duplicated refresh fatal (the loser presents a consumed
//! token). A command has no such hazard — N parallel invocations running it N
//! times cost only time — while holding the lock across an arbitrary user
//! command would park every other sn invocation behind it.

use crate::config::{self, ResolvedProfile, ResolvedToken, TokenSet};
use crate::error::{Error, Result};
use serde_json::Value;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Re-run the command this many seconds before a cached token's stated expiry.
const REFRESH_SKEW_SECS: u64 = 60;

/// How long a token command may run when `--timeout` is not given.
const DEFAULT_COMMAND_TIMEOUT_SECS: u64 = 30;

/// How much of a failing command's stderr to quote.
const STDERR_TAIL_CHARS: usize = 500;

/// The bearer token for an `auth = "token"` profile: the stored one, a cached
/// command result still inside its stated lifetime, or a fresh command run.
pub fn ensure_token(profile: &ResolvedProfile, timeout: Option<u64>) -> Result<String> {
    match source(profile)? {
        ResolvedToken::Static(t) => Ok(t.clone()),
        ResolvedToken::Command { command, cached } => {
            if let Some(t) = cached
                && t.expires_at.is_some()
                && !t.is_expired(REFRESH_SKEW_SECS)
            {
                return Ok(t.access_token.clone());
            }
            Ok(fetch(profile, command, timeout)?.access_token)
        }
    }
}

/// Run the command now regardless of any cache (`sn profile refresh`).
pub fn force_refresh(profile: &ResolvedProfile, timeout: Option<u64>) -> Result<TokenSet> {
    match source(profile)? {
        ResolvedToken::Static(_) => Err(Error::Usage(format!(
            "profile '{}' uses a static token, which cannot be refreshed; \
             rewrite it with `sn profile add --force` or give it a --token-command",
            profile.name
        ))),
        ResolvedToken::Command { command, .. } => fetch(profile, command, timeout),
    }
}

fn source(profile: &ResolvedProfile) -> Result<&ResolvedToken> {
    profile.token.as_ref().ok_or_else(|| {
        Error::Config(format!(
            "no token or token_command configured for profile '{}'; run `sn init`",
            profile.name
        ))
    })
}

/// Run `command` and update the cache per the contract in the module docs.
fn fetch(profile: &ResolvedProfile, command: &str, timeout: Option<u64>) -> Result<TokenSet> {
    let limit = Duration::from_secs(timeout.unwrap_or(DEFAULT_COMMAND_TIMEOUT_SECS));
    let tokens = run_command(command, limit, profile)?;
    let cacheable = tokens.expires_at.is_some() && !tokens.is_expired(REFRESH_SKEW_SECS);
    let had_cache = matches!(
        &profile.token,
        Some(ResolvedToken::Command {
            cached: Some(_),
            ..
        })
    );
    if cacheable {
        config::save_token_cache(&profile.name, Some(&tokens))?;
    } else if had_cache {
        // The previous result is stale by definition (we just replaced it);
        // don't leave it on disk to be mistaken for current.
        config::save_token_cache(&profile.name, None)?;
    }
    Ok(tokens)
}

#[cfg(unix)]
fn shell(command: &str) -> Command {
    let mut c = Command::new("sh");
    c.arg("-c").arg(command);
    c
}

#[cfg(windows)]
fn shell(command: &str) -> Command {
    let mut c = Command::new("cmd");
    c.arg("/C").arg(command);
    c
}

/// Spawn `command`, wait at most `limit`, and parse its stdout.
///
/// The child sees `SN_PROFILE` and `SN_INSTANCE`, so one script can serve
/// several profiles (the kubectl exec plugin's `KUBERNETES_EXEC_INFO`, reduced
/// to what a ServiceNow token request can use).
pub fn run_command(command: &str, limit: Duration, profile: &ResolvedProfile) -> Result<TokenSet> {
    let mut child = shell(command)
        .env("SN_PROFILE", &profile.name)
        .env("SN_INSTANCE", &profile.instance)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::Config(format!("cannot run token_command: {e}")))?;

    // Drain both pipes on their own threads: a child that fills one pipe while
    // we wait on the other would otherwise deadlock. Channels rather than
    // joins, so a grandchild that inherited a pipe and outlives the child
    // cannot hold us past the deadline.
    let drain = |mut r: Box<dyn Read + Send>| {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = r.read_to_end(&mut buf);
            let _ = tx.send(buf);
        });
        rx
    };
    let out_rx = drain(Box::new(child.stdout.take().expect("piped stdout")));
    let err_rx = drain(Box::new(child.stderr.take().expect("piped stderr")));

    let deadline = Instant::now() + limit;
    let timed_out = || {
        Error::Config(format!(
            "token_command did not finish within {}s (raise --timeout if it is just slow)",
            limit.as_secs()
        ))
    };
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(timed_out());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => return Err(Error::Config(format!("waiting for token_command: {e}"))),
        }
    };
    let remaining = || deadline.saturating_duration_since(Instant::now());
    let stdout = out_rx.recv_timeout(remaining()).map_err(|_| timed_out())?;

    if !status.success() {
        let stderr = err_rx.recv_timeout(remaining()).unwrap_or_default();
        let stderr = String::from_utf8_lossy(&stderr);
        let stderr = stderr.trim();
        let code = status
            .code()
            .map_or_else(|| "a signal".to_string(), |c| format!("status {c}"));
        let mut msg = format!("token_command exited with {code}");
        if !stderr.is_empty() {
            msg.push_str(": ");
            msg.push_str(&tail(stderr, STDERR_TAIL_CHARS));
        }
        return Err(Error::Config(msg));
    }
    parse_output(&stdout)
}

/// The last `n` characters of `s`, on a char boundary, marked when cut.
fn tail(s: &str, n: usize) -> String {
    let count = s.chars().count();
    if count <= n {
        return s.to_string();
    }
    let cut: String = s.chars().skip(count - n).collect();
    format!("…{cut}")
}

/// Interpret a token command's stdout.
///
/// * A JSON object: the token is the first string among `access_token`,
///   `accessToken`, `token`. Expiry is absolute from `expires_at`, `expires_on`
///   or `expiresAt` (Unix seconds — milliseconds are recognized by magnitude),
///   else relative from `expires_in`. Numbers and numeric strings both count.
///   This reads `az account get-access-token -o json` and most brokers as-is.
/// * Anything else: the whole trimmed output is the token, which must then be
///   one whitespace-free printable-ASCII word (a leading `bearer` scheme word
///   is tolerated and dropped).
///
/// Errors never quote the output: it is, or contains, a credential.
pub fn parse_output(stdout: &[u8]) -> Result<TokenSet> {
    let text = std::str::from_utf8(stdout)
        .map_err(|_| Error::Config("token_command printed non-UTF-8 output".into()))?
        .trim();
    if text.is_empty() {
        return Err(Error::Config(
            "token_command succeeded but printed nothing on stdout".into(),
        ));
    }
    if text.starts_with('{') {
        let v: Value = serde_json::from_str(text).map_err(|_| {
            Error::Config("token_command printed something JSON-like that does not parse".into())
        })?;
        let token = ["access_token", "accessToken", "token"]
            .iter()
            .find_map(|k| v.get(*k).and_then(Value::as_str))
            .ok_or_else(|| {
                Error::Config(
                    "token_command printed JSON with no access_token, accessToken or token field"
                        .into(),
                )
            })?;
        let token = checked(token)?;
        let absolute = ["expires_at", "expires_on", "expiresAt"]
            .iter()
            .find_map(|k| v.get(*k).and_then(as_u64))
            .map(|t| if t > 100_000_000_000 { t / 1000 } else { t });
        let expires_at = absolute.or_else(|| {
            v.get("expires_in")
                .and_then(as_u64)
                .map(|s| config::now_unix().saturating_add(s))
        });
        return Ok(TokenSet {
            access_token: token,
            refresh_token: None,
            expires_at,
            token_type: None,
        });
    }
    let bare = match text.split_once(char::is_whitespace) {
        Some((scheme, rest)) if scheme.eq_ignore_ascii_case("bearer") => rest.trim(),
        _ => text,
    };
    Ok(TokenSet {
        access_token: checked(bare)?,
        refresh_token: None,
        expires_at: None,
        token_type: None,
    })
}

/// A token must be sendable as a header value: one printable-ASCII word.
fn checked(token: &str) -> Result<String> {
    if token.is_empty() || !token.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(Error::Config(
            "token_command output is not a single token (one line, no spaces) or a JSON object"
                .into(),
        ));
    }
    Ok(token.to_string())
}

fn as_u64(v: &Value) -> Option<u64> {
    v.as_u64()
        .or_else(|| v.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64))
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_token_is_trimmed() {
        let t = parse_output(b"  abc.def-123\n").unwrap();
        assert_eq!(t.access_token, "abc.def-123");
        assert!(t.expires_at.is_none());
    }

    #[test]
    fn a_scheme_word_is_dropped() {
        assert_eq!(parse_output(b"bearer XYZ\n").unwrap().access_token, "XYZ");
    }

    #[test]
    fn multi_word_output_is_refused_without_quoting_it() {
        let e = parse_output(b"secret-part other-part")
            .unwrap_err()
            .to_string();
        assert!(!e.contains("secret-part"), "{e}");
    }

    #[test]
    fn empty_output_is_an_error() {
        assert!(parse_output(b"\n \n").is_err());
    }

    #[test]
    fn json_with_expires_in_is_absolute_after_parse() {
        let before = config::now_unix();
        let t = parse_output(br#"{"access_token":"AT","expires_in":3600}"#).unwrap();
        assert_eq!(t.access_token, "AT");
        let exp = t.expires_at.unwrap();
        assert!(exp >= before + 3600 && exp <= config::now_unix() + 3600);
    }

    #[test]
    fn az_cli_shape_is_understood() {
        let t = parse_output(
            br#"{"accessToken":"AZ","expiresOn":"2030-01-01 00:00:00.000000","expires_on":1893456000,"tokenType":"Bearer"}"#,
        )
        .unwrap();
        assert_eq!(t.access_token, "AZ");
        assert_eq!(t.expires_at, Some(1_893_456_000));
    }

    #[test]
    fn millisecond_and_string_expiries_normalize_to_seconds() {
        let t = parse_output(br#"{"token":"T","expiresAt":1893456000000}"#).unwrap();
        assert_eq!(t.expires_at, Some(1_893_456_000));
        let t = parse_output(br#"{"token":"T","expires_at":"1893456000"}"#).unwrap();
        assert_eq!(t.expires_at, Some(1_893_456_000));
    }

    #[test]
    fn json_without_a_token_field_names_the_fields_not_the_content() {
        let e = parse_output(br#"{"secret":"shh"}"#)
            .unwrap_err()
            .to_string();
        assert!(e.contains("access_token"), "{e}");
        assert!(!e.contains("shh"), "{e}");
    }

    #[test]
    fn tail_cuts_on_char_boundaries() {
        assert_eq!(tail("abc", 5), "abc");
        assert_eq!(tail("ééééé", 2), "…éé");
    }
}
