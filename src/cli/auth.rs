use crate::cli::init::normalize_instance;
use crate::cli::table::{build_client, build_profile};
use crate::cli::GlobalFlags;
use crate::config::{
    clear_oauth_tokens, config_path, credentials_path, load_config_from, load_credentials_from,
    now_unix, save_config_to, save_credentials_to, save_oauth_tokens, AuthMethod, Config,
    OAuthConfig, OAuthGrant, ResolvedProfile,
};
use crate::error::{Error, Result};
use crate::oauth;
use crate::output::{emit_value, map_stdout_err, Format};
use clap::{Args, Subcommand};
use serde_json::json;
use std::io::{self, Write};

#[derive(Subcommand, Debug)]
pub enum AuthSub {
    /// Verify credentials by calling /api/now/table/sys_user?sysparm_limit=1.
    Test,
    /// Authenticate via OAuth 2.0 (browser SSO or client-credentials) and cache tokens.
    Login(LoginArgs),
    /// Discard the profile's cached OAuth tokens.
    Logout,
    /// Show the resolved auth method and OAuth token status for a profile.
    Status,
    /// Force an OAuth token refresh now.
    Refresh,
}

#[derive(Args, Debug)]
pub struct LoginArgs {
    /// OAuth client_id from the ServiceNow Application Registry.
    #[arg(long)]
    pub client_id: Option<String>,
    /// OAuth client secret (confidential clients). Prompted when required and absent.
    #[arg(long)]
    pub client_secret: Option<String>,
    /// Loopback redirect URI; must match the Application Registry exactly.
    /// Defaults to http://localhost:8400/callback.
    #[arg(long, value_name = "URL")]
    pub redirect_uri: Option<String>,
    /// OAuth scope (e.g. useraccount).
    #[arg(long)]
    pub scope: Option<String>,
    /// Grant type: authorization_code (SSO, default) or client_credentials.
    #[arg(long, value_enum)]
    pub grant: Option<OAuthGrant>,
    /// Disable PKCE for the authorization-code flow.
    #[arg(long)]
    pub no_pkce: bool,
    /// Set or override the profile's instance.
    #[arg(long)]
    pub instance: Option<String>,
}

pub fn test(global: &GlobalFlags) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let v = client.get(
        "/api/now/table/sys_user",
        &[("sysparm_limit".into(), "1".into())],
    )?;
    let user = v["result"]
        .get(0)
        .and_then(|r| r.get("user_name"))
        .and_then(|x| x.as_str())
        .unwrap_or(&profile.username);
    let msg = json!({
        "ok": true,
        "instance": profile.instance,
        "username": user,
        "profile": profile.name,
    });
    writeln!(std::io::stderr(), "{msg}").ok();
    Ok(())
}

fn grant_str(g: OAuthGrant) -> &'static str {
    match g {
        OAuthGrant::AuthorizationCode => "authorization_code",
        OAuthGrant::ClientCredentials => "client_credentials",
    }
}

/// Resolve which profile name an auth command targets, mirroring
/// `resolve_profile`'s precedence (CLI flag > default_profile > "default").
fn resolve_name(global: &GlobalFlags, config: &Config) -> String {
    global
        .profile
        .clone()
        .or_else(|| config.default_profile.clone())
        .unwrap_or_else(|| "default".to_string())
}

fn prompt(msg: &str) -> Result<String> {
    print!("{msg}");
    io::stdout().flush().ok();
    let mut s = String::new();
    io::stdin()
        .read_line(&mut s)
        .map_err(|e| Error::Usage(format!("read input: {e}")))?;
    Ok(s.trim().to_string())
}

pub fn login(global: &GlobalFlags, args: LoginArgs) -> Result<()> {
    let cfg_path = config_path()?;
    let cred_path = credentials_path()?;
    let mut config = load_config_from(&cfg_path)?;
    let mut creds = load_credentials_from(&cred_path)?;

    let name = resolve_name(global, &config);
    let mut pc = config.profiles.get(&name).cloned().unwrap_or_default();
    let existing = pc.oauth.clone();

    if let Some(inst) = &args.instance {
        pc.instance = normalize_instance(inst);
    }
    if pc.instance.trim().is_empty() && global.instance_override.is_none() {
        return Err(Error::Usage(format!(
            "profile '{name}' has no instance configured; pass --instance"
        )));
    }

    // Merge flags over any previously-stored OAuth config.
    let client_id = match args
        .client_id
        .or_else(|| existing.as_ref().map(|e| e.client_id.clone()))
        .filter(|s| !s.is_empty())
    {
        Some(c) => c,
        None => {
            let c = prompt("OAuth client_id: ")?;
            if c.is_empty() {
                return Err(Error::Usage("client_id is required".into()));
            }
            c
        }
    };
    let grant = args
        .grant
        .or_else(|| existing.as_ref().map(|e| e.grant))
        .unwrap_or_default();
    let pkce = if args.no_pkce {
        false
    } else {
        existing.as_ref().map(|e| e.pkce).unwrap_or(true)
    };
    let oc = OAuthConfig {
        client_id,
        redirect_uri: args
            .redirect_uri
            .or_else(|| existing.as_ref().and_then(|e| e.redirect_uri.clone())),
        scope: args
            .scope
            .or_else(|| existing.as_ref().and_then(|e| e.scope.clone())),
        auth_path: existing.as_ref().and_then(|e| e.auth_path.clone()),
        token_path: existing.as_ref().and_then(|e| e.token_path.clone()),
        grant,
        pkce,
    };
    pc.auth = AuthMethod::Oauth;
    pc.oauth = Some(oc);
    config.profiles.insert(name.clone(), pc);
    if config.default_profile.is_none() {
        config.default_profile = Some(name.clone());
    }

    // Persist the client secret (and prompt for it when client_credentials
    // needs one but none was supplied or stored).
    {
        let cred = creds.profiles.entry(name.clone()).or_default();
        if let Some(secret) = args.client_secret {
            cred.client_secret = Some(secret);
        }
        if matches!(grant, OAuthGrant::ClientCredentials) && cred.client_secret.is_none() {
            let s = rpassword::prompt_password("OAuth client_secret: ")
                .map_err(|e| Error::Usage(format!("read secret: {e}")))?;
            cred.client_secret = Some(s);
        }
    }

    save_config_to(&cfg_path, &config)?;
    save_credentials_to(&cred_path, &creds)?;

    // Resolve the now-persisted profile and run the flow.
    let (profile, user) = complete_oauth_login(global, &name, grant)?;
    let msg = json!({
        "ok": true,
        "profile": name,
        "instance": profile.instance,
        "auth": "oauth",
        "grant": grant_str(grant),
        "user": user,
    });
    writeln!(std::io::stderr(), "{msg}").ok();
    Ok(())
}

