# `sn` CLI v1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build an agent-forward Rust CLI that wraps the ServiceNow Table API plus two schema-discovery endpoints, producing a single binary with stable JSON contract, named profiles, and deterministic exit codes.

**Architecture:** Single Rust binary. `clap` derive for parsing (parse-time validation of flag/enum combinations), `reqwest` blocking client for HTTP (one request per invocation, no async runtime needed), `serde`/`serde_json` for JSON, `toml` for config files, `directories` for XDG-style config paths. Business logic lives in `src/{config,client,query,body,output,error}.rs`; CLI wiring lives in `src/cli/` with one module per subcommand. Integration tests use `wiremock` to mock ServiceNow and `assert_cmd` to drive the compiled binary.

**Tech Stack:** Rust (stable), clap 4 (derive), reqwest (blocking, rustls-tls), serde, serde_json, toml, directories, rpassword, thiserror, is-terminal, backoff, wiremock, assert_cmd, predicates.

**Spec:** `docs/superpowers/specs/2026-04-22-sn-cli-table-api-design.md` — read this before starting any task.

---

## File structure

Locked from spec §10:

```
Cargo.toml
rust-toolchain.toml
.gitignore
.github/workflows/
    ci.yml                    # PR checks: test, clippy, fmt
    release.yml               # tag-triggered cargo-dist release
src/
    main.rs                   # parse Cli, dispatch, map errors → exit codes
    error.rs                  # thiserror Error enum, exit-code mapping
    output.rs                 # JSON/JSONL emitter, TTY detection, stderr errors
    config.rs                 # Config/Credentials structs, load/save, paths
    client.rs                 # reqwest wrapper, auth, retry, pagination iterator
    query.rs                  # sysparm_* query-string assembler
    body.rs                   # --data / --field parser + merger
    observability.rs          # -v/-vv/-vvv logging to stderr with masking
    cli/
        mod.rs                # Cli struct, GlobalFlags, Subcommand enum
        init.rs               # `sn init`
        auth.rs               # `sn auth test`
        profile.rs            # `sn profile {list,show,remove,use}`
        table.rs              # `sn table {list,get,create,update,replace,delete}`
        schema.rs             # `sn schema {tables,columns,choices}`
        introspect.rs         # `sn introspect`
tests/
    common/mod.rs             # shared wiremock helpers
    config.rs                 # config file roundtrip tests
    table_list.rs             # end-to-end list tests
    table_crud.rs             # get/create/update/replace/delete
    schema.rs                 # schema endpoints
    pagination.rs             # --all + Link header follow
    auth.rs                   # init + auth test
```

Every Rust file starts small and stays focused. `client.rs` is the largest (it owns HTTP + retry + pagination) but its public surface is narrow: `Client::new(profile) -> Client`, `client.get(path, query) -> Response`, `client.paginated(path, query) -> impl Iterator<Item = Result<Value>>`.

---

## Global conventions (apply to every task)

- **Crate name:** `sn` (binary `sn`).
- **Edition:** 2021.
- **MSRV:** 1.75 (bump only if a specific crate requires newer).
- **Error-carrying result type:** `type Result<T, E = Error> = std::result::Result<T, E>;` re-exported from `error.rs`.
- **Tests first:** every task with non-trivial logic writes a failing test, runs it to confirm it fails, implements, runs it to confirm it passes. Thin wiring (dispatch, imports) may skip test-first when noted.
- **Commit style:** Conventional Commits. `feat:` for new features, `test:` for test-only, `chore:` for infra, `refactor:` for no-behavior-change edits. One commit per task unless a task has natural sub-commits.
- **One commit per task by default.** Co-author trailer (`Co-Authored-By: Claude ...`) is optional for local work.
- **Formatting:** `cargo fmt` before every commit.
- **Lints:** `cargo clippy --all-targets --all-features -- -D warnings` must pass before every commit.

---

## Task 1: Initialize Rust project scaffold

**Files:**
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `.gitignore`
- Create: `src/main.rs`

- [ ] **Step 1: Write `Cargo.toml`**

```toml
[package]
name = "sn"
version = "0.1.0"
edition = "2021"
rust-version = "1.75"
description = "Agent-forward CLI wrapper for the ServiceNow Table API"
license = "MIT OR Apache-2.0"
repository = "https://github.com/ibrahimsafah/sn"
default-run = "sn"

[[bin]]
name = "sn"
path = "src/main.rs"

[dependencies]
clap = { version = "4", features = ["derive", "suggestions", "wrap_help"] }
reqwest = { version = "0.12", default-features = false, features = ["blocking", "json", "rustls-tls"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
directories = "5"
rpassword = "7"
thiserror = "1"
is-terminal = "0.4"
backoff = "0.4"
url = "2"

[dev-dependencies]
wiremock = "0.6"
assert_cmd = "2"
predicates = "3"
tempfile = "3"
serial_test = "3"

[profile.release]
strip = true
lto = "thin"
codegen-units = 1
```

- [ ] **Step 2: Write `rust-toolchain.toml`**

```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
```

- [ ] **Step 3: Write `.gitignore`**

```
/target/
**/*.rs.bk
Cargo.lock.old
/dist/
.DS_Store
```

Note: `Cargo.lock` IS tracked because this is a binary crate.

- [ ] **Step 4: Write minimal `src/main.rs`**

```rust
fn main() {
    println!("sn {}", env!("CARGO_PKG_VERSION"));
}
```

- [ ] **Step 5: Verify the project builds**

Run: `cargo build`
Expected: compiles cleanly, produces `target/debug/sn`.

Run: `./target/debug/sn`
Expected: prints `sn 0.1.0`.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock rust-toolchain.toml .gitignore src/main.rs
git commit -m "chore: scaffold Rust project with core dependencies"
```

---

## Task 2: Error types and exit-code mapping

**Files:**
- Create: `src/error.rs`
- Modify: `src/main.rs` (add `mod error;`)

Spec references: §6.4, §6.5.

- [ ] **Step 1: Write failing test**

Create `src/error.rs`:

```rust
use serde::Serialize;
use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("usage error: {0}")]
    Usage(String),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("API error ({status}): {message}")]
    Api {
        status: u16,
        message: String,
        detail: Option<String>,
        transaction_id: Option<String>,
        sn_error: Option<serde_json::Value>,
    },

    #[error("auth error ({status}): {message}")]
    Auth {
        status: u16,
        message: String,
        transaction_id: Option<String>,
    },

    #[error("transport error: {0}")]
    Transport(String),
}

impl Error {
    /// Map each error variant to the exit code defined in spec §6.5.
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::Usage(_) | Error::Config(_) => 1,
            Error::Api { .. } => 2,
            Error::Transport(_) => 3,
            Error::Auth { .. } => 4,
        }
    }

    /// JSON envelope matching spec §6.4.
    pub fn to_stderr_json(&self) -> serde_json::Value {
        #[derive(Serialize)]
        struct Envelope<'a> {
            error: Inner<'a>,
        }
        #[derive(Serialize)]
        struct Inner<'a> {
            message: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            detail: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            status_code: Option<u16>,
            #[serde(skip_serializing_if = "Option::is_none")]
            transaction_id: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            sn_error: Option<&'a serde_json::Value>,
        }
        let (message, detail, status_code, tx, sn) = match self {
            Error::Usage(m) => (m.clone(), None, None, None, None),
            Error::Config(m) => (m.clone(), None, None, None, None),
            Error::Api { status, message, detail, transaction_id, sn_error } => (
                message.clone(),
                detail.as_deref(),
                Some(*status),
                transaction_id.as_deref(),
                sn_error.as_ref(),
            ),
            Error::Auth { status, message, transaction_id } => (
                message.clone(),
                None,
                Some(*status),
                transaction_id.as_deref(),
                None,
            ),
            Error::Transport(m) => (m.clone(), None, None, None, None),
        };
        serde_json::to_value(Envelope {
            error: Inner { message, detail, status_code, transaction_id: tx, sn_error: sn },
        })
        .expect("envelope should serialize")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_match_spec() {
        assert_eq!(Error::Usage("x".into()).exit_code(), 1);
        assert_eq!(Error::Config("x".into()).exit_code(), 1);
        assert_eq!(Error::Api { status: 400, message: "x".into(), detail: None, transaction_id: None, sn_error: None }.exit_code(), 2);
        assert_eq!(Error::Transport("x".into()).exit_code(), 3);
        assert_eq!(Error::Auth { status: 401, message: "x".into(), transaction_id: None }.exit_code(), 4);
    }

    #[test]
    fn stderr_envelope_includes_optional_fields() {
        let e = Error::Api {
            status: 404,
            message: "not found".into(),
            detail: Some("no record".into()),
            transaction_id: Some("tx1".into()),
            sn_error: Some(serde_json::json!({"message": "nope"})),
        };
        let v = e.to_stderr_json();
        assert_eq!(v["error"]["message"], "not found");
        assert_eq!(v["error"]["detail"], "no record");
        assert_eq!(v["error"]["status_code"], 404);
        assert_eq!(v["error"]["transaction_id"], "tx1");
        assert_eq!(v["error"]["sn_error"]["message"], "nope");
    }

    #[test]
    fn stderr_envelope_omits_none_fields() {
        let e = Error::Transport("dns".into());
        let v = e.to_stderr_json();
        assert!(v["error"].get("status_code").is_none());
        assert!(v["error"].get("sn_error").is_none());
    }
}
```

- [ ] **Step 2: Register the module**

Modify `src/main.rs`:

```rust
mod error;

fn main() {
    println!("sn {}", env!("CARGO_PKG_VERSION"));
}
```

- [ ] **Step 3: Run tests to confirm they pass**

Run: `cargo test --lib error::`
Expected: `3 passed; 0 failed`.

- [ ] **Step 4: Commit**

```bash
git add src/error.rs src/main.rs
git commit -m "feat: add Error enum with exit codes and stderr JSON envelope"
```

---

## Task 3: Output module (stdout JSON, TTY detection, stderr errors)

**Files:**
- Create: `src/output.rs`
- Modify: `src/main.rs` (add `mod output;`)

Spec references: §6.1, §6.3, §6.4.

- [ ] **Step 1: Write failing test**

Create `src/output.rs`:

```rust
use crate::error::Error;
use is_terminal::IsTerminal;
use serde_json::Value;
use std::io::{self, Write};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// Pretty-printed when stdout is a TTY, compact when piped.
    Auto,
    /// Always pretty.
    Pretty,
    /// Always compact (single-line).
    Compact,
}

impl Format {
    pub fn resolve(self) -> ResolvedFormat {
        match self {
            Format::Pretty => ResolvedFormat::Pretty,
            Format::Compact => ResolvedFormat::Compact,
            Format::Auto => {
                if io::stdout().is_terminal() {
                    ResolvedFormat::Pretty
                } else {
                    ResolvedFormat::Compact
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolvedFormat {
    Pretty,
    Compact,
}

/// Emit a single JSON value to stdout, trailing newline.
pub fn emit_value<W: Write>(mut w: W, value: &Value, fmt: ResolvedFormat) -> io::Result<()> {
    match fmt {
        ResolvedFormat::Pretty => serde_json::to_writer_pretty(&mut w, value)?,
        ResolvedFormat::Compact => serde_json::to_writer(&mut w, value)?,
    }
    w.write_all(b"\n")
}

/// Emit a stream of JSON values as JSONL (one compact record per line, regardless of TTY).
pub fn emit_jsonl<W: Write, I: IntoIterator<Item = Value>>(mut w: W, iter: I) -> io::Result<()> {
    for v in iter {
        serde_json::to_writer(&mut w, &v)?;
        w.write_all(b"\n")?;
    }
    Ok(())
}

/// Emit an error to stderr as the documented JSON envelope.
pub fn emit_error<W: Write>(mut w: W, err: &Error) -> io::Result<()> {
    serde_json::to_writer(&mut w, &err.to_stderr_json())?;
    w.write_all(b"\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn compact_emits_single_line() {
        let mut buf = Vec::new();
        emit_value(&mut buf, &json!({"a": 1}), ResolvedFormat::Compact).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "{\"a\":1}\n");
    }

    #[test]
    fn pretty_emits_indented() {
        let mut buf = Vec::new();
        emit_value(&mut buf, &json!({"a": 1}), ResolvedFormat::Pretty).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("  \"a\": 1"));
    }

    #[test]
    fn jsonl_one_record_per_line() {
        let mut buf = Vec::new();
        emit_jsonl(&mut buf, vec![json!({"a": 1}), json!({"a": 2})]).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "{\"a\":1}\n{\"a\":2}\n");
    }

    #[test]
    fn error_envelope_goes_to_writer() {
        let e = Error::Usage("bad".into());
        let mut buf = Vec::new();
        emit_error(&mut buf, &e).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("\"message\":\"bad\""));
    }
}
```

- [ ] **Step 2: Register the module**

Modify `src/main.rs`:

```rust
mod error;
mod output;

fn main() {
    println!("sn {}", env!("CARGO_PKG_VERSION"));
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib output::`
Expected: `4 passed`.

- [ ] **Step 4: Commit**

```bash
git add src/output.rs src/main.rs
git commit -m "feat: add stdout/stderr JSON emitters with TTY-aware formatting"
```

---

## Task 4: Config path resolution and types

**Files:**
- Create: `src/config.rs`
- Modify: `src/main.rs` (add `mod config;`)

Spec references: §5.1, §5.2.

- [ ] **Step 1: Write failing test**

Create `src/config.rs`:

```rust
use crate::error::{Error, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Credentials {
    #[serde(default)]
    pub profiles: BTreeMap<String, ProfileCredentials>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfileCredentials {
    pub username: String,
    pub password: String,
}

/// Resolve the sn config directory via `directories::ProjectDirs`.
pub fn config_dir() -> Result<PathBuf> {
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
        let mut cfg = Config::default();
        cfg.default_profile = Some("dev".into());
        cfg.profiles.insert("dev".into(), ProfileConfig { instance: "example.com".into() });
        let s = toml::to_string(&cfg).unwrap();
        let parsed: Config = toml::from_str(&s).unwrap();
        assert_eq!(parsed, cfg);
    }

    #[test]
    fn credentials_roundtrip_via_toml() {
        let mut cr = Credentials::default();
        cr.profiles.insert(
            "dev".into(),
            ProfileCredentials { username: "u".into(), password: "p".into() },
        );
        let s = toml::to_string(&cr).unwrap();
        let parsed: Credentials = toml::from_str(&s).unwrap();
        assert_eq!(parsed, cr);
    }
}
```

- [ ] **Step 2: Register the module**

Modify `src/main.rs` to add `mod config;`.

- [ ] **Step 3: Run tests**

Run: `cargo test --lib config::`
Expected: `3 passed`.

- [ ] **Step 4: Commit**

```bash
git add src/config.rs src/main.rs
git commit -m "feat: add Config/Credentials types and path resolution"
```

---

## Task 5: Config load/save with precedence (flag > env > file)

**Files:**
- Modify: `src/config.rs`

Spec references: §5.3, §5.4.

- [ ] **Step 1: Write failing tests for load/save**

Append to `src/config.rs`:

```rust
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

pub fn load_config_from(path: &std::path::Path) -> Result<Config> {
    if !path.exists() {
        return Ok(Config::default());
    }
    let s = fs::read_to_string(path).map_err(|e| Error::Config(format!("read {}: {e}", path.display())))?;
    toml::from_str(&s).map_err(|e| Error::Config(format!("parse {}: {e}", path.display())))
}

pub fn load_credentials_from(path: &std::path::Path) -> Result<Credentials> {
    if !path.exists() {
        return Ok(Credentials::default());
    }
    let s = fs::read_to_string(path).map_err(|e| Error::Config(format!("read {}: {e}", path.display())))?;
    toml::from_str(&s).map_err(|e| Error::Config(format!("parse {}: {e}", path.display())))
}

pub fn save_config_to(path: &std::path::Path, cfg: &Config) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| Error::Config(format!("mkdir {}: {e}", parent.display())))?;
    }
    let s = toml::to_string_pretty(cfg).map_err(|e| Error::Config(format!("serialize config: {e}")))?;
    fs::write(path, s).map_err(|e| Error::Config(format!("write {}: {e}", path.display())))
}

