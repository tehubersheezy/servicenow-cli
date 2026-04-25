use crate::body::{build_body, BodyInput};
use crate::cli::table::{build_client, build_profile, format_from_flags, unwrap_or_raw};
use crate::cli::GlobalFlags;
use crate::error::Result;
use crate::output::emit_value;
use clap::Subcommand;
use std::io;

#[derive(Subcommand, Debug)]
pub enum ImportSub {
    /// Insert a single record into a staging table.
    Create(ImportCreateArgs),
    /// Insert multiple records into a staging table.
    Bulk(ImportBulkArgs),
    /// Retrieve an import set record.
    Get(ImportGetArgs),
}

#[derive(clap::Args, Debug)]
pub struct ImportCreateArgs {
    /// Staging table name.
    pub staging_table: String,
    /// Body source: inline JSON, @file (path), or @- (stdin). Use a file to avoid shell quoting on multi-line values.
    #[arg(long, conflicts_with = "field")]
    pub data: Option<String>,
    /// Repeatable name=value. Use name=@file to read the value from a file (e.g. multi-line text). Mutually exclusive with --data.
    #[arg(long = "field", conflicts_with = "data")]
    pub field: Vec<String>,
}

#[derive(clap::Args, Debug)]
pub struct ImportBulkArgs {
    /// Staging table name.
    pub staging_table: String,
    /// JSON array of records, @file, or @- for stdin.
    #[arg(long, required = true)]
    pub data: String,
}

#[derive(clap::Args, Debug)]
pub struct ImportGetArgs {
    /// Staging table name.
    pub staging_table: String,
    /// sys_id of the import set record.
    pub sys_id: String,
}

pub fn create(global: &GlobalFlags, args: ImportCreateArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("/api/now/import/{}", args.staging_table);
    let body_input = if let Some(d) = args.data {
        BodyInput::Data(d)
    } else if !args.field.is_empty() {
        BodyInput::Fields(args.field)
    } else {
        BodyInput::None
    };
    let body = build_body(body_input)?;
    let resp = client.post(&path, &[], &body)?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(crate::output::map_stdout_err)
}

pub fn bulk(global: &GlobalFlags, args: ImportBulkArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("/api/now/import/{}/insertMultiple", args.staging_table);
    let body = build_body(BodyInput::Data(args.data))?;
    let resp = client.post(&path, &[], &body)?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(crate::output::map_stdout_err)
}

pub fn get(global: &GlobalFlags, args: ImportGetArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("/api/now/import/{}/{}", args.staging_table, args.sys_id);
    let resp = client.get(&path, &[])?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(crate::output::map_stdout_err)
}
