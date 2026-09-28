//! `sn completion <shell>` — static scripts, or (`--dynamic`) a script that
//! calls back into `sn` on every TAB so table and column names can be
//! completed from the offline schema cache (`sn cache refresh`).
//!
//! The dynamic path is clap_complete's `CompleteEnv` engine: the registration
//! script re-invokes the binary with [`COMPLETE_VAR`] set, and
//! [`complete_if_requested`] — the first thing `main` does — answers with
//! candidates and exits. Candidates for table and column arguments come from
//! completers attached in [`completion_command`]; they read only the cache
//! file, never the network, and yield nothing rather than an error when there
//! is no cache, so a TAB can never hang on an instance or print a stack of JSON
//! into the prompt.

use crate::schema_cache::{self, SchemaIndex};
use clap_complete::engine::{ArgValueCompleter, CompletionCandidate};
use clap_complete::generate;
use std::ffi::{OsStr, OsString};
use std::io::{self, Write};

/// The environment variable that switches `sn` into completion mode. clap's
/// default is the bare `COMPLETE`, which is generic enough to be set in some
/// environment for another reason — and a stray value would turn *every* `sn`
/// invocation into a completion request (or an "unknown shell" exit), which for
/// an agent-driven binary is a silent, total outage. The registration script
/// carries this name itself, so namespacing it costs nothing.
pub const COMPLETE_VAR: &str = "SN_COMPLETE";

/// Shells supported by `sn completion`.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    Powershell,
    Elvish,
}

impl From<Shell> for clap_complete::Shell {
    fn from(s: Shell) -> Self {
        match s {
            Shell::Bash => clap_complete::Shell::Bash,
            Shell::Zsh => clap_complete::Shell::Zsh,
            Shell::Fish => clap_complete::Shell::Fish,
            Shell::Powershell => clap_complete::Shell::PowerShell,
            Shell::Elvish => clap_complete::Shell::Elvish,
        }
    }
}

impl Shell {
    /// The name clap_complete's runtime engine knows this shell by.
    fn env_name(self) -> &'static str {
        match self {
            Shell::Bash => "bash",
            Shell::Zsh => "zsh",
            Shell::Fish => "fish",
            Shell::Powershell => "powershell",
            Shell::Elvish => "elvish",
        }
    }
}

#[derive(clap::Args, Debug)]
pub struct CompletionArgs {
    /// Shell to generate completions for.
    pub shell: Shell,
    /// Emit a script that asks `sn` for candidates on every TAB, completing
    /// table and column names from the schema cache (`sn cache refresh`) as
    /// well as commands and flags. Load it at shell startup rather than saving
    /// it, e.g. `eval "$(sn completion zsh --dynamic)"` in ~/.zshrc, so it
    /// tracks upgrades.
    #[arg(long)]
    pub dynamic: bool,
}

/// Emit a shell completion script for the chosen shell to stdout.
pub fn run(args: CompletionArgs) -> crate::error::Result<()> {
    if args.dynamic {
        let script = registration(args.shell)?;
        let mut out = io::stdout().lock();
        out.write_all(&script)
            .and_then(|()| out.flush())
            .map_err(crate::output::map_stdout_err)?;
        return Ok(());
    }
    let mut cmd = crate::cli::command();
    let shell: clap_complete::Shell = args.shell.into();
    let mut out = io::stdout().lock();
    generate(shell, &mut cmd, "sn", &mut out);
    Ok(())
}

/// The dynamic registration script — byte-for-byte what `SN_COMPLETE=<shell>
/// sn` prints, since both name the same completer the same way.
pub fn registration(shell: Shell) -> crate::error::Result<Vec<u8>> {
    let shells = clap_complete::env::Shells::builtins();
    let completer = shells.completer(shell.env_name()).ok_or_else(|| {
        crate::error::Error::Usage(format!("no dynamic completion for {}", shell.env_name()))
    })?;
    let mut buf = Vec::new();
    completer
        .write_registration(COMPLETE_VAR, "sn", "sn", &completer_path(), &mut buf)
        .map_err(crate::output::map_stdout_err)?;
    Ok(buf)
}

/// How the registration script re-invokes this binary: `argv[0]` as typed —
/// a bare `sn` stays a `$PATH` lookup — made absolute when it is a relative
/// path, which is exactly what `CompleteEnv` does for its own registration.
fn completer_path() -> String {
    let argv0 = std::env::args_os()
        .next()
        .unwrap_or_else(|| OsString::from("sn"));
    let mut path = std::path::PathBuf::from(argv0);
    if path.components().count() > 1
        && let Ok(cwd) = std::env::current_dir()
    {
        path = cwd.join(path);
    }
    path.to_string_lossy().into_owned()
}

