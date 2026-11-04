//! `sn get logs` — recent system log (`syslog`) entries, newest first.
//!
//! The window is the server-evaluated relative operator
//! `sys_created_onRELATIVEGT@<unit>@ago@<n>` (measured fast on a Brazil PDI:
//! ~0.2s), so no timestamp has to cross a timezone. Every row is checked
//! against every term: its UTC `sys_created_on` must fall inside the window
//! (with a minute of clock slack), its level must be one asked for, and the
//! user/source/message filters must hold — a dropped term would otherwise
//! hand back old or unrelated entries as "recent". `syslog` is admin-only:
//! itil reads it as an empty table, which is an empty answer.
//!
//! Levels (measured choice list): -2 Trace, -1 Debug, 0 Information,
//! 1 Warning, 2 Error, 3 Fatal.

use super::common::{caller, dropped, opt, pairs, query_value, rows, value};
use crate::cli::GlobalFlags;
use crate::cli::kernel::{connect, write_response};
use crate::error::{Error, Result};
use serde_json::{Value, json};

#[derive(clap::Args, Debug)]
pub struct LogsArgs {
    /// How far back to look: a number and a unit, `s`, `m`, `h` or `d` (e.g.
    /// `30s`, `15m`, `2h`, `1d`).
    #[arg(long, default_value = "15m")]
    pub since: String,
    /// The least severe level to include; everything more severe comes too.
    #[arg(long, value_enum, default_value_t = LogLevel::Warning)]
    pub level: LogLevel,
    /// Only entries written by your own sessions.
    #[arg(long)]
    pub mine: bool,
    /// Only entries whose message contains this text (case-insensitive).
    #[arg(long, short = 'g')]
    pub grep: Option<String>,
    /// Only entries whose source contains this text (case-insensitive).
    #[arg(long)]
    pub source: Option<String>,
    /// Maximum entries to return.
    #[arg(long, short = 'l', default_value_t = 100)]
    pub limit: u32,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    /// Everything, down to trace.
    Trace,
    /// Debug and above.
    Debug,
    /// Information and above.
    Info,
    /// Warnings and above.
    Warning,
    /// Errors and fatals.
    Error,
    /// Fatals only.
    Fatal,
}

impl LogLevel {
    fn code(self) -> i8 {
        match self {
            LogLevel::Trace => -2,
            LogLevel::Debug => -1,
            LogLevel::Info => 0,
            LogLevel::Warning => 1,
            LogLevel::Error => 2,
            LogLevel::Fatal => 3,
        }
    }
}

fn level_name(code: i8) -> &'static str {
    match code {
        -2 => "trace",
        -1 => "debug",
        0 => "info",
        1 => "warning",
        2 => "error",
        3 => "fatal",
        _ => "unknown",
    }
}

/// `--since` as seconds plus the relative-operator unit and count that say
/// the same thing server-side.
fn parse_since(raw: &str) -> Result<(u64, &'static str, u64)> {
    let s = raw.trim();
    let bad = || {
        Error::Usage(format!(
            "--since '{raw}' is not a duration (a number and s, m, h or d — e.g. 15m)"
        ))
    };
    let (num, unit) = s.split_at(s.find(|c: char| !c.is_ascii_digit()).ok_or_else(bad)?);
    let n: u64 = num.parse().map_err(|_| bad())?;
    if n == 0 {
        return Err(bad());
    }
    let (secs, unit, count) = match unit {
        "s" => (n, "second", n),
        "m" => (n * 60, "minute", n),
        "h" => (n * 3600, "hour", n),
        "d" => (n * 86_400, "hour", n * 24),
        _ => return Err(bad()),
    };
    Ok((secs, unit, count))
}

const SLACK_SECS: u64 = 60;

pub fn run(global: &GlobalFlags, args: LogsArgs) -> Result<()> {
    let (window, unit, count) = parse_since(&args.since)?;
    let grep = args
        .grep
        .as_deref()
        .map(|g| query_value(g, "--grep"))
        .transpose()?;
    let source = args
        .source
        .as_deref()
        .map(|s| query_value(s, "--source"))
        .transpose()?;
    let client = connect(global)?;
    let user = if args.mine {
        Some(caller(&client)?.0)
    } else {
        None
    };

    let floor = args.level.code();
    let levels: Vec<String> = (floor..=3).map(|l| l.to_string()).collect();
    let mut q = format!(
        "sys_created_onRELATIVEGT@{unit}@ago@{count}^levelIN{}",
        levels.join(",")
    );
    if let Some(u) = &user {
        q.push_str(&format!("^sys_created_by={u}"));
    }
    if let Some(s) = source {
        q.push_str(&format!("^sourceLIKE{s}"));
    }
    if let Some(g) = grep {
        q.push_str(&format!("^messageLIKE{g}"));
    }
    q.push_str("^ORDERBYDESCsys_created_on");

    let resp = client.get(
        "/api/now/table/syslog",
        &pairs(&[
            ("sysparm_query", &q),
            (
                "sysparm_fields",
                "sys_id,sys_created_on,level,source,message,sys_created_by",
            ),
            ("sysparm_display_value", "false"),
            ("sysparm_limit", &args.limit.to_string()),
        ]),
    )?;
    let cutoff = now_secs().saturating_sub(window + SLACK_SECS);
    let filters = Filters {
        cutoff,
        floor,
        user: user.as_deref(),
        source,
        grep,
    };
    let mut out = Vec::new();
    for r in rows(resp) {
        out.push(filters.check(&r)?);
    }
    write_response(global, &Value::Array(out))
}

