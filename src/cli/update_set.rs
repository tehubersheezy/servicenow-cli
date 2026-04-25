use crate::cli::table::{build_client, build_profile, format_from_flags, unwrap_or_raw};
use crate::cli::GlobalFlags;
use crate::error::Result;
use crate::output::emit_value;
use clap::Subcommand;
use std::io;

#[derive(Subcommand, Debug)]
pub enum UpdateSetSub {
    /// Create a new Update Set.
    Create(UpdateSetCreateArgs),
    /// Retrieve a remote Update Set into this instance.
    Retrieve(UpdateSetRetrieveArgs),
    /// Preview a retrieved remote Update Set.
    Preview(UpdateSetIdArg),
    /// Commit a previewed remote Update Set.
    Commit(UpdateSetIdArg),
    /// Commit multiple remote Update Sets at once.
    CommitMultiple(UpdateSetCommitMultipleArgs),
    /// Back out (undo) an applied Update Set.
    BackOut(UpdateSetBackOutArgs),
}

#[derive(clap::Args, Debug)]
pub struct UpdateSetCreateArgs {
    /// Name for the new Update Set (required).
    #[arg(long, required = true)]
    pub name: String,
    /// Optional description.
    #[arg(long)]
    pub description: Option<String>,
    /// sys_id to assign to the new Update Set.
    #[arg(long)]
    pub sys_id: Option<String>,
    /// Application scope.
    #[arg(long)]
    pub scope: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct UpdateSetRetrieveArgs {
    /// sys_id of the Update Set to retrieve (required).
    #[arg(long, required = true)]
    pub update_set_id: String,
    /// sys_id of the source record.
    #[arg(long)]
    pub source_id: Option<String>,
    /// Instance ID of the source ServiceNow instance.
    #[arg(long)]
    pub source_instance_id: Option<String>,
    /// Automatically preview after retrieval.
    #[arg(long)]
    pub auto_preview: bool,
    /// Clean up retrieved set after preview/commit.
    #[arg(long)]
    pub cleanup_retrieved: bool,
    /// Block until the operation completes (polls progress API).
    #[arg(long)]
    pub wait: bool,
}

/// Shared arg struct for preview and commit (single path param).
#[derive(clap::Args, Debug)]
pub struct UpdateSetIdArg {
    /// Remote Update Set sys_id.
    pub remote_update_set_id: String,
    /// Block until the operation completes (polls progress API).
    #[arg(long)]
    pub wait: bool,
}

#[derive(clap::Args, Debug)]
pub struct UpdateSetCommitMultipleArgs {
    /// Comma-separated list of remote Update Set sys_ids.
    #[arg(long, required = true)]
    pub ids: String,
    /// Block until the operation completes (polls progress API).
    #[arg(long)]
    pub wait: bool,
}

#[derive(clap::Args, Debug)]
pub struct UpdateSetBackOutArgs {
    /// sys_id of the Update Set to back out (required).
    #[arg(long, required = true)]
    pub update_set_id: String,
    /// Also roll back any application installs included in the set.
    #[arg(long)]
    pub rollback_installs: bool,
    /// Block until the operation completes (polls progress API).
    #[arg(long)]
    pub wait: bool,
}

pub fn create(global: &GlobalFlags, args: UpdateSetCreateArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let mut query: Vec<(String, String)> = vec![("name".into(), args.name)];
    if let Some(v) = args.description {
        query.push(("description".into(), v));
    }
    if let Some(v) = args.sys_id {
        query.push(("sys_id".into(), v));
    }
    if let Some(v) = args.scope {
        query.push(("scope".into(), v));
    }
    let resp = client.post(
        "/api/sn_cicd/update_set/create",
        &query,
        &serde_json::json!({}),
    )?;
    let out = unwrap_or_raw(resp, global.output);
    crate::cli::table::write_response(global, &out)
}

pub fn retrieve(global: &GlobalFlags, args: UpdateSetRetrieveArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let mut query: Vec<(String, String)> = vec![("update_set_id".into(), args.update_set_id)];
    if let Some(v) = args.source_id {
        query.push(("source_id".into(), v));
    }
    if let Some(v) = args.source_instance_id {
        query.push(("source_instance_id".into(), v));
    }
    if args.auto_preview {
        query.push(("auto_preview".into(), "true".into()));
    }
    if args.cleanup_retrieved {
        query.push(("cleanup_retrieved".into(), "true".into()));
    }
    let resp = client.post(
        "/api/sn_cicd/update_set/retrieve",
        &query,
        &serde_json::json!({}),
    )?;
    let out = unwrap_or_raw(resp, global.output);
    if args.wait {
        if let Some(progress_id) = out
            .get("links")
            .and_then(|l| l.get("progress"))
            .and_then(|p| p.get("id"))
            .and_then(|id| id.as_str())
        {
            let final_result =
                crate::cli::progress::wait_for_completion(&client, progress_id, global)?;
            return emit_value(
                io::stdout().lock(),
                &final_result,
                format_from_flags(global),
            )
            .map_err(crate::output::map_stdout_err);
        }
    }
    crate::cli::table::write_response(global, &out)
}

pub fn preview(global: &GlobalFlags, args: UpdateSetIdArg) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!(
        "/api/sn_cicd/update_set/preview/{}",
        args.remote_update_set_id
    );
    let resp = client.post(&path, &[], &serde_json::json!({}))?;
    let out = unwrap_or_raw(resp, global.output);
    if args.wait {
        if let Some(progress_id) = out
            .get("links")
            .and_then(|l| l.get("progress"))
            .and_then(|p| p.get("id"))
            .and_then(|id| id.as_str())
        {
            let final_result =
                crate::cli::progress::wait_for_completion(&client, progress_id, global)?;
            return emit_value(
                io::stdout().lock(),
                &final_result,
                format_from_flags(global),
            )
            .map_err(crate::output::map_stdout_err);
        }
    }
    crate::cli::table::write_response(global, &out)
}