pub fn save_credentials_to(path: &std::path::Path, cr: &Credentials) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| Error::Config(format!("mkdir {}: {e}", parent.display())))?;
    }
    let s = toml::to_string_pretty(cr).map_err(|e| Error::Config(format!("serialize credentials: {e}")))?;
    fs::write(path, s).map_err(|e| Error::Config(format!("write {}: {e}", path.display())))?;
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(path).map_err(|e| Error::Config(format!("stat {}: {e}", path.display())))?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(path, perms).map_err(|e| Error::Config(format!("chmod {}: {e}", path.display())))?;
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
}

pub struct ProfileResolverInputs<'a> {
    pub cli_profile: Option<&'a str>,
    pub env_profile: Option<&'a str>,
    pub cli_instance_override: Option<&'a str>,
    pub env_instance: Option<&'a str>,
    pub env_username: Option<&'a str>,
    pub env_password: Option<&'a str>,
    pub config: &'a Config,
    pub credentials: &'a Credentials,
}

pub fn resolve_profile(inputs: ProfileResolverInputs<'_>) -> Result<ResolvedProfile> {
    let name = inputs.cli_profile
        .map(ToString::to_string)
        .or_else(|| inputs.env_profile.map(ToString::to_string))
        .or_else(|| inputs.config.default_profile.clone())
        .unwrap_or_else(|| "default".to_string());

    let profile_cfg = inputs.config.profiles.get(&name);
    let profile_cred = inputs.credentials.profiles.get(&name);

    let instance = inputs.cli_instance_override
        .map(ToString::to_string)
        .or_else(|| inputs.env_instance.map(ToString::to_string))
        .or_else(|| profile_cfg.map(|p| p.instance.clone()))
        .ok_or_else(|| Error::Config(format!(
            "no instance configured for profile '{name}'; run `sn init` or set SN_INSTANCE"
        )))?;

    let username = inputs.env_username
        .map(ToString::to_string)
        .or_else(|| profile_cred.map(|p| p.username.clone()))
        .ok_or_else(|| Error::Config(format!(
            "no username configured for profile '{name}'; run `sn init` or set SN_USERNAME"
        )))?;

    let password = inputs.env_password
        .map(ToString::to_string)
        .or_else(|| profile_cred.map(|p| p.password.clone()))
        .ok_or_else(|| Error::Config(format!(
            "no password configured for profile '{name}'; run `sn init` or set SN_PASSWORD"
        )))?;

    Ok(ResolvedProfile { name, instance, username, password })
}

#[cfg(test)]
mod resolution_tests {
    use super::*;

    fn sample_config() -> Config {
        let mut cfg = Config::default();
        cfg.default_profile = Some("dev".into());
        cfg.profiles.insert("dev".into(), ProfileConfig { instance: "dev.example.com".into() });
        cfg.profiles.insert("prod".into(), ProfileConfig { instance: "prod.example.com".into() });
        cfg
    }

    fn sample_credentials() -> Credentials {
        let mut cr = Credentials::default();
        cr.profiles.insert("dev".into(), ProfileCredentials { username: "dev-u".into(), password: "dev-p".into() });
        cr.profiles.insert("prod".into(), ProfileCredentials { username: "prod-u".into(), password: "prod-p".into() });
        cr
    }

    #[test]
    fn default_profile_when_none_specified() {
        let cfg = sample_config();
        let cr = sample_credentials();
        let r = resolve_profile(ProfileResolverInputs {
            cli_profile: None, env_profile: None,
            cli_instance_override: None, env_instance: None, env_username: None, env_password: None,
            config: &cfg, credentials: &cr,
        }).unwrap();
        assert_eq!(r.name, "dev");
        assert_eq!(r.instance, "dev.example.com");
    }

    #[test]
    fn cli_flag_wins_over_env_and_default() {
        let cfg = sample_config();
        let cr = sample_credentials();
        let r = resolve_profile(ProfileResolverInputs {
            cli_profile: Some("prod"), env_profile: Some("dev"),
            cli_instance_override: None, env_instance: None, env_username: None, env_password: None,
            config: &cfg, credentials: &cr,
        }).unwrap();
        assert_eq!(r.name, "prod");
        assert_eq!(r.instance, "prod.example.com");
    }

    #[test]
    fn env_overrides_per_field() {
        let cfg = sample_config();
        let cr = sample_credentials();
        let r = resolve_profile(ProfileResolverInputs {
            cli_profile: None, env_profile: None,
            cli_instance_override: None,
            env_instance: Some("override.example.com"),
            env_username: Some("env-u"), env_password: Some("env-p"),
            config: &cfg, credentials: &cr,
        }).unwrap();
        assert_eq!(r.instance, "override.example.com");
        assert_eq!(r.username, "env-u");
        assert_eq!(r.password, "env-p");
    }

    #[test]
    fn missing_instance_errors_clearly() {
        let cfg = Config::default();
        let cr = Credentials::default();
        let err = resolve_profile(ProfileResolverInputs {
            cli_profile: None, env_profile: None,
            cli_instance_override: None, env_instance: None,
            env_username: Some("u"), env_password: Some("p"),
            config: &cfg, credentials: &cr,
        }).unwrap_err();
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
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test --lib config::`
Expected: all tests pass (6 on Unix, 5 on Windows).

- [ ] **Step 3: Commit**

```bash
git add src/config.rs
git commit -m "feat: add config/credentials load/save with precedence resolver"
```

---

## Task 6: HTTP client — basic auth, request/response, error mapping

**Files:**
- Create: `src/client.rs`
- Modify: `src/main.rs` (add `mod client;`)
- Create: `tests/common/mod.rs`

Spec references: §6.4, §6.5 (exit codes), §8 (errors).

- [ ] **Step 1: Write `src/client.rs` (without retry logic yet)**

```rust
use crate::config::ResolvedProfile;
use crate::error::{Error, Result};
use reqwest::blocking::{Client as ReqwestClient, RequestBuilder, Response};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, CONTENT_TYPE, USER_AGENT};
use reqwest::{Method, StatusCode};
use serde_json::Value;
use std::time::Duration;

pub struct Client {
    http: ReqwestClient,
    base_url: String,
    username: String,
    password: String,
}

pub struct ClientBuilder {
    timeout: Duration,
    user_agent: String,
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            user_agent: format!("sn/{}", env!("CARGO_PKG_VERSION")),
        }
    }
}

impl ClientBuilder {
    pub fn timeout(mut self, d: Duration) -> Self { self.timeout = d; self }
    pub fn build(self, profile: &ResolvedProfile) -> Result<Client> {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(USER_AGENT, HeaderValue::from_str(&self.user_agent).unwrap());
        let http = ReqwestClient::builder()
            .timeout(self.timeout)
            .default_headers(headers)
            .build()
            .map_err(|e| Error::Transport(format!("build client: {e}")))?;
        let base_url = normalize_base_url(&profile.instance);
        Ok(Client { http, base_url, username: profile.username.clone(), password: profile.password.clone() })
    }
}

fn normalize_base_url(instance: &str) -> String {
    if instance.starts_with("http://") || instance.starts_with("https://") {
        instance.trim_end_matches('/').to_string()
    } else {
        format!("https://{}", instance.trim_end_matches('/'))
    }
}

impl Client {
    pub fn builder() -> ClientBuilder { ClientBuilder::default() }

    fn request(&self, method: Method, path: &str) -> RequestBuilder {
        let url = format!("{}{}", self.base_url, path);
        self.http.request(method, url).basic_auth(&self.username, Some(&self.password))
    }

    pub fn get(&self, path: &str, query: &[(String, String)]) -> Result<Value> {
        let resp = self.request(Method::GET, path).query(query).send()
            .map_err(|e| Error::Transport(format!("GET {path}: {e}")))?;
        parse_response(resp)
    }

    pub fn post(&self, path: &str, query: &[(String, String)], body: &Value) -> Result<Value> {
        let resp = self.request(Method::POST, path)
            .query(query)
            .header(CONTENT_TYPE, "application/json")
            .json(body)
            .send()
            .map_err(|e| Error::Transport(format!("POST {path}: {e}")))?;
        parse_response(resp)
    }

    pub fn put(&self, path: &str, query: &[(String, String)], body: &Value) -> Result<Value> {
        let resp = self.request(Method::PUT, path)
            .query(query)
            .header(CONTENT_TYPE, "application/json")
            .json(body)
            .send()
            .map_err(|e| Error::Transport(format!("PUT {path}: {e}")))?;
        parse_response(resp)
    }

    pub fn patch(&self, path: &str, query: &[(String, String)], body: &Value) -> Result<Value> {
        let resp = self.request(Method::PATCH, path)
            .query(query)
            .header(CONTENT_TYPE, "application/json")
            .json(body)
            .send()
            .map_err(|e| Error::Transport(format!("PATCH {path}: {e}")))?;
        parse_response(resp)
    }

    pub fn delete(&self, path: &str, query: &[(String, String)]) -> Result<()> {
        let resp = self.request(Method::DELETE, path).query(query).send()
            .map_err(|e| Error::Transport(format!("DELETE {path}: {e}")))?;
        let status = resp.status();
        let tx = transaction_id(&resp);
        if status.is_success() {
            Ok(())
        } else {
            Err(from_http(status, tx, resp))
        }
    }
}

fn transaction_id(resp: &Response) -> Option<String> {
    resp.headers().get("X-Transaction-ID").and_then(|v| v.to_str().ok()).map(ToString::to_string)
}

fn parse_response(resp: Response) -> Result<Value> {
    let status = resp.status();
    let tx = transaction_id(&resp);
    if status.is_success() {
        resp.json::<Value>().map_err(|e| Error::Transport(format!("parse response: {e}")))
    } else {
        Err(from_http(status, tx, resp))
    }
}

fn from_http(status: StatusCode, tx: Option<String>, resp: Response) -> Error {
    let body: Option<Value> = resp.json().ok();
    let (message, detail, sn_error) = body
        .as_ref()
        .and_then(|v| v.get("error"))
        .map(|err| (
            err.get("message").and_then(|m| m.as_str()).unwrap_or("ServiceNow error").to_string(),
            err.get("detail").and_then(|d| d.as_str()).map(ToString::to_string),
            Some(err.clone()),
        ))
        .unwrap_or_else(|| (format!("HTTP {status}"), None, None));
    match status.as_u16() {
        401 | 403 => Error::Auth { status: status.as_u16(), message, transaction_id: tx },
        s => Error::Api { status: s, message, detail, transaction_id: tx, sn_error },
    }
}
```

- [ ] **Step 2: Register module and add common test helpers**

Modify `src/main.rs` to include `mod client;`.

Create `tests/common/mod.rs`:

```rust
use sn as _; // ensure crate is linked

use wiremock::MockServer;

pub async fn start_mock() -> MockServer { MockServer::start().await }

pub fn mock_profile(instance: &str) -> sn::config::ResolvedProfile {
    sn::config::ResolvedProfile {
        name: "test".into(),
        instance: instance.to_string(),
        username: "admin".into(),
        password: "pw".into(),
    }
}
```

Note: `tests/common/mod.rs` requires exposing items. Add `pub mod config;`, `pub mod client;`, `pub mod error;`, `pub mod output;` to a new library target. Create `src/lib.rs`:

```rust
pub mod client;
pub mod config;
pub mod error;
pub mod output;
```

Update `Cargo.toml` to add a lib target:

```toml
[lib]
name = "sn"
path = "src/lib.rs"
```

And update `src/main.rs` to use the crate's library:

```rust
use sn::error::Error;

fn main() {
    println!("sn {}", env!("CARGO_PKG_VERSION"));
    drop(Error::Usage("placeholder".into()));
}
```

(The placeholder drop ensures `Error` is referenced until dispatch exists. Removed in Task 11.)

- [ ] **Step 3: Write an integration test**

Create `tests/client_basic.rs`:

```rust
mod common;

