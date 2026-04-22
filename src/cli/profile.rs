use crate::cli::ProfileSub;
use crate::config::{
    config_path, credentials_path, load_config_from, load_credentials_from, save_config_to,
    save_credentials_to, ProfileConfig,
};
use crate::error::{Error, Result};
use crate::output::{emit_value, Format};
use serde_json::json;
use std::io;

pub fn run(sub: ProfileSub) -> Result<()> {
    match sub {
        ProfileSub::List => list(),
        ProfileSub::Show { name } => show(name),
        ProfileSub::Remove { name } => remove(name),
        ProfileSub::Use { name } => set_default(name),
    }
}

fn list() -> Result<()> {
    let cfg = load_config_from(&config_path()?)?;
    let names: Vec<&String> = cfg.profiles.keys().collect();
    emit_value(io::stdout().lock(), &json!(names), Format::Auto.resolve())
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

fn show(name: Option<String>) -> Result<()> {
    let cfg = load_config_from(&config_path()?)?;
    let name = name
        .or_else(|| cfg.default_profile.clone())
        .ok_or_else(|| Error::Usage("no profile name and no default_profile configured".into()))?;
    let p: &ProfileConfig = cfg
        .profiles
        .get(&name)
        .ok_or_else(|| Error::Usage(format!("profile '{name}' not found")))?;
    emit_value(
        io::stdout().lock(),
        &json!({"name": name, "instance": p.instance}),
        Format::Auto.resolve(),
    )
    .map_err(|e| Error::Usage(format!("stdout: {e}")))
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
