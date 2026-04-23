use crate::body::{build_body, BodyInput};
use crate::cli::table::{build_client, build_profile, format_from_flags, unwrap_or_raw};
use crate::cli::{
    ChangeApprovalsArgs, ChangeCiAddArgs, ChangeCiSub, ChangeConflictAddArgs, ChangeConflictSub,
    ChangeCreateArgs, ChangeDeleteArgs, ChangeGetArgs, ChangeListArgs, ChangeOptionalIdArg,
    ChangeRiskArgs, ChangeSysIdArg, ChangeTaskCreateArgs, ChangeTaskDeleteArgs, ChangeTaskGetArgs,
    ChangeTaskListArgs, ChangeTaskSub, ChangeTaskUpdateArgs, ChangeType, ChangeUpdateArgs,
    GlobalFlags,
};
use crate::error::{Error, Result};
use crate::output::emit_value;
use std::io;

fn base_path(ct: Option<ChangeType>) -> &'static str {
    match ct {
        Some(ChangeType::Normal) => "/api/sn_chg_rest/change/normal",
        Some(ChangeType::Emergency) => "/api/sn_chg_rest/change/emergency",
        Some(ChangeType::Standard) => "/api/sn_chg_rest/change/standard",
        None => "/api/sn_chg_rest/change",
    }
}