use serde_json::json;
use wiremock::matchers::{basic_auth, method, path};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test(flavor = "current_thread")]
async fn get_success_returns_parsed_json() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(basic_auth("admin", "pw"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": [{"sys_id": "a"}]})))
        .mount(&server)
        .await;

    let profile = common::mock_profile(&server.uri());
    let client = sn::client::Client::builder().build(&profile).unwrap();
    let body = client.get("/api/now/table/incident", &[]).unwrap();
    assert_eq!(body["result"][0]["sys_id"], "a");
}

#[tokio::test(flavor = "current_thread")]
async fn http_404_maps_to_api_error() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident/none"))
        .respond_with(ResponseTemplate::new(404)
            .insert_header("X-Transaction-ID", "tx-abc")
            .set_body_json(json!({"error": {"message": "No Record found", "detail": "ACL restrictions"}})))
        .mount(&server)
        .await;

    let profile = common::mock_profile(&server.uri());
    let client = sn::client::Client::builder().build(&profile).unwrap();
    let err = client.get("/api/now/table/incident/none", &[]).unwrap_err();
    match err {
        sn::error::Error::Api { status, message, transaction_id, sn_error, .. } => {
            assert_eq!(status, 404);
            assert_eq!(message, "No Record found");
            assert_eq!(transaction_id.as_deref(), Some("tx-abc"));
            assert!(sn_error.is_some());
        }
        other => panic!("expected Api error, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn http_401_maps_to_auth_error() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET")).and(path("/api/now/table/x"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({"error": {"message": "Unauthorized"}})))
        .mount(&server).await;
    let profile = common::mock_profile(&server.uri());
    let client = sn::client::Client::builder().build(&profile).unwrap();
    let err = client.get("/api/now/table/x", &[]).unwrap_err();
    assert!(matches!(err, sn::error::Error::Auth { status: 401, .. }));
}
```

Add `tokio` to dev-dependencies in `Cargo.toml`:

```toml
tokio = { version = "1", features = ["rt", "macros"] }
```

- [ ] **Step 4: Run tests**

Run: `cargo test --test client_basic`
Expected: `3 passed`.

- [ ] **Step 5: Commit**

```bash
git add src/client.rs src/lib.rs src/main.rs tests/common/ tests/client_basic.rs Cargo.toml Cargo.lock
git commit -m "feat: add HTTP client with basic auth and error mapping"
```

---

## Task 7: HTTP client — retry/backoff for 429 and 5xx

**Files:**
- Modify: `src/client.rs`

Spec references: §8.

- [ ] **Step 1: Add retry wrapper to `src/client.rs`**

Append near `ClientBuilder`:

```rust
#[derive(Clone, Copy, Debug)]
pub struct RetryPolicy {
    pub enabled: bool,
    pub max_attempts: u32,
    pub initial_backoff: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self { enabled: true, max_attempts: 3, initial_backoff: Duration::from_millis(500) }
    }
}
```

Add a field to `Client` and a method on `ClientBuilder`:

```rust
pub struct Client {
    http: ReqwestClient,
    base_url: String,
    username: String,
    password: String,
    retry: RetryPolicy,
}

impl ClientBuilder {
    pub fn retry(mut self, policy: RetryPolicy) -> Self { self.retry = policy; self }
}
```

Update `ClientBuilder` to hold the retry policy with a default, and pass it through in `build()`.

Add internal retry helper (in `client.rs`):

```rust
fn should_retry(status: StatusCode) -> bool {
    status.as_u16() == 429 || matches!(status.as_u16(), 502 | 503 | 504)
}

fn execute_with_retry<F>(policy: RetryPolicy, mut send: F) -> Result<Value>
where
    F: FnMut() -> std::result::Result<Response, reqwest::Error>,
{
    let mut attempt: u32 = 0;
    let mut backoff = policy.initial_backoff;
    loop {
        attempt += 1;
        match send() {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    return resp.json::<Value>()
                        .map_err(|e| Error::Transport(format!("parse response: {e}")));
                }
                let retryable = policy.enabled && should_retry(status) && attempt < policy.max_attempts;
                if !retryable {
                    let tx = transaction_id(&resp);
                    return Err(from_http(status, tx, resp));
                }
                std::thread::sleep(jittered(backoff));
                backoff = backoff.saturating_mul(2);
            }
            Err(e) => {
                return Err(Error::Transport(format!("{e}")));
            }
        }
    }
}

fn jittered(d: Duration) -> Duration {
    use std::time::SystemTime;
    let nanos = SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0) as u64;
    let jitter_ms = nanos % 250; // up to 250ms of jitter
    d + Duration::from_millis(jitter_ms)
}
```

Change each HTTP method on `Client` to route through `execute_with_retry`. Example for `get`:

```rust
pub fn get(&self, path: &str, query: &[(String, String)]) -> Result<Value> {
    let url = format!("{}{}", self.base_url, path);
    let http = self.http.clone();
    let user = self.username.clone();
    let pass = self.password.clone();
    let query = query.to_vec();
    execute_with_retry(self.retry, move || {
        http.request(Method::GET, &url)
            .basic_auth(&user, Some(&pass))
            .query(&query)
            .send()
    })
}
```

Do the same wrapping for `post`, `put`, `patch`; for `delete`, create a sibling `execute_no_body_with_retry` that returns `Result<()>` on 2xx.

- [ ] **Step 2: Write integration tests**

Create `tests/client_retry.rs`:

```rust
mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate, Times};

#[tokio::test(flavor = "current_thread")]
async fn retries_on_503_then_succeeds() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET")).and(path("/x"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(2)
        .mount(&server).await;
    Mock::given(method("GET")).and(path("/x"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .mount(&server).await;
    let profile = common::mock_profile(&server.uri());
    let client = sn::client::Client::builder()
        .retry(sn::client::RetryPolicy { enabled: true, max_attempts: 3, initial_backoff: std::time::Duration::from_millis(1) })
        .build(&profile).unwrap();
    let v = client.get("/x", &[]).unwrap();
    assert_eq!(v["ok"], true);
}

#[tokio::test(flavor = "current_thread")]
async fn disabled_retry_returns_first_failure() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET")).and(path("/x"))
        .respond_with(ResponseTemplate::new(503)).mount(&server).await;
    let profile = common::mock_profile(&server.uri());
    let client = sn::client::Client::builder()
        .retry(sn::client::RetryPolicy { enabled: false, max_attempts: 3, initial_backoff: std::time::Duration::from_millis(1) })
        .build(&profile).unwrap();
    let err = client.get("/x", &[]).unwrap_err();
    assert!(matches!(err, sn::error::Error::Api { status: 503, .. }));
}

#[tokio::test(flavor = "current_thread")]
async fn does_not_retry_4xx_except_429() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET")).and(path("/x"))
        .respond_with(ResponseTemplate::new(400))
        .expect(1)
        .mount(&server).await;
    let profile = common::mock_profile(&server.uri());
    let client = sn::client::Client::builder()
        .retry(sn::client::RetryPolicy { enabled: true, max_attempts: 3, initial_backoff: std::time::Duration::from_millis(1) })
        .build(&profile).unwrap();
    let _ = client.get("/x", &[]).unwrap_err();
    // Expected assertion runs on drop of server
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test --test client_retry`
Expected: `3 passed`.

- [ ] **Step 4: Commit**

```bash
git add src/client.rs tests/client_retry.rs
git commit -m "feat: add retry/backoff for 429 and 5xx responses"
```

---

## Task 8: Query builder

**Files:**
- Create: `src/query.rs`
- Modify: `src/lib.rs` (add `pub mod query;`)

Spec references: §3.

- [ ] **Step 1: Write `src/query.rs` with failing tests**

```rust
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DisplayValue {
    True,
    False,
    All,
}

impl DisplayValue {
    pub fn as_str(self) -> &'static str {
        match self { Self::True => "true", Self::False => "false", Self::All => "all" }
    }
}

#[derive(Debug, Default, Clone)]
pub struct ListQuery {
    pub query: Option<String>,
    pub fields: Option<String>,
    pub page_size: Option<u32>,
    pub offset: Option<u32>,
    pub display_value: Option<DisplayValue>,
    pub exclude_reference_link: Option<bool>,
    pub suppress_pagination_header: Option<bool>,
    pub view: Option<String>,
    pub query_category: Option<String>,
    pub query_no_domain: Option<bool>,
    pub no_count: Option<bool>,
}

#[derive(Debug, Default, Clone)]
pub struct GetQuery {
    pub fields: Option<String>,
    pub display_value: Option<DisplayValue>,
    pub exclude_reference_link: Option<bool>,
    pub view: Option<String>,
    pub query_no_domain: Option<bool>,
}

#[derive(Debug, Default, Clone)]
pub struct WriteQuery {
    pub fields: Option<String>,
    pub display_value: Option<DisplayValue>,
    pub exclude_reference_link: Option<bool>,
    pub input_display_value: Option<bool>,
    pub suppress_auto_sys_field: Option<bool>,
    pub view: Option<String>,
    pub query_no_domain: Option<bool>, // PATCH/PUT only; POST ignores
}

#[derive(Debug, Default, Clone)]
pub struct DeleteQuery {
    pub query_no_domain: Option<bool>,
}

fn push(pairs: &mut Vec<(String, String)>, key: &str, val: Option<String>) {
    if let Some(v) = val { pairs.push((key.into(), v)); }
}

fn push_bool(pairs: &mut Vec<(String, String)>, key: &str, val: Option<bool>) {
    if let Some(v) = val { pairs.push((key.into(), v.to_string())); }
}

fn push_u32(pairs: &mut Vec<(String, String)>, key: &str, val: Option<u32>) {
    if let Some(v) = val { pairs.push((key.into(), v.to_string())); }
}

impl ListQuery {
    pub fn to_pairs(&self) -> Vec<(String, String)> {
        let mut p = Vec::new();
        push(&mut p, "sysparm_query", self.query.clone());
        push(&mut p, "sysparm_fields", self.fields.clone());
        push_u32(&mut p, "sysparm_limit", self.page_size);
        push_u32(&mut p, "sysparm_offset", self.offset);
        push(&mut p, "sysparm_display_value", self.display_value.map(|d| d.as_str().to_string()));
        push_bool(&mut p, "sysparm_exclude_reference_link", self.exclude_reference_link);
        push_bool(&mut p, "sysparm_suppress_pagination_header", self.suppress_pagination_header);
        push(&mut p, "sysparm_view", self.view.clone());
        push(&mut p, "sysparm_query_category", self.query_category.clone());
        push_bool(&mut p, "sysparm_query_no_domain", self.query_no_domain);
        push_bool(&mut p, "sysparm_no_count", self.no_count);
        p
    }
}

impl GetQuery {
    pub fn to_pairs(&self) -> Vec<(String, String)> {
        let mut p = Vec::new();
        push(&mut p, "sysparm_fields", self.fields.clone());
        push(&mut p, "sysparm_display_value", self.display_value.map(|d| d.as_str().to_string()));
        push_bool(&mut p, "sysparm_exclude_reference_link", self.exclude_reference_link);
        push(&mut p, "sysparm_view", self.view.clone());
        push_bool(&mut p, "sysparm_query_no_domain", self.query_no_domain);
        p
    }
}

impl WriteQuery {
    pub fn to_pairs(&self) -> Vec<(String, String)> {
        let mut p = Vec::new();
        push(&mut p, "sysparm_fields", self.fields.clone());
        push(&mut p, "sysparm_display_value", self.display_value.map(|d| d.as_str().to_string()));
        push_bool(&mut p, "sysparm_exclude_reference_link", self.exclude_reference_link);
        push_bool(&mut p, "sysparm_input_display_value", self.input_display_value);
        push_bool(&mut p, "sysparm_suppress_auto_sys_field", self.suppress_auto_sys_field);
        push(&mut p, "sysparm_view", self.view.clone());
        push_bool(&mut p, "sysparm_query_no_domain", self.query_no_domain);
        p
    }
}

