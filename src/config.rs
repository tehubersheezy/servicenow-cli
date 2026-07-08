use crate::error::{Error, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Config {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_profile: Option<String>,
    #[serde(default)]
    pub profiles: BTreeMap<String, ProfileConfig>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfileConfig {
    pub instance: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_proxy: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub insecure: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ca_cert: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_ca_cert: Option<String>,
    /// How requests authenticate. Defaults to `basic` so pre-OAuth profiles
    /// deserialize unchanged.
    #[serde(default, skip_serializing_if = "AuthMethod::is_basic")]
    pub auth: AuthMethod,
    /// Non-secret OAuth settings (the client secret + tokens live in
    /// `credentials.toml`). Present only when `auth = "oauth"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth: Option<OAuthConfig>,
}

/// Which credential mechanism a profile authenticates with.
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
#[value(rename_all = "lowercase")]
pub enum AuthMethod {
    /// HTTP Basic with a stored username/password.
    #[default]
    Basic,
    /// OAuth 2.0 bearer token (Authorization Code for SSO, or Client Credentials).
    Oauth,
}

impl AuthMethod {
    pub fn is_basic(&self) -> bool {
        matches!(self, AuthMethod::Basic)
    }
}

/// OAuth 2.0 grant type. `authorization_code` is the interactive (SSO) flow;
/// `client_credentials` is the service-to-service flow.
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum OAuthGrant {
    #[default]
    AuthorizationCode,
    ClientCredentials,
}

impl OAuthGrant {
    fn is_default(&self) -> bool {
        matches!(self, OAuthGrant::AuthorizationCode)
    }
}

fn default_true() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

/// Non-secret OAuth configuration persisted in `config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OAuthConfig {
    pub client_id: String,
    /// Loopback redirect registered in ServiceNow's Application Registry.
    /// Defaults to `http://localhost:8400/callback`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_uri: Option<String>,
    /// Authorization endpoint path. Defaults to `/oauth_auth.do`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_path: Option<String>,
    /// Token endpoint path. Defaults to `/oauth_token.do`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_path: Option<String>,
    #[serde(default, skip_serializing_if = "OAuthGrant::is_default")]
    pub grant: OAuthGrant,
    /// Use PKCE (S256) for the authorization-code flow. Defaults to true.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub pkce: bool,
}

/// An OAuth token set, persisted in `credentials.toml` (chmod 0600 on Unix).
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenSet {
    pub access_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// Absolute Unix-seconds expiry of the access token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
}

impl TokenSet {
    /// True when the access token is within `skew_secs` of (or past) expiry.
    /// Tokens with an unknown expiry are treated as valid until a 401 forces a
    /// re-login.
    pub fn is_expired(&self, skew_secs: u64) -> bool {
        match self.expires_at {
            Some(exp) => now_unix().saturating_add(skew_secs) >= exp,
            None => false,
        }
    }
}

/// Current wall-clock time in Unix seconds.
pub fn now_unix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Default loopback redirect URI for the authorization-code flow.
pub fn default_redirect_uri() -> String {
    "http://localhost:8400/callback".to_string()
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Credentials {
    #[serde(default)]
    pub profiles: BTreeMap<String, ProfileCredentials>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfileCredentials {
    /// Empty for OAuth profiles (which have no stored password).
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_password: Option<String>,
    /// OAuth client secret (confidential clients). Public/PKCE clients omit it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    /// Cached OAuth tokens, refreshed transparently when stale.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_tokens: Option<TokenSet>,
}

/// Resolve the sn config directory.
///
/// If the `SN_CONFIG_DIR` environment variable is set to a non-empty value,
/// it is used as-is — this is the documented cross-platform override, and it
/// points **directly** at the directory containing `config.toml` and
/// `credentials.toml` (no `sn` subdirectory is appended). Otherwise the
/// platform-native location from `directories::ProjectDirs` is used.
pub fn config_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("SN_CONFIG_DIR") {
        if !dir.is_empty() {
            return Ok(PathBuf::from(dir));
        }
    }
    ProjectDirs::from("", "", "sn")
        .map(|pd| pd.config_dir().to_path_buf())
        .ok_or_else(|| Error::Config("cannot resolve home directory for config".into()))
}

pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.toml"))
}

pub fn credentials_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("credentials.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_end_in_expected_filenames() {
        let cfg = config_path().unwrap();
        assert_eq!(cfg.file_name().unwrap(), "config.toml");
        let cred = credentials_path().unwrap();
        assert_eq!(cred.file_name().unwrap(), "credentials.toml");
    }

    #[test]
    fn profiles_roundtrip_via_toml() {
        let mut profiles = BTreeMap::new();
        profiles.insert(
            "dev".into(),
            ProfileConfig {
                instance: "example.com".into(),
                ..Default::default()
            },
        );
        let cfg = Config {
            default_profile: Some("dev".into()),
            profiles,
        };
        let s = toml::to_string(&cfg).unwrap();
        let parsed: Config = toml::from_str(&s).unwrap();
        assert_eq!(parsed, cfg);
    }

    #[test]
    fn credentials_roundtrip_via_toml() {
        let mut profiles = BTreeMap::new();
        profiles.insert(
            "dev".into(),
            ProfileCredentials {
                username: "u".into(),
                password: "p".into(),
                ..Default::default()
            },
        );
        let cr = Credentials { profiles };
        let s = toml::to_string(&cr).unwrap();
        let parsed: Credentials = toml::from_str(&s).unwrap();
        assert_eq!(parsed, cr);
    }

    #[test]
    fn oauth_profile_config_roundtrips() {
        let mut profiles = BTreeMap::new();
        profiles.insert(
            "sso".into(),
            ProfileConfig {
                instance: "acme.service-now.com".into(),
                auth: AuthMethod::Oauth,
                oauth: Some(OAuthConfig {
                    client_id: "abc".into(),
                    redirect_uri: Some("http://localhost:8400/callback".into()),
                    auth_path: None,
                    token_path: None,
                    grant: OAuthGrant::AuthorizationCode,
                    pkce: true,
                }),
                ..Default::default()
            },
        );
        let cfg = Config {
            default_profile: Some("sso".into()),
            profiles,
        };
        let s = toml::to_string(&cfg).unwrap();
        let parsed: Config = toml::from_str(&s).unwrap();
        assert_eq!(parsed, cfg);
    }

    #[test]
    fn oauth_credentials_roundtrip_with_tokens() {
        let mut profiles = BTreeMap::new();
        profiles.insert(
            "sso".into(),
            ProfileCredentials {
                client_secret: Some("shh".into()),
                oauth_tokens: Some(TokenSet {
                    access_token: "AT".into(),
                    refresh_token: Some("RT".into()),
                    expires_at: Some(1_700_000_000),
                    token_type: Some("Bearer".into()),
                }),
                ..Default::default()
            },
        );
        let cr = Credentials { profiles };
        let s = toml::to_string(&cr).unwrap();
        let parsed: Credentials = toml::from_str(&s).unwrap();
        assert_eq!(parsed, cr);
    }

    #[test]
    fn legacy_basic_config_without_auth_field_still_parses() {
        // A pre-OAuth config.toml has no `auth`/`oauth` keys; it must deserialize
        // as a Basic profile unchanged.
        let toml = r#"
default_profile = "dev"
[profiles.dev]
instance = "dev.example.com"
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        let p = cfg.profiles.get("dev").unwrap();
        assert_eq!(p.auth, AuthMethod::Basic);
        assert!(p.oauth.is_none());
    }

    #[test]
    fn basic_profile_omits_auth_and_oauth_keys_when_serialized() {
        // Backward-compatibility: serializing a plain Basic profile must not
        // emit `auth = "basic"` or an empty `[oauth]` table.
        let mut profiles = BTreeMap::new();
        profiles.insert(
            "dev".into(),
            ProfileConfig {
                instance: "dev.example.com".into(),
                ..Default::default()
            },
        );
        let cfg = Config {
            default_profile: Some("dev".into()),
            profiles,
        };
        let s = toml::to_string(&cfg).unwrap();
        assert!(!s.contains("auth ="), "unexpected auth key:\n{s}");
        assert!(
            !s.contains("[profiles.dev.oauth]"),
            "unexpected oauth table:\n{s}"
        );
    }
}

