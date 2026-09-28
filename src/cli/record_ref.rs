//! Record references — the `table:identifier` input form.
//!
//! One syntax names a record everywhere a command needs one: `table:sys_id`
//! (used directly) or `table:number` (resolved through one lookup). Parsing is
//! split from resolution on purpose: parse errors are pure argv mistakes
//! (exit 1) and must precede `connect` — and, for destructive verbs, the
//! confirmation gate — while resolution is a network call (exit 2 on failure)
//! that runs only after both.
//!
//! Resolution queries `number={n}` with `sysparm_limit=2`, the same canary
//! `auth::identify` uses: ServiceNow silently drops a query term it cannot
//! parse and returns unfiltered rows, so on a table with no usable `number`
//! field the lookup would otherwise resolve to whichever record sorts first.
//! A second row is proof the term is gone, and the error says so instead of
//! returning a stranger's sys_id. (A table whose *total* row count is 1
//! defeats the canary — the same accepted residual as `identify_via_sys_user`.)
//!
//! An identifier that is 32 ASCII hex chars is classified as a sys_id and
//! never looked up. A record number that happens to be exactly 32 hex chars
//! would be misclassified; real numbers are prefix+digits and far shorter, and
//! the escape hatch is `sn table list <t> --query number=<n>`.
//!
//! `sn get`'s bare-number form (`sn get INC0010001`) names no table, so the
//! number's prefix has to: a fresh entry in the per-instance prefix cache
//! (`number_prefixes.toml` in the config dir), else the built-in ITSM + SIR
//! map, else one `sys_number` lookup — cached when it names exactly one table.
//! Parsing still happens before the network (the prefix is split off there);
//! only the prefix → table step waits for `connect`. `sys_number` is
//! admin-only by default, so for most profiles the built-in map is all there
//! is, and an unreadable `sys_number` degrades to it rather than to a guess.
//!
//! The table `sys_number` names is where the *counter* is defined, and a table
//! with no Number Maintenance row of its own numbers with its nearest
//! ancestor's (measured: 1,801 `cert_follow_on_task` rows numbered `TASK…`,
//! from `task`'s row). The number is still found on that table — a base-table
//! read includes its extensions — and `sn get` then re-targets to the row's
//! `sys_class_name`, so the record is read as what it is.
//!
//! The parse functions and their result types are `pub` (not `pub(crate)`)
//! only so the out-of-workspace `fuzz/` crate can reach them; nothing outside
//! this crate should treat them as API.

use crate::cli::journal::{validate_identifier, validate_sys_id};
use crate::client::Client;
use crate::config::{
    PrefixEntry, load_prefix_cache_from, now_unix, prefix_cache_path, update_prefix_cache_at,
};
use crate::error::{Error, NO_HTTP_STATUS, Result};
use crate::observability::log_note;
use serde_json::Value;

/// A parsed record reference: the table plus either a sys_id or a number.
#[derive(Debug, PartialEq)]
pub struct RecordRef {
    pub table: String,
    pub id: RefId,
}

#[derive(Debug, PartialEq)]
pub enum RefId {
    SysId(String),
    Number(String),
}

impl std::fmt::Display for RecordRef {
    /// `table/sys_id` for a sys_id (the wording confirm prompts always used),
    /// `table:number` for a number — the guard names what the caller typed,
    /// because learning the sys_id would take a network call the guard must
    /// not make.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.id {
            RefId::SysId(id) => write!(f, "{}/{}", self.table, id),
            RefId::Number(n) => write!(f, "{}:{}", self.table, n),
        }
    }
}

/// Built-in number-prefix map (ITSM + Security Incident Response) for
/// `sn get`'s bare-number form. The instance's own map lives in `sys_number`
/// and is consulted for any prefix not found here (see [`resolve_prefix`]);
/// this list is what keeps the stock prefixes free of that request, and the
/// fallback when `sys_number` is unreadable (it is admin-only by default).
const NUMBER_PREFIXES: [(&str, &str); 9] = [
    ("CHG", "change_request"),
    ("CTASK", "change_task"),
    ("INC", "incident"),
    ("KB", "kb_knowledge"),
    ("PRB", "problem"),
    ("REQ", "sc_request"),
    ("RITM", "sc_req_item"),
    ("SCTASK", "sc_task"),
    ("SIR", "sn_si_incident"),
];

