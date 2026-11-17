pub mod aggregate;
pub mod apisearch;
pub mod app;
pub mod app_manager;
pub mod args;
pub mod atf;
pub mod attachment;
pub mod auth;
pub mod cache;
pub mod catalog;
pub mod change;
pub mod cicd;
pub mod cmdb;
pub mod codesearch;
pub mod completion;
pub mod context;
pub mod decision;
pub mod doctor;
pub mod flow;
pub mod get;
pub mod get_record;
pub mod gr;
pub mod graphql;
pub mod identify;
pub mod impersonate;
pub mod import;
pub mod init;
pub mod introspect;
pub mod journal;
pub(crate) mod kernel;
pub mod open_record;
pub mod ping;
pub mod playbook;
pub mod profile;
pub mod progress;
pub mod raw;
pub mod record_ref;
pub mod schema;
pub mod scores;
pub mod script;
pub mod table;
pub mod update_set;
pub mod variables;
pub mod watch;
pub mod xml;

pub use aggregate::AggregateArgs;
pub use apisearch::{
    ApiListArgs, ApiSearchArgs, ApiSpecArgs, ApisearchArgs, ApisearchSub, SpecFormat,
};
pub use app::{AppInstallArgs, AppPublishArgs, AppRollbackArgs, AppSub};
pub use app_manager::AppManagerSub;
pub use args::{BodyArgs, DisplayValueArg, DisplayValueOpt, Paging, SetLimit, WaitArgs};
pub use atf::{AtfResultsArgs, AtfRunArgs, AtfSub};
pub use attachment::{
    AttachmentDeleteArgs, AttachmentDownloadArgs, AttachmentGetArgs, AttachmentListArgs,
    AttachmentSub, AttachmentUploadArgs,
};
pub use cache::{CacheColumnsArgs, CacheSub, CacheTablesArgs};
pub use catalog::{
    CatalogCartEmptyArgs, CatalogCartItemArgs, CatalogCartUpdateArgs, CatalogCategoriesArgs,
    CatalogCategoryArgs, CatalogGetArgs, CatalogItemArgs, CatalogItemsArgs, CatalogListArgs,
    CatalogOrderArgs, CatalogSub,
};
pub use change::{
    ChangeApprovalsArgs, ChangeCiAddArgs, ChangeCiSub, ChangeConflictAddArgs, ChangeConflictSub,
    ChangeCreateArgs, ChangeDeleteArgs, ChangeGetArgs, ChangeListArgs, ChangeOptionalIdArg,
    ChangeRiskArgs, ChangeSub, ChangeSysIdArg, ChangeTaskCreateArgs, ChangeTaskDeleteArgs,
    ChangeTaskGetArgs, ChangeTaskListArgs, ChangeTaskSub, ChangeTaskUpdateArgs, ChangeType,
    ChangeUpdateArgs,
};
pub use cicd::CicdSub;
pub use cmdb::{
    CmdbCreateArgs, CmdbGetArgs, CmdbListArgs, CmdbMetaArgs, CmdbRelationAddArgs,
    CmdbRelationDeleteArgs, CmdbRelationSub, CmdbSub, CmdbUpdateArgs,
};
pub use codesearch::CodesearchArgs;
pub use completion::{CompletionArgs, Shell as CompletionShell};
pub use context::{ContextSub, ContextTargetArgs};
pub use decision::{DecisionListArgs, DecisionRunArgs, DecisionShowArgs, DecisionSub};
pub use doctor::DoctorArgs;
pub use flow::FlowSub;
pub use get::{GetArgs, GetNoun};
pub use get_record::GetRecordArgs;
pub use gr::GrArgs;
pub use graphql::GraphqlArgs;
pub use identify::{IdentifyArgs, IdentifyEnhancedArgs, IdentifySub};
pub use impersonate::ImpersonateArgs;
pub use import::{ImportBulkArgs, ImportCreateArgs, ImportGetArgs, ImportSub};
pub use init::InitArgs;
pub use journal::{JournalArgs, JournalSource};
pub use open_record::OpenArgs;
pub use playbook::{PlaybookLaunchArgs, PlaybookListArgs, PlaybookSub, PlaybookTriggerArgs};
pub use profile::{ProfileAddArgs, ProfileSub};
pub use progress::ProgressArgs;
pub use raw::RawArgs;
pub use schema::{SchemaChoicesArgs, SchemaColumnsArgs, SchemaSub, SchemaTablesArgs};
pub use scores::{ScoresFavoriteArgs, ScoresListArgs, ScoresSub, SortBy, SortDir};
pub use script::{ScriptRunArgs, ScriptSub};
pub use table::{
    TableCreateArgs, TableDeleteArgs, TableGetArgs, TableListArgs, TableSub, TableUpdateArgs,
};
pub use update_set::{
    UpdateSetBackOutArgs, UpdateSetCommitMultipleArgs, UpdateSetCreateArgs, UpdateSetIdArg,
    UpdateSetRetrieveArgs, UpdateSetSub,
};
pub use variables::{VariablesGetArgs, VariablesSetArgs, VariablesSub};
pub use watch::{Operation, WatchArgs, WatchLimits};
pub use xml::{XmlExportArgs, XmlImportArgs, XmlSub};

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};