pub fn load_config_from(path: &std::path::Path) -> Result<Config> {
    if !path.exists() {
        return Ok(Config::default());
    }
    let s = fs::read_to_string(path)
        .map_err(|e| Error::Config(format!("read {}: {e}", path.display())))?;
    toml::from_str(&s).map_err(|e| Error::Config(format!("parse {}: {e}", path.display())))
}

pub fn load_credentials_from(path: &std::path::Path) -> Result<Credentials> {
    if !path.exists() {
        return Ok(Credentials::default());
    }
    let s = fs::read_to_string(path)
        .map_err(|e| Error::Config(format!("read {}: {e}", path.display())))?;
    toml::from_str(&s).map_err(|e| Error::Config(format!("parse {}: {e}", path.display())))
}

pub fn save_config_to(path: &std::path::Path, cfg: &Config) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| Error::Config(format!("mkdir {}: {e}", parent.display())))?;
    }
    let s =
        toml::to_string_pretty(cfg).map_err(|e| Error::Config(format!("serialize config: {e}")))?;
    fs::write(path, s).map_err(|e| Error::Config(format!("write {}: {e}", path.display())))
}

pub fn save_credentials_to(path: &std::path::Path, cr: &Credentials) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| Error::Config(format!("mkdir {}: {e}", parent.display())))?;
    }
    let s = toml::to_string_pretty(cr)
        .map_err(|e| Error::Config(format!("serialize credentials: {e}")))?;
    fs::write(path, s).map_err(|e| Error::Config(format!("write {}: {e}", path.display())))?;
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(path)
            .map_err(|e| Error::Config(format!("stat {}: {e}", path.display())))?
            .permissions();
        perms.set_mode(0o600);
        fs::set_permissions(path, perms)
            .map_err(|e| Error::Config(format!("chmod {}: {e}", path.display())))?;
    }
    Ok(())
}

