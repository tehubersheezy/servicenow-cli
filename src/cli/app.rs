use crate::cli::table::{build_client, build_profile, format_from_flags, unwrap_or_raw};
use crate::cli::GlobalFlags;
use crate::error::{Error, Result};
use crate::output::emit_value;
use clap::Subcommand;
use std::io;

#[derive(Subcommand, Debug)]
pub enum AppSub {
    /// Install an application from the app repository.
    Install(AppInstallArgs),
    /// Publish an application to the app repository.
    Publish(AppPublishArgs),
    /// Roll back an application to a previous version.
    Rollback(AppRollbackArgs),
}

#[derive(clap::Args, Debug)]
pub struct AppInstallArgs {
    /// sys_id of the application.
    #[arg(long)]
    pub sys_id: Option<String>,
    /// Application scope (e.g. `x_acme_myapp`).
    #[arg(long)]
    pub scope: Option<String>,
    /// Version to install.
    #[arg(long)]
    pub version: Option<String>,
    /// Automatically upgrade the base application if needed.
    #[arg(long)]
    pub auto_upgrade_base_app: bool,
    /// Version of the base application to use.
    #[arg(long)]
    pub base_app_version: Option<String>,
    /// Block until the operation completes (polls progress API).
    #[arg(long)]
    pub wait: bool,
}

#[derive(clap::Args, Debug)]
pub struct AppPublishArgs {
    /// sys_id of the application.
    #[arg(long)]
    pub sys_id: Option<String>,
    /// Application scope (e.g. `x_acme_myapp`).
    #[arg(long)]
    pub scope: Option<String>,
    /// Version to publish.
    #[arg(long)]
    pub version: Option<String>,
    /// Developer notes for this publish.
    #[arg(long)]
    pub dev_notes: Option<String>,
    /// Block until the operation completes (polls progress API).
    #[arg(long)]
    pub wait: bool,
}

#[derive(clap::Args, Debug)]
pub struct AppRollbackArgs {
    /// sys_id of the application.
    #[arg(long)]
    pub sys_id: Option<String>,
    /// Application scope (e.g. `x_acme_myapp`).
    #[arg(long)]
    pub scope: Option<String>,
    /// Version to roll back to (required).
    #[arg(long, required = true)]
    pub version: String,
    /// Block until the operation completes (polls progress API).
    #[arg(long)]
    pub wait: bool,
}

pub fn install(global: &GlobalFlags, args: AppInstallArgs) -> Result<()> {
    if args.sys_id.is_none() && args.scope.is_none() {
        return Err(Error::Usage(
            "either --sys-id or --scope is required".into(),
        ));
    }
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let mut query: Vec<(String, String)> = Vec::new();
    if let Some(v) = args.sys_id {
        query.push(("sys_id".into(), v));
    }
    if let Some(v) = args.scope {
        query.push(("scope".into(), v));
    }
    if let Some(v) = args.version {
        query.push(("version".into(), v));
    }
    if args.auto_upgrade_base_app {
        query.push(("auto_upgrade_base_app".into(), "true".into()));
    }
    if let Some(v) = args.base_app_version {
        query.push(("base_app_version".into(), v));
    }
    let resp = client.post(
        "/api/sn_cicd/app_repo/install",
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

pub fn publish(global: &GlobalFlags, args: AppPublishArgs) -> Result<()> {
    if args.sys_id.is_none() && args.scope.is_none() {
        return Err(Error::Usage(
            "either --sys-id or --scope is required".into(),
        ));
    }
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let mut query: Vec<(String, String)> = Vec::new();
    if let Some(v) = args.sys_id {
        query.push(("sys_id".into(), v));
    }
    if let Some(v) = args.scope {
        query.push(("scope".into(), v));
    }
    if let Some(v) = args.version {
        query.push(("version".into(), v));
    }
    if let Some(v) = args.dev_notes {
        query.push(("dev_notes".into(), v));
    }
    let resp = client.post(
        "/api/sn_cicd/app_repo/publish",
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

pub fn rollback(global: &GlobalFlags, args: AppRollbackArgs) -> Result<()> {
    if args.sys_id.is_none() && args.scope.is_none() {
        return Err(Error::Usage(
            "either --sys-id or --scope is required".into(),
        ));
    }
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let mut query: Vec<(String, String)> = Vec::new();
    if let Some(v) = args.sys_id {
        query.push(("sys_id".into(), v));
    }
    if let Some(v) = args.scope {
        query.push(("scope".into(), v));
    }
    query.push(("version".into(), args.version));
    let resp = client.post(
        "/api/sn_cicd/app_repo/rollback",
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
