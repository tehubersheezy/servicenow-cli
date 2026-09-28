use crate::cli::GlobalFlags;
use crate::cli::journal::validate_identifier;
use crate::cli::kernel::{build_client, build_profile, write_response};
use crate::cli::record_ref::{self, RefId};
use crate::client::normalize_base_url;
use crate::error::{Error, Result};
use serde_json::json;

#[derive(clap::Args, Debug)]
pub struct OpenArgs {
    /// Table name (e.g. `incident`), or a combined `table:sys_id` /
    /// `table:number` reference (e.g. `incident:INC0010001`).
    pub table: String,
    /// sys_id of the record. Omit it (and use a bare TABLE) to open the
    /// table's list view instead of a record form.
    pub sys_id: Option<String>,
    /// Encoded query filtering the list view, e.g. `active=true^priority=1`.
    /// List view only: refused together with a record (SYS_ID or a
    /// `table:id` reference).
    #[arg(long, short = 'q', alias = "sysparm-query", conflicts_with = "sys_id")]
    pub query: Option<String>,
    /// Print the URL to stdout instead of opening a browser.
    #[arg(long)]
    pub print_url: bool,
}

/// What the positionals name: one record's form, or a table's list view.
enum Target {
    Record(record_ref::RecordRef),
    List {
        table: String,
        query: Option<String>,
    },
}

/// Decide record vs list from argv alone — no network, exit 1 on a mistake.
/// A second positional or a `:` in the first token names a record (no table
/// name can contain a `:`); a bare table names its list. `--query` only
/// means something on a list, so pairing it with a record is refused rather
/// than silently dropped.
fn target(args: &OpenArgs) -> Result<Target> {
    if args.sys_id.is_some() || args.table.contains(':') {
        if args.query.is_some() {
            return Err(Error::Usage(
                "--query filters a list view; drop it to open the record, or pass \
                 a bare TABLE to open the filtered list"
                    .into(),
            ));
        }
        return record_ref::parse_pair(&args.table, args.sys_id.as_deref(), "table")
            .map(Target::Record);
    }
    validate_identifier(&args.table, "table")?;
    Ok(Target::List {
        table: args.table.clone(),
        // An empty filter is the unfiltered list; `sysparm_query=` adds nothing.
        query: args.query.clone().filter(|q| !q.is_empty()),
    })
}

/// Percent-encode everything outside RFC 3986's unreserved set. An encoded
/// query carries `=`, `^`, spaces, `&`, `%`, `@`… — every one of which means
/// something to a URL parser — so nothing but the unreserved set may ride raw.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `{instance}/nav_to.do?uri=<encoded target>` — `nav_to.do` keeps the
/// platform chrome (and redirects into the Next Experience shell where that
/// is on). The target is itself a URL, so it is encoded as a whole for its
/// ride inside `uri=`; a list's query is encoded once more *before* that, as
/// the value of the target's own `sysparm_query=`. Two layers, two decodes:
/// `nav_to.do` peels the outer one, the list page the inner. Encoding the
/// query only once would hand the list page a raw `&` or `%` to misparse.
fn ui_url(instance: &str, target_path: &str) -> String {
    format!("{instance}/nav_to.do?uri={}", percent_encode(target_path))
}

fn list_path(table: &str, query: Option<&str>) -> String {
    match query {
        Some(q) => format!("/{table}_list.do?sysparm_query={}", percent_encode(q)),
        None => format!("/{table}_list.do"),
    }
}