/// Help heading for raw `sysparm_*` passthroughs that most callers never touch.
/// Keeps the default "Options" section down to the working set of each command.
pub(crate) const ADVANCED: &str = "Advanced options";

/// The command tree, with every usage line rewritten to put `[OPTIONS]` last.
///
/// clap renders `sn table list [OPTIONS] <TABLE>`, which reads as though flags
/// come first; nobody types it that way. `override_usage` is verbatim, so each
/// line is rebuilt from the command's own positionals and must carry the full
/// path. Parsing is untouched — flags still work in any position.
pub fn command() -> clap::Command {
    with_usage(Cli::command(), "sn")
}

/// Groups whose verb may be omitted: `sn table incident <sys_id>` is `get`,
/// `sn table incident` is `list`. Both mirror a REST path that is already
/// `{noun}/{id}` (`/api/now/table/{table}/{sys_id}`,
/// `/api/now/cmdb/instance/{class}/{sys_id}`), so the verb is implied by the
/// method for a read. Groups with non-CRUD subcommands (`change nextstates`,
/// `catalog checkout`) are deliberately excluded — there the first token is not
/// reliably a noun.
const IMPLIED_VERB_GROUPS: [&str; 2] = ["table", "cmdb"];

/// Parse argv through [`command`] so error messages carry the same usage lines
/// as `--help`, recovering an omitted read verb where that is unambiguous.
pub fn parse() -> Result<Cli, clap::Error> {
    parse_from(std::env::args_os().collect())
}

/// [`parse`] over an explicit argv (`argv[0]` is the binary name) — how
/// `sn impersonate` parses the command it wraps, with the same shorthand and
/// the same usage lines as a top-level invocation.
pub fn parse_from(argv: Vec<std::ffi::OsString>) -> Result<Cli, clap::Error> {
    let err = match command().try_get_matches_from(argv.clone()) {
        Ok(matches) => return Cli::from_arg_matches(&matches),
        Err(err) => err,
    };
    for candidate in implied_verb_argv(&argv, &err) {
        if let Ok(matches) = command().try_get_matches_from(candidate) {
            return Cli::from_arg_matches(&matches);
        }
    }
    Err(err)
}

/// Rewrites of argv with a read verb inserted, most specific first. Empty when
/// the shorthand does not apply.
///
/// Which verb wins is a decision, not a guess, carried by the token's shape:
/// a `:` marks a `table:id` record reference (a table name can never contain
/// one), so only `get` is offered. A plain token tries `list` first — `list`
/// rejects a second positional, so `sn table incident abc` falls through to
/// `get`, while `sn table incident` stops at `list` before `get` (whose
/// sys_id positional is optional for the reference form) could claim it.
fn implied_verb_argv(
    argv: &[std::ffi::OsString],
    err: &clap::Error,
) -> Vec<Vec<std::ffi::OsString>> {
    use clap::error::{ContextKind, ContextValue, ErrorKind};

    if err.kind() != ErrorKind::InvalidSubcommand {
        return Vec::new();
    }
    // clap matched the token against a real verb by edit distance, so this is a
    // misspelling (`sn table lst incident`), not a table name. Leave its "did
    // you mean" intact rather than firing a GET at a table called `lst`.
    if let Some(ContextValue::Strings(near)) = err.get(ContextKind::SuggestedSubcommand)
        && !near.is_empty()
    {
        return Vec::new();
    }
    let Some(ContextValue::String(token)) = err.get(ContextKind::InvalidSubcommand) else {
        return Vec::new();
    };
    let Some(group) = usage_group(err) else {
        return Vec::new();
    };
    if !IMPLIED_VERB_GROUPS.contains(&group.as_str()) {
        return Vec::new();
    }
    // The rejected token sits directly after its group in argv.
    let Some(at) = argv
        .windows(2)
        .position(|w| w[0] == *group.as_str() && w[1] == *token.as_str())
    else {
        return Vec::new();
    };
    let verbs: &[&str] = if token.contains(':') {
        &["get"]
    } else {
        &["list", "get"]
    };
    verbs
        .iter()
        .map(|verb| {
            let mut rewritten = argv.to_vec();
            rewritten.insert(at + 1, std::ffi::OsString::from(verb));
            rewritten
        })
        .collect()
}