struct Filters<'a> {
    cutoff: u64,
    floor: i8,
    user: Option<&'a str>,
    source: Option<&'a str>,
    grep: Option<&'a str>,
}

impl Filters<'_> {
    /// The row as output, or the term it proves was dropped.
    fn check(&self, r: &Value) -> Result<Value> {
        let created = value(r, "sys_created_on").unwrap_or_default();
        if utc_secs(&created).is_none_or(|t| t < self.cutoff) {
            return Err(dropped("sys_created_on", "syslog"));
        }
        let level: i8 = value(r, "level")
            .and_then(|l| l.parse().ok())
            .unwrap_or(i8::MIN);
        if level < self.floor {
            return Err(dropped("level", "syslog"));
        }
        let by = value(r, "sys_created_by");
        if let Some(u) = self.user
            && !by.as_deref().is_some_and(|b| b.eq_ignore_ascii_case(u))
        {
            return Err(dropped("sys_created_by", "syslog"));
        }
        let source = value(r, "source");
        let message = value(r, "message");
        let contains = |hay: &Option<String>, needle: &str| {
            hay.as_deref()
                .is_some_and(|h| h.to_lowercase().contains(&needle.to_lowercase()))
        };
        if self.source.is_some_and(|s| !contains(&source, s)) {
            return Err(dropped("source", "syslog"));
        }
        if self.grep.is_some_and(|g| !contains(&message, g)) {
            return Err(dropped("message", "syslog"));
        }
        Ok(json!({
            "time": created,
            "level": level_name(level),
            "source": opt(source),
            "message": opt(message),
            "created_by": opt(by),
            "sys_id": opt(value(r, "sys_id")),
        }))
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// `YYYY-MM-DD HH:MM:SS` (UTC, as `sysparm_display_value=false` returns it)
/// to Unix seconds.
fn utc_secs(s: &str) -> Option<u64> {
    let (date, time) = s.split_once(' ')?;
    let mut d = date.splitn(3, '-').map(|p| p.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let mut t = time.splitn(3, ':').map(|p| p.parse::<i64>().ok());
    let (hh, mm, ss) = (t.next()??, t.next()??, t.next()??);
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) {
        return None;
    }
    // Howard Hinnant's days-from-civil.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + hh * 3600 + mm * 60 + ss).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn since_units() {
        assert_eq!(parse_since("15m").unwrap(), (900, "minute", 15));
        assert_eq!(parse_since("2d").unwrap(), (172_800, "hour", 48));
        assert_eq!(parse_since("30s").unwrap(), (30, "second", 30));
        for bad in ["", "m", "15", "0m", "5w", "-5m"] {
            assert!(parse_since(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn utc_parse_matches_known_epochs() {
        assert_eq!(utc_secs("1970-01-01 00:00:00"), Some(0));
        assert_eq!(utc_secs("2026-11-04 17:22:03"), Some(1_793_812_923));
        assert_eq!(utc_secs("2024-02-29 12:00:00"), Some(1_709_208_000));
        assert_eq!(utc_secs("garbage"), None);
    }

    fn filters(cutoff: u64) -> Filters<'static> {
        Filters {
            cutoff,
            floor: 1,
            user: Some("abey"),
            source: None,
            grep: Some("denied"),
        }
    }

    #[test]
    fn rows_are_held_to_every_term() {
        let ok = json!({"sys_created_on": "2026-11-04 17:22:03", "level": "2",
                        "sys_created_by": "Abey", "message": "Access DENIED", "source": "x"});
        let out = filters(1_793_812_000).check(&ok).unwrap();
        assert_eq!(out["level"], "error");

        let old = json!({"sys_created_on": "2020-01-01 00:00:00", "level": "2",
                         "sys_created_by": "abey", "message": "denied"});
        assert!(filters(1_793_812_000).check(&old).is_err());
        let info = json!({"sys_created_on": "2026-11-04 17:22:03", "level": "0",
                          "sys_created_by": "abey", "message": "denied"});
        assert!(filters(0).check(&info).is_err());
        let other = json!({"sys_created_on": "2026-11-04 17:22:03", "level": "2",
                           "sys_created_by": "bob", "message": "denied"});
        assert!(filters(0).check(&other).is_err());
        let msg = json!({"sys_created_on": "2026-11-04 17:22:03", "level": "2",
                         "sys_created_by": "abey", "message": "fine"});
        assert!(filters(0).check(&msg).is_err());
    }
}
