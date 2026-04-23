use crate::cli::table::{bool_opt, build_client, build_profile, format_from_flags, unwrap_or_raw};
use crate::cli::{AtfResultsArgs, AtfRunArgs, GlobalFlags};
use crate::error::{Error, Result};
use crate::output::emit_value;
use std::io;

pub fn run(global: &GlobalFlags, args: AtfRunArgs) -> Result<()> {
    if args.suite_id.is_none() && args.suite_name.is_none() {
        return Err(Error::Usage(
            "either --suite-id or --suite-name is required".into(),
        ));
    }
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let mut query: Vec<(String, String)> = Vec::new();
    if let Some(v) = args.suite_id {
        query.push(("test_suite_sys_id".into(), v));
    }
    if let Some(v) = args.suite_name {
        query.push(("test_suite_name".into(), v));
    }
    if let Some(v) = args.browser_name {
        query.push(("browser_name".into(), v));
    }
    if let Some(v) = args.browser_version {
        query.push(("browser_version".into(), v));
    }
    if let Some(v) = args.os_name {
        query.push(("os_name".into(), v));
    }
    if let Some(v) = args.os_version {
        query.push(("os_version".into(), v));
    }
    if bool_opt(args.run_in_cloud).is_some() {
        query.push(("run_in_cloud".into(), "true".into()));
    }
    if bool_opt(args.performance_run).is_some() {
        query.push(("performance_run".into(), "true".into()));
    }
    let resp = client.post("/api/sn_cicd/testsuite/run", &query, &serde_json::json!({}))?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

pub fn results(global: &GlobalFlags, args: AtfResultsArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("/api/sn_cicd/testsuite/results/{}", args.result_id);
    let resp = client.get(&path, &[])?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}
