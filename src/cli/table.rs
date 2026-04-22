use crate::cli::{GlobalFlags, OutputMode, TableDeleteArgs, TableGetArgs, TableListArgs};
use crate::client::{Client, RetryPolicy};
use crate::config::{
    config_path, credentials_path, load_config_from, load_credentials_from, resolve_profile,
    ProfileResolverInputs, ResolvedProfile,
};
use crate::error::{Error, Result};
use crate::output::{emit_value, Format, ResolvedFormat};
use crate::query::{DeleteQuery, GetQuery, ListQuery};
use is_terminal::IsTerminal;
use serde_json::Value;
use std::io;

pub fn list(global: &GlobalFlags, args: TableListArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let retry = retry_policy(global.no_retry);
    let client = Client::builder().retry(retry).build(&profile)?;

    if args.all {
        return Err(Error::Usage(
            "--all not yet implemented (lands in Task 20)".into(),
        ));
    }

    let q = ListQuery {
        query: args.query,
        fields: args.fields,
        page_size: Some(args.page_size),
        offset: args.offset,
        display_value: args.display_value.map(Into::into),
        exclude_reference_link: bool_opt(args.exclude_reference_link),
        suppress_pagination_header: bool_opt(args.suppress_pagination_header),
        view: args.view,
        query_category: args.query_category,
        query_no_domain: bool_opt(args.query_no_domain),
        no_count: bool_opt(args.no_count),
    };
    let path = format!("/api/now/table/{}", args.table);
    let resp: Value = client.get(&path, &q.to_pairs())?;
    let out = unwrap_or_raw(resp, global.output);
    let fmt = format_from_flags(global);
    emit_value(io::stdout().lock(), &out, fmt).map_err(|e| Error::Usage(format!("stdout: {e}")))?;
    Ok(())
}

pub(crate) fn build_profile(global: &GlobalFlags) -> Result<ResolvedProfile> {
    let config = load_config_from(&config_path()?)?;
    let creds = load_credentials_from(&credentials_path()?)?;
    let env_profile = std::env::var("SN_PROFILE").ok();
    let env_instance = std::env::var("SN_INSTANCE").ok();
    let env_username = std::env::var("SN_USERNAME").ok();
    let env_password = std::env::var("SN_PASSWORD").ok();
    resolve_profile(ProfileResolverInputs {
        cli_profile: global.profile.as_deref(),
        env_profile: env_profile.as_deref(),
        cli_instance_override: global.instance_override.as_deref(),
        env_instance: env_instance.as_deref(),
        env_username: env_username.as_deref(),
        env_password: env_password.as_deref(),
        config: &config,
        credentials: &creds,
    })
}

pub(crate) fn retry_policy(no_retry: bool) -> RetryPolicy {
    if no_retry {
        RetryPolicy {
            enabled: false,
            ..Default::default()
        }
    } else {
        RetryPolicy::default()
    }
}

pub(crate) fn bool_opt(b: bool) -> Option<bool> {
    if b {
        Some(true)
    } else {
        None
    }
}

pub(crate) fn format_from_flags(g: &GlobalFlags) -> ResolvedFormat {
    if g.pretty {
        Format::Pretty.resolve()
    } else if g.compact {
        Format::Compact.resolve()
    } else {
        Format::Auto.resolve()
    }
}

pub(crate) fn unwrap_or_raw(v: Value, mode: OutputMode) -> Value {
    match mode {
        OutputMode::Raw => v,
        OutputMode::Default => v.get("result").cloned().unwrap_or(v),
    }
}

pub fn get(global: &GlobalFlags, args: TableGetArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = Client::builder()
        .retry(retry_policy(global.no_retry))
        .build(&profile)?;
    let q = GetQuery {
        fields: args.fields,
        display_value: args.display_value.map(Into::into),
        exclude_reference_link: bool_opt(args.exclude_reference_link),
        view: args.view,
        query_no_domain: bool_opt(args.query_no_domain),
    };
    let path = format!("/api/now/table/{}/{}", args.table, args.sys_id);
    let resp = client.get(&path, &q.to_pairs())?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

pub fn delete(global: &GlobalFlags, args: TableDeleteArgs) -> Result<()> {
    if !args.yes {
        if !std::io::stdin().is_terminal() {
            return Err(Error::Usage(
                "delete requires --yes when stdin is not a terminal".into(),
            ));
        }
        eprint!("Delete {}/{}? [y/N]: ", args.table, args.sys_id);
        let mut s = String::new();
        std::io::stdin()
            .read_line(&mut s)
            .map_err(|e| Error::Usage(format!("read stdin: {e}")))?;
        if !matches!(s.trim(), "y" | "Y" | "yes" | "YES") {
            return Err(Error::Usage("aborted".into()));
        }
    }
    let profile = build_profile(global)?;
    let client = Client::builder()
        .retry(retry_policy(global.no_retry))
        .build(&profile)?;
    let q = DeleteQuery {
        query_no_domain: bool_opt(args.query_no_domain),
    };
    let path = format!("/api/now/table/{}/{}", args.table, args.sys_id);
    client.delete(&path, &q.to_pairs())
}
