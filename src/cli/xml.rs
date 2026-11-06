//! `sn xml export` / `sn xml import` — move records between instances as an
//! unload document, the UI's "Export → XML" and "Import XML".
//!
//! Export is the classic `<table>.do?UNL` processor and import is
//! `sys_upload.do`; a file from one loads through the other. `?XML` is not the
//! same thing: its `<xml>` root has no `action` attributes and no related rows.
//!
//! Measured on a Brazil PDI (dev457896, 2026-11-05), and load-bearing:
//!
//! - **A term on a column that does not exist is dropped.** The export and
//!   `/api/now/stats` both answer as if it were not there (`zzz_nope=1` is the
//!   whole table), so neither can check the other. The query's columns are
//!   checked against `ui/meta` before anything is exported.
//! - **An unknown table answers `200` with an empty body**, and `ui/meta`
//!   answers `{"columns":{}}`.
//! - **`sys_upload.do` says nothing.** Success, a missing or bad CSRF token, an
//!   unparseable file, an unknown table and a non-admin caller all answer `200`
//!   with an empty body. `sn xml import` reports the upload, not the load.
//! - **It needs a non-empty `sysparm_target`, whose value is ignored**: the file
//!   names its tables. Blank or absent loads nothing.
//! - **`action="DELETE"` deletes the row**, so an import is gated even though it
//!   is "only" a load.
//!
//! - **`.do?UNL` refuses a bearer token.** `401` with the token, with or without
//!   the session's cookies, and with the cookies alone until the session has
//!   loaded a page. After one `GET /home.do` the cookies alone work, and adding
//!   the token back is `401` again.
//!
//! itil: export is 403. Import is the silent no-op above.

use crate::cli::api::path_with_query;
use crate::cli::attachment::stream_to_stdout;
use crate::cli::get::common::identifier;
use crate::cli::kernel::{
    SessionScope, confirm_destructive, connect_without_redirects, write_response,
};
use crate::cli::{GlobalFlags, OutputMode};
use crate::client::{Auth, Client, SessionJar, UI_SESSION_PATH, UiFile, UiResponse};
use crate::error::{Error, Result};
use clap::Subcommand;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

const UPLOAD_PATH: &str = "/sys_upload.do";
const META_PATH: &str = "/api/now/ui/meta";
const HOME_PATH: &str = "/home.do";
/// Any non-empty value loads; the file names its own tables (measured).
const TARGET: &str = "sn_xml_import";

#[derive(Subcommand, Debug)]
pub enum XmlSub {
    /// Write the records a query matches to stdout as unload XML, with their
    /// journal entries and attachments. `sn xml import` loads the result.
    Export(XmlExportArgs),
    /// Upload an unload XML file. Writes straight to the tables the file names,
    /// keeping sys_ids and skipping business rules. Overwrites existing rows and
    /// honours DELETE actions: gated behind --yes. Requires admin. The instance
    /// reports nothing about the load, so a success means the file was accepted.
    Import(XmlImportArgs),
}

#[derive(clap::Args, Debug)]
pub struct XmlExportArgs {
    /// Table to export. A parent table's rows arrive under their child class.
    #[arg(value_name = "TABLE")]
    pub table: String,
    /// Encoded query selecting the records (`active=true^priority=1`).
    #[arg(value_name = "QUERY")]
    pub query: String,
    /// Export at most this many records. The instance caps an export at its
    /// `glide.xml.export.limit` (10,000 by default) whatever this says.
    #[arg(long, short = 'l', value_parser = clap::value_parser!(u32).range(1..))]
    pub limit: Option<u32>,
}

#[derive(clap::Args, Debug)]
pub struct XmlImportArgs {
    /// Unload XML file from `sn xml export`, or `-` for stdin.
    #[arg(value_name = "FILE")]
    pub file: String,
    /// Skip the confirmation prompt (required when stdin is not a terminal).
    #[arg(long, short = 'y')]
    pub yes: bool,
}

