//! `sn get` — the read verb.
//!
//! Given a record reference (`sn get INC0010001`, `sn get incident:<sys_id>`)
//! it is the composite record read in `get_record.rs`. Given a noun it reads
//! one piece of caller or instance state: `sn get scope`, `sn get updateset`,
//! `sn get me`, … A noun can never be mistaken for a reference: a reference
//! holds a `:` or ends in a counter's digits, and no noun does — so clap's
//! subcommand match decides it, with no lookup.

mod common;
mod instance;
mod last;
mod logs;
mod me;
mod mine;
mod plugin;
mod property;
mod scope;
mod script;
mod xml;

use crate::cli::GlobalFlags;
use crate::cli::get_record::GetRecordArgs;
use crate::error::{Error, Result};
use clap::Subcommand;

pub use last::LastArgs;
pub use logs::{LogLevel, LogsArgs};
pub use mine::MineArgs;
pub use plugin::PluginArgs;
pub use property::PropertyArgs;
pub use scope::UpdatesetArgs;
pub use script::{ScriptArgs, ScriptType};
pub use xml::XmlArgs;

#[derive(clap::Args, Debug)]
#[command(
    args_conflicts_with_subcommands = true,
    subcommand_negates_reqs = true,
    subcommand_value_name = "NOUN",
    subcommand_help_heading = "Nouns"
)]
pub struct GetArgs {
    #[command(subcommand)]
    pub noun: Option<GetNoun>,
    #[command(flatten)]
    pub record: GetRecordArgs,
}

#[derive(Subcommand, Debug)]
pub enum GetNoun {
    /// The current application scope, with its version and vendor prefix.
    Scope,
    /// An update set — the current one by default — with its state, scope,
    /// change count and latest changes.
    Updateset(UpdatesetArgs),
    /// A system property's value, type and scope (`sn get property
    /// glide.ui.session_timeout`; a trailing `*` lists by prefix).
    Property(PropertyArgs),
    /// A script's source by name: script includes, business rules, client
    /// scripts, UI actions, UI scripts, scheduled jobs and fix scripts.
    Script(ScriptArgs),
    /// A record's unload XML — the UI's "Export → XML", importable on another
    /// instance. Written to stdout as XML, not JSON.
    Xml(XmlArgs),
    /// A column's choice list (`sn get choices incident.state`).
    Choices(ChoicesArgs),
    /// Whether plugins are installed and active, by plugin id.
    Plugin(PluginArgs),
    /// The calling user: their record, roles and groups.
    Me,
    /// The instance: release, build, patch and the node that answered.
    Instance,
    /// Open tasks assigned to you, plus unassigned ones in your groups.
    Mine(MineArgs),
    /// The task you most recently updated, read like `sn get <REF>`.
    Last(LastArgs),
    /// Recent system log entries, warnings and errors by default (`--since 15m`).
    Logs(LogsArgs),
}

/// The nouns, for the hint a near-miss token gets. Pinned to [`GetNoun`] by a
/// test so the two cannot drift.
const NOUNS: [&str; 12] = [
    "scope",
    "updateset",
    "property",
    "script",
    "xml",
    "choices",
    "plugin",
    "me",
    "instance",
    "mine",
    "last",
    "logs",
];

#[derive(clap::Args, Debug)]
pub struct ChoicesArgs {
    /// The column, as `table.field` (e.g. `incident.state`) or as two words
    /// (`incident state`).
    #[arg(value_name = "TABLE.FIELD")]
    pub column: String,
    /// The field, when the table is given on its own.
    #[arg(value_name = "FIELD")]
    pub field: Option<String>,
}

pub fn run(global: &GlobalFlags, args: GetArgs) -> Result<()> {
    let Some(noun) = args.noun else {
        return crate::cli::get_record::run(global, args.record);
    };
    match noun {
        GetNoun::Scope => scope::scope(global),
        GetNoun::Updateset(a) => scope::updateset(global, a),
        GetNoun::Property(a) => property::run(global, a),
        GetNoun::Script(a) => script::run(global, a),
        GetNoun::Xml(a) => xml::run(global, a),
        GetNoun::Choices(a) => choices(global, a),
        GetNoun::Plugin(a) => plugin::run(global, a),
        GetNoun::Me => me::run(global),
        GetNoun::Instance => instance::run(global),
        GetNoun::Mine(a) => mine::run(global, a),
        GetNoun::Last(a) => last::run(global, a),
        GetNoun::Logs(a) => logs::run(global, a),
    }
}

/// `choices` is `sn schema choices` under the read verb, with the column
/// spelled the way it is written everywhere else (`table.field`).
fn choices(global: &GlobalFlags, args: ChoicesArgs) -> Result<()> {
    let (table, field) = split_column(&args.column, args.field.as_deref())?;
    crate::cli::schema::choices(
        global,
        crate::cli::schema::SchemaChoicesArgs {
            table: table.to_string(),
            field: field.to_string(),
        },
    )
}

fn split_column<'a>(column: &'a str, field: Option<&'a str>) -> Result<(&'a str, &'a str)> {
    let (table, field) = match (column.split_once('.'), field) {
        (Some(_), Some(_)) => {
            return Err(Error::Usage(format!(
                "'{column}' already names a field; pass `table.field` or `table field`, not both"
            )));
        }
        (Some((t, f)), None) => (t, f),
        (None, Some(f)) => (column, f),
        (None, None) => {
            return Err(Error::Usage(format!(
                "'{column}' names no field; pass `table.field` (e.g. incident.state)"
            )));
        }
    };
    common::identifier(table, "table")?;
    common::identifier(field, "field")?;
    Ok((table, field))
}

/// A REF token that is not a reference may be a mistyped noun. A token with
/// no `:` and no digit cannot be a reference at all, so the error says which
/// nouns exist instead of only what a reference looks like.
pub(crate) fn noun_hint(token: &str, err: Error) -> Error {
    if token.contains(':') || token.bytes().any(|b| b.is_ascii_digit()) {
        return err;
    }
    match err {
        Error::Usage(msg) => Error::Usage(format!(
            "{msg}; nor is it a noun `sn get` knows ({})",
            NOUNS.join(", ")
        )),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[derive(clap::Parser)]
    struct Probe {
        #[command(subcommand)]
        noun: GetNoun,
    }

    #[test]
    fn noun_list_matches_the_enum() {
        let names: Vec<String> = Probe::command()
            .get_subcommands()
            .map(|c| c.get_name().to_string())
            .collect();
        assert_eq!(names, NOUNS);
    }

    #[test]
    fn column_spellings() {
        assert_eq!(
            split_column("incident.state", None).unwrap(),
            ("incident", "state")
        );
        assert_eq!(
            split_column("incident", Some("state")).unwrap(),
            ("incident", "state")
        );
        assert!(split_column("incident", None).is_err());
        assert!(split_column("incident.state", Some("x")).is_err());
        assert!(split_column("inc^x.state", None).is_err());
    }

    #[test]
    fn hint_only_for_tokens_that_cannot_be_references() {
        let e = || Error::Usage("orig".into());
        assert!(noun_hint("scop", e()).to_string().contains("updateset"));
        assert!(!noun_hint("INC001", e()).to_string().contains("updateset"));
        assert!(
            !noun_hint("incident:x", e())
                .to_string()
                .contains("updateset")
        );
    }
}
