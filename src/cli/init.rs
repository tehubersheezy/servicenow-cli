use crate::cli::InitArgs;
use crate::client::{Client, RetryPolicy};
use crate::config::{
    config_path, credentials_path, load_config_from, load_credentials_from, save_config_to,
    save_credentials_to, ProfileConfig, ProfileCredentials, ResolvedProfile,
};
use crate::error::{Error, Result};
use std::io::{self, Write};

pub fn run(args: InitArgs) -> Result<()> {
    let profile_name = args
        .profile
        .unwrap_or_else(|| prompt("Profile name [default]: ", Some("default".into())))
        .trim()
        .to_string();

    let instance = match args.instance {
        Some(v) => v,
        None => prompt("Instance (e.g. acme.service-now.com): ", None),
    };
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
        },
    );
    creds.profiles.insert(
        profile_name.clone(),
        ProfileCredentials {
            username: username.clone(),
            password: password.clone(),
        },
    );

    save_config_to(&cfg_path, &config)?;
    save_credentials_to(&cred_path, &creds)?;

    let profile = ResolvedProfile {
        name: profile_name.clone(),
        instance,
        username,
        password,
    };
    let client = Client::builder()
        .retry(RetryPolicy::default())
        .build(&profile)?;
    client.get(
        "/api/now/table/sys_user",
        &[("sysparm_limit".into(), "1".into())],
    )?;

    eprintln!("profile '{profile_name}' saved and verified.");
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