/// Answer a completion request and exit, if [`COMPLETE_VAR`] says this is one.
/// Must run before anything is written to stdout.
pub fn complete_if_requested() {
    clap_complete::CompleteEnv::with_factory(completion_command)
        .var(COMPLETE_VAR)
        .complete();
}

/// [`crate::cli::command`] with the cache-backed completers attached. Built
/// only for completion requests; the parser and `sn introspect` never see the
/// extensions.
pub fn completion_command() -> clap::Command {
    crate::cli::command().mut_subcommands(|group| {
        let name = group.get_name().to_string();
        attach(group, &name)
    })
}

/// What a table-valued positional may be.
#[derive(Clone, Copy)]
enum Tables {
    Any,
    /// The root and everything extending it (`cmdb_ci` for a CMDB class).
    Extending(&'static str),
}

/// How a column-valued argument is spelled.
#[derive(Clone, Copy)]
enum Columns {
    /// One column name (`sn schema choices <TABLE> <FIELD>`).
    One,
    /// A comma-separated list (`-f number,short_description`).
    List,
    /// `name=value` (`--field short_description=…`): the name part only.
    Assign,
    /// An encoded query: the field of its last `^`-separated term.
    Query,
}

fn attach(cmd: clap::Command, group: &str) -> clap::Command {
    let group_owned = group.to_string();
    let cmd = cmd.mut_args(|arg| {
        let id = arg.get_id().as_str();
        let positional = arg.is_positional();
        let tables = match (id, positional) {
            ("table", true) => Some(Tables::Any),
            ("class", true) => Some(Tables::Extending("cmdb_ci")),
            ("staging_table", true) => Some(Tables::Extending("sys_import_set_row")),
            _ => None,
        };
        if let Some(kind) = tables {
            return arg.add(ArgValueCompleter::new(move |current: &OsStr| {
                complete_tables(kind, current)
            }));
        }
        let columns = match (id, positional) {
            ("field", true) => Some(Columns::One),
            // `sn variables set --field` names catalog variables, not columns.
            ("field", false) if group_owned != "variables" => Some(Columns::Assign),
            (
                "fields" | "group_by" | "order_by" | "on_change" | "avg_fields" | "min_fields"
                | "max_fields" | "sum_fields",
                false,
            ) => Some(Columns::List),
            ("query", false) => Some(Columns::Query),
            _ => None,
        };
        match columns {
            Some(kind) => arg.add(ArgValueCompleter::new(move |current: &OsStr| {
                complete_columns(kind, current)
            })),
            None => arg,
        }
    });
    let group = group.to_string();
    cmd.mut_subcommands(move |sub| attach(sub, &group))
}

/// The words being completed: what the registration script passes after `--`
/// (`sn -- sn table list incident -f sh`).
fn request_words() -> Vec<OsString> {
    let mut args = std::env::args_os().skip_while(|a| a != "--");
    args.next();
    args.collect()
}

/// The profile and table named on the line being completed, by a lenient
/// parse of the whole line through the real command tree — so it knows every
/// flag's arity and where the positionals are, rather than guessing from word
/// shapes.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct LineContext {
    pub profile: Option<String>,
    pub table: Option<String>,
}

pub fn line_context(words: &[OsString]) -> LineContext {
    let mut ctx = LineContext::default();
    let Ok(matches) = crate::cli::command()
        .ignore_errors(true)
        .try_get_matches_from(words)
    else {
        return ctx;
    };
    let mut level = Some(&matches);
    while let Some(m) = level {
        if let Some(p) = string_arg(m, "profile") {
            ctx.profile = Some(p);
        }
        for id in ["table", "class", "staging_table"] {
            if let Some(t) = string_arg(m, id) {
                // `incident:INC0010001` is a record reference; the table is
                // everything before the colon.
                let t = t.split(':').next().unwrap_or_default().to_string();
                if !t.is_empty() {
                    ctx.table = Some(t);
                }
            }
        }
        level = m.subcommand().map(|(_, sub)| sub);
    }
    ctx
}

fn string_arg(m: &clap::ArgMatches, id: &str) -> Option<String> {
    m.try_get_one::<String>(id).ok().flatten().cloned()
}