/// Resolved profile ready to make HTTP calls. Built by applying precedence:
/// CLI flag > env var > file value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedProfile {
    pub name: String,
    pub instance: String,
    pub username: String,
    pub password: String,
    pub proxy: Option<String>,
    pub no_proxy: Option<String>,
    pub insecure: bool,
    pub ca_cert: Option<String>,
    pub proxy_ca_cert: Option<String>,
    pub proxy_username: Option<String>,
    pub proxy_password: Option<String>,
    /// How this profile authenticates.
    pub auth_method: AuthMethod,
    /// OAuth state (config + cached tokens). `Some` iff `auth_method` is Oauth.
    pub oauth: Option<ResolvedOauth>,
}

/// Fully-resolved OAuth state for a single invocation: client config plus any
/// cached tokens. All endpoint/redirect defaults are already applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedOauth {
    pub client_id: String,
    pub client_secret: Option<String>,
    pub redirect_uri: String,
    pub auth_path: String,
    pub token_path: String,
    pub grant: OAuthGrant,
    pub pkce: bool,
    pub tokens: Option<TokenSet>,
}

pub struct ProfileResolverInputs<'a> {
    pub cli_profile: Option<&'a str>,
    pub cli_instance_override: Option<&'a str>,
    pub cli_username: Option<&'a str>,
    pub cli_password: Option<&'a str>,
    pub cli_proxy: Option<&'a str>,
    pub env_proxy: Option<&'a str>,
    pub cli_no_proxy: bool,
    pub env_no_proxy: Option<&'a str>,
    pub cli_insecure: bool,
    pub env_insecure: Option<&'a str>,
    pub cli_ca_cert: Option<&'a str>,
    pub env_ca_cert: Option<&'a str>,
    pub cli_proxy_ca_cert: Option<&'a str>,
    pub env_proxy_ca_cert: Option<&'a str>,
    pub config: &'a Config,
    pub credentials: &'a Credentials,
}