impl DeleteQuery {
    pub fn to_pairs(&self) -> Vec<(String, String)> {
        let mut p = Vec::new();
        push_bool(&mut p, "sysparm_query_no_domain", self.query_no_domain);
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_query_emits_only_set_pairs() {
        let q = ListQuery { query: Some("active=true".into()), page_size: Some(10), ..Default::default() };
        let pairs = q.to_pairs();
        assert_eq!(pairs, vec![
            ("sysparm_query".into(), "active=true".into()),
            ("sysparm_limit".into(), "10".into()),
        ]);
    }

    #[test]
    fn display_value_serialises_as_lowercase_string() {
        let q = ListQuery { display_value: Some(DisplayValue::All), ..Default::default() };
        assert_eq!(q.to_pairs(), vec![("sysparm_display_value".into(), "all".into())]);
    }

    #[test]
    fn write_query_respects_all_fields() {
        let q = WriteQuery {
            fields: Some("a,b".into()),
            input_display_value: Some(true),
            suppress_auto_sys_field: Some(true),
            display_value: Some(DisplayValue::False),
            ..Default::default()
        };
        let pairs = q.to_pairs();
        assert!(pairs.contains(&("sysparm_fields".into(), "a,b".into())));
        assert!(pairs.contains(&("sysparm_input_display_value".into(), "true".into())));
        assert!(pairs.contains(&("sysparm_suppress_auto_sys_field".into(), "true".into())));
        assert!(pairs.contains(&("sysparm_display_value".into(), "false".into())));
    }

    #[test]
    fn empty_query_emits_no_pairs() {
        let q = ListQuery::default();
        assert!(q.to_pairs().is_empty());
    }
}
```

- [ ] **Step 2: Register module**

In `src/lib.rs` add `pub mod query;`.

- [ ] **Step 3: Run tests**

Run: `cargo test --lib query::`
Expected: `4 passed`.

- [ ] **Step 4: Commit**

```bash
git add src/query.rs src/lib.rs
git commit -m "feat: add sysparm_* query builder structs for each verb"
```

---

## Task 9: Body builder (`--data` / `--field`)

**Files:**
- Create: `src/body.rs`
- Modify: `src/lib.rs` (add `pub mod body;`)

Spec references: §4.

- [ ] **Step 1: Write `src/body.rs`**

```rust
use crate::error::{Error, Result};
use serde_json::{Map, Value};
use std::fs;
use std::io::{self, Read};

/// Raw user input describing where the body comes from.
#[derive(Debug, Clone)]
pub enum BodyInput {
    /// `--data '<json>'` literal or `--data @file` or `--data @-` (stdin).
    Data(String),
    /// Repeated `--field name=value` (or `name=@file`).
    Fields(Vec<String>),
    None,
}

pub fn build_body(input: BodyInput) -> Result<Value> {
    match input {
        BodyInput::Data(spec) => parse_data_spec(&spec),
        BodyInput::Fields(specs) => parse_field_specs(&specs),
        BodyInput::None => Err(Error::Usage(
            "a request body is required; pass --data or one or more --field".into(),
        )),
    }
}

fn parse_data_spec(raw: &str) -> Result<Value> {
    let source = if raw == "@-" {
        let mut s = String::new();
        io::stdin().read_to_string(&mut s)
            .map_err(|e| Error::Usage(format!("read stdin: {e}")))?;
        s
    } else if let Some(path) = raw.strip_prefix('@') {
        fs::read_to_string(path).map_err(|e| Error::Usage(format!("read {path}: {e}")))?
    } else {
        raw.to_string()
    };
    let value: Value = serde_json::from_str(&source)
        .map_err(|e| Error::Usage(format!("--data is not valid JSON: {e}")))?;
    if !value.is_object() {
        return Err(Error::Usage("--data must be a JSON object at the top level".into()));
    }
    Ok(value)
}

fn parse_field_specs(specs: &[String]) -> Result<Value> {
    if specs.is_empty() {
        return Err(Error::Usage("at least one --field is required".into()));
    }
    let mut map: Map<String, Value> = Map::new();
    for spec in specs {
        let (name, raw_value) = spec.split_once('=')
            .ok_or_else(|| Error::Usage(format!("--field '{spec}' must be in name=value form")))?;
        if name.is_empty() {
            return Err(Error::Usage(format!("--field '{spec}' has empty name")));
        }
        if map.contains_key(name) {
            return Err(Error::Usage(format!("--field '{name}' specified more than once")));
        }
        let value = coerce_field_value(raw_value)?;
        map.insert(name.to_string(), value);
    }
    Ok(Value::Object(map))
}

fn coerce_field_value(raw: &str) -> Result<Value> {
    if let Some(path) = raw.strip_prefix('@') {
        let s = fs::read_to_string(path).map_err(|e| Error::Usage(format!("read {path}: {e}")))?;
        return Ok(Value::String(s));
    }
    // Try JSON scalars first (true/false/null/number), fall back to string.
    if let Ok(v) = serde_json::from_str::<Value>(raw) {
        if v.is_boolean() || v.is_null() || v.is_number() {
            return Ok(v);
        }
    }
    Ok(Value::String(raw.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_inline_json_parses() {
        let v = build_body(BodyInput::Data(r#"{"a": 1}"#.into())).unwrap();
        assert_eq!(v["a"], 1);
    }

    #[test]
    fn data_top_level_must_be_object() {
        let err = build_body(BodyInput::Data("[1,2,3]".into())).unwrap_err();
        assert!(matches!(err, Error::Usage(_)));
    }

    #[test]
    fn fields_merge_into_object() {
        let v = build_body(BodyInput::Fields(vec!["a=1".into(), "b=x".into(), "c=true".into()])).unwrap();
        assert_eq!(v["a"], 1);
        assert_eq!(v["b"], "x");
        assert_eq!(v["c"], true);
    }

    #[test]
    fn duplicate_field_is_usage_error() {
        let err = build_body(BodyInput::Fields(vec!["a=1".into(), "a=2".into()])).unwrap_err();
        assert!(matches!(err, Error::Usage(_)));
    }

    #[test]
    fn field_file_reference() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("v.txt");
        std::fs::write(&path, "hello world").unwrap();
        let spec = format!("k=@{}", path.to_str().unwrap());
        let v = build_body(BodyInput::Fields(vec![spec])).unwrap();
        assert_eq!(v["k"], "hello world");
    }

    #[test]
    fn data_at_file_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("b.json");
        std::fs::write(&path, r#"{"x": 10}"#).unwrap();
        let v = build_body(BodyInput::Data(format!("@{}", path.to_str().unwrap()))).unwrap();
        assert_eq!(v["x"], 10);
    }
}
```

- [ ] **Step 2: Register module**

In `src/lib.rs` add `pub mod body;`.

- [ ] **Step 3: Run tests**

Run: `cargo test --lib body::`
Expected: `6 passed`.

- [ ] **Step 4: Commit**

```bash
git add src/body.rs src/lib.rs
git commit -m "feat: add body builder for --data and --field flags"
```

---

## Task 10: CLI root — `Cli` struct, global flags, `Subcommand` enum

**Files:**
- Create: `src/cli/mod.rs`
- Modify: `src/lib.rs` (add `pub mod cli;`)

Spec references: §2, §6 global flags.

- [ ] **Step 1: Write `src/cli/mod.rs`**

```rust
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug)]
#[command(name = "sn", version, about = "ServiceNow CLI (Table API + schema)")]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalFlags,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(clap::Args, Debug, Clone, Default)]
pub struct GlobalFlags {
    /// Profile name (overrides SN_PROFILE and default_profile).
    #[arg(long, global = true)]
    pub profile: Option<String>,

    /// Override the profile's instance URL for this invocation.
    #[arg(long, global = true, value_name = "URL")]
    pub instance_override: Option<String>,

    /// Output mode. `default` (unwrapped result) or `raw` (full SN envelope).
    #[arg(long, global = true, value_enum, default_value_t = OutputMode::Default)]
    pub output: OutputMode,

    /// Force pretty-printed JSON regardless of TTY detection.
    #[arg(long, global = true, conflicts_with = "compact")]
    pub pretty: bool,

    /// Force compact JSON regardless of TTY detection.
    #[arg(long, global = true, conflicts_with = "pretty")]
    pub compact: bool,

    /// Request timeout in seconds (overrides SN_TIMEOUT).
    #[arg(long, global = true, value_name = "SECS")]
    pub timeout: Option<u64>,

    /// Disable automatic retry for 429/5xx responses.
    #[arg(long, global = true)]
    pub no_retry: bool,

    /// Verbosity: -v, -vv, -vvv (see spec §9).
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,
}

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
#[value(rename_all = "lowercase")]
pub enum OutputMode { Default, Raw }

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Create or update a profile interactively.
    Init(InitArgs),
    /// Auth operations.
    Auth {
        #[command(subcommand)]
        sub: AuthSub,
    },
    /// Manage profiles.
    Profile {
        #[command(subcommand)]
        sub: ProfileSub,
    },
    /// Table API CRUD.
    Table {
        #[command(subcommand)]
        sub: TableSub,
    },
    /// Schema discovery.
    Schema {
        #[command(subcommand)]
        sub: SchemaSub,
    },
    /// Dump the full command tree as JSON for agent/MCP generation.
    Introspect,
}

#[derive(clap::Args, Debug)]
pub struct InitArgs {
    #[arg(long)]
    pub profile: Option<String>,
    #[arg(long)]
    pub instance: Option<String>,
    #[arg(long)]
    pub username: Option<String>,
    #[arg(long)]
    pub password: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum AuthSub {
    /// Verify credentials by calling /api/now/table/sys_user?sysparm_limit=1.
    Test,
}

#[derive(Subcommand, Debug)]
pub enum ProfileSub {
    List,
    Show { name: Option<String> },
    Remove { name: String },
    Use { name: String },
}

// Placeholders; filled in by later tasks.
#[derive(Subcommand, Debug)]
pub enum TableSub {
    #[command(about = "List records")]
    List(TableListArgs),
    #[command(about = "Get a single record by sys_id")]
    Get(TableGetArgs),
    #[command(about = "Create a record")]
    Create(TableCreateArgs),
    #[command(about = "Patch a record (partial update)")]
    Update(TableUpdateArgs),
    #[command(about = "Replace a record (PUT, full overwrite)")]
    Replace(TableReplaceArgs),
    #[command(about = "Delete a record")]
    Delete(TableDeleteArgs),
}

#[derive(clap::Args, Debug, Default)]
pub struct TableListArgs { /* expanded in Task 15 */
    pub table: String,
}
#[derive(clap::Args, Debug, Default)]
pub struct TableGetArgs { pub table: String, pub sys_id: String }
#[derive(clap::Args, Debug, Default)]
pub struct TableCreateArgs { pub table: String }
#[derive(clap::Args, Debug, Default)]
pub struct TableUpdateArgs { pub table: String, pub sys_id: String }
#[derive(clap::Args, Debug, Default)]
pub struct TableReplaceArgs { pub table: String, pub sys_id: String }
#[derive(clap::Args, Debug, Default)]
pub struct TableDeleteArgs { pub table: String, pub sys_id: String }

#[derive(Subcommand, Debug)]
pub enum SchemaSub {
    Tables(SchemaTablesArgs),
    Columns(SchemaColumnsArgs),
    Choices(SchemaChoicesArgs),
}

#[derive(clap::Args, Debug, Default)]
pub struct SchemaTablesArgs { /* expanded in Task 21 */ }
#[derive(clap::Args, Debug, Default)]
pub struct SchemaColumnsArgs { pub table: String }
#[derive(clap::Args, Debug, Default)]
pub struct SchemaChoicesArgs { pub table: String, pub field: String }

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_compiles_and_debugs() {
        Cli::command().debug_assert();
    }

    #[test]
    fn pretty_and_compact_conflict() {
        let err = Cli::try_parse_from(["sn", "--pretty", "--compact", "introspect"]).unwrap_err();
        // clap emits conflict error; kind may differ by version, just assert it's an error
        let _ = err.kind();
    }
}
```

- [ ] **Step 2: Register module**

In `src/lib.rs`:

```rust
pub mod cli;
pub mod body;
pub mod client;
pub mod config;
pub mod error;
pub mod output;
pub mod query;
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib cli::`
Expected: `2 passed`.

- [ ] **Step 4: Commit**

```bash
git add src/cli/mod.rs src/lib.rs
git commit -m "feat: add clap CLI skeleton with global flags and subcommand enum"
```

---

## Task 11: `main.rs` dispatch and exit-code glue

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Rewrite `src/main.rs`**

```rust
use clap::Parser;
use sn::cli::{Cli, Command};
use sn::error::{Error, Result};
use sn::output::emit_error;
use std::io;
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            let _ = emit_error(io::stderr().lock(), &err);
            ExitCode::from(err.exit_code() as u8)
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Introspect => {
            // Filled in by Task 24.
            println!("{{\"todo\": \"introspect\"}}");
            Ok(())
        }
        _ => Err(Error::Usage("command not implemented yet".into())),
    }
}
```

- [ ] **Step 2: Verify `sn --help` works**

Run: `cargo build && ./target/debug/sn --help`
Expected: prints usage including `table`, `schema`, `init`, `auth`, `profile`, `introspect`.

Run: `./target/debug/sn introspect`
Expected: prints `{"todo": "introspect"}`, exit 0.

Run: `./target/debug/sn table list incident; echo "exit=$?"`
Expected: stderr JSON error, exit 1.

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat: wire main.rs to parse Cli, dispatch, and map errors to exit codes"
```

---

## Task 12: `sn init` command

**Files:**
- Create: `src/cli/init.rs`
- Modify: `src/cli/mod.rs` (add `pub mod init;`)
- Modify: `src/main.rs` (dispatch `Command::Init`)

Spec references: §5.5, §5.6.

- [ ] **Step 1: Write `src/cli/init.rs`**

```rust
use crate::cli::InitArgs;
use crate::client::{Client, RetryPolicy};
use crate::config::{
    config_path, credentials_path, load_config_from, load_credentials_from,
    save_config_to, save_credentials_to, ProfileConfig, ProfileCredentials, ResolvedProfile,
};
use crate::error::{Error, Result};
use std::io::{self, Write};

pub fn run(args: InitArgs) -> Result<()> {
    let profile_name = args.profile.unwrap_or_else(|| prompt("Profile name [default]: ", Some("default".into()))).trim().to_string();

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
        None => rpassword::prompt_password("Password: ").map_err(|e| Error::Usage(format!("read password: {e}")))?,
    };

    if instance.trim().is_empty() || username.trim().is_empty() || password.is_empty() {
        return Err(Error::Usage("instance, username, and password are required".into()));
    }

    // Load, upsert, save.
    let cfg_path = config_path()?;
    let cred_path = credentials_path()?;
    let mut config = load_config_from(&cfg_path)?;
    let mut creds = load_credentials_from(&cred_path)?;

    if config.default_profile.is_none() {
        config.default_profile = Some(profile_name.clone());
    }
    config.profiles.insert(profile_name.clone(), ProfileConfig { instance: instance.clone() });
    creds.profiles.insert(profile_name.clone(), ProfileCredentials { username: username.clone(), password: password.clone() });

    save_config_to(&cfg_path, &config)?;
    save_credentials_to(&cred_path, &creds)?;

    // Verify creds by calling auth_test (reuse client).
    let profile = ResolvedProfile {
        name: profile_name.clone(),
        instance, username, password,
    };
    let client = Client::builder().retry(RetryPolicy::default()).build(&profile)?;
    client.get("/api/now/table/sys_user", &[("sysparm_limit".into(), "1".into())])?;

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
```

- [ ] **Step 2: Register and dispatch**

In `src/cli/mod.rs` add `pub mod init;`.

In `src/main.rs`, extend `run`:

```rust
fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Init(args) => sn::cli::init::run(args),
        Command::Introspect => { println!("{{\"todo\": \"introspect\"}}"); Ok(()) }
        _ => Err(Error::Usage("command not implemented yet".into())),
    }
}
```

- [ ] **Step 3: Integration test**

Create `tests/init.rs` that runs `sn init --profile t --instance http://127.0.0.1:<port> --username u --password p` against a wiremock-backed sys_user endpoint, asserting the config/credentials files appear in a temporary `XDG_CONFIG_HOME`.

```rust
mod common;

use assert_cmd::Command;
use predicates::str::contains;
use serde_json::json;
use wiremock::matchers::{basic_auth, method, path};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test(flavor = "current_thread")]
async fn init_writes_files_and_verifies_creds() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET")).and(path("/api/now/table/sys_user")).and(basic_auth("u", "p"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": []})))
        .mount(&server).await;

    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("sn").unwrap();
    cmd.env("XDG_CONFIG_HOME", tmp.path())
        .args([
            "init",
            "--profile", "t",
            "--instance", &server.uri(),
            "--username", "u",
            "--password", "p",
        ])
        .assert()
        .success()
        .stderr(contains("saved and verified"));

    let cfg = tmp.path().join("sn/config.toml");
    assert!(cfg.exists());
    let cred = tmp.path().join("sn/credentials.toml");
    assert!(cred.exists());
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --test init`
Expected: passes on Linux; on macOS the `XDG_CONFIG_HOME` env is ignored by `directories` — if so, fall back to asserting only that init ran successfully. Mark test `#[cfg(target_os = "linux")]`.

- [ ] **Step 5: Commit**

```bash
git add src/cli/init.rs src/cli/mod.rs src/main.rs tests/init.rs
git commit -m "feat: add sn init command with credential verification"
```

---

## Task 13: `sn auth test` command

**Files:**
- Create: `src/cli/auth.rs`
- Modify: `src/cli/mod.rs` and `src/main.rs`

Spec references: §5.6.

- [ ] **Step 1: Write `src/cli/auth.rs`**

```rust
use crate::cli::GlobalFlags;
use crate::client::{Client, RetryPolicy};
use crate::error::Result;
use crate::config::{config_path, credentials_path, load_config_from, load_credentials_from, resolve_profile, ProfileResolverInputs};
use serde_json::json;
use std::io::Write;

pub fn test(global: &GlobalFlags) -> Result<()> {
    let config = load_config_from(&config_path()?)?;
    let creds = load_credentials_from(&credentials_path()?)?;
    let profile = resolve_profile(ProfileResolverInputs {
        cli_profile: global.profile.as_deref(),
        env_profile: std::env::var("SN_PROFILE").ok().as_deref(),
        cli_instance_override: global.instance_override.as_deref(),
        env_instance: std::env::var("SN_INSTANCE").ok().as_deref(),
        env_username: std::env::var("SN_USERNAME").ok().as_deref(),
        env_password: std::env::var("SN_PASSWORD").ok().as_deref(),
        config: &config, credentials: &creds,
    })?;
    let retry = if global.no_retry { RetryPolicy { enabled: false, ..Default::default() } } else { RetryPolicy::default() };
    let client = Client::builder().retry(retry).build(&profile)?;
    let v = client.get("/api/now/table/sys_user", &[("sysparm_limit".into(), "1".into())])?;
    let user = v["result"].get(0).and_then(|r| r.get("user_name")).and_then(|x| x.as_str()).unwrap_or(&profile.username);
    let msg = json!({"ok": true, "instance": profile.instance, "username": user, "profile": profile.name});
    writeln!(std::io::stderr(), "{msg}").ok();
    Ok(())
}
```