pub fn commit(global: &GlobalFlags, args: UpdateSetIdArg) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!(
        "/api/sn_cicd/update_set/commit/{}",
        args.remote_update_set_id
    );
    let resp = client.post(&path, &[], &serde_json::json!({}))?;
    let out = unwrap_or_raw(resp, global.output);
    if args.wait {
        if let Some(progress_id) = out
            .get("links")
            .and_then(|l| l.get("progress"))
            .and_then(|p| p.get("id"))
            .and_then(|id| id.as_str())
        {
            let final_result =
                crate::cli::progress::wait_for_completion(&client, progress_id, global)?;
            return emit_value(
                io::stdout().lock(),
                &final_result,
                format_from_flags(global),
            )
            .map_err(crate::output::map_stdout_err);
        }
    }
    crate::cli::table::write_response(global, &out)
}

pub fn commit_multiple(global: &GlobalFlags, args: UpdateSetCommitMultipleArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let query = vec![("remote_update_set_ids".into(), args.ids)];
    let resp = client.post(
        "/api/sn_cicd/update_set/commitMultiple",
        &query,
        &serde_json::json!({}),
    )?;
    let out = unwrap_or_raw(resp, global.output);
    if args.wait {
        if let Some(progress_id) = out
            .get("links")
            .and_then(|l| l.get("progress"))
            .and_then(|p| p.get("id"))
            .and_then(|id| id.as_str())
        {
            let final_result =
                crate::cli::progress::wait_for_completion(&client, progress_id, global)?;
            return emit_value(
                io::stdout().lock(),
                &final_result,
                format_from_flags(global),
            )
            .map_err(crate::output::map_stdout_err);
        }
    }
    crate::cli::table::write_response(global, &out)
}

pub fn back_out(global: &GlobalFlags, args: UpdateSetBackOutArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let mut query: Vec<(String, String)> = vec![("update_set_id".into(), args.update_set_id)];
    if args.rollback_installs {
        query.push(("rollback_installs".into(), "true".into()));
    }
    let resp = client.post(
        "/api/sn_cicd/update_set/back_out",
        &query,
        &serde_json::json!({}),
    )?;
    let out = unwrap_or_raw(resp, global.output);
    if args.wait {
        if let Some(progress_id) = out
            .get("links")
            .and_then(|l| l.get("progress"))
            .and_then(|p| p.get("id"))
            .and_then(|id| id.as_str())
        {
            let final_result =
                crate::cli::progress::wait_for_completion(&client, progress_id, global)?;
            return emit_value(
                io::stdout().lock(),
                &final_result,
                format_from_flags(global),
            )
            .map_err(crate::output::map_stdout_err);
        }
    }
    crate::cli::table::write_response(global, &out)
}
