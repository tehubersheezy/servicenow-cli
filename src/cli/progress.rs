use crate::cli::table::{build_client, build_profile, unwrap_or_raw};
use crate::cli::{GlobalFlags, OutputMode};
use crate::client::Client;
use crate::error::{Error, Result};
use serde_json::Value;

#[derive(clap::Args, Debug)]
pub struct ProgressArgs {
    /// Progress ID returned by app/updateset/atf operations.
    pub progress_id: String,
}

pub fn run(global: &GlobalFlags, args: ProgressArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("/api/sn_cicd/progress/{}", args.progress_id);
    let resp = client.get(&path, &[])?;
    let out = unwrap_or_raw(resp, global.output);
    crate::cli::table::write_response(global, &out)
}

/// Poll `GET /api/sn_cicd/progress/{progress_id}` in a loop until the operation
/// reaches a terminal state (Successful, Failed, or Cancelled) and return the
/// final result value.
///
/// Status codes:
/// - "0" = Pending, "1" = Running, "2" = Successful, "3" = Failed, "4" = Cancelled
pub(crate) fn wait_for_completion(
    client: &Client,
    progress_id: &str,
    global: &GlobalFlags,
) -> Result<Value> {
    let path = format!("/api/sn_cicd/progress/{}", progress_id);
    loop {
        let resp = client.get(&path, &[])?;
        let result = unwrap_or_raw(resp, OutputMode::Default);

        let status = result.get("status").and_then(|s| s.as_str()).unwrap_or("1");

        match status {
            "2" => return Ok(result),
            "3" | "4" => {
                let msg = result
                    .get("status_message")
                    .and_then(|s| s.as_str())
                    .unwrap_or("operation failed");
                return Err(Error::Api {
                    status: 0,
                    message: msg.to_string(),
                    detail: result
                        .get("status_detail")
                        .and_then(|s| s.as_str())
                        .map(String::from),
                    transaction_id: None,
                    sn_error: Some(result),
                });
            }
            _ => {
                if global.verbose > 0 {
                    if let Some(pct) = result.get("percent_complete").and_then(|v| v.as_str()) {
                        eprintln!("sn: progress {}%", pct);
                    }
                }
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
        }
    }
}