pub fn resolve_profile(inputs: ProfileResolverInputs<'_>) -> Result<ResolvedProfile> {
    let name = inputs
        .cli_profile
        .map(ToString::to_string)
        .or_else(|| inputs.config.default_profile.clone())
        .unwrap_or_else(|| "default".to_string());

    let profile_cfg = inputs.config.profiles.get(&name);
    let profile_cred = inputs.credentials.profiles.get(&name);

    let instance = inputs
        .cli_instance_override
        .map(ToString::to_string)
        .or_else(|| profile_cfg.map(|p| p.instance.clone()))
        .ok_or_else(|| {
            Error::Config(format!(
                "no instance configured for profile '{name}'; run `sn init` or pass --instance-override"
            ))
        })?;

    let auth_method = profile_cfg.map(|p| p.auth).unwrap_or_default();

    let username_opt = inputs
        .cli_username
        .map(ToString::to_string)
        .or_else(|| profile_cred.map(|p| p.username.clone()));
    let password_opt = inputs
        .cli_password
        .map(ToString::to_string)
        .or_else(|| profile_cred.map(|p| p.password.clone()));

    // Basic auth requires a username/password; OAuth profiles don't store them.
    let (username, password) = match auth_method {
        AuthMethod::Basic => (
            username_opt.ok_or_else(|| {
                Error::Config(format!(
                    "no username configured for profile '{name}'; run `sn init` or pass --username"
                ))
            })?,
            password_opt.ok_or_else(|| {
                Error::Config(format!(
                    "no password configured for profile '{name}'; run `sn init` or pass --password"
                ))
            })?,
        ),
        AuthMethod::Oauth => (
            username_opt.unwrap_or_default(),
            password_opt.unwrap_or_default(),
        ),
    };

    let oauth = match auth_method {
        AuthMethod::Basic => None,
        AuthMethod::Oauth => {
            let cfg = profile_cfg.and_then(|p| p.oauth.as_ref()).ok_or_else(|| {
                Error::Config(format!(
                    "profile '{name}' uses oauth but has no [oauth] config; run `sn auth login`"
                ))
            })?;
            Some(ResolvedOauth {
                client_id: cfg.client_id.clone(),
                client_secret: profile_cred.and_then(|c| c.client_secret.clone()),
                redirect_uri: cfg
                    .redirect_uri
                    .clone()
                    .unwrap_or_else(default_redirect_uri),
                auth_path: cfg
                    .auth_path
                    .clone()
                    .unwrap_or_else(|| "/oauth_auth.do".to_string()),
                token_path: cfg
                    .token_path
                    .clone()
                    .unwrap_or_else(|| "/oauth_token.do".to_string()),
                grant: cfg.grant,
                pkce: cfg.pkce,
                tokens: profile_cred.and_then(|c| c.oauth_tokens.clone()),
            })
        }
    };

    let proxy = if inputs.cli_no_proxy {
        None
    } else {
        inputs
            .cli_proxy
            .map(ToString::to_string)
            .or_else(|| inputs.env_proxy.map(ToString::to_string))
            .or_else(|| profile_cfg.and_then(|p| p.proxy.clone()))
    };

    let no_proxy = inputs
        .env_no_proxy
        .map(ToString::to_string)
        .or_else(|| profile_cfg.and_then(|p| p.no_proxy.clone()));

    let insecure = inputs.cli_insecure
        || inputs
            .env_insecure
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
        || profile_cfg.map(|p| p.insecure).unwrap_or(false);

    let ca_cert = inputs
        .cli_ca_cert
        .map(ToString::to_string)
        .or_else(|| inputs.env_ca_cert.map(ToString::to_string))
        .or_else(|| profile_cfg.and_then(|p| p.ca_cert.clone()));

    let proxy_ca_cert = inputs
        .cli_proxy_ca_cert
        .map(ToString::to_string)
        .or_else(|| inputs.env_proxy_ca_cert.map(ToString::to_string))
        .or_else(|| profile_cfg.and_then(|p| p.proxy_ca_cert.clone()));

    let proxy_username = profile_cred.and_then(|p| p.proxy_username.clone());
    let proxy_password = profile_cred.and_then(|p| p.proxy_password.clone());

    Ok(ResolvedProfile {
        name,
        instance,
        username,
        password,
        proxy,
        no_proxy,
        insecure,
        ca_cert,
        proxy_ca_cert,
        proxy_username,
        proxy_password,
        auth_method,
        oauth,
    })
}

/// Persist a refreshed/new OAuth token set for `profile_name` into
/// `credentials.toml`, leaving every other field untouched.
pub fn save_oauth_tokens(profile_name: &str, tokens: &TokenSet) -> Result<()> {
    let path = credentials_path()?;
    let mut creds = load_credentials_from(&path)?;
    creds
        .profiles
        .entry(profile_name.to_string())
        .or_default()
        .oauth_tokens = Some(tokens.clone());
    save_credentials_to(&path, &creds)
}

/// Drop any cached OAuth tokens for `profile_name` (used by `sn auth logout`).
pub fn clear_oauth_tokens(profile_name: &str) -> Result<()> {
    let path = credentials_path()?;
    let mut creds = load_credentials_from(&path)?;
    if let Some(p) = creds.profiles.get_mut(profile_name) {
        p.oauth_tokens = None;
    }
    save_credentials_to(&path, &creds)
}

#[cfg(test)]
mod resolution_tests {
    use super::*;

    fn sample_config() -> Config {
        let mut cfg = Config {
            default_profile: Some("dev".into()),
            ..Default::default()
        };
        cfg.profiles.insert(
            "dev".into(),
            ProfileConfig {
                instance: "dev.example.com".into(),
                ..Default::default()
            },
        );
        cfg.profiles.insert(
            "prod".into(),
            ProfileConfig {
                instance: "prod.example.com".into(),
                ..Default::default()
            },
        );
        cfg
    }