/// 32 ASCII hex chars — the shape of every platform-generated sys_id.
pub(crate) fn is_sys_id(s: &str) -> bool {
    s.len() == 32 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Split and validate one `table:identifier` token. Splits on the first `:`
/// (a table name can never contain one), then validates each half on its own —
/// the identifier half through the same charset guard every sys_id gets, so
/// nothing spliced into an encoded query can carry a `:` or a `^`.
pub fn parse_ref(token: &str, what: &str) -> Result<RecordRef> {
    let Some((table, id)) = token.split_once(':') else {
        return Err(Error::Usage(format!(
            "'{token}' is not a {what}:identifier reference"
        )));
    };
    validate_identifier(table, what)?;
    validate_sys_id(id)?;
    let id = if is_sys_id(id) {
        RefId::SysId(id.to_string())
    } else {
        RefId::Number(id.to_string())
    };
    Ok(RecordRef {
        table: table.to_string(),
        id,
    })
}

/// The `(first, second)` positional pair every record-addressing command has.
/// Two tokens keep today's behavior exactly (second is the sys_id, verbatim);
/// one token must be a combined reference. A ref *and* a second token is
/// refused rather than picking a winner — either reading loses something
/// silently.
pub fn parse_pair(first: &str, second: Option<&str>, what: &str) -> Result<RecordRef> {
    match second {
        Some(sys_id) => {
            if first.contains(':') {
                return Err(Error::Usage(format!(
                    "give the record once: either `<{0}> <SYS_ID>` or a combined \
                     `<{0}>:<ID>` reference, not both",
                    what.to_uppercase()
                )));
            }
            validate_identifier(first, what)?;
            validate_sys_id(sys_id)?;
            Ok(RecordRef {
                table: first.to_string(),
                id: RefId::SysId(sys_id.to_string()),
            })
        }
        None => {
            if !first.contains(':') {
                return Err(Error::Usage(format!(
                    "missing SYS_ID: pass `<{0}> <SYS_ID>` or a combined \
                     `<{0}>:<SYS_ID|NUMBER>` reference (e.g. incident:INC0010001)",
                    what.to_uppercase()
                )));
            }
            parse_ref(first, what)
        }
    }
}

/// `attachment upload`'s flag pair: `--record` may carry the whole reference,
/// `--table` is the split form's other half.
pub(crate) fn parse_flag_pair(table: Option<&str>, record: &str) -> Result<RecordRef> {
    if record.contains(':') {
        if table.is_some() {
            return Err(Error::Usage(
                "give the table once: either --table with a bare --record sys_id, \
                 or a combined --record `table:id` reference, not both"
                    .into(),
            ));
        }
        return parse_ref(record, "table");
    }
    let Some(table) = table else {
        return Err(Error::Usage(
            "--table is required unless --record is a `table:identifier` reference".into(),
        ));
    };
    validate_identifier(table, "table")?;
    validate_sys_id(record)?;
    Ok(RecordRef {
        table: table.to_string(),
        id: RefId::SysId(record.to_string()),
    })
}

/// A record number's prefix: everything before its trailing run of digits.
///
/// `sys_number` builds a number as `prefix` + a zero-padded counter, so the
/// digits at the end are the counter and the rest is the prefix — including
/// prefixes that carry digits or underscores themselves (`K8S_DEPLOY`,
/// `MID_`, both stock). `None` when there is no counter (`INC`) or no prefix
/// (`0010001`): neither is a record number. A prefix that itself *ends* in a
/// digit cannot be told apart from its counter, and loses that digit here.
pub fn number_prefix(number: &str) -> Option<&str> {
    let prefix = number.trim_end_matches(|c: char| c.is_ascii_digit());
    (!prefix.is_empty() && prefix.len() < number.len()).then_some(prefix)
}

/// The built-in map's table for `prefix`, matched whole and case-sensitively
/// (SCTASK is never truncated to SC; `inc` is not INC).
pub(crate) fn builtin_table(prefix: &str) -> Option<&'static str> {
    NUMBER_PREFIXES
        .iter()
        .find(|(p, _)| *p == prefix)
        .map(|(_, t)| *t)
}