pub fn export(global: &GlobalFlags, args: XmlExportArgs) -> Result<()> {
    if global.output != OutputMode::Default {
        return Err(Error::Usage(
            "sn xml export writes unload XML; it cannot be combined with --output".into(),
        ));
    }
    let table = identifier(&args.table, "table name")?;
    let query = args.query.trim();
    if query.is_empty() {
        return Err(Error::Usage("the query is empty".into()));
    }
    // A refused `.do` can redirect to a login page; followed, that page would
    // be written out as the export.
    let client = connect_without_redirects(global)?;
    check_columns(&client, table, query)?;

    let mut params = vec![
        ("UNL".to_string(), String::new()),
        ("sysparm_query".to_string(), query.to_string()),
    ];
    if let Some(limit) = args.limit {
        params.push(("sysparm_record_count".to_string(), limit.to_string()));
    }
    let path = path_with_query(&format!("/{table}.do"), &params);
    let (client, _session) = unload_client(global, client)?;
    stream_to_stdout(&mut client.download_file(&path)?)
}

/// The client to fetch an unload with. The processor takes basic auth itself,
/// but answers `401` to a bearer token, even alongside a valid session cookie:
/// it needs the session's cookies alone, on a session that has loaded a page.
/// The returned scope must outlive the client's requests.
fn unload_client(global: &GlobalFlags, client: Client) -> Result<(Client, Option<SessionScope>)> {
    let (client, scope) = match client.auth() {
        Auth::Basic { .. } | Auth::None => return Ok((client, None)),
        // `sn impersonate` already runs on a cookie-only session.
        Auth::Session(_) => (client, None),
        Auth::Bearer { .. } | Auth::ApiKey { .. } => {
            let (session, cookies) = client.get_secret_with_cookies(UI_SESSION_PATH)?;
            let token = session
                .get("SessionToken")
                .and_then(Value::as_str)
                .map(str::to_string);
            let scope = SessionScope::enter(Arc::new(SessionJar::new(cookies, token)));
            (connect_without_redirects(global)?, Some(scope))
        }
    };
    let status = client.get_discard(HOME_PATH)?;
    if !status.is_success() {
        return Err(Error::Instance {
            message: format!(
                "{HOME_PATH} answered HTTP {} for the export's session",
                status.as_u16()
            ),
            detail: None,
        });
    }
    Ok((client, scope))
}

pub fn import(global: &GlobalFlags, args: XmlImportArgs) -> Result<()> {
    let what = if args.file == "-" {
        "stdin"
    } else {
        &args.file
    };
    confirm_destructive(args.yes, "import", what)?;
    let bytes = read_file(&args.file)?;
    let size = bytes.len();

    let client = connect_without_redirects(global)?;
    let session = client.ui_session()?;
    let name = file_name(&args.file);
    let file = UiFile {
        field: "attachFile",
        name: name.clone(),
        bytes,
    };
    let resp = client.post_ui_upload(
        UPLOAD_PATH,
        &session,
        &[("sysparm_target".to_string(), TARGET.to_string())],
        file,
    )?;
    judge_upload(&resp)?;
    write_response(global, &json!({"uploaded": name, "bytes": size}))
}

/// Each query column must exist, walking dot-walks through their references.
fn check_columns(client: &Client, table: &str, query: &str) -> Result<()> {
    let mut schemas = HashMap::new();
    if columns(client, &mut schemas, table)?.is_empty() {
        return Err(Error::Usage(format!("no table '{table}' on this instance")));
    }
    for field in query_fields(query) {
        let mut on = table.to_string();
        let mut segments = field.split('.').peekable();
        while let Some(segment) = segments.next() {
            let cols = columns(client, &mut schemas, &on)?;
            let Some(column) = cols.get(segment) else {
                return Err(Error::Usage(format!(
                    "'{field}' in the query: {on} has no column '{segment}', and the instance \
                     would drop that term and export unfiltered rows"
                )));
            };
            if segments.peek().is_some() {
                let Some(next) = column.get("reference").and_then(Value::as_str) else {
                    return Err(Error::Usage(format!(
                        "'{field}' in the query: {on}.{segment} is not a reference, so it \
                         cannot be dot-walked"
                    )));
                };
                on = next.to_string();
            }
        }
    }
    Ok(())
}

fn columns<'a>(
    client: &Client,
    schemas: &'a mut HashMap<String, Value>,
    table: &str,
) -> Result<&'a serde_json::Map<String, Value>> {
    if !schemas.contains_key(table) {
        let resp = client.get(&format!("{META_PATH}/{table}"), &[])?;
        let cols = resp
            .pointer("/result/columns")
            .filter(|c| c.is_object())
            .cloned()
            .ok_or_else(|| Error::Instance {
                message: format!("{META_PATH}/{table} returned no columns object"),
                detail: None,
            })?;
        schemas.insert(table.to_string(), cols);
    }
    Ok(schemas[table].as_object().expect("checked on insert"))
}