Note: env-var reads use `ok().as_deref()` which won't compile as written because `Option<String>::as_deref()` returns `Option<&str>`. Fix by binding locals:

```rust
let env_profile = std::env::var("SN_PROFILE").ok();
let env_instance = std::env::var("SN_INSTANCE").ok();
let env_username = std::env::var("SN_USERNAME").ok();
let env_password = std::env::var("SN_PASSWORD").ok();
let profile = resolve_profile(ProfileResolverInputs {
    cli_profile: global.profile.as_deref(),
    env_profile: env_profile.as_deref(),
    cli_instance_override: global.instance_override.as_deref(),
    env_instance: env_instance.as_deref(),
    env_username: env_username.as_deref(),
    env_password: env_password.as_deref(),
    config: &config, credentials: &creds,
})?;
```

- [ ] **Step 2: Register and dispatch**

In `src/cli/mod.rs` add `pub mod auth;`.

In `src/main.rs`:

```rust
Command::Auth { sub } => match sub {
    AuthSub::Test => sn::cli::auth::test(&cli.global),
},
```

Import `AuthSub` at top of `main.rs`.

- [ ] **Step 3: Integration test**

Create `tests/auth.rs`:

```rust
mod common;

use assert_cmd::Command;
use serde_json::json;
use wiremock::matchers::{basic_auth, method, path};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test(flavor = "current_thread")]
async fn auth_test_ok() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET")).and(path("/api/now/table/sys_user")).and(basic_auth("u", "p"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": [{"user_name": "api.user"}]})))
        .mount(&server).await;
    let mut cmd = Command::cargo_bin("sn").unwrap();
    cmd.env("SN_INSTANCE", server.uri())
        .env("SN_USERNAME", "u")
        .env("SN_PASSWORD", "p")
        .args(["auth", "test"])
        .assert()
        .success();
}

#[tokio::test(flavor = "current_thread")]
async fn auth_test_401_exit_4() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET")).and(path("/api/now/table/sys_user"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({"error": {"message": "nope"}})))
        .mount(&server).await;
    let mut cmd = Command::cargo_bin("sn").unwrap();
    cmd.env("SN_INSTANCE", server.uri())
        .env("SN_USERNAME", "u")
        .env("SN_PASSWORD", "p")
        .args(["auth", "test"])
        .assert()
        .code(4);
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --test auth`
Expected: `2 passed`.

- [ ] **Step 5: Commit**

```bash
git add src/cli/auth.rs src/cli/mod.rs src/main.rs tests/auth.rs
git commit -m "feat: add sn auth test command"
```

---

## Task 14: `sn profile` subcommands

**Files:**
- Create: `src/cli/profile.rs`
- Modify: `src/cli/mod.rs` and `src/main.rs`

- [ ] **Step 1: Write `src/cli/profile.rs`**

```rust
use crate::cli::ProfileSub;
use crate::config::{
    config_path, credentials_path, load_config_from, load_credentials_from,
    save_config_to, ProfileConfig, ProfileCredentials,
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
    let name = name.or_else(|| cfg.default_profile.clone())
        .ok_or_else(|| Error::Usage("no profile name and no default_profile configured".into()))?;
    let p: &ProfileConfig = cfg.profiles.get(&name)
        .ok_or_else(|| Error::Usage(format!("profile '{name}' not found")))?;
    emit_value(io::stdout().lock(), &json!({"name": name, "instance": p.instance}), Format::Auto.resolve())
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
    crate::config::save_credentials_to(&cred_path, &creds)?;
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
```

Ensure unused `ProfileCredentials` import is removed if clippy complains.

- [ ] **Step 2: Register and dispatch**

In `src/cli/mod.rs` add `pub mod profile;`.

In `src/main.rs` add:

```rust
Command::Profile { sub } => sn::cli::profile::run(sub),
```

Import `ProfileSub` alongside the others.

- [ ] **Step 3: Smoke test**

Run: `cargo build && ./target/debug/sn profile list`
Expected: either an empty array `[]` or whatever profiles exist.

Run: `./target/debug/sn profile show`
Expected: if no profiles, exit 1 with JSON error on stderr.

- [ ] **Step 4: Commit**

```bash
git add src/cli/profile.rs src/cli/mod.rs src/main.rs
git commit -m "feat: add sn profile list/show/remove/use subcommands"
```

---

## Task 15: `sn table list` (single page, without `--all`)

**Files:**
- Create: `src/cli/table.rs`
- Modify: `src/cli/mod.rs` (flesh out `TableListArgs`)
- Modify: `src/main.rs`

Spec references: §2, §3 (list params), §6.1 (output), §7 (manual pagination).

- [ ] **Step 1: Expand `TableListArgs` in `src/cli/mod.rs`**

Replace the placeholder with:

```rust
#[derive(clap::Args, Debug)]
pub struct TableListArgs {
    /// Table name (e.g. `incident`).
    pub table: String,
    /// Encoded query, e.g. `active=true^priority=1`.
    #[arg(long, alias = "sysparm-query")]
    pub query: Option<String>,
    /// Comma-separated fields to return.
    #[arg(long, alias = "sysparm-fields")]
    pub fields: Option<String>,
    /// Records per API call (default 1000).
    #[arg(long, alias = "limit", alias = "sysparm-limit", default_value_t = 1000)]
    pub page_size: u32,
    /// Starting offset for manual pagination (ignored with --all).
    #[arg(long, alias = "sysparm-offset")]
    pub offset: Option<u32>,
    /// Resolve reference/choice fields: false (default), true, or all.
    #[arg(long, alias = "sysparm-display-value", value_enum)]
    pub display_value: Option<DisplayValueArg>,
    /// Strip reference-link URLs from reference fields.
    #[arg(long, alias = "sysparm-exclude-reference-link")]
    pub exclude_reference_link: bool,
    /// Skip X-Total-Count calculation.
    #[arg(long, alias = "sysparm-suppress-pagination-header")]
    pub suppress_pagination_header: bool,
    /// Apply a named form/list view.
    #[arg(long, alias = "sysparm-view")]
    pub view: Option<String>,
    /// Query category for index selection.
    #[arg(long, alias = "sysparm-query-category")]
    pub query_category: Option<String>,
    /// Cross-domain access if authorized.
    #[arg(long, alias = "sysparm-query-no-domain")]
    pub query_no_domain: bool,
    /// Skip the count query.
    #[arg(long, alias = "sysparm-no-count")]
    pub no_count: bool,
    /// Auto-paginate: stream every matching record (JSONL unless --array).
    #[arg(long)]
    pub all: bool,
    /// With --all, buffer into a single JSON array instead of JSONL.
    #[arg(long, requires = "all")]
    pub array: bool,
    /// Cap total records returned (default 100000; 0 = unlimited).
    #[arg(long, default_value_t = 100_000)]
    pub max_records: u32,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "lowercase")]
pub enum DisplayValueArg { True, False, All }

impl From<DisplayValueArg> for crate::query::DisplayValue {
    fn from(v: DisplayValueArg) -> Self {
        match v {
            DisplayValueArg::True => crate::query::DisplayValue::True,
            DisplayValueArg::False => crate::query::DisplayValue::False,
            DisplayValueArg::All => crate::query::DisplayValue::All,
        }
    }
}
```

- [ ] **Step 2: Write `src/cli/table.rs` (list only for now)**

```rust
use crate::cli::{GlobalFlags, OutputMode, TableListArgs};
use crate::client::{Client, RetryPolicy};
use crate::config::{config_path, credentials_path, load_config_from, load_credentials_from, resolve_profile, ProfileResolverInputs};
use crate::error::{Error, Result};
use crate::output::{emit_value, Format};
use crate::query::ListQuery;
use serde_json::Value;
use std::io;

pub fn list(global: &GlobalFlags, args: TableListArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let retry = retry_policy(global.no_retry);
    let client = Client::builder().retry(retry).build(&profile)?;

    // Manual mode only in this task; Task 20 adds --all.
    if args.all {
        return Err(Error::Usage("--all not yet implemented (see Task 20)".into()));
    }

    let q = ListQuery {
        query: args.query,
        fields: args.fields,
        page_size: Some(args.page_size),
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

pub(crate) fn build_profile(global: &GlobalFlags) -> Result<crate::config::ResolvedProfile> {
    let config = load_config_from(&config_path()?)?;
    let creds = load_credentials_from(&credentials_path()?)?;
    let env_profile = std::env::var("SN_PROFILE").ok();
    let env_instance = std::env::var("SN_INSTANCE").ok();
    let env_username = std::env::var("SN_USERNAME").ok();
    let env_password = std::env::var("SN_PASSWORD").ok();
    resolve_profile(ProfileResolverInputs {
        cli_profile: global.profile.as_deref(),
        env_profile: env_profile.as_deref(),
        cli_instance_override: global.instance_override.as_deref(),
        env_instance: env_instance.as_deref(),
        env_username: env_username.as_deref(),
        env_password: env_password.as_deref(),
        config: &config, credentials: &creds,
    })
}

pub(crate) fn retry_policy(no_retry: bool) -> RetryPolicy {
    if no_retry { RetryPolicy { enabled: false, ..Default::default() } } else { RetryPolicy::default() }
}

pub(crate) fn bool_opt(b: bool) -> Option<bool> { if b { Some(true) } else { None } }

pub(crate) fn format_from_flags(g: &GlobalFlags) -> crate::output::ResolvedFormat {
    if g.pretty { Format::Pretty.resolve() }
    else if g.compact { Format::Compact.resolve() }
    else { Format::Auto.resolve() }
}

pub(crate) fn unwrap_or_raw(v: Value, mode: OutputMode) -> Value {
    match mode {
        OutputMode::Raw => v,
        OutputMode::Default => v.get("result").cloned().unwrap_or(v),
    }
}
```

- [ ] **Step 3: Register and dispatch**

In `src/cli/mod.rs` add `pub mod table;`.

In `src/main.rs`:

```rust
Command::Table { sub } => match sub {
    TableSub::List(args) => sn::cli::table::list(&cli.global, args),
    TableSub::Get(_) | TableSub::Create(_) | TableSub::Update(_)
    | TableSub::Replace(_) | TableSub::Delete(_) => Err(Error::Usage("table subcommand not yet wired".into())),
},
```

Import `TableSub` with the others.

- [ ] **Step 4: Integration test**

Create `tests/table_list.rs`:

```rust
mod common;

use assert_cmd::Command;
use serde_json::json;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test(flavor = "current_thread")]
async fn list_default_unwraps_result() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET")).and(path("/api/now/table/incident")).and(query_param("sysparm_limit", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": [{"number": "INC1"}, {"number": "INC2"}]})))
        .mount(&server).await;
    let mut cmd = Command::cargo_bin("sn").unwrap();
    let out = cmd.env("SN_INSTANCE", server.uri())
        .env("SN_USERNAME", "u").env("SN_PASSWORD", "p")
        .args(["table", "list", "incident", "--page-size", "5", "--compact"])
        .assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert_eq!(stdout.trim(), r#"[{"number":"INC1"},{"number":"INC2"}]"#);
}

#[tokio::test(flavor = "current_thread")]
async fn list_raw_preserves_envelope() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET")).and(path("/api/now/table/incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": [{"n": 1}]})))
        .mount(&server).await;
    let mut cmd = Command::cargo_bin("sn").unwrap();
    let out = cmd.env("SN_INSTANCE", server.uri()).env("SN_USERNAME", "u").env("SN_PASSWORD", "p")
        .args(["--output", "raw", "--compact", "table", "list", "incident"])
        .assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert_eq!(stdout.trim(), r#"{"result":[{"n":1}]}"#);
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test --test table_list`
Expected: `2 passed`.

- [ ] **Step 6: Commit**

```bash
git add src/cli/table.rs src/cli/mod.rs src/main.rs tests/table_list.rs
git commit -m "feat: add sn table list (single-page)"
```

---

## Task 16: `sn table get`

**Files:**
- Modify: `src/cli/mod.rs` (flesh out `TableGetArgs`)
- Modify: `src/cli/table.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Expand `TableGetArgs` in `src/cli/mod.rs`**

```rust
#[derive(clap::Args, Debug)]
pub struct TableGetArgs {
    pub table: String,
    pub sys_id: String,
    #[arg(long, alias = "sysparm-fields")]
    pub fields: Option<String>,
    #[arg(long, alias = "sysparm-display-value", value_enum)]
    pub display_value: Option<DisplayValueArg>,
    #[arg(long, alias = "sysparm-exclude-reference-link")]
    pub exclude_reference_link: bool,
    #[arg(long, alias = "sysparm-view")]
    pub view: Option<String>,
    #[arg(long, alias = "sysparm-query-no-domain")]
    pub query_no_domain: bool,
}
```

- [ ] **Step 2: Add `get` to `src/cli/table.rs`**

```rust
use crate::cli::TableGetArgs;
use crate::query::GetQuery;

pub fn get(global: &GlobalFlags, args: TableGetArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = Client::builder().retry(retry_policy(global.no_retry)).build(&profile)?;
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
```

- [ ] **Step 3: Dispatch in `main.rs`**

Replace the pending arm:

```rust
TableSub::Get(args) => sn::cli::table::get(&cli.global, args),
```

- [ ] **Step 4: Integration test**

Append to `tests/table_list.rs` or create `tests/table_get.rs`:

```rust
mod common;

use assert_cmd::Command;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test(flavor = "current_thread")]
async fn get_unwraps_single_record() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET")).and(path("/api/now/table/incident/abc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": {"sys_id": "abc", "number": "INC1"}})))
        .mount(&server).await;
    let mut cmd = Command::cargo_bin("sn").unwrap();
    let out = cmd.env("SN_INSTANCE", server.uri()).env("SN_USERNAME", "u").env("SN_PASSWORD", "p")
        .args(["--compact", "table", "get", "incident", "abc"])
        .assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert_eq!(stdout.trim(), r#"{"number":"INC1","sys_id":"abc"}"#);
}

