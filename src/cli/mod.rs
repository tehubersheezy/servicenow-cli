pub mod auth;
pub mod init;
pub mod introspect;
pub mod profile;
pub mod table;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug)]
#[command(name = "sn", version, about = "ServiceNow CLI (Table API + schema)")]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalFlags,

    #[command(subcommand)]
    pub command: Command,
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

    /// Disable automatic retry for 429/5xx responses.
    #[arg(long, global = true)]
    pub no_retry: bool,

    /// Verbosity: -v, -vv, -vvv (see spec §9).
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,
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
}

#[derive(clap::Args, Debug)]
pub struct InitArgs {
    #[arg(long)]
    pub profile: Option<String>,
    #[arg(long)]
    pub instance: Option<String>,
    #[arg(long)]
    pub username: Option<String>,
    #[arg(long)]
    pub password: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum AuthSub {
    /// Verify credentials by calling /api/now/table/sys_user?sysparm_limit=1.
    Test,
}

#[derive(Subcommand, Debug)]
pub enum ProfileSub {
    List,
    Show { name: Option<String> },
    Remove { name: String },
    Use { name: String },
}

// Placeholders; filled in by later tasks.
#[derive(Subcommand, Debug)]
pub enum TableSub {
    #[command(about = "List records")]
    List(TableListArgs),
    #[command(about = "Get a single record by sys_id")]
    Get(TableGetArgs),
    #[command(about = "Create a record")]
    Create(TableCreateArgs),
    #[command(about = "Patch a record (partial update)")]
    Update(TableUpdateArgs),
    #[command(about = "Replace a record (PUT, full overwrite)")]
    Replace(TableReplaceArgs),
    #[command(about = "Delete a record")]
    Delete(TableDeleteArgs),
}

#[derive(clap::Args, Debug)]
pub struct TableListArgs {
    /// Table name (e.g. `incident`).
    pub table: String,
    /// Encoded query, e.g. `active=true^priority=1`.
    #[arg(long, alias = "sysparm-query")]
    pub query: Option<String>,
    /// Comma-separated fields to return.
    #[arg(long, alias = "sysparm-fields")]
    pub fields: Option<String>,
    /// Records per API call (default 1000).
    #[arg(long, alias = "limit", alias = "sysparm-limit", default_value_t = 1000)]
    pub page_size: u32,
    /// Starting offset for manual pagination (ignored with --all).
    #[arg(long, alias = "sysparm-offset")]
    pub offset: Option<u32>,
    /// Resolve reference/choice fields: false (default), true, or all.
    #[arg(long, alias = "sysparm-display-value", value_enum)]
    pub display_value: Option<DisplayValueArg>,
    /// Strip reference-link URLs from reference fields.
    #[arg(long, alias = "sysparm-exclude-reference-link")]
    pub exclude_reference_link: bool,
    /// Skip X-Total-Count calculation.
    #[arg(long, alias = "sysparm-suppress-pagination-header")]
    pub suppress_pagination_header: bool,
    /// Apply a named form/list view.
    #[arg(long, alias = "sysparm-view")]
    pub view: Option<String>,
    /// Query category for index selection.
    #[arg(long, alias = "sysparm-query-category")]
    pub query_category: Option<String>,
    /// Cross-domain access if authorized.
    #[arg(long, alias = "sysparm-query-no-domain")]
    pub query_no_domain: bool,
    /// Skip the count query.
    #[arg(long, alias = "sysparm-no-count")]
    pub no_count: bool,
    /// Auto-paginate: stream every matching record (JSONL unless --array).
    #[arg(long)]
    pub all: bool,
    /// With --all, buffer into a single JSON array instead of JSONL.
    #[arg(long, requires = "all")]
    pub array: bool,
    /// Cap total records returned (default 100000; 0 = unlimited).
    #[arg(long, default_value_t = 100_000)]
    pub max_records: u32,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "lowercase")]
pub enum DisplayValueArg {
    True,
    False,
    All,
}

impl From<DisplayValueArg> for crate::query::DisplayValue {
    fn from(v: DisplayValueArg) -> Self {
        match v {
            DisplayValueArg::True => crate::query::DisplayValue::True,
            DisplayValueArg::False => crate::query::DisplayValue::False,
            DisplayValueArg::All => crate::query::DisplayValue::All,
        }
    }
}
#[derive(clap::Args, Debug)]
pub struct TableGetArgs {
    pub table: String,
    pub sys_id: String,
    #[arg(long, alias = "sysparm-fields")]
    pub fields: Option<String>,
    #[arg(long, alias = "sysparm-display-value", value_enum)]
    pub display_value: Option<DisplayValueArg>,
    #[arg(long, alias = "sysparm-exclude-reference-link")]
    pub exclude_reference_link: bool,
    #[arg(long, alias = "sysparm-view")]
    pub view: Option<String>,
    #[arg(long, alias = "sysparm-query-no-domain")]
    pub query_no_domain: bool,
}
#[derive(clap::Args, Debug)]
pub struct TableCreateArgs {
    pub table: String,
    /// Inline JSON, @file, or @- for stdin.
    #[arg(long, conflicts_with = "field")]
    pub data: Option<String>,
    /// Repeatable name=value. Mutually exclusive with --data.
    #[arg(long = "field", conflicts_with = "data")]
    pub field: Vec<String>,
    #[arg(long, alias = "sysparm-fields")]
    pub fields: Option<String>,
    #[arg(long, alias = "sysparm-display-value", value_enum)]
    pub display_value: Option<DisplayValueArg>,
    #[arg(long, alias = "sysparm-exclude-reference-link")]
    pub exclude_reference_link: bool,
    #[arg(long, alias = "sysparm-input-display-value")]
    pub input_display_value: bool,
    #[arg(long, alias = "sysparm-suppress-auto-sys-field")]
    pub suppress_auto_sys_field: bool,
    #[arg(long, alias = "sysparm-view")]
    pub view: Option<String>,
}
#[derive(clap::Args, Debug, Default)]
pub struct TableUpdateArgs {
    pub table: String,
    pub sys_id: String,
}
#[derive(clap::Args, Debug, Default)]
pub struct TableReplaceArgs {
    pub table: String,
    pub sys_id: String,
}
#[derive(clap::Args, Debug)]
pub struct TableDeleteArgs {
    pub table: String,
    pub sys_id: String,
    /// Skip confirmation prompt (required for non-interactive use).
    #[arg(long, short = 'y')]
    pub yes: bool,
    #[arg(long, alias = "sysparm-query-no-domain")]
    pub query_no_domain: bool,
}

#[derive(Subcommand, Debug)]
pub enum SchemaSub {
    Tables(SchemaTablesArgs),
    Columns(SchemaColumnsArgs),
    Choices(SchemaChoicesArgs),
}

#[derive(clap::Args, Debug, Default)]
pub struct SchemaTablesArgs {/* expanded in Task 21 */}
#[derive(clap::Args, Debug, Default)]
pub struct SchemaColumnsArgs {
    pub table: String,
}
#[derive(clap::Args, Debug, Default)]
pub struct SchemaChoicesArgs {
    pub table: String,
    pub field: String,
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
