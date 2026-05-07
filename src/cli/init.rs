use crate::cli::GlobalFlags;
use crate::client::Client;
use crate::config::{
    config_path, credentials_path, load_config_from, load_credentials_from, save_config_to,
    save_credentials_to, ProfileConfig, ProfileCredentials, ResolvedProfile,
};
use crate::error::{Error, Result};
use std::io::{self, Write};
use std::time::Duration;

#[derive(clap::Args, Debug)]
pub struct InitArgs {
    #[arg(long)]
    pub profile: Option<String>,
    #[arg(long)]
    pub instance: Option<String>,
    #[arg(long)]
    pub username: Option<String>,
    /// Convenience flag; prefer the interactive prompt — `--password` is visible
    /// in `ps` output and shell history.
    #[arg(long)]
    pub password: Option<String>,
}

pub fn run(global: &GlobalFlags, args: InitArgs) -> Result<()> {
    let profile_name = args
        .profile
        .unwrap_or_else(|| prompt("Profile name [default]: ", Some("default".into())))
        .trim()
        .to_string();

    let instance_input = match args.instance {
        Some(v) => v,
        None => prompt(
            "Instance (e.g. 'dev380385' or 'https://acme.service-now.com'): ",
            None,
        ),
    };
    let instance = normalize_instance(&instance_input);
    let username = match args.username {
        Some(v) => v,
        None => prompt("Username: ", None),
    };
    let password = match args.password {
        Some(v) => v,
        None => rpassword::prompt_password("Password: ")
            .map_err(|e| Error::Usage(format!("read password: {e}")))?,
    };

    if instance.trim().is_empty() || username.trim().is_empty() || password.is_empty() {
        return Err(Error::Usage(
            "instance, username, and password are required".into(),
        ));
    }

    // Proxy/TLS settings from global flags are persisted with the profile so
    // subsequent invocations pick them up automatically.
    let proxy = if global.no_proxy {
        None
    } else {
        global.proxy.clone()
    };
    let insecure = global.insecure;
    let ca_cert = global.ca_cert.clone();
    let proxy_ca_cert = global.proxy_ca_cert.clone();

    let cfg_path = config_path()?;
    let cred_path = credentials_path()?;
    let mut config = load_config_from(&cfg_path)?;
    let mut creds = load_credentials_from(&cred_path)?;

    if config.default_profile.is_none() {
        config.default_profile = Some(profile_name.clone());
    }
    config.profiles.insert(
        profile_name.clone(),
        ProfileConfig {
            instance: instance.clone(),
            proxy: proxy.clone(),
            insecure,
            ca_cert: ca_cert.clone(),
            proxy_ca_cert: proxy_ca_cert.clone(),
            ..Default::default()
        },
    );
    creds.profiles.insert(
        profile_name.clone(),
        ProfileCredentials {
            username: username.clone(),
            password: password.clone(),
            ..Default::default()
        },
    );

    save_config_to(&cfg_path, &config)?;
    save_credentials_to(&cred_path, &creds)?;

    let profile = ResolvedProfile {
        name: profile_name.clone(),
        instance,
        username,
        password,
        proxy,
        no_proxy: None,
        insecure,
        ca_cert,
        proxy_ca_cert,
        proxy_username: None,
        proxy_password: None,
    };
    let mut builder = Client::builder()
        .proxy(profile.proxy.clone())
        .insecure(profile.insecure)
        .ca_cert(profile.ca_cert.clone())
        .proxy_ca_cert(profile.proxy_ca_cert.clone());
    if let Some(secs) = global.timeout {
        builder = builder.timeout(Duration::from_secs(secs));
    }
    let client = builder.build(&profile)?;
    client.get(
        "/api/now/table/sys_user",
        &[("sysparm_limit".into(), "1".into())],
    )?;

    eprintln!(
        "profile '{profile_name}' saved and verified ({}).",
        profile.instance
    );
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
fn normalize_instance(input: &str) -> String {
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