#[tokio::test(flavor = "current_thread")]
async fn get_404_exit_2() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET")).and(path("/api/now/table/incident/missing"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"error": {"message": "No Record found"}})))
        .mount(&server).await;
    let mut cmd = Command::cargo_bin("sn").unwrap();
    cmd.env("SN_INSTANCE", server.uri()).env("SN_USERNAME", "u").env("SN_PASSWORD", "p")
        .args(["table", "get", "incident", "missing"])
        .assert().code(2);
}
```

- [ ] **Step 5: Run tests & commit**

Run: `cargo test --test table_get`. Expected: `2 passed`.

```bash
git add src/cli/mod.rs src/cli/table.rs src/main.rs tests/table_get.rs
git commit -m "feat: add sn table get"
```

---

## Task 17: `sn table create`

**Files:**
- Modify: `src/cli/mod.rs` (flesh out `TableCreateArgs`)
- Modify: `src/cli/table.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Expand `TableCreateArgs`**

In `src/cli/mod.rs`:

```rust
#[derive(clap::Args, Debug)]
pub struct TableCreateArgs {
    pub table: String,
    /// Inline JSON, @file, or @- for stdin.
    #[arg(long, conflicts_with = "field")]
    pub data: Option<String>,
    /// Repeatable name=value. Mutually exclusive with --data.
    #[arg(long = "field", conflicts_with = "data")]
    pub field: Vec<String>,
    #[arg(long, alias = "sysparm-fields")]
    pub fields: Option<String>,
    #[arg(long, alias = "sysparm-display-value", value_enum)]
    pub display_value: Option<DisplayValueArg>,
    #[arg(long, alias = "sysparm-exclude-reference-link")]
    pub exclude_reference_link: bool,
    #[arg(long, alias = "sysparm-input-display-value")]
    pub input_display_value: bool,
    #[arg(long, alias = "sysparm-suppress-auto-sys-field")]
    pub suppress_auto_sys_field: bool,
    #[arg(long, alias = "sysparm-view")]
    pub view: Option<String>,
}
```

- [ ] **Step 2: Implement `create` in `src/cli/table.rs`**

```rust
use crate::body::{build_body, BodyInput};
use crate::cli::TableCreateArgs;
use crate::query::WriteQuery;

pub fn create(global: &GlobalFlags, args: TableCreateArgs) -> Result<()> {
    let body_input = match (args.data, args.field.is_empty()) {
        (Some(d), true) => BodyInput::Data(d),
        (None, false) => BodyInput::Fields(args.field),
        (None, true) => return Err(Error::Usage("provide --data or one or more --field".into())),
        (Some(_), false) => return Err(Error::Usage("--data and --field are mutually exclusive".into())),
    };
    let body = build_body(body_input)?;

    let profile = build_profile(global)?;
    let client = Client::builder().retry(retry_policy(global.no_retry)).build(&profile)?;
    let q = WriteQuery {
        fields: args.fields,
        display_value: args.display_value.map(Into::into),
        exclude_reference_link: bool_opt(args.exclude_reference_link),
        input_display_value: bool_opt(args.input_display_value),
        suppress_auto_sys_field: bool_opt(args.suppress_auto_sys_field),
        view: args.view,
        query_no_domain: None, // POST ignores
    };
    let path = format!("/api/now/table/{}", args.table);
    let resp = client.post(&path, &q.to_pairs(), &body)?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}
```

- [ ] **Step 3: Dispatch in `main.rs`**

```rust
TableSub::Create(args) => sn::cli::table::create(&cli.global, args),
```

- [ ] **Step 4: Integration test**

Create `tests/table_create.rs`:

```rust
mod common;

use assert_cmd::Command;
use serde_json::json;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test(flavor = "current_thread")]
async fn create_with_fields() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("POST")).and(path("/api/now/table/incident"))
        .and(body_partial_json(json!({"short_description": "sd", "urgency": 2})))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({"result": {"sys_id": "new", "short_description": "sd"}})))
        .mount(&server).await;
    let mut cmd = Command::cargo_bin("sn").unwrap();
    cmd.env("SN_INSTANCE", server.uri()).env("SN_USERNAME", "u").env("SN_PASSWORD", "p")
        .args(["--compact", "table", "create", "incident",
               "--field", "short_description=sd",
               "--field", "urgency=2"])
        .assert().success()
        .stdout(predicates::str::contains("\"sys_id\":\"new\""));
}

#[tokio::test(flavor = "current_thread")]
async fn data_and_field_together_is_usage_error() {
    let mut cmd = Command::cargo_bin("sn").unwrap();
    cmd.env("SN_INSTANCE", "http://127.0.0.1:1").env("SN_USERNAME", "u").env("SN_PASSWORD", "p")
        .args(["table", "create", "incident", "--data", "{}", "--field", "x=1"])
        .assert().code(2); // clap exits with 2 for conflicts; check actual value and adjust
}
```

Note: clap's conflicting-arg exit is typically `2` from clap itself. If the test fails because our mapping yields `1`, change the assertion to match whatever clap emits (this is a usage error either way).

- [ ] **Step 5: Run tests & commit**

Run: `cargo test --test table_create`. Expected: `2 passed`.

```bash
git add src/cli/mod.rs src/cli/table.rs src/main.rs tests/table_create.rs
git commit -m "feat: add sn table create"
```

---

## Task 18: `sn table update` (PATCH) and `sn table replace` (PUT)

**Files:**
- Modify: `src/cli/mod.rs` (flesh out `TableUpdateArgs`, `TableReplaceArgs`)
- Modify: `src/cli/table.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Expand update/replace args**

Both take a `sys_id` and the same write-body flags. In `src/cli/mod.rs`:

```rust
#[derive(clap::Args, Debug)]
pub struct TableUpdateArgs {
    pub table: String,
    pub sys_id: String,
    #[arg(long, conflicts_with = "field")]
    pub data: Option<String>,
    #[arg(long = "field", conflicts_with = "data")]
    pub field: Vec<String>,
    #[arg(long, alias = "sysparm-fields")] pub fields: Option<String>,
    #[arg(long, alias = "sysparm-display-value", value_enum)] pub display_value: Option<DisplayValueArg>,
    #[arg(long, alias = "sysparm-exclude-reference-link")] pub exclude_reference_link: bool,
    #[arg(long, alias = "sysparm-input-display-value")] pub input_display_value: bool,
    #[arg(long, alias = "sysparm-suppress-auto-sys-field")] pub suppress_auto_sys_field: bool,
    #[arg(long, alias = "sysparm-view")] pub view: Option<String>,
    #[arg(long, alias = "sysparm-query-no-domain")] pub query_no_domain: bool,
}

#[derive(clap::Args, Debug)]
pub struct TableReplaceArgs {
    pub table: String,
    pub sys_id: String,
    #[arg(long, conflicts_with = "field")]
    pub data: Option<String>,
    #[arg(long = "field", conflicts_with = "data")]
    pub field: Vec<String>,
    #[arg(long, alias = "sysparm-fields")] pub fields: Option<String>,
    #[arg(long, alias = "sysparm-display-value", value_enum)] pub display_value: Option<DisplayValueArg>,
    #[arg(long, alias = "sysparm-exclude-reference-link")] pub exclude_reference_link: bool,
    #[arg(long, alias = "sysparm-input-display-value")] pub input_display_value: bool,
    #[arg(long, alias = "sysparm-suppress-auto-sys-field")] pub suppress_auto_sys_field: bool,
    #[arg(long, alias = "sysparm-view")] pub view: Option<String>,
    #[arg(long, alias = "sysparm-query-no-domain")] pub query_no_domain: bool,
}
```

- [ ] **Step 2: Implement `update` and `replace` in `src/cli/table.rs`**

```rust
use crate::cli::{TableReplaceArgs, TableUpdateArgs};

pub fn update(global: &GlobalFlags, args: TableUpdateArgs) -> Result<()> {
    write_op(
        global, args.table, args.sys_id,
        args.data, args.field,
        args.fields, args.display_value, args.exclude_reference_link,
        args.input_display_value, args.suppress_auto_sys_field, args.view, args.query_no_domain,
        HttpMutation::Patch,
    )
}

pub fn replace(global: &GlobalFlags, args: TableReplaceArgs) -> Result<()> {
    write_op(
        global, args.table, args.sys_id,
        args.data, args.field,
        args.fields, args.display_value, args.exclude_reference_link,
        args.input_display_value, args.suppress_auto_sys_field, args.view, args.query_no_domain,
        HttpMutation::Put,
    )
}

enum HttpMutation { Patch, Put }

#[allow(clippy::too_many_arguments)]
fn write_op(
    global: &GlobalFlags,
    table: String,
    sys_id: String,
    data: Option<String>,
    field: Vec<String>,
    fields: Option<String>,
    display_value: Option<crate::cli::DisplayValueArg>,
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
        (Some(_), false) => return Err(Error::Usage("--data and --field are mutually exclusive".into())),
    };
    let body = build_body(body_input)?;
    let profile = build_profile(global)?;
    let client = Client::builder().retry(retry_policy(global.no_retry)).build(&profile)?;
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
```

- [ ] **Step 3: Dispatch in `main.rs`**

```rust
TableSub::Update(args) => sn::cli::table::update(&cli.global, args),
TableSub::Replace(args) => sn::cli::table::replace(&cli.global, args),
```

- [ ] **Step 4: Integration test**

Create `tests/table_write.rs`:

```rust
mod common;

use assert_cmd::Command;
use serde_json::json;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test(flavor = "current_thread")]
async fn update_sends_patch_with_only_named_fields() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("PATCH")).and(path("/api/now/table/incident/abc"))
        .and(body_partial_json(json!({"state": "2"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": {"sys_id": "abc", "state": "2"}})))
        .mount(&server).await;
    let mut cmd = Command::cargo_bin("sn").unwrap();
    cmd.env("SN_INSTANCE", server.uri()).env("SN_USERNAME", "u").env("SN_PASSWORD", "p")
        .args(["--compact", "table", "update", "incident", "abc", "--field", "state=2"])
        .assert().success();
}

#[tokio::test(flavor = "current_thread")]
async fn replace_sends_put_with_full_body() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("PUT")).and(path("/api/now/table/incident/abc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": {"sys_id": "abc"}})))
        .mount(&server).await;
    let mut cmd = Command::cargo_bin("sn").unwrap();
    cmd.env("SN_INSTANCE", server.uri()).env("SN_USERNAME", "u").env("SN_PASSWORD", "p")
        .args(["--compact", "table", "replace", "incident", "abc", "--data", "{\"number\":\"INC1\"}"])
        .assert().success();
}
```

- [ ] **Step 5: Run tests & commit**

Run: `cargo test --test table_write`. Expected: `2 passed`.

```bash
git add src/cli/mod.rs src/cli/table.rs src/main.rs tests/table_write.rs
git commit -m "feat: add sn table update (PATCH) and replace (PUT)"
```

---

## Task 19: `sn table delete`

**Files:**
- Modify: `src/cli/mod.rs` (flesh out `TableDeleteArgs`)
- Modify: `src/cli/table.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Expand `TableDeleteArgs`**

```rust
#[derive(clap::Args, Debug)]
pub struct TableDeleteArgs {
    pub table: String,
    pub sys_id: String,
    /// Skip confirmation prompt (required for non-interactive use).
    #[arg(long, short = 'y')]
    pub yes: bool,
    #[arg(long, alias = "sysparm-query-no-domain")]
    pub query_no_domain: bool,
}
```

- [ ] **Step 2: Implement `delete` in `src/cli/table.rs`**

```rust
use crate::cli::TableDeleteArgs;
use crate::query::DeleteQuery;
use is_terminal::IsTerminal;

pub fn delete(global: &GlobalFlags, args: TableDeleteArgs) -> Result<()> {
    if !args.yes {
        if !std::io::stdin().is_terminal() {
            return Err(Error::Usage(
                "delete requires --yes when stdin is not a terminal".into(),
            ));
        }
        eprint!("Delete {}/{}? [y/N]: ", args.table, args.sys_id);
        let mut s = String::new();
        std::io::stdin().read_line(&mut s).map_err(|e| Error::Usage(format!("read stdin: {e}")))?;
        if !matches!(s.trim(), "y" | "Y" | "yes" | "YES") {
            return Err(Error::Usage("aborted".into()));
        }
    }
    let profile = build_profile(global)?;
    let client = Client::builder().retry(retry_policy(global.no_retry)).build(&profile)?;
    let q = DeleteQuery { query_no_domain: bool_opt(args.query_no_domain) };
    let path = format!("/api/now/table/{}/{}", args.table, args.sys_id);
    client.delete(&path, &q.to_pairs())
}
```

- [ ] **Step 3: Dispatch in `main.rs`**

```rust
TableSub::Delete(args) => sn::cli::table::delete(&cli.global, args),
```

- [ ] **Step 4: Integration test**

Append to `tests/table_write.rs`:

```rust
#[tokio::test(flavor = "current_thread")]
async fn delete_with_yes_succeeds() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("DELETE")).and(path("/api/now/table/incident/abc"))
        .respond_with(ResponseTemplate::new(204)).mount(&server).await;
    let mut cmd = Command::cargo_bin("sn").unwrap();
    cmd.env("SN_INSTANCE", server.uri()).env("SN_USERNAME", "u").env("SN_PASSWORD", "p")
        .args(["table", "delete", "incident", "abc", "--yes"])
        .assert().success().stdout("");
}

#[tokio::test(flavor = "current_thread")]
async fn delete_without_yes_in_non_tty_errors() {
    let mut cmd = Command::cargo_bin("sn").unwrap();
    cmd.env("SN_INSTANCE", "http://127.0.0.1:1").env("SN_USERNAME", "u").env("SN_PASSWORD", "p")
        .args(["table", "delete", "incident", "abc"])
        .assert().code(1);
}
```

- [ ] **Step 5: Run tests & commit**

Run: `cargo test --test table_write`. Expected: `4 passed` total.

```bash
git add src/cli/mod.rs src/cli/table.rs src/main.rs tests/table_write.rs
git commit -m "feat: add sn table delete with --yes guardrail"
```

---

## Task 20: `--all` pagination with Link header following

**Files:**
- Modify: `src/client.rs` (expose paginator)
- Modify: `src/cli/table.rs` (use paginator in list)
- Add test: `tests/pagination.rs`

Spec references: §7.

- [ ] **Step 1: Add paginator to `src/client.rs`**

Append:

```rust
impl Client {
    /// Stream records from a paginated list endpoint, following Link: rel="next" headers.
    pub fn paginate(
        &self,
        initial_path: &str,
        initial_query: &[(String, String)],
        max_records: Option<u32>,
    ) -> Paginator<'_> {
        Paginator::new(self, initial_path.to_string(), initial_query.to_vec(), max_records)
    }
}

pub struct Paginator<'a> {
    client: &'a Client,
    next_url: Option<String>,
    next_query: Vec<(String, String)>,
    buffer: std::collections::VecDeque<Value>,
    emitted: u32,
    cap: Option<u32>,
    finished: bool,
}

impl<'a> Paginator<'a> {
    fn new(client: &'a Client, path: String, query: Vec<(String, String)>, cap: Option<u32>) -> Self {
        Self {
            client,
            next_url: Some(format!("{}{path}", client.base_url)),
            next_query: query,
            buffer: std::collections::VecDeque::new(),
            emitted: 0,
            cap,
            finished: false,
        }
    }

    fn fetch_next_page(&mut self) -> Result<()> {
        let Some(url) = self.next_url.take() else { self.finished = true; return Ok(()); };
        let req = self.client.http.request(Method::GET, &url)
            .basic_auth(&self.client.username, Some(&self.client.password))
            .query(&self.next_query);
        let resp = execute_request_with_retry(self.client.retry, || req.try_clone().unwrap().send())?;
        let status = resp.status();
        let tx = transaction_id(&resp);
        let link = resp.headers().get("Link").and_then(|v| v.to_str().ok()).map(ToString::to_string);
        if !status.is_success() {
            return Err(from_http(status, tx, resp));
        }
        let body: Value = resp.json().map_err(|e| Error::Transport(format!("parse response: {e}")))?;
        if let Value::Array(records) = body.get("result").cloned().unwrap_or(Value::Array(vec![])) {
            for r in records { self.buffer.push_back(r); }
        }
        self.next_query.clear(); // next link carries all params
        self.next_url = link.and_then(parse_next_link);
        if self.next_url.is_none() { self.finished = true; }
        Ok(())
    }
}

fn parse_next_link(header: String) -> Option<String> {
    // ServiceNow Link: <...>;rel="next", <...>;rel="first", ...
    for part in header.split(',') {
        let part = part.trim();
        if let Some((url_part, rel_part)) = part.split_once(';') {
            let rel = rel_part.trim();
            if rel.contains("rel=\"next\"") {
                let u = url_part.trim().trim_start_matches('<').trim_end_matches('>');
                return Some(u.to_string());
            }
        }
    }
    None
}

impl<'a> Iterator for Paginator<'a> {
    type Item = Result<Value>;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(cap) = self.cap {
            if cap != 0 && self.emitted >= cap { return None; }
        }
        if self.buffer.is_empty() && !self.finished {
            if let Err(e) = self.fetch_next_page() {
                self.finished = true;
                return Some(Err(e));
            }
        }
        match self.buffer.pop_front() {
            Some(v) => { self.emitted += 1; Some(Ok(v)) }
            None => None,
        }
    }
}

fn execute_request_with_retry<F>(policy: RetryPolicy, mut send: F) -> std::result::Result<Response, Error>
where F: FnMut() -> std::result::Result<Response, reqwest::Error>
{
    let mut attempt = 0;
    let mut backoff = policy.initial_backoff;
    loop {
        attempt += 1;
        match send() {
            Ok(resp) => {
                let status = resp.status();
                let retryable = policy.enabled && should_retry(status) && attempt < policy.max_attempts;
                if !status.is_success() && retryable {
                    std::thread::sleep(jittered(backoff));
                    backoff = backoff.saturating_mul(2);
                    continue;
                }
                return Ok(resp);
            }
            Err(e) => return Err(Error::Transport(format!("{e}"))),
        }
    }
}
```

(The existing `execute_with_retry` in Task 7 stays for body-returning operations; this helper is needed so the paginator can inspect status/headers.)

- [ ] **Step 2: Use paginator in `table::list`**

Replace the `if args.all { ... }` stub with real logic:

```rust
if args.all {
    let mut q = ListQuery {
        query: args.query.clone(),
        fields: args.fields.clone(),
        page_size: Some(args.page_size),
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
    let cap = if args.max_records == 0 { None } else { Some(args.max_records) };
    let it = client.paginate(&path, &q.to_pairs(), cap);

    if args.array {
        let mut out = Vec::new();
        for r in it { out.push(r?); }
        emit_value(io::stdout().lock(), &Value::Array(out), format_from_flags(global))
            .map_err(|e| Error::Usage(format!("stdout: {e}")))?;
    } else {
        let mut stdout = io::stdout().lock();
        for r in it {
            let v = r?;
            serde_json::to_writer(&mut stdout, &v).map_err(|e| Error::Usage(format!("stdout: {e}")))?;
            stdout.write_all(b"\n").map_err(|e| Error::Usage(format!("stdout: {e}")))?;
        }
    }
    return Ok(());
}
```

Add `use std::io::Write;` near the top of `table.rs`.

- [ ] **Step 3: Integration test**

Create `tests/pagination.rs`:

```rust
mod common;

use assert_cmd::Command;
use serde_json::json;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test(flavor = "current_thread")]
async fn paginates_following_link_header() {
    let server = wiremock::MockServer::start().await;
    let next_link = format!("<{}/api/now/table/incident?sysparm_offset=2>;rel=\"next\"", server.uri());
    Mock::given(method("GET")).and(path("/api/now/table/incident")).and(query_param("sysparm_limit", "2"))
        .respond_with(ResponseTemplate::new(200)
            .insert_header("Link", &next_link)
            .set_body_json(json!({"result": [{"n": 1}, {"n": 2}]})))
        .expect(1).mount(&server).await;
    Mock::given(method("GET")).and(path("/api/now/table/incident")).and(query_param("sysparm_offset", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": [{"n": 3}]})))
        .expect(1).mount(&server).await;

    let mut cmd = Command::cargo_bin("sn").unwrap();
    let out = cmd.env("SN_INSTANCE", server.uri()).env("SN_USERNAME", "u").env("SN_PASSWORD", "p")
        .args(["table", "list", "incident", "--page-size", "2", "--all"])
        .assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert_eq!(stdout.lines().count(), 3);
    assert!(stdout.contains("\"n\":1"));
    assert!(stdout.contains("\"n\":3"));
}

#[tokio::test(flavor = "current_thread")]
async fn max_records_caps_output() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET")).and(path("/api/now/table/incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": [{"n":1},{"n":2},{"n":3}]})))
        .mount(&server).await;
    let mut cmd = Command::cargo_bin("sn").unwrap();
    let out = cmd.env("SN_INSTANCE", server.uri()).env("SN_USERNAME", "u").env("SN_PASSWORD", "p")
        .args(["table", "list", "incident", "--all", "--max-records", "2"])
        .assert().success();
    assert_eq!(String::from_utf8(out.get_output().stdout.clone()).unwrap().lines().count(), 2);
}
```

- [ ] **Step 4: Run tests & commit**

Run: `cargo test --test pagination`. Expected: `2 passed`.

```bash
git add src/client.rs src/cli/table.rs tests/pagination.rs
git commit -m "feat: auto-paginate sn table list --all via Link header"
```

---

## Task 21: `sn schema tables`

**Files:**
- Create: `src/cli/schema.rs`
- Modify: `src/cli/mod.rs` (flesh out `SchemaTablesArgs`, register module)
- Modify: `src/main.rs`

Spec references: §2 schema commands, §13 spec-deviations (undocumented endpoint).

- [ ] **Step 1: Expand `SchemaTablesArgs`**

```rust
#[derive(clap::Args, Debug, Default)]
pub struct SchemaTablesArgs {
    /// Case-insensitive substring match on label or value.
    #[arg(long)]
    pub filter: Option<String>,
    /// Only tables marked `reference=true` in the schema.
    #[arg(long)]
    pub reference_only: bool,
}
```

- [ ] **Step 2: Write `src/cli/schema.rs`**

```rust
use crate::cli::{GlobalFlags, OutputMode, SchemaTablesArgs};
use crate::client::Client;
use crate::error::{Error, Result};
use crate::output::{emit_value, Format};
use serde_json::Value;
use std::io;

pub fn tables(global: &GlobalFlags, args: SchemaTablesArgs) -> Result<()> {
    let profile = crate::cli::table::build_profile(global)?;
    let retry = crate::cli::table::retry_policy(global.no_retry);
    let client = Client::builder().retry(retry).build(&profile)?;
    let resp = client.get("/api/now/doc/table/schema", &[])?;
    let list = match (global.output, resp.get("result")) {
        (OutputMode::Raw, _) => resp.clone(),
        (_, Some(Value::Array(a))) => Value::Array(filter_tables(a.clone(), &args)),
        _ => resp.clone(),
    };
    let fmt = crate::cli::table::format_from_flags(global);
    emit_value(io::stdout().lock(), &list, fmt).map_err(|e| Error::Usage(format!("stdout: {e}")))
}

fn filter_tables(items: Vec<Value>, args: &SchemaTablesArgs) -> Vec<Value> {
    let needle = args.filter.as_deref().map(str::to_lowercase);
    items.into_iter().filter(|t| {
        if args.reference_only && !t.get("reference").and_then(|v| v.as_bool()).unwrap_or(false) {
            return false;
        }
        if let Some(n) = &needle {
            let label = t.get("label").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
            let value = t.get("value").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
            if !label.contains(n) && !value.contains(n) {
                return false;
            }
        }
        true
    }).collect()
}
```

Note: `build_profile`, `retry_policy`, and `format_from_flags` must be made `pub(crate)` in `cli/table.rs` (they already are in Task 15's code).

- [ ] **Step 3: Register and dispatch**

In `src/cli/mod.rs` add `pub mod schema;`.

In `src/main.rs`:

```rust
Command::Schema { sub } => match sub {
    SchemaSub::Tables(args) => sn::cli::schema::tables(&cli.global, args),
    SchemaSub::Columns(_) | SchemaSub::Choices(_) => Err(Error::Usage("schema subcommand not yet wired".into())),
},
```

Import `SchemaSub` at top.

- [ ] **Step 4: Integration test**

Create `tests/schema.rs`:

```rust
mod common;

use assert_cmd::Command;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

#[tokio::test(flavor = "current_thread")]
async fn schema_tables_filter() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET")).and(path("/api/now/doc/table/schema"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": [
            {"label": "Incident", "value": "incident", "reference": false},
            {"label": "Incident Task", "value": "incident_task", "reference": false},
            {"label": "User", "value": "sys_user", "reference": true}
        ]})))
        .mount(&server).await;
    let mut cmd = Command::cargo_bin("sn").unwrap();
    let out = cmd.env("SN_INSTANCE", server.uri()).env("SN_USERNAME", "u").env("SN_PASSWORD", "p")
        .args(["--compact", "schema", "tables", "--filter", "incident"])
        .assert().success();
    let s = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(s.contains("\"incident\""));
    assert!(s.contains("\"incident_task\""));
    assert!(!s.contains("sys_user"));
}
```

- [ ] **Step 5: Run tests & commit**

Run: `cargo test --test schema`. Expected: `1 passed`.

```bash
git add src/cli/schema.rs src/cli/mod.rs src/main.rs tests/schema.rs
git commit -m "feat: add sn schema tables"
```

---

## Task 22: `sn schema columns`

**Files:**
- Modify: `src/cli/mod.rs` (`SchemaColumnsArgs`)
- Modify: `src/cli/schema.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Expand `SchemaColumnsArgs`**

```rust
#[derive(clap::Args, Debug)]
pub struct SchemaColumnsArgs {
    pub table: String,
    #[arg(long)] pub filter: Option<String>,
    #[arg(long, value_name = "TYPE")] pub r#type: Option<String>,
    #[arg(long)] pub mandatory: bool,
    #[arg(long)] pub writable: bool,
    #[arg(long)] pub choices_only: bool,
    #[arg(long)] pub references_only: bool,
}
```

- [ ] **Step 2: Implement in `src/cli/schema.rs`**

```rust
use crate::cli::SchemaColumnsArgs;

pub fn columns(global: &GlobalFlags, args: SchemaColumnsArgs) -> Result<()> {
    let profile = crate::cli::table::build_profile(global)?;
    let client = Client::builder().retry(crate::cli::table::retry_policy(global.no_retry)).build(&profile)?;
    let path = format!("/api/now/ui/meta/{}", args.table);
    let resp = client.get(&path, &[])?;
    let list = match global.output {
        OutputMode::Raw => resp.clone(),
        OutputMode::Default => {
            let cols = resp.get("result").and_then(|r| r.get("columns"))
                .cloned().unwrap_or(Value::Object(serde_json::Map::new()));
            Value::Array(filter_columns(cols, &args))
        }
    };
    emit_value(io::stdout().lock(), &list, crate::cli::table::format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

fn filter_columns(cols: Value, args: &SchemaColumnsArgs) -> Vec<Value> {
    let cols_obj = match cols { Value::Object(m) => m, _ => return vec![] };
    cols_obj.into_iter()
        .map(|(name, mut v)| {
            if let Value::Object(ref mut m) = v { m.insert("name".into(), Value::String(name)); }
            v
        })
        .filter(|v| keep_column(v, args))
        .collect()
}

fn keep_column(col: &Value, args: &SchemaColumnsArgs) -> bool {
    let getb = |k: &str| col.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    let gets = |k: &str| col.get(k).and_then(|v| v.as_str()).unwrap_or("");
    if args.mandatory && !getb("mandatory") { return false; }
    if args.writable && getb("read_only") { return false; }
    if args.choices_only && col.get("choices").and_then(|v| v.as_array()).map_or(true, |a| a.is_empty()) { return false; }
    if args.references_only && gets("type") != "reference" { return false; }
    if let Some(t) = args.r#type.as_deref() {
        if !gets("type").eq_ignore_ascii_case(t) { return false; }
    }
    if let Some(n) = args.filter.as_deref().map(str::to_lowercase) {
        let name = gets("name").to_lowercase();
        let label = gets("label").to_lowercase();
        if !name.contains(&n) && !label.contains(&n) { return false; }
    }
    true
}
```

- [ ] **Step 3: Dispatch in `main.rs`**

```rust
SchemaSub::Columns(args) => sn::cli::schema::columns(&cli.global, args),
```

- [ ] **Step 4: Integration test**

Append to `tests/schema.rs`:

```rust
#[tokio::test(flavor = "current_thread")]
async fn schema_columns_writable_filter() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET")).and(path("/api/now/ui/meta/incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": {"columns": {
            "short_description": {"label": "Short description", "type": "string", "mandatory": true, "read_only": false},
            "sys_id": {"label": "Sys ID", "type": "GUID", "mandatory": false, "read_only": true},
            "state": {"label": "State", "type": "integer", "mandatory": true, "read_only": false, "choices": [{"value": "1", "label": "New"}]}
        }}})))
        .mount(&server).await;
    let mut cmd = Command::cargo_bin("sn").unwrap();
    let out = cmd.env("SN_INSTANCE", server.uri()).env("SN_USERNAME", "u").env("SN_PASSWORD", "p")
        .args(["--compact", "schema", "columns", "incident", "--writable"])
        .assert().success();
    let s = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(s.contains("short_description"));
    assert!(s.contains("state"));
    assert!(!s.contains("sys_id"));
}
```

- [ ] **Step 5: Run tests & commit**

Run: `cargo test --test schema`. Expected: `2 passed`.

```bash
git add src/cli/mod.rs src/cli/schema.rs src/main.rs tests/schema.rs
git commit -m "feat: add sn schema columns"
```

---

## Task 23: `sn schema choices`

**Files:**
- Modify: `src/cli/mod.rs` (`SchemaChoicesArgs`)
- Modify: `src/cli/schema.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: `SchemaChoicesArgs` is already `pub table: String, pub field: String`. Keep as-is.**