    fn sample_credentials() -> Credentials {
        let mut cr = Credentials::default();
        cr.profiles.insert(
            "dev".into(),
            ProfileCredentials {
                username: "dev-u".into(),
                password: "dev-p".into(),
                ..Default::default()
            },
        );
        cr.profiles.insert(
            "prod".into(),
            ProfileCredentials {
                username: "prod-u".into(),
                password: "prod-p".into(),
                ..Default::default()
            },
        );
        cr
    }

    fn base_inputs<'a>(cfg: &'a Config, cr: &'a Credentials) -> ProfileResolverInputs<'a> {
        ProfileResolverInputs {
            cli_profile: None,
            cli_instance_override: None,
            cli_username: None,
            cli_password: None,
            cli_proxy: None,
            env_proxy: None,
            cli_no_proxy: false,
            env_no_proxy: None,
            cli_insecure: false,
            env_insecure: None,
            cli_ca_cert: None,
            env_ca_cert: None,
            cli_proxy_ca_cert: None,
            env_proxy_ca_cert: None,
            config: cfg,
            credentials: cr,
        }
    }

    #[test]
    fn default_profile_when_none_specified() {
        let cfg = sample_config();
        let cr = sample_credentials();
        let r = resolve_profile(base_inputs(&cfg, &cr)).unwrap();
        assert_eq!(r.name, "dev");
        assert_eq!(r.instance, "dev.example.com");
    }

    #[test]
    fn cli_profile_wins_over_default() {
        let cfg = sample_config();
        let cr = sample_credentials();
        let r = resolve_profile(ProfileResolverInputs {
            cli_profile: Some("prod"),
            ..base_inputs(&cfg, &cr)
        })
        .unwrap();
        assert_eq!(r.name, "prod");
        assert_eq!(r.instance, "prod.example.com");
    }

    #[test]
    fn cli_overrides_per_field() {
        let cfg = sample_config();
        let cr = sample_credentials();
        let r = resolve_profile(ProfileResolverInputs {
            cli_instance_override: Some("override.example.com"),
            cli_username: Some("cli-u"),
            cli_password: Some("cli-p"),
            ..base_inputs(&cfg, &cr)
        })
        .unwrap();
        assert_eq!(r.instance, "override.example.com");
        assert_eq!(r.username, "cli-u");
        assert_eq!(r.password, "cli-p");
    }

    #[test]
    fn missing_instance_errors_clearly() {
        let cfg = Config::default();
        let cr = Credentials::default();
        let err = resolve_profile(ProfileResolverInputs {
            cli_username: Some("u"),
            cli_password: Some("p"),
            ..base_inputs(&cfg, &cr)
        })
        .unwrap_err();
        assert!(matches!(err, Error::Config(_)));
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let cfg_path = dir.path().join("config.toml");
        let cr_path = dir.path().join("credentials.toml");
        let cfg = sample_config();
        let cr = sample_credentials();
        save_config_to(&cfg_path, &cfg).unwrap();
        save_credentials_to(&cr_path, &cr).unwrap();
        assert_eq!(load_config_from(&cfg_path).unwrap(), cfg);
        assert_eq!(load_credentials_from(&cr_path).unwrap(), cr);
    }

    #[cfg(unix)]
    #[test]
    fn credentials_file_is_chmod_600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials.toml");
        save_credentials_to(&path, &sample_credentials()).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    // -----------------------------------------------------------------------
    // Contract regression tests: env vars must NEVER override credentials.
    //
    // Background: a user reported "profile switching is broken because env
    // vars override profiles." That report is FALSE for the current code, and
    // these tests lock the contract in so it can never silently regress.
    // `resolve_profile()` only consults env-derived inputs for proxy/TLS
    // settings — never for instance, username, password, or profile name.
    // -----------------------------------------------------------------------