/// The command path an error came from (`["sn", "cicd", "app"]`), read back
/// out of its usage line — which [`usage_line`] renders as `sn <path> <COMMAND>`.
/// `None` for anything but a group that rejected its subcommand.
fn usage_path(err: &clap::Error) -> Option<Vec<String>> {
    use clap::error::{ContextKind, ContextValue};

    let ContextValue::StyledStr(usage) = err.get(ContextKind::Usage)? else {
        return None;
    };
    let text = usage.to_string();
    let mut words: Vec<&str> = text.split_whitespace().collect();
    if words.pop()? != "<COMMAND>" {
        return None;
    }
    let start = words.iter().position(|w| *w == "sn")?;
    Some(words[start..].iter().map(|w| w.to_string()).collect())
}

/// The last word of [`usage_path`]: the group itself, `sn` for the root.
fn usage_group(err: &clap::Error) -> Option<String> {
    usage_path(err)?.pop()
}

/// Top-level commands that moved under `sn cicd` (0.15.0). Nothing else
/// answers to these names, so an `InvalidSubcommand` for one at the top level
/// gets pointed at its new home instead of clap's near-miss guess.
const MOVED_TO_CICD: &[&str] = &["app", "updateset", "atf", "progress"];

/// Top-level commands that were renamed outright, as `(old, new)`.
const RENAMED: &[(&str, &str)] = &[("api", "apisearch")];

/// Top-level names that no longer exist, and where each went: `sn app …` →
/// "moved to `sn cicd app`", `sn api …` → "is now `sn apisearch`", `sn user` →
/// `sn get me`, `sn context` → `sn get` / `sn set`. Only for the top level: a
/// group that rejects the token is answered by [`valid_subcommands`] instead.
pub fn moved_command_hint(err: &clap::Error) -> Option<String> {
    use clap::error::{ContextKind, ContextValue};
    // The root's usage line is `sn <COMMAND>`, so its group reads as `sn`.
    if usage_group(err).is_some_and(|g| g != "sn") {
        return None;
    }
    let ContextValue::String(token) = err.get(ContextKind::InvalidSubcommand)? else {
        return None;
    };
    if let Some((_, new)) = RENAMED.iter().find(|(old, _)| old == token) {
        return Some(format!(
            "  `sn {token}` is now `sn {new}` (`sn {new} <TERM>`, `sn {new} list`, `sn {new} spec`)"
        ));
    }
    match token.as_str() {
        "user" => Some("  `sn user me` was removed; use `sn get me`".into()),
        "context" => Some(
            "  `sn context` was removed; read with `sn get scope` / `sn get updateset`, \
             switch with `sn set scope` / `sn set updateset`"
                .into(),
        ),
        t if MOVED_TO_CICD.contains(&t) => Some(format!("  `sn {t}` moved to `sn cicd {t}`")),
        _ => None,
    }
}

/// The subcommands a group accepts, for an error message that would otherwise
/// name none of them, with the group's path below `sn` (`cicd app`). `None`
/// when the group can't be resolved.
pub fn valid_subcommands(err: &clap::Error) -> Option<(String, Vec<String>)> {
    let path = usage_path(err)?;
    let below: Vec<&str> = path.iter().skip(1).map(String::as_str).collect();
    let mut cmd = command();
    cmd.build();
    let node = find_command(&cmd, &below)?;
    let names: Vec<String> = node
        .get_subcommands()
        .filter(|s| s.get_name() != "help")
        .map(|s| s.get_name().to_string())
        .collect();
    (!names.is_empty() && !below.is_empty()).then(|| (below.join(" "), names))
}