/// `sn get`'s REF positional, parsed without the network.
#[derive(Debug, PartialEq)]
pub enum GetRef {
    /// A `table:identifier` reference: the caller named the table.
    Ref(RecordRef),
    /// A bare record number. Its table is the prefix's, which may take a
    /// `sys_number` lookup and so is resolved after `connect`
    /// ([`resolve_prefix`]).
    Bare { number: String, prefix: String },
}

/// `sn get`'s REF positional: a `table:identifier` reference or a bare record
/// number. A bare sys_id names no table and is refused rather than guessed,
/// and so is a token with no prefix/counter shape — neither is worth a
/// `sys_number` request to reject.
pub fn parse_get_ref(token: &str) -> Result<GetRef> {
    if token.contains(':') {
        return parse_ref(token, "table").map(GetRef::Ref);
    }
    if is_sys_id(token) {
        return Err(Error::Usage(format!(
            "a bare sys_id names no table; use `table:{token}`"
        )));
    }
    validate_sys_id(token)?;
    let Some(prefix) = number_prefix(token) else {
        return Err(Error::Usage(format!(
            "'{token}' is not a record number (expected a prefix and a counter, e.g. \
             INC0010001); use a `table:identifier` reference to name the table yourself"
        )));
    };
    Ok(GetRef::Bare {
        number: token.to_string(),
        prefix: prefix.to_string(),
    })
}

/// How long a cached `sys_number` answer is trusted before it is asked again.
/// Staleness mostly heals itself sooner — a number that misses its cached
/// table triggers a re-check ([`recheck_prefix`]) — so this only bounds the
/// case a miss cannot reveal: a second table adopting the prefix later.
const PREFIX_CACHE_TTL_SECS: u64 = 7 * 24 * 60 * 60;

/// How long a cache write may wait for `.sn.lock`. A cache write is an
/// optimisation, so on contention it is skipped rather than stalling the read.
const PREFIX_CACHE_LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// Upper bound on `sys_number` rows per prefix lookup. Prefixes are *not*
/// unique (measured: six shared by two tables each on a stock Australia PDI),
/// so this is not a limit-2 canary — the canary is that every row returned
/// must carry the prefix asked about (see [`lookup_sys_number`]).
const PREFIX_LOOKUP_LIMIT: usize = 10;

/// Where a prefix's table came from. Only an answer `sys_number` just gave
/// is final; a cached or built-in one earns a re-check when the number is not
/// on that table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrefixSource {
    Cache,
    Builtin,
    Instance,
}

/// What `sys_number` said about one prefix.
#[derive(Debug, PartialEq)]
enum SysNumber {
    /// Exactly one table numbers with this prefix.
    One(String),
    /// Several do — the prefix alone cannot name the table.
    Many(Vec<String>),
    /// None does. `case_variants` are prefixes matching only when case is
    /// ignored (the instance's query is case-insensitive; prefixes are not).
    None { case_variants: Vec<String> },
}