    #[test]
    fn env_vars_never_leak_into_credentials() {
        // Structural guarantee: `ProfileResolverInputs` has no
        // `env_username` / `env_password` / `env_instance` field. There is
        // deliberately no env-driven path for credential or instance
        // selection — the only env-driven inputs are proxy/TLS related
        // (`env_proxy`, `env_no_proxy`, `env_insecure`, `env_ca_cert`,
        // `env_proxy_ca_cert`). This test verifies that even when those env
        // inputs are set to garbage, the resolved instance/username/password
        // come exclusively from the profile file.
        let cfg = sample_config();
        let cr = sample_credentials();
        let r = resolve_profile(ProfileResolverInputs {
            cli_profile: Some("dev"),
            env_proxy: Some("http://garbage.invalid:9999"),
            env_no_proxy: Some("garbage"),
            env_insecure: Some("1"),
            env_ca_cert: Some("/garbage/ca.pem"),
            env_proxy_ca_cert: Some("/garbage/proxy-ca.pem"),
            ..base_inputs(&cfg, &cr)
        })
        .unwrap();
        assert_eq!(r.name, "dev");
        assert_eq!(r.instance, "dev.example.com");
        assert_eq!(r.username, "dev-u");
        assert_eq!(r.password, "dev-p");
    }

    #[test]
    fn proxy_precedence_cli_beats_env_beats_profile() {
        let mut cfg = sample_config();
        cfg.profiles.get_mut("dev").unwrap().proxy = Some("http://profile.proxy:1".into());
        let cr = sample_credentials();

        // CLI > env > profile.
        let r = resolve_profile(ProfileResolverInputs {
            cli_proxy: Some("http://cli.proxy:3"),
            env_proxy: Some("http://env.proxy:2"),
            ..base_inputs(&cfg, &cr)
        })
        .unwrap();
        assert_eq!(r.proxy.as_deref(), Some("http://cli.proxy:3"));

        // Drop CLI: env wins.
        let r = resolve_profile(ProfileResolverInputs {
            env_proxy: Some("http://env.proxy:2"),
            ..base_inputs(&cfg, &cr)
        })
        .unwrap();
        assert_eq!(r.proxy.as_deref(), Some("http://env.proxy:2"));

        // Drop env: profile wins.
        let r = resolve_profile(base_inputs(&cfg, &cr)).unwrap();
        assert_eq!(r.proxy.as_deref(), Some("http://profile.proxy:1"));
    }

    #[test]
    fn no_proxy_flag_clears_all_proxies() {
        let mut cfg = sample_config();
        cfg.profiles.get_mut("dev").unwrap().proxy = Some("http://profile.proxy:1".into());
        let cr = sample_credentials();
        let r = resolve_profile(ProfileResolverInputs {
            cli_proxy: Some("http://cli.proxy:3"),
            env_proxy: Some("http://env.proxy:2"),
            cli_no_proxy: true,
            ..base_inputs(&cfg, &cr)
        })
        .unwrap();
        assert_eq!(r.proxy, None);
    }

    #[test]
    fn env_no_proxy_string_propagates() {
        let cfg = sample_config();
        let cr = sample_credentials();
        let r = resolve_profile(ProfileResolverInputs {
            env_no_proxy: Some("localhost,127.0.0.1"),
            ..base_inputs(&cfg, &cr)
        })
        .unwrap();
        assert_eq!(r.no_proxy.as_deref(), Some("localhost,127.0.0.1"));
    }

    #[test]
    fn env_insecure_recognizes_1_and_true() {
        let cfg = sample_config();
        let cr = sample_credentials();

        // "1" flips the flag.
        let r = resolve_profile(ProfileResolverInputs {
            env_insecure: Some("1"),
            ..base_inputs(&cfg, &cr)
        })
        .unwrap();
        assert!(r.insecure);

        // "true" (lowercase) flips the flag.
        let r = resolve_profile(ProfileResolverInputs {
            env_insecure: Some("true"),
            ..base_inputs(&cfg, &cr)
        })
        .unwrap();
        assert!(r.insecure);

        // "TRUE" (uppercase) flips the flag.
        let r = resolve_profile(ProfileResolverInputs {
            env_insecure: Some("TRUE"),
            ..base_inputs(&cfg, &cr)
        })
        .unwrap();
        assert!(r.insecure);

        // "0" does NOT flip the flag.
        let r = resolve_profile(ProfileResolverInputs {
            env_insecure: Some("0"),
            ..base_inputs(&cfg, &cr)
        })
        .unwrap();
        assert!(!r.insecure);

        // "false" does NOT flip the flag.
        let r = resolve_profile(ProfileResolverInputs {
            env_insecure: Some("false"),
            ..base_inputs(&cfg, &cr)
        })
        .unwrap();
        assert!(!r.insecure);

        // Arbitrary garbage does NOT flip the flag.
        let r = resolve_profile(ProfileResolverInputs {
            env_insecure: Some("yes-please"),
            ..base_inputs(&cfg, &cr)
        })
        .unwrap();
        assert!(!r.insecure);
    }

