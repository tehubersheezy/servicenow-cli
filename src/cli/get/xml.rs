//! `sn get xml <REF>` — a record's unload XML, the UI's "Export → XML".
//!
//! The export is the classic `<table>.do?UNL` processor (measured on a Brazil
//! PDI, admin; itil gets 403). Two measured hazards shape the request:
//!
//! - **An unknown parameter exports the whole table.** `sysparm_sys_id=` is
//!   not one the processor reads, and answered with 2 MB of every incident;
//!   only `sysparm_query=sys_id=…` filters, and a dropped term fails the same
//!   way. So `sysparm_record_count=2` caps the export (honoured: a bogus term
//!   returned two records), and exactly one top-level record carrying the
//!   asked sys_id is the only answer accepted.
//! - **The export includes related rows** — an incident's `sys_journal_field`
//!   entries ride along as sibling elements — which is what makes it
//!   importable whole, so they are kept.
//!
//! Output is the XML itself, not JSON.

use crate::cli::get_record::locate;
use crate::cli::kernel::connect;
use crate::cli::record_ref::parse_get_ref;
use crate::cli::{GlobalFlags, OutputMode};
use crate::error::{Error, NO_HTTP_STATUS, Result};
use std::io::Write;

#[derive(clap::Args, Debug)]
pub struct XmlArgs {
    /// Record reference: `table:sys_id`, `table:number`, or a bare number
    /// (`INC0010001`), as `sn get` takes.
    #[arg(value_name = "REF")]
    pub reference: String,
}

pub fn run(global: &GlobalFlags, args: XmlArgs) -> Result<()> {
    if global.output != OutputMode::Default {
        return Err(Error::Usage(
            "sn get xml writes the record's XML export; it cannot be combined with --output".into(),
        ));
    }
    let parsed = parse_get_ref(&args.reference)?;
    let client = connect(global)?;
    let (table, found) = locate(&client, parsed, false)?;
    let sys_id = found.sys_id;

    let xml = client.get_text(
        &format!("/{table}.do"),
        &[
            ("UNL".into(), String::new()),
            ("sysparm_query".into(), format!("sys_id={sys_id}")),
            ("sysparm_record_count".into(), "2".into()),
        ],
    )?;
    check_export(&xml, &table, &sys_id)?;
    let mut out = std::io::stdout().lock();
    out.write_all(xml.as_bytes())
        .and_then(|_| {
            if xml.ends_with('\n') {
                Ok(())
            } else {
                out.write_all(b"\n")
            }
        })
        .and_then(|_| out.flush())
        .map_err(|e| Error::Transport(format!("write stdout: {e}")))
}

/// Exactly one `<table action=…>` element, and it is the record asked for.
fn check_export(xml: &str, table: &str, sys_id: &str) -> Result<()> {
    let opener = format!("<{table} action=");
    let records = xml.matches(&opener).count();
    if records == 0 {
        if xml.contains("<unload") {
            return Err(Error::Api {
                status: NO_HTTP_STATUS,
                message: format!(
                    "the export of {table}/{sys_id} came back empty (not readable by this \
                     profile)"
                ),
                detail: None,
                transaction_id: None,
                sn_error: None,
            });
        }
        return Err(Error::Instance {
            message: format!("{table}.do?UNL did not answer with an unload document"),
            detail: Some(xml.chars().take(200).collect()),
        });
    }
    if records > 1 || !xml.contains(&format!("<sys_id>{sys_id}</sys_id>")) {
        return Err(Error::Instance {
            message: format!(
                "the export did not filter to {table}/{sys_id} (the instance dropped the \
                 sys_id= term), so it is not written"
            ),
            detail: None,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "abc";

    fn doc(body: &str) -> String {
        format!("<?xml version=\"1.0\"?>\n<unload unload_date=\"x\">{body}</unload>")
    }

    #[test]
    fn one_matching_record_with_related_rows_passes() {
        let xml = doc(
            "<incident action=\"INSERT_OR_UPDATE\"><sys_id>abc</sys_id></incident>\
             <sys_journal_field action=\"INSERT_OR_UPDATE\"><sys_id>j</sys_id></sys_journal_field>",
        );
        assert!(check_export(&xml, "incident", ID).is_ok());
    }

    #[test]
    fn two_records_or_a_stranger_fail_closed() {
        let two = doc("<incident action=\"I\"><sys_id>abc</sys_id></incident>\
             <incident action=\"I\"><sys_id>def</sys_id></incident>");
        assert!(matches!(
            check_export(&two, "incident", ID),
            Err(Error::Instance { .. })
        ));
        let stranger = doc("<incident action=\"I\"><sys_id>def</sys_id></incident>");
        assert!(matches!(
            check_export(&stranger, "incident", ID),
            Err(Error::Instance { .. })
        ));
    }

    #[test]
    fn empty_unload_is_not_found() {
        let empty = "<?xml version=\"1.0\"?>\n<unload unload_date=\"x\"/>";
        assert!(matches!(
            check_export(empty, "incident", ID),
            Err(Error::Api { .. })
        ));
        assert!(matches!(
            check_export("<html>login</html>", "incident", ID),
            Err(Error::Instance { .. })
        ));
    }
}