- [ ] **Step 2: Implement `choices`**

```rust
use crate::cli::SchemaChoicesArgs;

pub fn choices(global: &GlobalFlags, args: SchemaChoicesArgs) -> Result<()> {
    let profile = crate::cli::table::build_profile(global)?;
    let client = Client::builder().retry(crate::cli::table::retry_policy(global.no_retry)).build(&profile)?;
    let path = format!("/api/now/ui/meta/{}", args.table);
    let resp = client.get(&path, &[])?;
    let out = match global.output {
        OutputMode::Raw => resp.clone(),
        OutputMode::Default => {
            let choices = resp.get("result")
                .and_then(|r| r.get("columns"))
                .and_then(|c| c.get(&args.field))
                .and_then(|f| f.get("choices"))
                .cloned()
                .ok_or_else(|| Error::Usage(format!("no choices found on field '{}' in table '{}'", args.field, args.table)))?;
            choices
        }
    };
    emit_value(io::stdout().lock(), &out, crate::cli::table::format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}
```

- [ ] **Step 3: Dispatch**

```rust
SchemaSub::Choices(args) => sn::cli::schema::choices(&cli.global, args),
```

- [ ] **Step 4: Integration test**

Append to `tests/schema.rs`:

```rust
#[tokio::test(flavor = "current_thread")]
async fn schema_choices_for_field() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET")).and(path("/api/now/ui/meta/incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": {"columns": {
            "state": {"label": "State", "type": "integer",
                      "choices": [{"value": "1", "label": "New"}, {"value": "2", "label": "In Progress"}]}
        }}})))
        .mount(&server).await;
    let mut cmd = Command::cargo_bin("sn").unwrap();
    let out = cmd.env("SN_INSTANCE", server.uri()).env("SN_USERNAME", "u").env("SN_PASSWORD", "p")
        .args(["--compact", "schema", "choices", "incident", "state"])
        .assert().success();
    let s = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(s.contains("\"New\""));
    assert!(s.contains("\"In Progress\""));
}

#[tokio::test(flavor = "current_thread")]
async fn schema_choices_missing_field_is_usage_error() {
    let server = wiremock::MockServer::start().await;
    Mock::given(method("GET")).and(path("/api/now/ui/meta/incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": {"columns": {"state": {"choices": []}}}})))
        .mount(&server).await;
    let mut cmd = Command::cargo_bin("sn").unwrap();
    cmd.env("SN_INSTANCE", server.uri()).env("SN_USERNAME", "u").env("SN_PASSWORD", "p")
        .args(["schema", "choices", "incident", "bogus_field"])
        .assert().code(1);
}
```

- [ ] **Step 5: Run tests & commit**

Run: `cargo test --test schema`. Expected: `4 passed`.

```bash
git add src/cli/schema.rs src/main.rs tests/schema.rs
git commit -m "feat: add sn schema choices"
```

---

## Task 24: `sn introspect`

**Files:**
- Create: `src/cli/introspect.rs`
- Modify: `src/cli/mod.rs` (register), `src/main.rs` (dispatch)

Spec references: §12.

- [ ] **Step 1: Write `src/cli/introspect.rs`**

The goal is structured JSON describing every subcommand and flag. `clap::Command` exposes enough reflection.

```rust
use crate::cli::Cli;
use crate::error::{Error, Result};
use crate::output::{emit_value, Format};
use clap::{Arg, Command as ClapCommand, CommandFactory};
use serde_json::{json, Value};
use std::io;

pub fn run() -> Result<()> {
    let cmd = Cli::command();
    let tree = describe_command(&cmd, "sn");
    emit_value(io::stdout().lock(), &tree, Format::Auto.resolve())
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

fn describe_command(cmd: &ClapCommand, name: &str) -> Value {
    let args: Vec<Value> = cmd.get_arguments()
        .filter(|a| !a.is_hide_set())
        .map(describe_arg)
        .collect();
    let subs: Vec<Value> = cmd.get_subcommands()
        .map(|sc| describe_command(sc, sc.get_name()))
        .collect();
    json!({
        "name": name,
        "about": cmd.get_about().map(|s| s.to_string()),
        "args": args,
        "subcommands": subs,
    })
}

fn describe_arg(a: &Arg) -> Value {
    json!({
        "name": a.get_id().as_str(),
        "long": a.get_long(),
        "short": a.get_short(),
        "aliases": a.get_all_aliases().map(|v| v.collect::<Vec<_>>()).unwrap_or_default(),
        "help": a.get_help().map(|s| s.to_string()),
        "required": a.is_required_set(),
        "takes_value": !a.get_num_args().map_or(false, |n| n.min_values() == 0),
        "possible_values": a.get_possible_values().iter().map(|p| p.get_name().to_string()).collect::<Vec<_>>(),
    })
}
```

- [ ] **Step 2: Register and dispatch**

In `src/cli/mod.rs`: `pub mod introspect;`

In `src/main.rs`:

```rust
Command::Introspect => sn::cli::introspect::run(),
```

(Remove the earlier placeholder println!.)

- [ ] **Step 3: Smoke test**

Run: `./target/debug/sn introspect | jq '.subcommands[].name'`
Expected: prints `"init"`, `"auth"`, `"profile"`, `"table"`, `"schema"`, `"introspect"`.

- [ ] **Step 4: Integration test**

Create `tests/introspect.rs`:

```rust
use assert_cmd::Command;
use serde_json::Value;

#[test]
fn introspect_lists_all_subcommands() {
    let out = Command::cargo_bin("sn").unwrap().args(["introspect"]).assert().success();
    let v: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    let names: Vec<String> = v["subcommands"].as_array().unwrap().iter()
        .filter_map(|s| s["name"].as_str().map(String::from)).collect();
    for expected in ["init", "auth", "profile", "table", "schema", "introspect"] {
        assert!(names.iter().any(|n| n == expected), "missing subcommand {expected}");
    }
}
```

- [ ] **Step 5: Run tests & commit**

Run: `cargo test --test introspect`. Expected: `1 passed`.

```bash
git add src/cli/introspect.rs src/cli/mod.rs src/main.rs tests/introspect.rs
git commit -m "feat: add sn introspect to dump command tree as JSON"
```

---

## Task 25: Observability (`-v` / `-vv` / `-vvv`) with masked Authorization

**Files:**
- Create: `src/observability.rs`
- Modify: `src/lib.rs`, `src/client.rs`, `src/main.rs`

Spec references: §9.

- [ ] **Step 1: Write `src/observability.rs`**

```rust
use std::sync::atomic::{AtomicU8, Ordering};

static LEVEL: AtomicU8 = AtomicU8::new(0);

pub fn set_level(level: u8) { LEVEL.store(level, Ordering::SeqCst); }
pub fn level() -> u8 { LEVEL.load(Ordering::SeqCst) }

pub fn log_request(method: &str, url: &str) {
    if level() >= 1 { eprintln!("sn: {method} {url}"); }
}

pub fn log_response(status: u16, elapsed_ms: u128) {
    if level() >= 1 { eprintln!("sn: -> {status} ({elapsed_ms}ms)"); }
}

pub fn log_request_headers(headers: &reqwest::header::HeaderMap) {
    if level() >= 2 {
        for (k, v) in headers {
            let name = k.as_str();
            let value = if name.eq_ignore_ascii_case("authorization") {
                "Basic ****".to_string()
            } else {
                v.to_str().unwrap_or("<bin>").to_string()
            };
            eprintln!("sn: > {name}: {value}");
        }
    }
}

pub fn log_response_headers(headers: &reqwest::header::HeaderMap) {
    if level() >= 2 {
        for (k, v) in headers {
            eprintln!("sn: < {}: {}", k.as_str(), v.to_str().unwrap_or("<bin>"));
        }
    }
}

pub fn log_body(direction: &str, body: &str) {
    if level() >= 3 {
        let trimmed = if body.len() > 4096 { &body[..4096] } else { body };
        eprintln!("sn: {direction} body: {trimmed}");
    }
}
```

- [ ] **Step 2: Register and wire into `src/client.rs`**

In `src/lib.rs`: `pub mod observability;`.

In `src/client.rs`, wrap each HTTP call: log request line before sending, capture the request body if `-vvv`, then after response: log status + elapsed + response headers + body. Use `std::time::Instant` for timing.

Pattern (inside each HTTP method, before returning response):

```rust
crate::observability::log_request("GET", &url);
// clone req for body logging if needed
let start = std::time::Instant::now();
let resp = send()?;
let elapsed_ms = start.elapsed().as_millis();
crate::observability::log_response(resp.status().as_u16(), elapsed_ms);
crate::observability::log_response_headers(resp.headers());
```

For request-header logging, you need to build the request once (not clone). Simplest: log the `Authorization: Basic ****` line manually inside the request builder path, since reqwest hides the internal header map until send.

- [ ] **Step 3: Wire verbosity flag in `src/main.rs`**

```rust
fn main() -> ExitCode {
    let cli = Cli::parse();
    sn::observability::set_level(cli.global.verbose);
    match run(cli) { ... }
}
```

- [ ] **Step 4: Smoke test**

Run: `./target/debug/sn -v table list incident --page-size 1 2>&1 | grep 'sn:'`
Expected: at least one line of `sn: GET ...` / `sn: -> 200 ...` on stderr.

- [ ] **Step 5: Commit**

```bash
git add src/observability.rs src/lib.rs src/client.rs src/main.rs
git commit -m "feat: add -v/-vv/-vvv logging with masked Authorization"
```

---

## Task 26: GitHub Actions CI workflow

**Files:**
- Create: `.github/workflows/ci.yml`

Spec references: §11.

- [ ] **Step 1: Write `.github/workflows/ci.yml`**

```yaml
name: CI
on:
  pull_request:
  push:
    branches: [main]

jobs:
  test:
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all -- --check
      - run: cargo clippy --all-targets --all-features -- -D warnings
      - run: cargo test --all-features --workspace
```

- [ ] **Step 2: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "chore: add CI workflow (fmt, clippy, test on linux/macos/windows)"
```

---

## Task 27: Release workflow with `cargo-dist`

**Files:**
- Modify: `Cargo.toml` (cargo-dist metadata)
- Create: `.github/workflows/release.yml`

Spec references: §11.

- [ ] **Step 1: Install cargo-dist locally and initialise**

Run: `cargo install cargo-dist`
Run: `cargo dist init --yes --installer shell --installer powershell --installer homebrew`

This writes `[workspace.metadata.dist]` to `Cargo.toml` and creates `.github/workflows/release.yml` with target definitions.

- [ ] **Step 2: Verify the release workflow covers the spec targets**

Ensure the generated workflow's `targets` list includes:
- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `x86_64-pc-windows-msvc`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`

If any are missing, add them under `[workspace.metadata.dist]` `targets = [...]`.

- [ ] **Step 3: Dry-run**

Run: `cargo dist plan`
Expected: lists every target's artifact name.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml .github/workflows/release.yml
git commit -m "chore: configure cargo-dist release workflow"
```

---

## Self-review checklist (run before declaring plan complete)

### Spec coverage

- [x] §1 Guiding principle — enforced by the contract in tasks 2/3/6 (errors on stderr, exit codes, unwrapped JSON).
- [x] §2 Command tree — every subcommand implemented: tasks 12 (init), 13 (auth test), 14 (profile), 15–19 (table CRUD), 20 (--all), 21–23 (schema), 24 (introspect).
- [x] §3 Parameter mapping — table 15 + 16 + 17 + 18 + 19 flags cover all 13 sysparm parameters with friendly + raw aliases. `--page-size` task 15 replaces `--limit` primary (alias `--limit` retained).
- [x] §4 Body input — task 9 covers `--data` + `--field` + file/stdin references + merging.
- [x] §5 Auth & config — tasks 4, 5, 12, 13 cover path resolution, file layout, precedence, init, auth test, per-value env overrides.
- [x] §6 Output contract — tasks 2 (errors), 3 (stdout), 11 (exit codes), 15/16/17/18 (unwrapped vs raw).
- [x] §7 Pagination — task 20 covers manual, streaming, array, max-records, --limit-ignored.
- [x] §8 Retries/timeouts — task 7 (retry policy), task 6 (timeout via builder), task 25 env override wiring.
- [x] §9 Observability — task 25.
- [x] §10 Project layout — matches file paths across tasks.
- [x] §11 Distribution/CI — tasks 26, 27.
- [x] §12 Agent documentation — `introspect` in task 24; `--help` via clap is automatic; agent guide already committed.
- [x] §13 Spec deviations — each deviation corresponds to shipped behavior (offset in task 15; update/replace split in task 18; JSON-only ACCEPT in task 6; schema endpoints in tasks 21–23).

### Placeholder scan

- No "TBD", "TODO", "implement later" strings. Every step has concrete code or commands.
- Every test case has a concrete assertion.

### Type consistency

- `ResolvedProfile` is the single type passed into `Client::builder().build()`.
- `DisplayValue` (query.rs) ↔ `DisplayValueArg` (cli/mod.rs) with explicit `From` impl.
- `ListQuery::page_size` field is `Option<u32>` across builder and CLI conversion.
- `BodyInput` has the same three variants everywhere.
- Exit code integers in `Error::exit_code` match those asserted in tests.

### Scope check

One coherent subsystem (single binary, no shared-state subsystems) — correctly shaped for a single plan.

---

## Execution handoff

Plan complete and saved to `docs/superpowers/plans/2026-04-22-sn-cli-v1-implementation.md`. Two execution options:

1. **Subagent-Driven (recommended)** — dispatch a fresh subagent per task with a review pass between tasks. Fast iteration, clean context, parallel reviewer.
2. **Inline Execution** — execute tasks in the current session using `superpowers:executing-plans`, batching a few tasks between checkpoints.

Which approach?