    #[test]
    fn cli_insecure_or_env_or_profile_ored() {
        let mut cfg = sample_config();
        let cr = sample_credentials();

        // CLI alone is enough.
        let r = resolve_profile(ProfileResolverInputs {
            cli_insecure: true,
            ..base_inputs(&cfg, &cr)
        })
        .unwrap();
        assert!(r.insecure);

        // Env alone is enough.
        let r = resolve_profile(ProfileResolverInputs {
            env_insecure: Some("1"),
            ..base_inputs(&cfg, &cr)
        })
        .unwrap();
        assert!(r.insecure);

        // Profile alone is enough.
        cfg.profiles.get_mut("dev").unwrap().insecure = true;
        let r = resolve_profile(base_inputs(&cfg, &cr)).unwrap();
        assert!(r.insecure);

        // All three false/unset → resolved is false.
        cfg.profiles.get_mut("dev").unwrap().insecure = false;
        let r = resolve_profile(base_inputs(&cfg, &cr)).unwrap();
        assert!(!r.insecure);
    }

    #[test]
    fn ca_cert_precedence_cli_beats_env_beats_profile() {
        let mut cfg = sample_config();
        cfg.profiles.get_mut("dev").unwrap().ca_cert = Some("/profile/ca.pem".into());
        let cr = sample_credentials();

        // CLI > env > profile.
        let r = resolve_profile(ProfileResolverInputs {
            cli_ca_cert: Some("/cli/ca.pem"),
            env_ca_cert: Some("/env/ca.pem"),
            ..base_inputs(&cfg, &cr)
        })
        .unwrap();
        assert_eq!(r.ca_cert.as_deref(), Some("/cli/ca.pem"));

        // Drop CLI: env wins.
        let r = resolve_profile(ProfileResolverInputs {
            env_ca_cert: Some("/env/ca.pem"),
            ..base_inputs(&cfg, &cr)
        })
        .unwrap();
        assert_eq!(r.ca_cert.as_deref(), Some("/env/ca.pem"));

        // Drop env: profile wins.
        let r = resolve_profile(base_inputs(&cfg, &cr)).unwrap();
        assert_eq!(r.ca_cert.as_deref(), Some("/profile/ca.pem"));
    }

    #[test]
    fn proxy_ca_cert_precedence_cli_beats_env_beats_profile() {
        let mut cfg = sample_config();
        cfg.profiles.get_mut("dev").unwrap().proxy_ca_cert = Some("/profile/proxy-ca.pem".into());
        let cr = sample_credentials();

        // CLI > env > profile.
        let r = resolve_profile(ProfileResolverInputs {
            cli_proxy_ca_cert: Some("/cli/proxy-ca.pem"),
            env_proxy_ca_cert: Some("/env/proxy-ca.pem"),
            ..base_inputs(&cfg, &cr)
        })
        .unwrap();
        assert_eq!(r.proxy_ca_cert.as_deref(), Some("/cli/proxy-ca.pem"));

        // Drop CLI: env wins.
        let r = resolve_profile(ProfileResolverInputs {
            env_proxy_ca_cert: Some("/env/proxy-ca.pem"),
            ..base_inputs(&cfg, &cr)
        })
        .unwrap();
        assert_eq!(r.proxy_ca_cert.as_deref(), Some("/env/proxy-ca.pem"));

        // Drop env: profile wins.
        let r = resolve_profile(base_inputs(&cfg, &cr)).unwrap();
        assert_eq!(r.proxy_ca_cert.as_deref(), Some("/profile/proxy-ca.pem"));
    }

    #[test]
    fn proxy_credentials_only_come_from_profile_file() {
        let cfg = sample_config();
        let mut cr = sample_credentials();
        let dev = cr.profiles.get_mut("dev").unwrap();
        dev.proxy_username = Some("proxy-user".into());
        dev.proxy_password = Some("proxy-pass".into());

        let r = resolve_profile(base_inputs(&cfg, &cr)).unwrap();
        assert_eq!(r.proxy_username.as_deref(), Some("proxy-user"));
        assert_eq!(r.proxy_password.as_deref(), Some("proxy-pass"));
    }

    #[test]
    fn unknown_profile_name_falls_through_to_missing_field_error() {
        let cfg = sample_config();
        let cr = sample_credentials();
        let err = resolve_profile(ProfileResolverInputs {
            cli_profile: Some("nonexistent"),
            ..base_inputs(&cfg, &cr)
        })
        .unwrap_err();
        match err {
            Error::Config(msg) => assert!(
                msg.contains("nonexistent"),
                "error message should name the profile, got: {msg}"
            ),
            other => panic!("expected Error::Config, got: {other:?}"),
        }
    }

