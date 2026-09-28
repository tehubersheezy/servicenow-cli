//! `sn cache` — the offline schema index (see [`crate::schema_cache`]).
//!
//! `refresh` is the only verb that touches the network. `status`, `tables` and
//! `columns` read the file and nothing else, which is also all dynamic shell
//! completion does.

use crate::cli::GlobalFlags;
use crate::cli::kernel::{build_client, build_profile, write_response};
use crate::config::{config_path, load_config_from, now_unix, resolve_profile_name};
use crate::error::{Error, Result};
use crate::schema_cache::{self, SchemaIndex};
use clap::Subcommand;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::Instant;

/// Per-request timeout for `refresh` when `--timeout` is not given. The
/// aggregate queries behind it take seconds each on a quiet instance and tens
/// of seconds on a busy one — the global 30s default would fail them on
/// exactly the instances where a cache is most worth having.
const REFRESH_TIMEOUT_SECS: u64 = 120;

#[derive(Subcommand, Debug)]
pub enum CacheSub {
    /// Build (or rebuild) the offline schema index for the profile's instance:
    /// every table, its parent, and its columns. Needs sys_dictionary read
    /// (admin); other profiles get a tables-only index. Requests default to a
    /// 120s timeout here.
    Refresh,
    /// Show where the index lives, when it was built and what it holds. Offline.
    Status,
    /// List table names from the index, optionally only those starting with PREFIX. Offline.
    Tables(CacheTablesArgs),
    /// List a table's columns from the index — its own plus every inherited one. Offline.
    Columns(CacheColumnsArgs),
}

#[derive(clap::Args, Debug)]
pub struct CacheTablesArgs {
    /// Only tables whose name starts with this (e.g. `cmdb_ci_`).
    pub prefix: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct CacheColumnsArgs {
    /// Table name (e.g. `incident`).
    pub table: String,
}

/// The selected profile's instance, read from `config.toml` alone — no
/// credentials, no OAuth refresh, no network. The offline verbs and the shell
/// completers need only the cache key.
pub(crate) fn profile_instance(profile: Option<&str>) -> Result<String> {
    let config = load_config_from(&config_path()?)?;
    let name = resolve_profile_name(profile, &config)?;
    config
        .profiles
        .get(&name)
        .map(|p| p.instance.clone())
        .filter(|i| !i.trim().is_empty())
        .ok_or_else(|| {
            Error::Config(format!(
                "no instance configured for profile '{name}'; run `sn init`"
            ))
        })
}

pub fn refresh(global: &GlobalFlags) -> Result<()> {
    let profile = build_profile(global)?;
    let timeout = global.timeout.unwrap_or(REFRESH_TIMEOUT_SECS);
    let client = build_client(&profile, Some(timeout))?;
    let started = Instant::now();
    let (index, stats) = schema_cache::build(&client, &profile.instance, now_unix())?;
    let path = schema_cache::cache_path(&profile.instance)?;
    schema_cache::save(&path, &index)?;
    if !index.columns_indexed {
        eprintln!(
            "sn: warning: profile '{}' cannot read sys_dictionary (403); indexed table names only. \
             Column lookups and completion need a profile with admin",
            profile.name
        );
    }
    let mut report = summary(&index, &path);
    if let Some(fields) = report.as_object_mut() {
        // Just built: its age is the build's own duration, reported below.
        fields.remove("age_secs");
        fields.remove("exists");
    }
    report["requests"] = json!(stats.requests);
    report["elapsed_ms"] = json!(started.elapsed().as_millis() as u64);
    write_response(global, &report)
}

pub fn status(global: &GlobalFlags) -> Result<()> {
    let instance = profile_instance(global.profile.as_deref())?;
    let path = schema_cache::cache_path(&instance)?;
    let report = match schema_cache::load(&path)? {
        Some(index) => summary(&index, &path),
        None => json!({
            "instance": instance,
            "path": path.display().to_string(),
            "exists": false,
        }),
    };
    write_response(global, &report)
}

pub fn tables(global: &GlobalFlags, args: CacheTablesArgs) -> Result<()> {
    let (index, _) = load_required(global)?;
    let prefix = args.prefix.unwrap_or_default();
    let names: Vec<Value> = index
        .tables_with_prefix(&prefix)
        .map(|t| Value::String(t.to_string()))
        .collect();
    write_response(global, &Value::Array(names))
}

pub fn columns(global: &GlobalFlags, args: CacheColumnsArgs) -> Result<()> {
    let (index, path) = load_required(global)?;
    if !index.columns_indexed {
        return Err(Error::Config(format!(
            "the schema cache at {} names tables only (it was built by a profile that cannot \
             read sys_dictionary); rebuild it with an admin profile: `sn cache refresh`",
            path.display()
        )));
    }
    let cols = index.columns(&args.table).ok_or_else(|| {
        Error::Usage(format!(
            "table '{}' is not in the schema cache for {}; check the name \
             (`sn cache tables <PREFIX>`), or run `sn cache refresh` if it is new",
            args.table, index.instance
        ))
    })?;
    write_response(
        global,
        &Value::Array(cols.into_iter().map(Value::String).collect()),
    )
}

fn load_required(global: &GlobalFlags) -> Result<(SchemaIndex, PathBuf)> {
    let instance = profile_instance(global.profile.as_deref())?;
    let path = schema_cache::cache_path(&instance)?;
    let index = schema_cache::load(&path)?.ok_or_else(|| {
        Error::Config(format!(
            "no schema cache for {instance}; run `sn cache refresh`"
        ))
    })?;
    Ok((index, path))
}

fn summary(index: &SchemaIndex, path: &std::path::Path) -> Value {
    json!({
        "instance": index.instance,
        "path": path.display().to_string(),
        "exists": true,
        "built_at": index.built_at,
        "age_secs": now_unix().saturating_sub(index.built_at),
        "tables": index.tables.len(),
        "columns": index.column_count(),
        "columns_indexed": index.columns_indexed,
    })
}