/// Resolve a bare number's prefix to a table: a fresh cache entry, then the
/// built-in map, then one `sys_number` lookup (cached when it names exactly
/// one table).
///
/// An unknown or ambiguous prefix is `Error::Usage` (exit 1) even though it
/// took a request to learn: the fix is the caller's to make — name the table
/// with a `table:number` reference. `sys_number` being unreadable (403 — it is
/// admin-only by default) degrades to the built-in map's answer, never to a
/// guess. A dropped `prefix=` term is `Error::Instance`.
pub(crate) fn resolve_prefix(client: &Client, prefix: &str) -> Result<(String, PrefixSource)> {
    if let Some(table) = cached_table(client, prefix) {
        return Ok((table, PrefixSource::Cache));
    }
    if let Some(table) = builtin_table(prefix) {
        return Ok((table.to_string(), PrefixSource::Builtin));
    }
    let known: Vec<&str> = NUMBER_PREFIXES.iter().map(|(p, _)| *p).collect();
    let known = known.join(", ");
    match lookup_sys_number(client, prefix) {
        Ok(SysNumber::One(table)) => {
            store_cached(client, prefix, Some(&table));
            Ok((table, PrefixSource::Instance))
        }
        Ok(SysNumber::Many(tables)) => Err(Error::Usage(format!(
            "prefix {prefix} is shared by several tables on this instance ({}); use a \
             `table:number` reference to name the table yourself",
            tables.join(", ")
        ))),
        Ok(SysNumber::None { case_variants }) => {
            let hint = if case_variants.is_empty() {
                String::new()
            } else {
                format!(
                    " (prefixes are case-sensitive: did you mean {}?)",
                    case_variants.join(" or ")
                )
            };
            Err(Error::Usage(format!(
                "no table on this instance numbers its records with prefix {prefix}{hint}; \
                 use a `table:number` reference to name the table yourself"
            )))
        }
        // The profile cannot read sys_number: only the built-in prefixes are
        // known, which is exactly what `sn get` knew before the lookup existed.
        // (401 stays an auth error: it is a verdict on the credentials, which
        // the record read would reach too.)
        Err(Error::Auth { status: 403, .. }) => Err(Error::Usage(format!(
            "prefix {prefix} is not a built-in one (known: {known}) and this profile \
                 cannot read the instance's sys_number table to look it up; use a \
                 `table:number` reference to name the table yourself"
        ))),
        Err(e) => Err(e),
    }
}

/// A cached or built-in prefix answer missed: the number is not on `tried`.
/// Ask `sys_number` whether the prefix now belongs elsewhere (a renumbered
/// table, a stale cache entry). `Some(table)` when it names exactly one table
/// other than `tried` — the caller retries there; `None` otherwise, and the
/// caller reports its original not-found. Every failure here is swallowed for
/// that reason: the not-found is already the truthful answer.
pub(crate) fn recheck_prefix(client: &Client, prefix: &str, tried: &str) -> Option<String> {
    match lookup_sys_number(client, prefix) {
        Ok(SysNumber::One(table)) => {
            // Agreeing with the built-in map needs no cache entry — and a
            // stale one shadowing the built-in is dropped.
            let keep = builtin_table(prefix) != Some(table.as_str());
            store_cached(client, prefix, keep.then_some(table.as_str()));
            (table != tried).then_some(table)
        }
        Ok(_) => {
            store_cached(client, prefix, None);
            None
        }
        Err(e) => {
            log_note(&format!("sys_number re-check for {prefix} failed: {e}"));
            None
        }
    }
}

