use crate::cli::table::{build_client, build_profile};
use crate::cli::GlobalFlags;
use crate::config::{
    clear_oauth_tokens, config_path, load_config_from, now_unix, resolve_profile_name,
    save_oauth_tokens, AuthMethod, OAuthGrant, ResolvedProfile,
};
use crate::error::{Error, Result};
use crate::oauth;
use crate::output::{emit_value, map_stdout_err, Format};
use clap::Subcommand;
use serde_json::json;
use std::io;

#[derive(Subcommand, Debug)]
pub enum AuthSub {
    /// Run the OAuth flow for the selected (already-configured) profile and cache tokens.
    Login,
    /// Discard the profile's cached OAuth tokens.
    Logout,
    /// Show the resolved auth method and OAuth token status for a profile.
    Status,
    /// Force an OAuth token refresh now.
    Refresh,
}

fn grant_str(g: OAuthGrant) -> &'static str {
    match g {
        OAuthGrant::AuthorizationCode => "authorization_code",
        OAuthGrant::ClientCredentials => "client_credentials",
    }
}

/// Pure session command: run the OAuth flow for an already-configured OAuth
/// profile and cache the tokens. All configuration lives in `sn init`.
pub fn login(global: &GlobalFlags) -> Result<()> {
    let config = load_config_from(&config_path()?)?;
    let name = resolve_profile_name(global.profile.as_deref(), &config)?;
    let grant = match config.profiles.get(&name) {
        Some(pc) if matches!(pc.auth, AuthMethod::Oauth) && pc.oauth.is_some() => {
            pc.oauth.as_ref().map(|o| o.grant).unwrap_or_default()
        }
        _ => {
            return Err(Error::Usage(format!(
                "profile '{name}' does not use oauth; run `sn init`"
            )))
        }
    };

    let (profile, user) = complete_oauth_login(global, &name, grant)?;
    let out = json!({
        "ok": true,
        "profile": name,
        "instance": profile.instance,
        "auth": "oauth",
        "grant": grant_str(grant),
        "user": user,
    });
    emit_value(io::stdout().lock(), &out, Format::Auto.resolve()).map_err(map_stdout_err)
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
    let name = resolve_profile_name(global.profile.as_deref(), &config)?;
    clear_oauth_tokens(&name)?;
    let out = json!({"ok": true, "profile": name, "loggedOut": true});
    emit_value(io::stdout().lock(), &out, Format::Auto.resolve()).map_err(map_stdout_err)
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
    let out = json!({
        "ok": true,
        "profile": profile.name,
        "refreshed": true,
        "expiresAt": tokens.expires_at,
    });
    emit_value(io::stdout().lock(), &out, Format::Auto.resolve()).map_err(map_stdout_err)
}
