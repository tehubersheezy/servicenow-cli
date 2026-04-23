use crate::cli::table::{bool_opt, build_client, build_profile, format_from_flags, unwrap_or_raw};
use crate::cli::{
    GlobalFlags, UpdateSetBackOutArgs, UpdateSetCommitMultipleArgs, UpdateSetCreateArgs,
    UpdateSetIdArg, UpdateSetRetrieveArgs,
};
use crate::error::{Error, Result};
use crate::output::emit_value;
use std::io;

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
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
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
    if bool_opt(args.auto_preview).is_some() {
        query.push(("auto_preview".into(), "true".into()));
    }
    if bool_opt(args.cleanup_retrieved).is_some() {
        query.push(("cleanup_retrieved".into(), "true".into()));
    }
    let resp = client.post(
        "/api/sn_cicd/update_set/retrieve",
        &query,
        &serde_json::json!({}),
    )?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
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
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
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
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
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
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

pub fn back_out(global: &GlobalFlags, args: UpdateSetBackOutArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let mut query: Vec<(String, String)> = vec![("update_set_id".into(), args.update_set_id)];
    if bool_opt(args.rollback_installs).is_some() {
        query.push(("rollback_installs".into(), "true".into()));
    }
    let resp = client.post(
        "/api/sn_cicd/update_set/back_out",
        &query,
        &serde_json::json!({}),
    )?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}
