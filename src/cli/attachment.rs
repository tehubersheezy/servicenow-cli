use crate::cli::table::{build_client, build_profile, format_from_flags, unwrap_or_raw};
use crate::cli::{
    AttachmentDeleteArgs, AttachmentDownloadArgs, AttachmentGetArgs, AttachmentListArgs,
    AttachmentUploadArgs, GlobalFlags,
};
use crate::error::{Error, Result};
use crate::output::emit_value;
use std::io::{self, Write};
use std::path::Path;

pub fn list(global: &GlobalFlags, args: AttachmentListArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let mut query: Vec<(String, String)> = Vec::new();
    if let Some(v) = args.query {
        query.push(("sysparm_query".into(), v));
    }
    query.push(("sysparm_limit".into(), args.setlimit.to_string()));
    if let Some(v) = args.offset {
        query.push(("sysparm_offset".into(), v.to_string()));
    }
    let resp = client.get("/api/now/attachment", &query)?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

pub fn get(global: &GlobalFlags, args: AttachmentGetArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("/api/now/attachment/{}", args.sys_id);
    let resp = client.get(&path, &[])?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

pub fn upload(global: &GlobalFlags, args: AttachmentUploadArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let file_path = Path::new(&args.file);
    let file_name = args.file_name.unwrap_or_else(|| {
        file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("upload")
            .to_string()
    });
    let content_type = args
        .content_type
        .unwrap_or_else(|| mime_from_extension(file_path).to_string());
    let body =
        std::fs::read(file_path).map_err(|e| Error::Usage(format!("read {}: {e}", args.file)))?;
    let mut query: Vec<(String, String)> = vec![
        ("table_name".into(), args.table),
        ("table_sys_id".into(), args.record),
        ("file_name".into(), file_name),
    ];
    if let Some(v) = args.encryption_context {
        query.push(("encryption_context".into(), v));
    }
    let resp = client.upload_file("/api/now/attachment/file", &query, body, &content_type)?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

pub fn download(global: &GlobalFlags, args: AttachmentDownloadArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("/api/now/attachment/{}/file", args.sys_id);
    let (bytes, _ct) = client.download_file(&path)?;
    if let Some(out_path) = args.output {
        std::fs::write(&out_path, &bytes)
            .map_err(|e| Error::Usage(format!("write {out_path}: {e}")))?;
        let meta = serde_json::json!({
            "path": out_path,
            "size": bytes.len()
        });
        emit_value(io::stdout().lock(), &meta, format_from_flags(global))
            .map_err(|e| Error::Usage(format!("stdout: {e}")))
    } else {
        io::stdout()
            .lock()
            .write_all(&bytes)
            .map_err(|e| Error::Usage(format!("stdout: {e}")))?;
        Ok(())
    }
}

pub fn delete(global: &GlobalFlags, args: AttachmentDeleteArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("/api/now/attachment/{}", args.sys_id);
    client.delete(&path, &[])?;
    Ok(())
}

fn mime_from_extension(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("json") => "application/json",
        Some("xml") => "application/xml",
        Some("csv") => "text/csv",
        Some("txt") | Some("log") => "text/plain",
        Some("pdf") => "application/pdf",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("zip") => "application/zip",
        Some("gz") | Some("gzip") => "application/gzip",
        Some("xlsx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        _ => "application/octet-stream",
    }
}
