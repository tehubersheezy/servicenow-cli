use crate::cli::auth::complete_oauth_login;
use crate::cli::table::{build_client, build_profile};
use crate::cli::GlobalFlags;
use crate::config::{
    config_path, credentials_path, default_redirect_uri, load_config_from, load_credentials_from,
    save_config_to, save_credentials_to, AuthMethod, OAuthConfig, OAuthGrant, ProfileConfig,
    ProfileCredentials,
};
use crate::error::{Error, Result};
use std::io::{self, Write};

#[derive(clap::Args, Debug)]
pub struct InitArgs {
    /// Profile name to create or update (default: "default").
    #[arg(long)]
    pub profile: Option<String>,
    /// Instance: short name (`dev380385`) or full URL.
    #[arg(long)]
    pub instance: Option<String>,
    /// Authentication method: `basic` (username/password) or `oauth` (SSO / OAuth 2.0).
    #[arg(long, value_enum)]
    pub auth: Option<AuthMethod>,
    /// Username (basic auth only).
    #[arg(long)]
    pub username: Option<String>,
    /// Password (basic auth only). Convenience flag; prefer the interactive
    /// prompt — `--password` is visible in `ps` output and shell history.
    #[arg(long)]
    pub password: Option<String>,
    /// OAuth client_id (oauth only).
    #[arg(long)]
    pub client_id: Option<String>,
    /// OAuth client secret (oauth confidential clients).
    #[arg(long)]
    pub client_secret: Option<String>,
    /// OAuth loopback redirect URI (oauth only). Defaults to http://localhost:8400/callback.
    #[arg(long, value_name = "URL")]
    pub redirect_uri: Option<String>,
    /// OAuth grant: authorization_code (SSO, default) or client_credentials.
    #[arg(long, value_enum)]
    pub grant: Option<OAuthGrant>,
    /// Disable PKCE for the authorization-code flow.
    #[arg(long)]
    pub no_pkce: bool,
}

pub fn run(global: &GlobalFlags, args: InitArgs) -> Result<()> {
    let profile_name = args
        .profile
        .clone()
        .unwrap_or_else(|| prompt("Profile name [default]: ", Some("default".into())))
        .trim()
        .to_string();

    let instance_input = match &args.instance {
        Some(v) => v.clone(),
        None => prompt(
            "Instance (e.g. 'dev380385' or 'https://acme.service-now.com'): ",
            None,
        ),
    };
    let instance = normalize_instance(&instance_input);
    if instance.trim().is_empty() {
        return Err(Error::Usage("instance is required".into()));
    }

    // Auth method: flag, else prompt (defaulting to basic).
    let auth_method = match args.auth {
        Some(m) => m,
        None => match prompt("Auth method (basic/oauth) [basic]: ", Some("basic".into()))
            .to_ascii_lowercase()
            .as_str()
        {
            "oauth" => AuthMethod::Oauth,
            "basic" => AuthMethod::Basic,
            other => {
                return Err(Error::Usage(format!(
                    "unknown auth method '{other}' (expected basic or oauth)"
                )))
            }
        },
    };

    // Re-running `sn init` over an existing profile is non-destructive: start
    // from the stored config/credentials and overwrite only what this run
    // configures. Fields init cannot set (proxy_username/proxy_password,
    // OAuth endpoint overrides) survive untouched.
    let cfg_path = config_path()?;
    let cred_path = credentials_path()?;
    let mut config = load_config_from(&cfg_path)?;
    let mut creds = load_credentials_from(&cred_path)?;

    let mut pc: ProfileConfig = config
        .profiles
        .get(&profile_name)
        .cloned()
        .unwrap_or_default();
    let mut cred: ProfileCredentials = creds
        .profiles
        .get(&profile_name)
        .cloned()
        .unwrap_or_default();

    pc.instance = instance.clone();

    // Proxy/TLS settings from global flags are persisted with the profile so
    // subsequent invocations pick them up automatically. Flags that were not
    // passed leave the stored values alone.
    if global.no_proxy {
        pc.proxy = None;
    } else if let Some(proxy) = &global.proxy {
        pc.proxy = Some(proxy.clone());
    }
    if global.insecure {
        pc.insecure = true;
    }
    if let Some(ca_cert) = &global.ca_cert {
        pc.ca_cert = Some(ca_cert.clone());
    }
    if let Some(proxy_ca_cert) = &global.proxy_ca_cert {
        pc.proxy_ca_cert = Some(proxy_ca_cert.clone());
    }
    // For the OAuth branch, the grant chosen below drives the post-save flow.
    let mut oauth_grant = OAuthGrant::default();

    match auth_method {
        AuthMethod::Basic => {
            let username = match &args.username {
                Some(v) => v.clone(),
                None => prompt("Username: ", None),
            };
            let password = match &args.password {
                Some(v) => v.clone(),
                None => rpassword::prompt_password("Password: ")
                    .map_err(|e| Error::Usage(format!("read password: {e}")))?,
            };
            if username.trim().is_empty() || password.is_empty() {
                return Err(Error::Usage(
                    "username and password are required for basic auth".into(),
                ));
            }
            // Switching an OAuth profile to basic: drop the OAuth config and
            // its secrets so the abandoned method can't leak or confuse.
            pc.auth = AuthMethod::Basic;
            pc.oauth = None;
            cred.client_secret = None;
            cred.oauth_tokens = None;
            cred.username = username;
            cred.password = password;
        }
        AuthMethod::Oauth => {
            let client_id = match &args.client_id {
                Some(v) => v.clone(),
                None => prompt("OAuth client_id: ", None),
            };
            if client_id.trim().is_empty() {
                return Err(Error::Usage("client_id is required for oauth".into()));
            }
            oauth_grant = args.grant.unwrap_or_default();

            // Secret directly follows the client_id it belongs to: required for
            // client_credentials, optional (public/PKCE) for authorization_code.
            let secret = match &args.client_secret {
                Some(s) => Some(s.clone()),
                None => {
                    let label = if matches!(oauth_grant, OAuthGrant::ClientCredentials) {
                        "OAuth client_secret: "
                    } else {
                        "OAuth client_secret (blank for public/PKCE client): "
                    };
                    let s = rpassword::prompt_password(label)
                        .map_err(|e| Error::Usage(format!("read secret: {e}")))?;
                    if s.is_empty() {
                        None
                    } else {
                        Some(s)
                    }
                }
            };
            if matches!(oauth_grant, OAuthGrant::ClientCredentials) && secret.is_none() {
                return Err(Error::Usage(
                    "client_credentials grant requires a client secret".into(),
                ));
            }

            // The loopback redirect only exists in the browser flow; don't ask
            // for one under client_credentials.
            let redirect_uri = if matches!(oauth_grant, OAuthGrant::ClientCredentials) {
                args.redirect_uri.clone()
            } else {
                args.redirect_uri.clone().or_else(|| {
                    let d = default_redirect_uri();
                    Some(prompt(&format!("Redirect URI [{d}]: "), Some(d)))
                })
            };

            // Preserve endpoint overrides init cannot configure; overwrite
            // everything the flow above collected. Switching a basic profile
            // to oauth clears the now-unused password.
            let existing_oauth = pc.oauth.take();
            pc.auth = AuthMethod::Oauth;
            pc.oauth = Some(OAuthConfig {
                client_id,
                redirect_uri,
                auth_path: existing_oauth.as_ref().and_then(|o| o.auth_path.clone()),
                token_path: existing_oauth.as_ref().and_then(|o| o.token_path.clone()),
                grant: oauth_grant,
                pkce: !args.no_pkce,
            });
            cred.password = String::new();
            cred.client_secret = secret;
        }
    }

    if config.default_profile.is_none() {
        config.default_profile = Some(profile_name.clone());
    }
    config.profiles.insert(profile_name.clone(), pc);
    creds.profiles.insert(profile_name.clone(), cred);
    save_config_to(&cfg_path, &config)?;
    save_credentials_to(&cred_path, &creds)?;

    // Verify (and, for OAuth, run the login flow).
    match auth_method {
        AuthMethod::Basic => {
            let mut scoped = global.clone();
            scoped.profile = Some(profile_name.clone());
            let profile = build_profile(&scoped)?;
            let client = build_client(&profile, scoped.timeout)?;
            client.get(
                "/api/now/table/sys_user",
                &[("sysparm_limit".into(), "1".into())],
            )?;
            eprintln!("profile '{profile_name}' saved and verified ({instance}).");
        }
        AuthMethod::Oauth => {
            let (_, user) = complete_oauth_login(global, &profile_name, oauth_grant)?;
            let who = user.unwrap_or_else(|| "(unknown)".into());
            eprintln!(
                "profile '{profile_name}' saved and authenticated via oauth ({instance}, user {who})."
            );
        }
    }
    Ok(())
}