/// The cache for the profile on the line (or the default profile). `None` on
/// any failure: completion reports nothing rather than an error.
fn load_index(profile: Option<&str>) -> Option<SchemaIndex> {
    let instance = crate::cli::cache::profile_instance(profile).ok()?;
    let path = schema_cache::cache_path(&instance).ok()?;
    schema_cache::load(&path).ok().flatten()
}

fn complete_tables(kind: Tables, current: &OsStr) -> Vec<CompletionCandidate> {
    let ctx = line_context(&request_words());
    match load_index(ctx.profile.as_deref()) {
        Some(index) => table_candidates(&index, kind, &current.to_string_lossy()),
        None => Vec::new(),
    }
}

fn complete_columns(kind: Columns, current: &OsStr) -> Vec<CompletionCandidate> {
    let ctx = line_context(&request_words());
    let Some(table) = ctx.table else {
        return Vec::new();
    };
    match load_index(ctx.profile.as_deref()) {
        Some(index) => column_candidates(&index, &table, kind, &current.to_string_lossy()),
        None => Vec::new(),
    }
}

fn table_candidates(index: &SchemaIndex, kind: Tables, current: &str) -> Vec<CompletionCandidate> {
    // Past a `:` the word is a record reference's identifier, not a table.
    if current.contains(':') {
        return Vec::new();
    }
    index
        .tables_with_prefix(current)
        .filter(|t| match kind {
            Tables::Any => true,
            Tables::Extending(root) => !index.tables.contains_key(root) || index.extends(t, root),
        })
        .map(CompletionCandidate::new)
        .collect()
}

/// Encoded-query keywords that can open a term before its field name, longest
/// first so `ORDERBYDESC` is not read as `OR` + `DERBYDESC`.
const TERM_KEYWORDS: [&str; 5] = ["ORDERBYDESC", "ORDERBY", "GROUPBY", "OR", "NQ"];

