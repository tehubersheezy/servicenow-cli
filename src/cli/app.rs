use crate::cli::table::{bool_opt, build_client, build_profile, format_from_flags, unwrap_or_raw};
use crate::cli::{AppInstallArgs, AppPublishArgs, AppRollbackArgs, GlobalFlags};
use crate::error::{Error, Result};
use crate::output::emit_value;
use std::io;

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
    if bool_opt(args.auto_upgrade_base_app).is_some() {
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
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
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
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
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
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}
