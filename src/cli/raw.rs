use crate::body::{build_body, BodyInput};
use crate::cli::GlobalFlags;
use crate::error::{Error, Result};
use serde_json::Value;

use super::table::{build_client, build_profile, write_response};

#[derive(clap::Args, Debug)]
pub struct RawArgs {
    /// HTTP method (GET, POST, PUT, PATCH, DELETE). Case-insensitive.
    pub method: String,
    /// Path on the instance, e.g. `/api/now/table/incident` or `/api/now/v2/table/incident/abc123`.
    pub path: String,
    /// Repeatable query parameter as `key=value` (e.g. `--query sysparm_limit=5`).
    #[arg(long = "query", short = 'q')]
    pub query: Vec<String>,
    /// Request body source: inline JSON, @file (path), or @- (stdin). Required for POST/PUT/PATCH if --field is not given.
    #[arg(long, conflicts_with = "field")]
    pub data: Option<String>,
    /// Repeatable name=value. Use name=@file for value from file. Mutually exclusive with --data.
    #[arg(long = "field", conflicts_with = "data")]
    pub field: Vec<String>,
}

pub fn run(global: &GlobalFlags, args: RawArgs) -> Result<()> {
    let method = args.method.to_uppercase();
    let q = parse_query(&args.query)?;

    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;

    let resp: Value = match method.as_str() {
        "GET" => {
            ensure_no_body(&args.data, &args.field)?;
            client.get(&args.path, &q)?
        }
        "DELETE" => {
            ensure_no_body(&args.data, &args.field)?;
            client.delete_json(&args.path, &q)?
        }
        "POST" | "PUT" | "PATCH" => {
            let body = build_request_body(args.data.clone(), args.field.clone())?;
            match method.as_str() {
                "POST" => client.post(&args.path, &q, &body)?,
                "PUT" => client.put(&args.path, &q, &body)?,
                "PATCH" => client.patch(&args.path, &q, &body)?,
                _ => unreachable!(),
            }
        }
        other => {
            return Err(Error::Usage(format!(
                "unknown method: {other}, expected GET/POST/PUT/PATCH/DELETE"
            )));
        }
    };

    write_response(global, &resp)
}

fn parse_query(raw: &[String]) -> Result<Vec<(String, String)>> {
    let mut out = Vec::with_capacity(raw.len());
    for item in raw {
        let (k, v) = item
            .split_once('=')
            .ok_or_else(|| Error::Usage(format!("--query '{item}' must be in key=value form")))?;
        if k.is_empty() {
            return Err(Error::Usage(format!("--query '{item}' has empty key")));
        }
        out.push((k.to_string(), v.to_string()));
    }
    Ok(out)
}

fn ensure_no_body(data: &Option<String>, field: &[String]) -> Result<()> {
    if data.is_some() || !field.is_empty() {
        return Err(Error::Usage(
            "--data/--field not allowed for GET/DELETE".into(),
        ));
    }
    Ok(())
}

fn build_request_body(data: Option<String>, field: Vec<String>) -> Result<Value> {
    if data.is_none() && field.is_empty() {
        return Ok(Value::Object(Default::default()));
    }
    let input = if let Some(d) = data {
        BodyInput::Data(d)
    } else if !field.is_empty() {
        BodyInput::Fields(field)
    } else {
        BodyInput::None
    };
    build_body(input)
}
