use crate::config::{
    config_path, credentials_path, default_redirect_uri, load_config_from, load_credentials_from,
    resolve_profile_name, save_config_to, save_credentials_to, AuthMethod, OAuthGrant,
    ProfileConfig,
};
use crate::error::{Error, Result};
use crate::output::{emit_value, Format};
use clap::Subcommand;
use serde_json::json;
use std::io;

#[derive(Subcommand, Debug)]
pub enum ProfileSub {
    List,
    Show { name: Option<String> },
    Remove { name: String },
    Use { name: String },
}

pub fn run(sub: ProfileSub) -> Result<()> {
    match sub {
        ProfileSub::List => list(),
        ProfileSub::Show { name } => show(name),
        ProfileSub::Remove { name } => remove(name),
        ProfileSub::Use { name } => set_default(name),
    }
}

fn auth_str(method: AuthMethod) -> &'static str {
    match method {
        AuthMethod::Basic => "basic",
        AuthMethod::Oauth => "oauth",
    }
}

fn grant_str(g: OAuthGrant) -> &'static str {
    match g {
        OAuthGrant::AuthorizationCode => "authorization_code",
        OAuthGrant::ClientCredentials => "client_credentials",
    }
}

fn list() -> Result<()> {
    let cfg = load_config_from(&config_path()?)?;
    let profiles: Vec<serde_json::Value> = cfg
        .profiles
        .iter()
        .map(|(name, p)| {
            json!({
                "name": name,
                "instance": p.instance,
                "auth": auth_str(p.auth),
                "default": cfg.default_profile.as_deref() == Some(name.as_str()),
            })
        })
        .collect();
    emit_value(
        io::stdout().lock(),
        &json!(profiles),
        Format::Auto.resolve(),
    )
    .map_err(crate::output::map_stdout_err)
}

fn show(name: Option<String>) -> Result<()> {
    let cfg = load_config_from(&config_path()?)?;
    let name = resolve_profile_name(name.as_deref(), &cfg)?;
    let p: &ProfileConfig = cfg
        .profiles
        .get(&name)
        .ok_or_else(|| Error::Usage(format!("profile '{name}' not found")))?;
    let creds = load_credentials_from(&credentials_path()?)?;
    let cred = creds.profiles.get(&name);

    // Names, instances, and OAuth client config are fine to print; passwords,
    // client secrets, and token values are NEVER emitted here.
    let mut out = json!({
        "name": name,
        "instance": p.instance,
        "auth": auth_str(p.auth),
    });
    match p.auth {
        AuthMethod::Basic => {
            if let Some(c) = cred {
                out["username"] = json!(c.username);
            }
        }
        AuthMethod::Oauth => {
            if let Some(o) = &p.oauth {
                out["client_id"] = json!(o.client_id);
                out["grant"] = json!(grant_str(o.grant));
                out["redirect_uri"] =
                    json!(o.redirect_uri.clone().unwrap_or_else(default_redirect_uri));
                out["pkce"] = json!(o.pkce);
            }
            let tokens = cred.and_then(|c| c.oauth_tokens.as_ref());
            out["loggedIn"] = json!(tokens.is_some());
            if let Some(t) = tokens {
                out["hasRefreshToken"] = json!(t.refresh_token.is_some());
                if let Some(exp) = t.expires_at {
                    out["expiresAt"] = json!(exp);
                }
            }
        }
    }
    emit_value(io::stdout().lock(), &out, Format::Auto.resolve())
        .map_err(crate::output::map_stdout_err)
}

fn remove(name: String) -> Result<()> {
    let cfg_path = config_path()?;
    let cred_path = credentials_path()?;
    let mut cfg = load_config_from(&cfg_path)?;
    let mut creds = load_credentials_from(&cred_path)?;
    cfg.profiles.remove(&name);
    creds.profiles.remove(&name);
    if cfg.default_profile.as_deref() == Some(&name) {
        cfg.default_profile = None;
    }
    save_config_to(&cfg_path, &cfg)?;
    save_credentials_to(&cred_path, &creds)?;
    Ok(())
}

fn set_default(name: String) -> Result<()> {
    let cfg_path = config_path()?;
    let mut cfg = load_config_from(&cfg_path)?;
    if !cfg.profiles.contains_key(&name) {
        return Err(Error::Usage(format!("profile '{name}' not found")));
    }
    cfg.default_profile = Some(name);
    save_config_to(&cfg_path, &cfg)?;
    Ok(())
}