pub fn run(global: &GlobalFlags, args: OpenArgs) -> Result<()> {
    let target = target(&args)?;
    let profile = build_profile(global)?;
    // Profiles store the bare host, so the scheme has to be put back on — a
    // scheme-less "acme.service-now.com/nav_to.do?..." is not a URL a browser
    // will open, and it's what every profile made the documented way produces.
    let instance = normalize_base_url(&profile.instance);
    let target_path = match target {
        // A sys_id reference and a list build a URL offline, as this command
        // always has; a number is the one form that needs the instance (one
        // lookup) to name the sys_id the form URL requires.
        Target::Record(r) => {
            let sys_id = match &r.id {
                RefId::SysId(id) => id.clone(),
                RefId::Number(_) => {
                    let client = build_client(&profile, global.timeout)?;
                    r.resolve(&client)?
                }
            };
            format!("/{}.do?sys_id={sys_id}", r.table)
        }
        Target::List { table, query } => list_path(&table, query.as_deref()),
    };
    let url = ui_url(&instance, &target_path);

    if args.print_url {
        println!("{url}");
        return Ok(());
    }

    webbrowser::open(&url).map_err(|e| Error::Transport(format!("open browser: {e}")))?;

    let out = json!({ "opened": true, "url": url });
    write_response(global, &out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(table: &str, sys_id: Option<&str>, query: Option<&str>) -> OpenArgs {
        OpenArgs {
            table: table.into(),
            sys_id: sys_id.map(Into::into),
            query: query.map(Into::into),
            print_url: true,
        }
    }

    #[test]
    fn unreserved_characters_ride_raw_everything_else_is_encoded() {
        assert_eq!(percent_encode("aZ09-._~"), "aZ09-._~");
        assert_eq!(
            percent_encode("a=b^c d&e%f+g/h?i#j@k:l"),
            "a%3Db%5Ec%20d%26e%25f%2Bg%2Fh%3Fi%23j%40k%3Al"
        );
        // Non-ASCII goes out as its UTF-8 bytes.
        assert_eq!(percent_encode("é"), "%C3%A9");
    }

    #[test]
    fn record_url_is_unchanged_by_the_shared_encoder() {
        // The shape `sn open <table> <sys_id>` has always produced.
        assert_eq!(
            ui_url("https://x.service-now.com", "/incident.do?sys_id=abc123"),
            "https://x.service-now.com/nav_to.do?uri=%2Fincident.do%3Fsys_id%3Dabc123"
        );
    }

    #[test]
    fn list_query_is_encoded_twice() {
        let url = ui_url(
            "https://x.service-now.com",
            &list_path("incident", Some("active=true^priority=1")),
        );
        assert_eq!(
            url,
            "https://x.service-now.com/nav_to.do?uri=%2Fincident_list.do%3Fsysparm_query%3Dactive%253Dtrue%255Epriority%253D1"
        );
    }

    #[test]
    fn a_literal_ampersand_or_percent_cannot_escape_the_query() {
        let url = ui_url(
            "https://x",
            &list_path("incident", Some("short_descriptionLIKEa&b%c")),
        );
        assert!(url.ends_with("sysparm_query%3Dshort_descriptionLIKEa%2526b%2525c"));
    }

    #[test]
    fn bare_table_is_the_list() {
        assert_eq!(list_path("incident", None), "/incident_list.do");
        match target(&args("incident", None, None)).unwrap() {
            Target::List { table, query } => {
                assert_eq!(table, "incident");
                assert!(query.is_none());
            }
            Target::Record(_) => panic!("expected list"),
        }
    }

    #[test]
    fn empty_query_is_the_unfiltered_list() {
        match target(&args("incident", None, Some(""))).unwrap() {
            Target::List { query, .. } => assert!(query.is_none()),
            Target::Record(_) => panic!("expected list"),
        }
    }

    #[test]
    fn query_with_a_record_is_refused() {
        for a in [
            args("incident", Some("abc"), Some("active=true")),
            args("incident:INC0010001", None, Some("active=true")),
        ] {
            let err = target(&a).err().expect("usage error");
            assert!(matches!(err, Error::Usage(_)), "{err:?}");
        }
    }

    #[test]
    fn list_table_name_is_validated() {
        let err = target(&args("Incident List", None, None)).err().unwrap();
        assert!(matches!(err, Error::Usage(_)), "{err:?}");
    }

    #[test]
    fn two_positionals_or_a_ref_name_a_record() {
        assert!(matches!(
            target(&args("incident", Some("abc"), None)).unwrap(),
            Target::Record(_)
        ));
        assert!(matches!(
            target(&args("incident:INC0010001", None, None)).unwrap(),
            Target::Record(_)
        ));
    }
}