fn prompt(msg: &str, default: Option<String>) -> String {
    print!("{msg}");
    io::stdout().flush().ok();
    let mut s = String::new();
    io::stdin().read_line(&mut s).ok();
    let trimmed = s.trim().to_string();
    if trimmed.is_empty() {
        default.unwrap_or_default()
    } else {
        trimmed
    }
}

/// Turn whatever the user typed into something `client.rs::normalize_base_url`
/// can use. A short instance name like `dev380385` becomes `dev380385.service-now.com`;
/// anything that already looks like a URL or FQDN is passed through untouched
/// (modulo a trailing slash).
pub(crate) fn normalize_instance(input: &str) -> String {
    let trimmed = input.trim().trim_end_matches('/');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") || trimmed.contains('.') {
        trimmed.to_string()
    } else {
        format!("{trimmed}.service-now.com")
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_instance;

    #[test]
    fn short_name_gets_service_now_suffix() {
        assert_eq!(normalize_instance("dev380385"), "dev380385.service-now.com");
    }

    #[test]
    fn fqdn_passes_through() {
        assert_eq!(
            normalize_instance("acme.service-now.com"),
            "acme.service-now.com"
        );
    }

    #[test]
    fn full_url_passes_through() {
        assert_eq!(
            normalize_instance("https://dev380385.service-now.com"),
            "https://dev380385.service-now.com"
        );
    }

    #[test]
    fn http_url_passes_through() {
        assert_eq!(
            normalize_instance("http://localhost:8080"),
            "http://localhost:8080"
        );
    }

    #[test]
    fn trailing_slash_stripped() {
        assert_eq!(
            normalize_instance("dev380385/"),
            "dev380385.service-now.com"
        );
        assert_eq!(
            normalize_instance("https://dev380385.service-now.com/"),
            "https://dev380385.service-now.com"
        );
    }

    #[test]
    fn whitespace_trimmed() {
        assert_eq!(
            normalize_instance("  dev380385 \n"),
            "dev380385.service-now.com"
        );
    }
}
