use crate::cli::table::{build_client, build_profile, format_from_flags};
use crate::cli::{GlobalFlags, OutputMode, SchemaChoicesArgs, SchemaColumnsArgs, SchemaTablesArgs};
use crate::error::{Error, Result};
use crate::output::emit_value;
use serde_json::Value;
use std::io;

pub fn tables(global: &GlobalFlags, args: SchemaTablesArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let resp = client.get("/api/now/doc/table/schema", &[])?;
    let list = match (global.output, resp.get("result")) {
        (OutputMode::Raw, _) => resp.clone(),
        (_, Some(Value::Array(a))) => Value::Array(filter_tables(a.clone(), &args)),
        _ => resp.clone(),
    };
    let fmt = format_from_flags(global);
    emit_value(io::stdout().lock(), &list, fmt).map_err(|e| Error::Usage(format!("stdout: {e}")))
}

fn filter_tables(items: Vec<Value>, args: &SchemaTablesArgs) -> Vec<Value> {
    let needle = args.filter.as_deref().map(str::to_lowercase);
    items
        .into_iter()
        .filter(|t| {
            if args.reference_only
                && !t
                    .get("reference")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
            {
                return false;
            }
            if let Some(n) = &needle {
                let label = t
                    .get("label")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_lowercase();
                let value = t
                    .get("value")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_lowercase();
                if !label.contains(n) && !value.contains(n) {
                    return false;
                }
            }
            true
        })
        .collect()
}

pub fn columns(global: &GlobalFlags, args: SchemaColumnsArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("/api/now/ui/meta/{}", args.table);
    let resp = client.get(&path, &[])?;
    let list = match global.output {
        OutputMode::Raw => resp.clone(),
        OutputMode::Default => {
            let cols = resp
                .get("result")
                .and_then(|r| r.get("columns"))
                .cloned()
                .unwrap_or(Value::Object(serde_json::Map::new()));
            Value::Array(filter_columns(cols, &args))
        }
    };
    emit_value(io::stdout().lock(), &list, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

fn filter_columns(cols: Value, args: &SchemaColumnsArgs) -> Vec<Value> {
    let cols_obj = match cols {
        Value::Object(m) => m,
        _ => return vec![],
    };
    cols_obj
        .into_iter()
        .map(|(name, mut v)| {
            if let Value::Object(ref mut m) = v {
                m.insert("name".into(), Value::String(name));
            }
            v
        })
        .filter(|v| keep_column(v, args))
        .collect()
}

fn keep_column(col: &Value, args: &SchemaColumnsArgs) -> bool {
    let getb = |k: &str| col.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    let gets = |k: &str| col.get(k).and_then(|v| v.as_str()).unwrap_or("");
    if args.mandatory && !getb("mandatory") {
        return false;
    }
    if args.writable && getb("read_only") {
        return false;
    }
    if args.choices_only
        && col
            .get("choices")
            .and_then(|v| v.as_array())
            .map_or(true, |a| a.is_empty())
    {
        return false;
    }
    if args.references_only && gets("type") != "reference" {
        return false;
    }
    if let Some(t) = args.r#type.as_deref() {
        if !gets("type").eq_ignore_ascii_case(t) {
            return false;
        }
    }
    if let Some(n) = args.filter.as_deref().map(str::to_lowercase) {
        let name = gets("name").to_lowercase();
        let label = gets("label").to_lowercase();
        if !name.contains(&n) && !label.contains(&n) {
            return false;
        }
    }
    true
}

pub fn choices(global: &GlobalFlags, args: SchemaChoicesArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("/api/now/ui/meta/{}", args.table);
    let resp = client.get(&path, &[])?;
    let out = match global.output {
        OutputMode::Raw => resp.clone(),
        OutputMode::Default => resp
            .get("result")
            .and_then(|r| r.get("columns"))
            .and_then(|c| c.get(&args.field))
            .and_then(|f| f.get("choices"))
            .cloned()
            .ok_or_else(|| {
                Error::Usage(format!(
                    "no choices found on field '{}' in table '{}'",
                    args.field, args.table
                ))
            })?,
    };
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}