/// Walk the tree by full path. A bare name is not enough: `sn script` and
/// `sn get script` are different commands.
fn find_command<'a>(cmd: &'a clap::Command, path: &[&str]) -> Option<&'a clap::Command> {
    path.iter()
        .try_fold(cmd, |node, name| node.find_subcommand(name))
}

/// What `--data` and `--field` accept, shown under the help of every command
/// that takes them.
const BODY_INPUT_HELP: &str = "BODY INPUT:
  --data '<inline JSON>'             inline JSON object
  --data @body.json                  read body from a file (multi-line safe)
  --data @-                          read body from stdin (e.g. piped from jq)
  --field name=value                 set one field
  --field description=@notes.md      read one field's value from a file";

fn with_usage(cmd: clap::Command, path: &str) -> clap::Command {
    let path = path.to_string();
    let usage = usage_line(&path, &cmd);
    // The `--data`/`--field` reference goes on the commands that take the
    // pair, found by arg id so a new `BodyArgs` site picks it up unasked.
    let takes_body = ["data", "field"]
        .iter()
        .all(|id| cmd.get_arguments().any(|a| a.get_id() == id));
    let cmd = if takes_body {
        cmd.after_help(BODY_INPUT_HELP)
    } else {
        cmd
    };
    cmd.override_usage(usage).mut_subcommands(move |sc| {
        let child = format!("{} {}", path, sc.get_name());
        with_usage(sc, &child)
    })
}

/// `sn table get <TABLE> <SYS_ID> [OPTIONS]` for a leaf, `sn table <COMMAND>`
/// for a group. Value names fall back to the uppercased arg id, which is what
/// clap would derive anyway — the tree is walked before `build()` fills them in.
fn usage_line(path: &str, cmd: &clap::Command) -> String {
    let mut usage = String::from(path);
    // A command that takes either a subcommand or its own positionals
    // (`sn get <REF>` / `sn get <NOUN>`) gets one line per form.
    if cmd.is_args_conflicts_with_subcommands_set() && cmd.get_positionals().next().is_some() {
        let noun = cmd.get_subcommand_value_name().unwrap_or("COMMAND");
        let leaf = leaf_usage(path, cmd);
        return format!("{leaf}\n       {path} <{noun}> [OPTIONS]");
    }
    if cmd.get_subcommands().next().is_some() {
        // A group whose bare invocation is itself a command has an optional
        // subcommand; promise `[COMMAND]`, not `<COMMAND>`.
        usage.push_str(if cmd.is_subcommand_required_set() {
            " <COMMAND>"
        } else {
            " [COMMAND]"
        });
        return usage;
    }
    leaf_usage(path, cmd)
}

/// `path <POSITIONALS> [OPTIONS]`.
fn leaf_usage(path: &str, cmd: &clap::Command) -> String {
    let mut usage = String::from(path);
    for arg in cmd.get_positionals() {
        let name = arg
            .get_value_names()
            .and_then(|names| names.first())
            .map(|n| n.to_string())
            .unwrap_or_else(|| arg.get_id().as_str().to_uppercase());
        usage.push(' ');
        // A trailing `last` positional is only reachable after `--`.
        if arg.is_last_set() {
            usage.push_str("-- ");
        }
        if arg.is_required_set() {
            usage.push_str(&format!("<{name}>"));
        } else {
            usage.push_str(&format!("[{name}]"));
        }
        if matches!(arg.get_action(), clap::ArgAction::Append) {
            usage.push_str("...");
        }
    }
    usage.push_str(" [OPTIONS]");
    usage
}

#[derive(Parser, Debug)]
#[command(
    name = "sn",
    version,
    about = "ServiceNow CLI for scripts and coding agents: JSON on stdout, JSON errors on stderr",
    disable_version_flag = true
)]
#[allow(clippy::manual_non_exhaustive)] // `version` field exists only to wire the -v flag.
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalFlags,

    #[command(subcommand)]
    pub command: Command,

    /// Print version and exit. `-V` is accepted as an alias.
    #[arg(short = 'v', short_alias = 'V', long = "version", action = clap::ArgAction::Version)]
    version: (),
}

/// Flags accepted by every command. They carry their own help heading so they
/// sink to the bottom of `--help` instead of interleaving with each command's
/// own flags — clap gives every `Args` container its own display-order counter
/// and merges them, which otherwise zippers 11 globals through the list.
#[derive(clap::Args, Debug, Clone, Default)]
#[command(next_help_heading = "Global options")]
pub struct GlobalFlags {
    /// Profile name (overrides default_profile).
    #[arg(long, short = 'p', global = true)]
    pub profile: Option<String>,

