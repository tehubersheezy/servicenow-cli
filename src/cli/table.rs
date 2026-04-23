use crate::body::{build_body, BodyInput};
use crate::cli::{
    DisplayValueArg, GlobalFlags, OutputMode, TableCreateArgs, TableDeleteArgs, TableGetArgs,
    TableListArgs, TableReplaceArgs, TableUpdateArgs,
};
use crate::client::Client;
use crate::config::{
    config_path, credentials_path, load_config_from, load_credentials_from, resolve_profile,
    ProfileResolverInputs, ResolvedProfile,
};
use crate::error::{Error, Result};
use crate::output::{emit_value, Format, ResolvedFormat};
use crate::query::{DeleteQuery, GetQuery, ListQuery, WriteQuery};
use is_terminal::IsTerminal;
use serde_json::Value;
use std::io::{self, Write};
use std::time::Duration;

pub fn list(global: &GlobalFlags, args: TableListArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;

    if args.all {
        let q = ListQuery {
            query: args.query.clone(),
            fields: args.fields.clone(),
            page_size: Some(args.setlimit),
            offset: None, // ignored with --all
            display_value: args.display_value.map(Into::into),
            exclude_reference_link: bool_opt(args.exclude_reference_link),
            suppress_pagination_header: bool_opt(args.suppress_pagination_header),
            view: args.view.clone(),
            query_category: args.query_category.clone(),
            query_no_domain: bool_opt(args.query_no_domain),
            no_count: bool_opt(args.no_count),
        };
        let path = format!("/api/now/table/{}", args.table);
        let cap = if args.max_records == 0 {
            None
        } else {
            Some(args.max_records)
        };
        let it = client.paginate(&path, &q.to_pairs(), cap);

        if args.array {
            let mut out = Vec::new();
            for r in it {
                out.push(r?);
            }
            emit_value(
                io::stdout().lock(),
                &Value::Array(out),
                format_from_flags(global),
            )
            .map_err(|e| Error::Usage(format!("stdout: {e}")))?;
        } else {
            let mut stdout = io::stdout().lock();
            for r in it {
                let v = r?;
                serde_json::to_writer(&mut stdout, &v)
                    .map_err(|e| Error::Usage(format!("stdout: {e}")))?;
                stdout
                    .write_all(b"\n")
                    .map_err(|e| Error::Usage(format!("stdout: {e}")))?;
            }
        }
        return Ok(());
    }

    let q = ListQuery {
        query: args.query,
        fields: args.fields,
        page_size: Some(args.setlimit),
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
    let env_proxy = std::env::var("SN_PROXY").ok();
    let env_no_proxy = std::env::var("SN_NO_PROXY").ok();
    let env_insecure = std::env::var("SN_INSECURE").ok();
    let env_ca_cert = std::env::var("SN_CA_CERT").ok();
    let env_proxy_ca_cert = std::env::var("SN_PROXY_CA_CERT").ok();
    resolve_profile(ProfileResolverInputs {
        cli_profile: global.profile.as_deref(),
        env_profile: env_profile.as_deref(),
        cli_instance_override: global.instance_override.as_deref(),
        env_instance: env_instance.as_deref(),
        env_username: env_username.as_deref(),
        env_password: env_password.as_deref(),
        cli_proxy: global.proxy.as_deref(),
        env_proxy: env_proxy.as_deref(),
        cli_no_proxy: global.no_proxy,
        env_no_proxy: env_no_proxy.as_deref(),
        cli_insecure: global.insecure,
        env_insecure: env_insecure.as_deref(),
        cli_ca_cert: global.ca_cert.as_deref(),
        env_ca_cert: env_ca_cert.as_deref(),
        cli_proxy_ca_cert: global.proxy_ca_cert.as_deref(),
        env_proxy_ca_cert: env_proxy_ca_cert.as_deref(),
        config: &config,
        credentials: &creds,
    })
}

pub(crate) fn build_client(profile: &ResolvedProfile, timeout: Option<u64>) -> Result<Client> {
    let mut b = Client::builder()
        .proxy(profile.proxy.clone())
        .no_proxy(profile.no_proxy.clone())
        .insecure(profile.insecure)
        .ca_cert(profile.ca_cert.clone())
        .proxy_ca_cert(profile.proxy_ca_cert.clone())
        .proxy_auth(
            profile.proxy_username.clone(),
            profile.proxy_password.clone(),
        );
    if let Some(secs) = timeout {
        b = b.timeout(Duration::from_secs(secs));
    }
    b.build(profile)
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
    let client = build_client(&profile, global.timeout)?;
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

pub fn create(global: &GlobalFlags, args: TableCreateArgs) -> Result<()> {
    let body_input = match (args.data, args.field.is_empty()) {
        (Some(d), true) => BodyInput::Data(d),
        (None, false) => BodyInput::Fields(args.field),
        (None, true) => return Err(Error::Usage("provide --data or one or more --field".into())),
        (Some(_), false) => {
            return Err(Error::Usage(
                "--data and --field are mutually exclusive".into(),
            ))
        }
    };
    let body = build_body(body_input)?;

    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let q = WriteQuery {
        fields: args.fields,
        display_value: args.display_value.map(Into::into),
        exclude_reference_link: bool_opt(args.exclude_reference_link),
        input_display_value: bool_opt(args.input_display_value),
        suppress_auto_sys_field: bool_opt(args.suppress_auto_sys_field),
        view: args.view,
        query_no_domain: None,
    };
    let path = format!("/api/now/table/{}", args.table);
    let resp = client.post(&path, &q.to_pairs(), &body)?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

pub fn update(global: &GlobalFlags, args: TableUpdateArgs) -> Result<()> {
    write_op(
        global,
        args.table,
        args.sys_id,
        args.data,
        args.field,
        args.fields,
        args.display_value,
        args.exclude_reference_link,
        args.input_display_value,
        args.suppress_auto_sys_field,
        args.view,
        args.query_no_domain,
        HttpMutation::Patch,
    )
}

pub fn replace(global: &GlobalFlags, args: TableReplaceArgs) -> Result<()> {
    write_op(
        global,
        args.table,
        args.sys_id,
        args.data,
        args.field,
        args.fields,
        args.display_value,
        args.exclude_reference_link,
        args.input_display_value,
        args.suppress_auto_sys_field,
        args.view,
        args.query_no_domain,
        HttpMutation::Put,
    )
}

enum HttpMutation {
    Patch,
    Put,
}

#[allow(clippy::too_many_arguments)]
fn write_op(
    global: &GlobalFlags,
    table: String,
    sys_id: String,
    data: Option<String>,
    field: Vec<String>,
    fields: Option<String>,
    display_value: Option<DisplayValueArg>,
    exclude_reference_link: bool,
    input_display_value: bool,
    suppress_auto_sys_field: bool,
    view: Option<String>,
    query_no_domain: bool,
    mutation: HttpMutation,
) -> Result<()> {
    let body_input = match (data, field.is_empty()) {
        (Some(d), true) => BodyInput::Data(d),
        (None, false) => BodyInput::Fields(field),
        (None, true) => return Err(Error::Usage("provide --data or one or more --field".into())),
        (Some(_), false) => {
            return Err(Error::Usage(
                "--data and --field are mutually exclusive".into(),
            ))
        }
    };
    let body = build_body(body_input)?;
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let q = WriteQuery {
        fields,
        display_value: display_value.map(Into::into),
        exclude_reference_link: bool_opt(exclude_reference_link),
        input_display_value: bool_opt(input_display_value),
        suppress_auto_sys_field: bool_opt(suppress_auto_sys_field),
        view,
        query_no_domain: bool_opt(query_no_domain),
    };
    let path = format!("/api/now/table/{}/{}", table, sys_id);
    let resp = match mutation {
        HttpMutation::Patch => client.patch(&path, &q.to_pairs(), &body)?,
        HttpMutation::Put => client.put(&path, &q.to_pairs(), &body)?,
    };
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
    let client = build_client(&profile, global.timeout)?;
    let q = DeleteQuery {
        query_no_domain: bool_opt(args.query_no_domain),
    };
    let path = format!("/api/now/table/{}/{}", args.table, args.sys_id);
    client.delete(&path, &q.to_pairs())
}
