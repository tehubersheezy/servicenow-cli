use crate::cli::table::{build_profile, format_from_flags};
use crate::cli::GlobalFlags;
use crate::error::{Error, Result};
use crate::output::{emit_value, map_stdout_err};
use serde_json::json;

#[derive(clap::Args, Debug)]
pub struct OpenArgs {
    /// Table name (e.g. `incident`).
    pub table: String,
    /// sys_id of the record.
    pub sys_id: String,
    /// Print the URL to stdout instead of opening a browser.
    #[arg(long)]
    pub print_url: bool,
}

pub fn run(global: &GlobalFlags, args: OpenArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let instance = profile.instance.trim_end_matches('/');
    let url = format!(
        "{instance}/nav_to.do?uri=%2F{table}.do%3Fsys_id%3D{sys_id}",
        table = args.table,
        sys_id = args.sys_id,
    );

    if args.print_url {
        println!("{url}");
        return Ok(());
    }

    webbrowser::open(&url).map_err(|e| Error::Transport(format!("open browser: {e}")))?;

    let out = json!({ "opened": true, "url": url });
    emit_value(std::io::stdout().lock(), &out, format_from_flags(global)).map_err(map_stdout_err)
}