    /// Output mode: `raw` keeps the full SN envelope, `table` renders columns.
    #[arg(long, global = true, value_enum, default_value_t = OutputMode::Default)]
    pub output: OutputMode,

    /// Force pretty-printed JSON regardless of TTY detection.
    #[arg(long, global = true, conflicts_with = "compact")]
    pub pretty: bool,

    /// Force compact JSON regardless of TTY detection.
    #[arg(long, global = true, conflicts_with = "pretty")]
    pub compact: bool,

    /// Request timeout in seconds. Defaults to 30s.
    #[arg(long, global = true, value_name = "SECS")]
    pub timeout: Option<u64>,

    /// Increase verbosity (-d requests, -dd headers, -ddd bodies).
    #[arg(short = 'd', long = "verbose", global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Proxy URL (http://, https://, socks5://). Overrides SN_PROXY and profile config.
    #[arg(long, global = true, value_name = "URL")]
    pub proxy: Option<String>,

    /// Bypass any configured proxy for this invocation.
    #[arg(long, global = true)]
    pub no_proxy: bool,

    /// Custom CA certificate for the proxy connection.
    #[arg(long, global = true, value_name = "PATH")]
    pub proxy_ca_cert: Option<String>,

    /// Disable TLS certificate verification (DANGEROUS).
    #[arg(long, global = true)]
    pub insecure: bool,