/// One `sys_number` read for `prefix`.
///
/// The canary is per row, not a row count: prefixes legitimately repeat, so
/// two rows prove nothing — but a row whose prefix is not the one asked about
/// (even ignoring case, which the instance's `=` does) proves the `prefix=`
/// term was dropped and the rows are unfiltered.
fn lookup_sys_number(client: &Client, prefix: &str) -> Result<SysNumber> {
    let pairs = vec![
        ("sysparm_query".to_string(), format!("prefix={prefix}")),
        ("sysparm_fields".to_string(), "prefix,category".to_string()),
        ("sysparm_limit".to_string(), PREFIX_LOOKUP_LIMIT.to_string()),
        ("sysparm_display_value".to_string(), "false".to_string()),
        (
            "sysparm_exclude_reference_link".to_string(),
            "true".to_string(),
        ),
    ];
    let resp = match client.get("/api/now/table/sys_number", &pairs) {
        Ok(resp) => resp,
        // Older releases answer an empty Table API list with 404 "No Record
        // found" rather than `[]`.
        Err(Error::Api { status: 404, .. }) => {
            return Ok(SysNumber::None {
                case_variants: Vec::new(),
            });
        }
        Err(e) => return Err(e),
    };
    let rows = resp
        .get("result")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut tables = Vec::new();
    let mut case_variants = Vec::new();
    for row in &rows {
        let got = row.get("prefix").and_then(Value::as_str).unwrap_or("");
        if !got.eq_ignore_ascii_case(prefix) {
            return Err(Error::Instance {
                message: format!(
                    "cannot resolve prefix {prefix}: the prefix={prefix} query term was \
                     dropped by the instance, so the sys_number rows returned are arbitrary"
                ),
                detail: Some("use a `table:number` reference to name the table yourself".into()),
            });
        }
        if got != prefix {
            case_variants.push(got.to_string());
            continue;
        }
        // `category` is a reference to sys_db_object keyed by name; without
        // exclude_reference_link it would arrive as `{link, value}`.
        let table = row
            .get("category")
            .and_then(|c| {
                c.as_str()
                    .or_else(|| c.get("value").and_then(Value::as_str))
            })
            .unwrap_or("");
        if table.is_empty() {
            // A counter with no table (measured: five stock rows have an
            // empty prefix; none here, but nothing can be resolved to it).
            continue;
        }
        // The name is spliced into a URL path and a GraphQL document, so it
        // gets the same guard a table name typed on argv does.
        validate_identifier(table, "table").map_err(|_| Error::Instance {
            message: format!(
                "sys_number names '{table}' for prefix {prefix}, which is not a table name"
            ),
            detail: None,
        })?;
        tables.push(table.to_string());
    }
    tables.sort();
    tables.dedup();
    case_variants.sort();
    case_variants.dedup();
    Ok(match tables.len() {
        0 => SysNumber::None { case_variants },
        1 => SysNumber::One(tables.remove(0)),
        _ => SysNumber::Many(tables),
    })
}

fn cached_table(client: &Client, prefix: &str) -> Option<String> {
    let path = prefix_cache_path().ok()?;
    let cache = load_prefix_cache_from(&path);
    let entry = cache.instances.get(client.base_url())?.get(prefix)?;
    let fresh = now_unix().saturating_sub(entry.fetched_at) < PREFIX_CACHE_TTL_SECS;
    // A hand-edited cache is still untrusted input to a URL path.
    (fresh && validate_identifier(&entry.table, "table").is_ok()).then(|| entry.table.clone())
}

/// Record (`Some`) or forget (`None`) a prefix's table. Best-effort: a cache
/// that cannot be written costs the next call one request, so failure is a
/// `-v` note, never an error.
fn store_cached(client: &Client, prefix: &str, table: Option<&str>) {
    let result = prefix_cache_path().and_then(|path| {
        // Forgetting what was never cached is not worth a lock and a write.
        if table.is_none()
            && !load_prefix_cache_from(&path)
                .instances
                .get(client.base_url())
                .is_some_and(|m| m.contains_key(prefix))
        {
            return Ok(());
        }
        update_prefix_cache_at(&path, PREFIX_CACHE_LOCK_WAIT, |cache| {
            let base = client.base_url().to_string();
            match table {
                Some(t) => {
                    cache.instances.entry(base).or_default().insert(
                        prefix.to_string(),
                        PrefixEntry {
                            table: t.to_string(),
                            fetched_at: now_unix(),
                        },
                    );
                }
                None => {
                    if let Some(m) = cache.instances.get_mut(&base) {
                        m.remove(prefix);
                        if m.is_empty() {
                            cache.instances.remove(&base);
                        }
                    }
                }
            }
        })
    });
    if let Err(e) = result {
        log_note(&format!("number-prefix cache not updated: {e}"));
    }
}