/// The column path each term of an encoded query names. Terms that do not
/// start with one (`123TEXTQUERY321=…`, `EQ`, `variables.…`, `ref_task.…`) are
/// not checkable and are left out, as is any query holding a `RLQUERY`, whose
/// terms address another table.
fn query_fields(query: &str) -> Vec<String> {
    if query.contains("RLQUERY") {
        return Vec::new();
    }
    let unescaped = query.replace("^^", "");
    let mut fields = Vec::new();
    for term in unescaped.split('^') {
        let term = term
            .strip_prefix("ORDERBYDESC")
            .or_else(|| term.strip_prefix("ORDERBY"))
            .or_else(|| term.strip_prefix("GROUPBY"))
            .or_else(|| term.strip_prefix("NQ"))
            .or_else(|| term.strip_prefix("OR"))
            .unwrap_or(term);
        let end = term
            .find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || "_.".contains(c)))
            .unwrap_or(term.len());
        let field = &term[..end];
        let checkable = field.starts_with(|c: char| c.is_ascii_lowercase())
            && !field.starts_with("variables.")
            && !field.starts_with("ref_");
        if checkable {
            fields.push(field.to_string());
        }
    }
    fields
}

fn read_file(file: &str) -> Result<Vec<u8>> {
    if file == "-" {
        let mut bytes = Vec::new();
        std::io::stdin()
            .read_to_end(&mut bytes)
            .map_err(|e| Error::Usage(format!("read stdin: {e}")))?;
        return Ok(bytes);
    }
    std::fs::read(file).map_err(|e| Error::Usage(format!("read {file}: {e}")))
}

fn file_name(file: &str) -> String {
    Path::new(file)
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| *n != "-")
        .unwrap_or("unload.xml")
        .to_string()
}

/// A `200` proves nothing, but anything else is a refusal.
fn judge_upload(resp: &UiResponse) -> Result<()> {
    let transaction_id = resp.transaction_id.clone();
    match resp.status {
        200..=299 => Ok(()),
        300..=399 => {
            let to = resp.location.as_deref().unwrap_or("(no Location)");
            // `sysparm_referring_url` is not sent, so no success redirects.
            Err(Error::Auth {
                status: resp.status,
                message: format!(
                    "sys_upload.do refused this user (redirected to {to}); importing XML \
                     requires the admin role"
                ),
                transaction_id,
            })
        }
        401 | 403 => Err(Error::Auth {
            status: resp.status,
            message: format!(
                "HTTP {}: sys_upload.do refused this user; importing XML requires the admin role",
                resp.status
            ),
            transaction_id,
        }),
        status => Err(Error::Api {
            status,
            message: format!("sys_upload.do answered HTTP {status}"),
            detail: None,
            transaction_id,
            sn_error: None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_fields_reads_every_term_shape() {
        assert_eq!(
            query_fields(
                "active=true^short_descriptionLIKEa^^b^ORpriority=1^NQstateIN1,2\
                 ^caller_id.nameSTARTSWITHx^ORDERBYDESCnumber^ORDERBYsys_id"
            ),
            [
                "active",
                "short_description",
                "priority",
                "state",
                "caller_id.name",
                "number",
                "sys_id"
            ]
        );
    }

    #[test]
    fn query_fields_skips_what_it_cannot_check() {
        assert!(query_fields("123TEXTQUERY321=x").is_empty());
        assert_eq!(query_fields("active=true^EQ"), ["active"]);
        assert!(query_fields("variables.abc=1^ref_incident.severity=1").is_empty());
        assert!(query_fields("RLQUERYsys_user.x,>=1^active=true^ENDRLQUERY").is_empty());
    }

    #[test]
    fn file_name_falls_back_for_stdin() {
        assert_eq!(file_name("/tmp/inc.xml"), "inc.xml");
        assert_eq!(file_name("-"), "unload.xml");
    }

    #[test]
    fn only_a_2xx_upload_passes() {
        let resp = |status| UiResponse {
            status,
            location: None,
            transaction_id: None,
            body: String::new(),
        };
        assert!(judge_upload(&resp(200)).is_ok());
        assert!(matches!(judge_upload(&resp(302)), Err(Error::Auth { .. })));
        assert!(matches!(judge_upload(&resp(403)), Err(Error::Auth { .. })));
        assert!(matches!(judge_upload(&resp(500)), Err(Error::Api { .. })));
    }
}