    /// Custom CA certificate bundle for the ServiceNow endpoint.
    #[arg(long, global = true, value_name = "PATH")]
    pub ca_cert: Option<String>,
}

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq, Default)]
#[value(rename_all = "lowercase")]
pub enum OutputMode {
    #[default]
    Default,
    Raw,
    /// Render JSON results as a human-readable columnar table (suitable for interactive use, not piping).
    Table,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Read one thing: a record by reference, or a noun (scope, updateset, me, logs, …).
    ///
    /// A record comes with its variables and journal entries (`sn get INC0010001`,
    /// `sn get incident:<sys_id>`). A noun reads something else (`sn get scope`,
    /// `sn get updateset`, `sn get me`, `sn get logs`, …).
    Get(GetArgs),
    /// Switch the application scope or update set. Verified by re-read.
    ///
    /// Changes the session context: the application scope or update set the
    /// caller's tracked writes are captured under (`sn set scope x_app`,
    /// `sn set updateset "My fixes"`).
    Set {
        #[command(subcommand)]
        sub: ContextSub,
    },
    /// Read and write records in any table (Table API).
    Table {
        #[command(subcommand)]
        sub: TableSub,
    },
    /// Read records via GraphQL with dot-walked reference fields.
    Gr(GrArgs),
    /// Aggregate statistics for a table (GET /api/now/stats/{table}).
    Aggregate(AggregateArgs),
    /// Comments and work notes for one record, parsed into entries.
    Journal(JournalArgs),
    /// Catalog variables on a record: read, and write with verification.
    Variables {
        #[command(subcommand)]
        sub: VariablesSub,
    },
    /// Attachment operations (upload, download, list, delete).
    Attachment {
        #[command(subcommand)]
        sub: AttachmentSub,
    },
    /// Watch records in a table change in real time (AMB websocket). Streams JSONL.
    Watch(WatchArgs),
    /// Open a record form or a list view in the web UI.
    Open(OpenArgs),
    /// Change Management operations (normal, emergency, standard).
    Change {
        #[command(subcommand)]
        sub: ChangeSub,
    },
    /// Service Catalog operations (catalogs, items, cart, ordering).
    Catalog {
        #[command(subcommand)]
        sub: CatalogSub,
    },
    /// CMDB CIs by class: list, get, create, update, relations and class metadata.
    Cmdb {
        #[command(subcommand)]
        sub: CmdbSub,
    },
    /// Identification and Reconciliation (CI create/update/query).
    Identify {
        #[command(subcommand)]
        sub: IdentifySub,
    },
    /// Import Set operations (staging table imports).
    Import {
        #[command(subcommand)]
        sub: ImportSub,
    },
    /// Move records between instances as unload XML: export a query, import
    /// the file.
    Xml {
        #[command(subcommand)]
        sub: XmlSub,
    },
    /// Flow Designer flows and subflows: find, inspect, and debug them.
    Flow {
        #[command(subcommand)]
        sub: FlowSub,
    },
    /// Decision tables: list them, show one's inputs and decisions, run one against inputs.
    Decision {
        #[command(subcommand)]
        sub: DecisionSub,
    },
    /// Playbook executions on a record: list, trigger by scoped name, launch by definition.
    Playbook {
        #[command(subcommand)]
        sub: PlaybookSub,
    },
    /// Performance Analytics scorecard operations.
    Scores {
        #[command(subcommand)]
        sub: ScoresSub,
    },
    /// CI/CD API: app repository, update sets, ATF runs and async job progress.
    Cicd {
        #[command(subcommand)]
        sub: CicdSub,
    },
    /// Application Manager: find and show store apps and plugins, install a plugin.
    ///
    /// Covers installed, updatable and available ones (`sn appmanager list`,
    /// `sn appmanager search <TERM>`, `sn appmanager show <ID>`, `sn appmanager
    /// install <PLUGIN_ID>`). The app repository is `sn cicd app`.
    Appmanager {
        #[command(subcommand)]
        sub: AppManagerSub,
    },
    /// List tables, columns and choice values from the instance.
    Schema {
        #[command(subcommand)]
        sub: SchemaSub,
    },
    /// Offline schema index: list tables and columns with no network.
    ///
    /// Build it once, then list tables and columns (and complete them in the
    /// shell) offline.
    Cache {
        #[command(subcommand)]
        sub: CacheSub,
    },
    /// Find REST endpoints on the instance by substring; `list` its APIs, print a `spec`.
    ///
    /// `sn apisearch <TERM>` matches API names, routes and descriptions and prints one row
    /// per endpoint, carrying namespace, API name, version, method and route — enough to
    /// call it with `sn raw`. `--output raw` prints the unfiltered catalogue instead, as
    /// `sn apisearch list` does. `sn apisearch spec` prints one API's OpenAPI spec.
    Apisearch(ApisearchArgs),
    /// Find a string in script and code fields across the instance.
    ///
    /// Uses the Code Search API (`sn codesearch <TERM>`). One row per matching field.
    ///
    /// Searches every table the instance's Code Search is configured for (business rules,
    /// script includes, UI actions, client scripts, ACLs, …) in every application scope, in one
    /// call. Each row is `{table, sys_id, name, field, count, lines}`; `lines` holds each matching
    /// line with its neighbours, `match` telling them apart. Feed `table` and `sys_id` to
    /// `sn table get` or `sn open`.
    ///
    /// Results are limited to records the caller can read: a caller without admin (or a
    /// delegated-developer role) typically sees few or none, under a successful exit. A search
    /// over every table can take a while, so `--timeout` defaults to 120s here.
    Codesearch(CodesearchArgs),
    /// Generic REST passthrough for unmodeled endpoints (`sn raw <METHOD> <PATH>`).
    Raw(RawArgs),
    /// Run a GraphQL query against POST /api/now/graphql (`sn graphql <QUERY>`).
    Graphql(GraphqlArgs),
    /// Run server-side JavaScript on the instance (Scripts - Background).
    Script {
        #[command(subcommand)]
        sub: ScriptSub,
    },
    /// Create or update a profile interactively.
    Init(InitArgs),
    /// Manage profiles and their auth sessions.
    Profile {
        #[command(subcommand)]
        sub: ProfileSub,
    },
    /// Health check the configured instance (auth + latency + build version).
    Ping,
    /// Preflight the instance: who you are, plus role, plugin and property checks.
    ///
    /// One round trip: who you are, whether you are admin, and pass/fail checks
    /// for required roles, plugins and properties.
    Doctor(DoctorArgs),
    /// Run one sn command as another user, then end the impersonation.
    Impersonate(ImpersonateArgs),
    /// Generate a shell completion script.
    ///
    /// `--dynamic` adds table and column names from `sn cache`.
    Completion(CompletionArgs),
    /// Dump the full command tree as JSON for agent/MCP generation.
    Introspect,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_compiles_and_debugs() {
        Cli::command().debug_assert();
    }

    #[test]
    fn pretty_and_compact_conflict() {
        let err = Cli::try_parse_from(["sn", "--pretty", "--compact", "introspect"]).unwrap_err();
        // clap emits conflict error; kind may differ by version, just assert it's an error
        let _ = err.kind();
    }
}