/// Run the OAuth flow for an already-persisted profile named `name`, cache the
/// resulting tokens, and verify them against the instance. Returns the resolved
/// profile and the authenticated user_name (if the verify call surfaced one).
/// Shared by `sn auth login` and `sn init`'s OAuth branch.
pub(crate) fn complete_oauth_login(
    global: &GlobalFlags,
    name: &str,
    grant: OAuthGrant,
) -> Result<(ResolvedProfile, Option<String>)> {
    // Scope resolution to this specific profile, independent of which profile
    // happens to be the global default.
    let mut scoped = global.clone();
    scoped.profile = Some(name.to_string());

    let profile = build_profile(&scoped)?;
    let tokens = match grant {
        OAuthGrant::AuthorizationCode => oauth::login_authorization_code(&profile, scoped.timeout)?,
        OAuthGrant::ClientCredentials => {
            let client = oauth::build_token_client(&profile, scoped.timeout)?;
            let o = profile
                .oauth
                .as_ref()
                .ok_or_else(|| Error::Config("oauth config missing after save".into()))?;
            oauth::client_credentials(&client, o)?
        }
    };
    save_oauth_tokens(name, &tokens)?;

    // Verify the freshly-issued token against the instance.
    let profile = build_profile(&scoped)?;
    let client = build_client(&profile, scoped.timeout)?;
    let v = client.get(
        "/api/now/table/sys_user",
        &[("sysparm_limit".into(), "1".into())],
    )?;
    let user = v["result"]
        .get(0)
        .and_then(|r| r.get("user_name"))
        .and_then(|x| x.as_str())
        .map(ToString::to_string);
    Ok((profile, user))
}

pub fn logout(global: &GlobalFlags) -> Result<()> {
    let config = load_config_from(&config_path()?)?;
    let name = resolve_name(global, &config);
    clear_oauth_tokens(&name)?;
    writeln!(
        std::io::stderr(),
        "{}",
        json!({"ok": true, "profile": name, "loggedOut": true})
    )
    .ok();
    Ok(())
}

pub fn status(global: &GlobalFlags) -> Result<()> {
    let profile = build_profile(global)?;
    let mut out = json!({
        "profile": profile.name,
        "instance": profile.instance,
        "auth": if matches!(profile.auth_method, AuthMethod::Oauth) { "oauth" } else { "basic" },
    });
    match profile.auth_method {
        AuthMethod::Basic => {
            out["username"] = json!(profile.username);
        }
        AuthMethod::Oauth => {
            if let Some(o) = &profile.oauth {
                out["grant"] = json!(grant_str(o.grant));
                out["clientId"] = json!(o.client_id);
                out["redirectUri"] = json!(o.redirect_uri);
                out["loggedIn"] = json!(o.tokens.is_some());
                if let Some(t) = &o.tokens {
                    out["hasRefreshToken"] = json!(t.refresh_token.is_some());
                    if let Some(exp) = t.expires_at {
                        out["expiresAt"] = json!(exp);
                        out["expiresInSecs"] = json!(exp as i64 - now_unix() as i64);
                        out["expired"] = json!(t.is_expired(0));
                    }
                }
            }
        }
    }
    emit_value(io::stdout().lock(), &out, Format::Auto.resolve()).map_err(map_stdout_err)
}

pub fn refresh(global: &GlobalFlags) -> Result<()> {
    let profile = build_profile(global)?;
    if !matches!(profile.auth_method, AuthMethod::Oauth) {
        return Err(Error::Usage(format!(
            "profile '{}' does not use oauth",
            profile.name
        )));
    }
    let tokens = oauth::force_refresh(&profile, global.timeout)?;
    let msg = json!({
        "ok": true,
        "profile": profile.name,
        "refreshed": true,
        "expiresAt": tokens.expires_at,
    });
    writeln!(std::io::stderr(), "{msg}").ok();
    Ok(())
}