fn column_candidates(
    index: &SchemaIndex,
    table: &str,
    kind: Columns,
    current: &str,
) -> Vec<CompletionCandidate> {
    let Some(columns) = index.columns(table) else {
        return Vec::new();
    };
    // Split the word into what stays as typed (`head`) and the partial column
    // name being completed (`partial`).
    let (head, partial, suffix) = match kind {
        Columns::One => ("", current, ""),
        Columns::List => match current.rfind(',') {
            Some(i) => (&current[..=i], &current[i + 1..], ""),
            None => ("", current, ""),
        },
        Columns::Assign => {
            if current.contains('=') {
                return Vec::new();
            }
            ("", current, "=")
        }
        Columns::Query => {
            let (before, term) = match current.rfind('^') {
                Some(i) => current.split_at(i + 1),
                None => ("", current),
            };
            let keyword = TERM_KEYWORDS
                .iter()
                .find(|k| term.starts_with(**k))
                .map_or(0, |k| k.len());
            (&current[..before.len() + keyword], &term[keyword..], "")
        }
    };
    // Past the column name (an operator, a dot-walk, a value) there is nothing
    // left to complete from a list of names.
    if !partial
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Vec::new();
    }
    columns
        .iter()
        .filter(|c| c.starts_with(partial))
        .map(|c| CompletionCandidate::new(format!("{head}{c}{suffix}")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema_cache::{FORMAT, TableEntry};
    use std::collections::BTreeMap;

    fn index() -> SchemaIndex {
        let mut tables = BTreeMap::new();
        let mut t = |name: &str, parent: Option<&str>, cols: &[&str]| {
            tables.insert(
                name.to_string(),
                TableEntry {
                    parent: parent.map(str::to_string),
                    columns: cols.iter().map(|c| c.to_string()).collect(),
                },
            );
        };
        t("task", None, &["number", "short_description", "state"]);
        t("incident", Some("task"), &["caller_id", "severity"]);
        t("incident_task", Some("task"), &[]);
        t("cmdb_ci", None, &["name"]);
        t("cmdb_ci_server", Some("cmdb_ci"), &["os"]);
        t("cmdb_model", None, &[]);
        SchemaIndex {
            format: FORMAT,
            instance: "x".into(),
            built_at: 0,
            columns_indexed: true,
            tables,
        }
    }

    fn values(c: Vec<CompletionCandidate>) -> Vec<String> {
        c.iter()
            .map(|c| c.get_value().to_string_lossy().into_owned())
            .collect()
    }

    fn words(line: &str) -> Vec<OsString> {
        line.split(' ').map(OsString::from).collect()
    }

    #[test]
    fn tables_complete_by_prefix_and_cmdb_classes_by_lineage() {
        let idx = index();
        assert_eq!(
            values(table_candidates(&idx, Tables::Any, "inc")),
            ["incident", "incident_task"]
        );
        assert_eq!(
            values(table_candidates(&idx, Tables::Extending("cmdb_ci"), "cmdb")),
            ["cmdb_ci", "cmdb_ci_server"]
        );
        assert!(table_candidates(&idx, Tables::Any, "incident:INC").is_empty());
    }

    #[test]
    fn a_missing_root_does_not_hide_every_table() {
        let idx = index();
        assert_eq!(
            values(table_candidates(
                &idx,
                Tables::Extending("sys_import_set_row"),
                "cmdb_m"
            )),
            ["cmdb_model"]
        );
    }

    #[test]
    fn column_lists_complete_the_last_segment_with_inherited_columns() {
        let idx = index();
        assert_eq!(
            values(column_candidates(
                &idx,
                "incident",
                Columns::List,
                "number,s"
            )),
            [
                "number,severity",
                "number,short_description",
                "number,state"
            ]
        );
        assert!(column_candidates(&idx, "incident", Columns::List, "caller_id.na").is_empty());
        assert!(column_candidates(&idx, "nope", Columns::List, "").is_empty());
    }

    #[test]
    fn field_assignments_complete_the_name_and_stop_at_the_value() {
        let idx = index();
        assert_eq!(
            values(column_candidates(&idx, "incident", Columns::Assign, "sh")),
            ["short_description="]
        );
        assert!(column_candidates(&idx, "incident", Columns::Assign, "state=1").is_empty());
    }

    #[test]
    fn queries_complete_the_field_of_the_last_term() {
        let idx = index();
        assert_eq!(
            values(column_candidates(
                &idx,
                "incident",
                Columns::Query,
                "active=true^ORsev"
            )),
            ["active=true^ORseverity"]
        );
        assert_eq!(
            values(column_candidates(
                &idx,
                "incident",
                Columns::Query,
                "ORDERBYDESCnu"
            )),
            ["ORDERBYDESCnumber"]
        );
        assert_eq!(
            values(column_candidates(&idx, "incident", Columns::Query, "st")),
            ["state"]
        );
        assert!(column_candidates(&idx, "incident", Columns::Query, "state=").is_empty());
    }

    #[test]
    fn line_context_finds_the_table_and_profile_through_a_partial_line() {
        assert_eq!(
            line_context(&words("sn table list incident -p dev -f sh")),
            LineContext {
                profile: Some("dev".into()),
                table: Some("incident".into())
            }
        );
        assert_eq!(
            line_context(&words(
                "sn --profile p2 table update incident:INC0010001 --field sh"
            )),
            LineContext {
                profile: Some("p2".into()),
                table: Some("incident".into())
            }
        );
        assert_eq!(
            line_context(&words("sn cmdb get cmdb_ci_server"))
                .table
                .as_deref(),
            Some("cmdb_ci_server")
        );
        assert_eq!(
            line_context(&words("sn change create --field sh")).table,
            None
        );
    }

    #[test]
    fn completers_attach_to_the_intended_arguments_only() {
        let mut cmd = completion_command();
        cmd.build();
        let arg = |path: &[&str], id: &str| -> bool {
            let mut c = &cmd;
            for p in path {
                c = c.find_subcommand(p).unwrap();
            }
            c.get_arguments()
                .find(|a| a.get_id() == id)
                .unwrap_or_else(|| panic!("{path:?} has no {id}"))
                .get::<ArgValueCompleter>()
                .is_some()
        };
        assert!(arg(&["table", "list"], "table"));
        assert!(arg(&["table", "list"], "fields"));
        assert!(arg(&["table", "list"], "query"));
        assert!(arg(&["table", "update"], "field"));
        assert!(arg(&["cmdb", "get"], "class"));
        assert!(arg(&["schema", "choices"], "field"));
        assert!(arg(&["gr"], "table"));
        assert!(!arg(&["variables", "set"], "field"));
    }

    #[test]
    fn registration_names_the_namespaced_variable() {
        for shell in [
            Shell::Bash,
            Shell::Zsh,
            Shell::Fish,
            Shell::Powershell,
            Shell::Elvish,
        ] {
            let script = String::from_utf8(registration(shell).unwrap()).unwrap();
            assert!(script.contains(COMPLETE_VAR), "{shell:?}: {script}");
        }
    }
}
