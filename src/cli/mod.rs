pub mod aggregate;
pub mod app;
pub mod atf;
pub mod attachment;
pub mod auth;
pub mod catalog;
pub mod change;
pub mod cmdb;
pub mod identify;
pub mod import;
pub mod init;
pub mod introspect;
pub mod profile;
pub mod progress;
pub mod schema;
pub mod scores;
pub mod table;
pub mod update_set;

pub use aggregate::AggregateArgs;
pub use app::{AppInstallArgs, AppPublishArgs, AppRollbackArgs, AppSub};
pub use atf::{AtfResultsArgs, AtfRunArgs, AtfSub};
pub use attachment::{
    AttachmentDeleteArgs, AttachmentDownloadArgs, AttachmentGetArgs, AttachmentListArgs,
    AttachmentSub, AttachmentUploadArgs,
};
pub use auth::AuthSub;
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
pub use cmdb::{
    CmdbCreateArgs, CmdbGetArgs, CmdbListArgs, CmdbMetaArgs, CmdbRelationAddArgs,
    CmdbRelationDeleteArgs, CmdbRelationSub, CmdbReplaceArgs, CmdbSub, CmdbUpdateArgs,
};
pub use identify::{IdentifyArgs, IdentifyEnhancedArgs, IdentifySub};
pub use import::{ImportBulkArgs, ImportCreateArgs, ImportGetArgs, ImportSub};
pub use init::InitArgs;
pub use profile::ProfileSub;
pub use progress::ProgressArgs;
pub use schema::{SchemaChoicesArgs, SchemaColumnsArgs, SchemaSub, SchemaTablesArgs};
pub use scores::{ScoresFavoriteArgs, ScoresListArgs, ScoresSub, SortBy, SortDir};
pub use table::{
    DisplayValueArg, TableCreateArgs, TableDeleteArgs, TableGetArgs, TableListArgs,
    TableReplaceArgs, TableSub, TableUpdateArgs,
};
pub use update_set::{
    UpdateSetBackOutArgs, UpdateSetCommitMultipleArgs, UpdateSetCreateArgs, UpdateSetIdArg,
    UpdateSetRetrieveArgs, UpdateSetSub,
};

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug)]
#[command(
    name = "sn",
    version,
    about = "ServiceNow CLI (Table API + schema + CICD)",
    disable_version_flag = true,
    after_help = "BODY INPUT (write commands accept any of these):\n  --data '<inline JSON>'             inline JSON object\n  --data @body.json                  read body from a file (multi-line safe)\n  --data @-                          read body from stdin (e.g. piped from jq)\n  --field name=value                 set one field\n  --field description=@notes.md      read one field's value from a file\n\nEXAMPLES:\n  sn table update incident <sys_id> --data @body.json\n  sn table update incident <sys_id> --field description=@notes.md\n  jq '.fields' patch.json | sn table update incident <sys_id> --data @-"
)]
#[allow(clippy::manual_non_exhaustive)] // `version` field exists only to wire the -v flag.
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalFlags,

    #[command(subcommand)]
    pub command: Command,

    /// Print version and exit.
    #[arg(short = 'v', long = "version", action = clap::ArgAction::Version)]
    version: (),
}

#[derive(clap::Args, Debug, Clone, Default)]
pub struct GlobalFlags {
    /// Profile name (overrides SN_PROFILE and default_profile).
    #[arg(long, global = true)]
    pub profile: Option<String>,

    /// Override the profile's instance URL for this invocation.
    #[arg(long, global = true, value_name = "URL")]
    pub instance_override: Option<String>,

    /// Output mode. `default` (unwrapped result) or `raw` (full SN envelope).
    #[arg(long, global = true, value_enum, default_value_t = OutputMode::Default)]
    pub output: OutputMode,

    /// Force pretty-printed JSON regardless of TTY detection.
    #[arg(long, global = true, conflicts_with = "compact")]
    pub pretty: bool,

    /// Force compact JSON regardless of TTY detection.
    #[arg(long, global = true, conflicts_with = "pretty")]
    pub compact: bool,

    /// Request timeout in seconds (overrides SN_TIMEOUT).
    #[arg(long, global = true, value_name = "SECS")]
    pub timeout: Option<u64>,

    /// Verbosity: --verbose, -vv, -vvv (see spec §9).
    #[arg(long, global = true, action = clap::ArgAction::Count)]
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
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Create or update a profile interactively.
    Init(InitArgs),
    /// Auth operations.
    Auth {
        #[command(subcommand)]
        sub: AuthSub,
    },
    /// Manage profiles.
    Profile {
        #[command(subcommand)]
        sub: ProfileSub,
    },
    /// Table API CRUD.
    Table {
        #[command(subcommand)]
        sub: TableSub,
    },
    /// Schema discovery.
    Schema {
        #[command(subcommand)]
        sub: SchemaSub,
    },
    /// Dump the full command tree as JSON for agent/MCP generation.
    Introspect,
    /// Get pipeline/deployment progress by ID.
    Progress(ProgressArgs),
    /// App repository operations (install, publish, rollback).
    App {
        #[command(subcommand)]
        sub: AppSub,
    },
    /// Update Set lifecycle operations.
    #[command(name = "updateset")]
    UpdateSet {
        #[command(subcommand)]
        sub: UpdateSetSub,
    },
    /// Aggregate statistics for a table (GET /api/now/stats/{table}).
    Aggregate(AggregateArgs),
    /// Performance Analytics scorecard operations.
    Scores {
        #[command(subcommand)]
        sub: ScoresSub,
    },
    /// Automated Test Framework operations.
    Atf {
        #[command(subcommand)]
        sub: AtfSub,
    },
    /// Change Management operations (normal, emergency, standard).
    Change {
        #[command(subcommand)]
        sub: ChangeSub,
    },
    /// Attachment operations (upload, download, list, delete).
    Attachment {
        #[command(subcommand)]
        sub: AttachmentSub,
    },
    /// CMDB Instance and Meta operations.
    Cmdb {
        #[command(subcommand)]
        sub: CmdbSub,
    },
    /// Import Set operations (staging table imports).
    Import {
        #[command(subcommand)]
        sub: ImportSub,
    },
    /// Service Catalog operations (catalogs, items, cart, ordering).
    Catalog {
        #[command(subcommand)]
        sub: CatalogSub,
    },
    /// Identification and Reconciliation (CI create/update/query).
    Identify {
        #[command(subcommand)]
        sub: IdentifySub,
    },
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
