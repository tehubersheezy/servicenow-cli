use crate::cli::{AggregateArgs, GlobalFlags};
use crate::error::{Error, Result};
use crate::output::emit_value;

use super::table::{bool_opt, build_client, build_profile, format_from_flags, unwrap_or_raw};

pub fn run(global: &GlobalFlags, args: AggregateArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;

    let mut q: Vec<(String, String)> = Vec::new();
    if let Some(v) = args.query {
        q.push(("sysparm_query".into(), v));
    }
    if let Some(v) = args.avg_fields {
        q.push(("sysparm_avg_fields".into(), v));
    }
    if let Some(b) = bool_opt(args.count) {
        q.push(("sysparm_count".into(), b.to_string()));
    }
    if let Some(v) = args.min_fields {
        q.push(("sysparm_min_fields".into(), v));
    }
    if let Some(v) = args.max_fields {
        q.push(("sysparm_max_fields".into(), v));
    }
    if let Some(v) = args.sum_fields {
        q.push(("sysparm_sum_fields".into(), v));
    }
    if let Some(v) = args.group_by {
        q.push(("sysparm_group_by".into(), v));
    }
    if let Some(v) = args.order_by {
        q.push(("sysparm_order_by".into(), v));
    }
    if let Some(v) = args.having {
        q.push(("sysparm_having".into(), v));
    }
    if let Some(dv) = args.display_value {
        let dv: crate::query::DisplayValue = dv.into();
        q.push(("sysparm_display_value".into(), dv.as_str().to_string()));
    }
    if let Some(v) = args.query_category {
        q.push(("sysparm_query_category".into(), v));
    }

    let path = format!("/api/now/stats/{}", args.table);
    let resp = client.get(&path, &q)?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(std::io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}