    #[test]
    fn profile_default_string_when_no_default_profile_configured() {
        let cfg = Config::default();
        let cr = Credentials::default();
        let r = resolve_profile(ProfileResolverInputs {
            cli_instance_override: Some("override.example.com"),
            cli_username: Some("u"),
            cli_password: Some("p"),
            ..base_inputs(&cfg, &cr)
        })
        .unwrap();
        assert_eq!(r.name, "default");
    }

    #[test]
    fn cli_profile_with_instance_override_uses_override_not_profile_instance() {
        let cfg = sample_config();
        let cr = sample_credentials();
        let r = resolve_profile(ProfileResolverInputs {
            cli_profile: Some("dev"),
            cli_instance_override: Some("foo.com"),
            ..base_inputs(&cfg, &cr)
        })
        .unwrap();
        assert_eq!(r.name, "dev");
        assert_eq!(r.instance, "foo.com");
        // Credentials still come from the "dev" profile.
        assert_eq!(r.username, "dev-u");
        assert_eq!(r.password, "dev-p");
    }

    #[test]
    fn default_profile_in_config_used_when_no_cli_profile() {
        let mut cfg = sample_config();
        cfg.default_profile = Some("prod".into());
        let cr = sample_credentials();
        let r = resolve_profile(base_inputs(&cfg, &cr)).unwrap();
        assert_eq!(r.name, "prod");
        assert_eq!(r.instance, "prod.example.com");
    }

    #[test]
    fn oauth_profile_resolves_without_requiring_username_password() {
        let mut cfg = Config {
            default_profile: Some("sso".into()),
            ..Default::default()
        };
        cfg.profiles.insert(
            "sso".into(),
            ProfileConfig {
                instance: "sso.example.com".into(),
                auth: AuthMethod::Oauth,
                oauth: Some(OAuthConfig {
                    client_id: "abc".into(),
                    redirect_uri: None,
                    auth_path: None,
                    token_path: None,
                    grant: OAuthGrant::AuthorizationCode,
                    pkce: true,
                }),
                ..Default::default()
            },
        );
        let mut cr = Credentials::default();
        cr.profiles.insert(
            "sso".into(),
            ProfileCredentials {
                client_secret: Some("shh".into()),
                oauth_tokens: Some(TokenSet {
                    access_token: "AT".into(),
                    refresh_token: Some("RT".into()),
                    expires_at: Some(now_unix() + 3600),
                    token_type: Some("Bearer".into()),
                }),
                ..Default::default()
            },
        );
        let r = resolve_profile(base_inputs(&cfg, &cr)).unwrap();
        assert_eq!(r.name, "sso");
        assert_eq!(r.auth_method, AuthMethod::Oauth);
        let o = r.oauth.expect("resolved oauth state");
        assert_eq!(o.client_id, "abc");
        // Defaults applied for the unset endpoint/redirect fields.
        assert_eq!(o.redirect_uri, default_redirect_uri());
        assert_eq!(o.auth_path, "/oauth_auth.do");
        assert_eq!(o.token_path, "/oauth_token.do");
        assert_eq!(o.client_secret.as_deref(), Some("shh"));
        assert_eq!(o.tokens.unwrap().access_token, "AT");
    }

    #[test]
    fn oauth_profile_without_config_section_errors() {
        let mut cfg = Config::default();
        cfg.profiles.insert(
            "sso".into(),
            ProfileConfig {
                instance: "sso.example.com".into(),
                auth: AuthMethod::Oauth,
                oauth: None,
                ..Default::default()
            },
        );
        let cr = Credentials::default();
        let err = resolve_profile(ProfileResolverInputs {
            cli_profile: Some("sso"),
            ..base_inputs(&cfg, &cr)
        })
        .unwrap_err();
        assert!(matches!(err, Error::Config(_)));
    }

    #[test]
    fn cli_profile_overrides_default_profile_in_config() {
        let mut cfg = sample_config();
        cfg.default_profile = Some("prod".into());
        let cr = sample_credentials();
        let r = resolve_profile(ProfileResolverInputs {
            cli_profile: Some("dev"),
            ..base_inputs(&cfg, &cr)
        })
        .unwrap();
        assert_eq!(r.name, "dev");
        assert_eq!(r.instance, "dev.example.com");
    }
}