impl RecordRef {
    /// The record's sys_id — free for a sys_id reference, one Table API lookup
    /// for a number. See the module doc for the limit-2 canary this rides on.
    pub(crate) fn resolve(&self, client: &Client) -> Result<String> {
        let number = match &self.id {
            RefId::SysId(id) => return Ok(id.clone()),
            RefId::Number(n) => n,
        };
        let pairs = vec![
            ("sysparm_query".to_string(), format!("number={number}")),
            ("sysparm_fields".to_string(), "sys_id".to_string()),
            ("sysparm_limit".to_string(), "2".to_string()),
            // Pinned false so the sys_id comes back raw whatever the
            // instance's display-value defaults are.
            ("sysparm_display_value".to_string(), "false".to_string()),
        ];
        let resp = client.get(&format!("/api/now/table/{}", self.table), &pairs)?;
        let rows = resp
            .get("result")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        match rows.len() {
            0 => Err(Error::Api {
                // The HTTP call succeeded; the *operation* found nothing. No
                // status is published rather than fabricating a 404.
                status: NO_HTTP_STATUS,
                message: format!("no {} record with number {number}", self.table),
                detail: None,
                transaction_id: None,
                sn_error: None,
            }),
            1 => rows[0]
                .pointer("/sys_id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .ok_or_else(|| Error::Instance {
                    message: format!(
                        "the {} record matching number {number} came back without a sys_id",
                        self.table
                    ),
                    detail: None,
                }),
            // `number` is unique where it exists, so a second row is proof the
            // instance dropped the term and returned unfiltered rows.
            _ => Err(Error::Instance {
                message: format!(
                    "cannot resolve {number}: the number={number} query term was dropped \
                     by the instance, so the rows returned are arbitrary"
                ),
                detail: Some(format!(
                    "{0} likely has no queryable `number` field; pass the record's \
                     sys_id instead ({0}:<sys_id>)",
                    self.table
                )),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEX: &str = "1c741bd70b2322007518478d83673af3";

    #[test]
    fn sys_id_classification() {
        assert!(is_sys_id(HEX));
        assert!(is_sys_id(&HEX.to_uppercase()));
        assert!(!is_sys_id(&HEX[..31]));
        assert!(!is_sys_id(&format!("{HEX}0")));
        assert!(!is_sys_id("INC0010001"));
    }

    #[test]
    fn parse_ref_splits_and_classifies() {
        let r = parse_ref(&format!("incident:{HEX}"), "table").unwrap();
        assert_eq!(r.table, "incident");
        assert_eq!(r.id, RefId::SysId(HEX.into()));

        let r = parse_ref("incident:INC0010001", "table").unwrap();
        assert_eq!(r.id, RefId::Number("INC0010001".into()));
    }

    #[test]
    fn parse_ref_rejects_bad_halves() {
        // Empty halves.
        assert!(matches!(parse_ref(":abc", "table"), Err(Error::Usage(_))));
        assert!(matches!(
            parse_ref("incident:", "table"),
            Err(Error::Usage(_))
        ));
        // Split is on the FIRST colon, so the second lands in the identifier
        // half and fails its charset guard — never a silent truncation.
        assert!(matches!(parse_ref("a:b:c", "table"), Err(Error::Usage(_))));
        // Uppercase table name.
        assert!(matches!(
            parse_ref("Incident:abc", "table"),
            Err(Error::Usage(_))
        ));
    }

    #[test]
    fn parse_pair_two_tokens_is_verbatim() {
        let r = parse_pair("incident", Some("abc"), "table").unwrap();
        assert_eq!(r.id, RefId::SysId("abc".into()));
    }

    #[test]
    fn parse_pair_ref_plus_second_token_is_refused() {
        let err = parse_pair(&format!("incident:{HEX}"), Some("abc"), "table").unwrap_err();
        assert!(err.to_string().contains("give the record once"), "{err}");
    }

    #[test]
    fn parse_pair_bare_single_token_names_both_forms() {
        let err = parse_pair("incident", None, "table").unwrap_err();
        let text = err.to_string();
        assert!(text.contains("missing SYS_ID"), "{text}");
        assert!(text.contains("incident:INC0010001"), "{text}");
    }

    #[test]
    fn flag_pair_forms() {
        let r = parse_flag_pair(None, &format!("incident:{HEX}")).unwrap();
        assert_eq!(r.table, "incident");

        let r = parse_flag_pair(Some("incident"), "abc").unwrap();
        assert_eq!(r.id, RefId::SysId("abc".into()));

        assert!(matches!(
            parse_flag_pair(Some("incident"), "incident:abc"),
            Err(Error::Usage(_))
        ));
        let err = parse_flag_pair(None, "abc").unwrap_err();
        assert!(err.to_string().contains("--table is required"), "{err}");
    }

    #[test]
    fn number_prefix_is_everything_before_the_counter() {
        assert_eq!(number_prefix("INC0010001"), Some("INC"));
        assert_eq!(number_prefix("SCTASK0010001"), Some("SCTASK"));
        // Stock prefixes carrying digits and underscores survive whole.
        assert_eq!(number_prefix("K8S_DEPLOY0001001"), Some("K8S_DEPLOY"));
        assert_eq!(number_prefix("MID_0001001"), Some("MID_"));
        assert_eq!(number_prefix("inc0010001"), Some("inc"));
        // No counter, or no prefix: not a record number.
        assert_eq!(number_prefix("INC"), None);
        assert_eq!(number_prefix("0010001"), None);
        assert_eq!(number_prefix("INC0010001A"), None);
        assert_eq!(number_prefix(""), None);
    }

    #[test]
    fn builtin_map_matches_whole_prefix_case_sensitively() {
        for (prefix, table) in [
            ("CHG", "change_request"),
            ("CTASK", "change_task"),
            ("INC", "incident"),
            ("KB", "kb_knowledge"),
            ("PRB", "problem"),
            ("REQ", "sc_request"),
            ("RITM", "sc_req_item"),
            ("SCTASK", "sc_task"),
            ("SIR", "sn_si_incident"),
        ] {
            assert_eq!(builtin_table(prefix), Some(table));
        }
        // Matched whole: SCTASK is never truncated to SC, and neither an
        // unknown prefix nor a lowercase spelling maps to anything.
        assert_eq!(builtin_table("SC"), None);
        assert_eq!(builtin_table("TASK"), None);
        assert_eq!(builtin_table("inc"), None);
    }

    #[test]
    fn get_ref_forms() {
        assert_eq!(
            parse_get_ref("INC0010001").unwrap(),
            GetRef::Bare {
                number: "INC0010001".into(),
                prefix: "INC".into(),
            }
        );
        // An unknown prefix parses: whether it names a table is the
        // instance's question, answered after connect.
        assert_eq!(
            parse_get_ref("HRC0001001").unwrap(),
            GetRef::Bare {
                number: "HRC0001001".into(),
                prefix: "HRC".into(),
            }
        );
        let GetRef::Ref(r) = parse_get_ref(&format!("sys_user:{HEX}")).unwrap() else {
            panic!("a table:identifier token is a Ref");
        };
        assert_eq!(r.table, "sys_user");

        let err = parse_get_ref(HEX).unwrap_err();
        assert!(err.to_string().contains("names no table"), "{err}");

        for token in ["INC", "0010001"] {
            let err = parse_get_ref(token).unwrap_err();
            let text = err.to_string();
            assert!(text.contains("not a record number"), "{text}");
            assert!(text.contains("table:identifier"), "{text}");
        }
    }

    #[test]
    fn display_names_what_the_caller_typed() {
        let r = parse_ref(&format!("incident:{HEX}"), "table").unwrap();
        assert_eq!(r.to_string(), format!("incident/{HEX}"));
        let r = parse_ref("incident:INC0010001", "table").unwrap();
        assert_eq!(r.to_string(), "incident:INC0010001");
    }
}