pub fn list(global: &GlobalFlags, args: ChangeListArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = base_path(args.r#type);
    let mut query: Vec<(String, String)> = Vec::new();
    if let Some(v) = args.query {
        query.push(("sysparm_query".into(), v));
    }
    if let Some(v) = args.fields {
        query.push(("sysparm_fields".into(), v));
    }
    query.push(("sysparm_limit".into(), args.setlimit.to_string()));
    if let Some(v) = args.offset {
        query.push(("sysparm_offset".into(), v.to_string()));
    }
    if let Some(v) = args.display_value {
        let dv: crate::query::DisplayValue = v.into();
        query.push(("sysparm_display_value".into(), dv.as_str().into()));
    }
    if args.exclude_reference_link {
        query.push(("sysparm_exclude_reference_link".into(), "true".into()));
    }
    if let Some(v) = args.view {
        query.push(("sysparm_view".into(), v));
    }
    let resp = client.get(path, &query)?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

pub fn get(global: &GlobalFlags, args: ChangeGetArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("{}/{}", base_path(args.r#type), args.sys_id);
    let mut query: Vec<(String, String)> = Vec::new();
    if let Some(v) = args.fields {
        query.push(("sysparm_fields".into(), v));
    }
    if let Some(v) = args.display_value {
        let dv: crate::query::DisplayValue = v.into();
        query.push(("sysparm_display_value".into(), dv.as_str().into()));
    }
    if args.exclude_reference_link {
        query.push(("sysparm_exclude_reference_link".into(), "true".into()));
    }
    if let Some(v) = args.view {
        query.push(("sysparm_view".into(), v));
    }
    let resp = client.get(&path, &query)?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

pub fn create(global: &GlobalFlags, args: ChangeCreateArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = match args.r#type {
        ChangeType::Standard => {
            let tmpl = args
                .template
                .ok_or_else(|| Error::Usage("--template is required for --type standard".into()))?;
            format!("/api/sn_chg_rest/change/standard/{tmpl}")
        }
        _ => base_path(Some(args.r#type)).to_string(),
    };
    let body_input = if let Some(d) = args.data {
        BodyInput::Data(d)
    } else if !args.field.is_empty() {
        BodyInput::Fields(args.field)
    } else {
        BodyInput::Data("{}".into())
    };
    let body = build_body(body_input)?;
    let mut query: Vec<(String, String)> = Vec::new();
    if let Some(v) = args.fields {
        query.push(("sysparm_fields".into(), v));
    }
    if let Some(v) = args.display_value {
        let dv: crate::query::DisplayValue = v.into();
        query.push(("sysparm_display_value".into(), dv.as_str().into()));
    }
    let resp = client.post(&path, &query, &body)?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

pub fn update(global: &GlobalFlags, args: ChangeUpdateArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("{}/{}", base_path(args.r#type), args.sys_id);
    let body_input = if let Some(d) = args.data {
        BodyInput::Data(d)
    } else if !args.field.is_empty() {
        BodyInput::Fields(args.field)
    } else {
        BodyInput::None
    };
    let body = build_body(body_input)?;
    let mut query: Vec<(String, String)> = Vec::new();
    if let Some(v) = args.fields {
        query.push(("sysparm_fields".into(), v));
    }
    if let Some(v) = args.display_value {
        let dv: crate::query::DisplayValue = v.into();
        query.push(("sysparm_display_value".into(), dv.as_str().into()));
    }
    let resp = client.patch(&path, &query, &body)?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

pub fn delete(global: &GlobalFlags, args: ChangeDeleteArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("{}/{}", base_path(args.r#type), args.sys_id);
    client.delete(&path, &[])?;
    Ok(())
}

pub fn nextstates(global: &GlobalFlags, args: ChangeSysIdArg) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("/api/sn_chg_rest/change/{}/nextstates", args.sys_id);
    let resp = client.get(&path, &[])?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

pub fn approvals(global: &GlobalFlags, args: ChangeApprovalsArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("/api/sn_chg_rest/change/{}/approvals", args.sys_id);
    let body_input = if let Some(d) = args.data {
        BodyInput::Data(d)
    } else if !args.field.is_empty() {
        BodyInput::Fields(args.field)
    } else {
        BodyInput::None
    };
    let body = build_body(body_input)?;
    let resp = client.patch(&path, &[], &body)?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

pub fn risk(global: &GlobalFlags, args: ChangeRiskArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("/api/sn_chg_rest/change/{}/risk", args.sys_id);
    let body_input = if let Some(d) = args.data {
        BodyInput::Data(d)
    } else if !args.field.is_empty() {
        BodyInput::Fields(args.field)
    } else {
        BodyInput::None
    };
    let body = build_body(body_input)?;
    let resp = client.patch(&path, &[], &body)?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

pub fn schedule(global: &GlobalFlags, args: ChangeSysIdArg) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("/api/sn_chg_rest/change/{}/schedule", args.sys_id);
    let resp = client.get(&path, &[])?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

pub fn models(global: &GlobalFlags, args: ChangeOptionalIdArg) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = match args.sys_id {
        Some(id) => format!("/api/sn_chg_rest/change/model/{id}"),
        None => "/api/sn_chg_rest/change/model".to_string(),
    };
    let resp = client.get(&path, &[])?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

pub fn templates(global: &GlobalFlags, args: ChangeOptionalIdArg) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = match args.sys_id {
        Some(id) => format!("/api/sn_chg_rest/change/standard/template/{id}"),
        None => "/api/sn_chg_rest/change/standard/template".to_string(),
    };
    let resp = client.get(&path, &[])?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

pub fn task(global: &GlobalFlags, sub: ChangeTaskSub) -> Result<()> {
    match sub {
        ChangeTaskSub::List(args) => task_list(global, args),
        ChangeTaskSub::Get(args) => task_get(global, args),
        ChangeTaskSub::Create(args) => task_create(global, args),
        ChangeTaskSub::Update(args) => task_update(global, args),
        ChangeTaskSub::Delete(args) => task_delete(global, args),
    }
}

fn task_list(global: &GlobalFlags, args: ChangeTaskListArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("/api/sn_chg_rest/change/{}/task", args.change_sys_id);
    let mut query: Vec<(String, String)> = Vec::new();
    if let Some(v) = args.fields {
        query.push(("sysparm_fields".into(), v));
    }
    query.push(("sysparm_limit".into(), args.setlimit.to_string()));
    let resp = client.get(&path, &query)?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

fn task_get(global: &GlobalFlags, args: ChangeTaskGetArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!(
        "/api/sn_chg_rest/change/{}/task/{}",
        args.change_sys_id, args.task_sys_id
    );
    let resp = client.get(&path, &[])?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

fn task_create(global: &GlobalFlags, args: ChangeTaskCreateArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("/api/sn_chg_rest/change/{}/task", args.change_sys_id);
    let body_input = if let Some(d) = args.data {
        BodyInput::Data(d)
    } else if !args.field.is_empty() {
        BodyInput::Fields(args.field)
    } else {
        BodyInput::Data("{}".into())
    };
    let body = build_body(body_input)?;
    let resp = client.post(&path, &[], &body)?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

fn task_update(global: &GlobalFlags, args: ChangeTaskUpdateArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!(
        "/api/sn_chg_rest/change/{}/task/{}",
        args.change_sys_id, args.task_sys_id
    );
    let body_input = if let Some(d) = args.data {
        BodyInput::Data(d)
    } else if !args.field.is_empty() {
        BodyInput::Fields(args.field)
    } else {
        BodyInput::None
    };
    let body = build_body(body_input)?;
    let resp = client.patch(&path, &[], &body)?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

fn task_delete(global: &GlobalFlags, args: ChangeTaskDeleteArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!(
        "/api/sn_chg_rest/change/{}/task/{}",
        args.change_sys_id, args.task_sys_id
    );
    client.delete(&path, &[])?;
    Ok(())
}

pub fn ci(global: &GlobalFlags, sub: ChangeCiSub) -> Result<()> {
    match sub {
        ChangeCiSub::List(args) => ci_list(global, args),
        ChangeCiSub::Add(args) => ci_add(global, args),
    }
}

fn ci_list(global: &GlobalFlags, args: ChangeSysIdArg) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("/api/sn_chg_rest/change/{}/ci", args.sys_id);
    let resp = client.get(&path, &[])?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

fn ci_add(global: &GlobalFlags, args: ChangeCiAddArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("/api/sn_chg_rest/change/{}/ci", args.change_sys_id);
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
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

pub fn conflict(global: &GlobalFlags, sub: ChangeConflictSub) -> Result<()> {
    match sub {
        ChangeConflictSub::Get(args) => conflict_get(global, args),
        ChangeConflictSub::Add(args) => conflict_add(global, args),
        ChangeConflictSub::Remove(args) => conflict_remove(global, args),
    }
}

fn conflict_get(global: &GlobalFlags, args: ChangeSysIdArg) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("/api/sn_chg_rest/change/{}/conflict", args.sys_id);
    let resp = client.get(&path, &[])?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

fn conflict_add(global: &GlobalFlags, args: ChangeConflictAddArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("/api/sn_chg_rest/change/{}/conflict", args.sys_id);
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
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

fn conflict_remove(global: &GlobalFlags, args: ChangeSysIdArg) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("/api/sn_chg_rest/change/{}/conflict", args.sys_id);
    client.delete(&path, &[])?;
    Ok(())
}
