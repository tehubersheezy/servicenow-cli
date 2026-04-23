use crate::cli::{GlobalFlags, ScoresFavoriteArgs, ScoresListArgs};
use crate::error::{Error, Result};
use crate::output::emit_value;

use super::table::{build_client, build_profile, format_from_flags, unwrap_or_raw};

const SCORECARDS_PATH: &str = "/api/now/pa/scorecards";

fn push_flag(q: &mut Vec<(String, String)>, key: &str, val: bool) {
    if val {
        q.push((key.into(), "true".into()));
    }
}

pub fn list(global: &GlobalFlags, args: ScoresListArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;

    let mut q: Vec<(String, String)> = Vec::new();
    if let Some(v) = args.uuid {
        q.push(("sysparm_uuid".into(), v));
    }
    if let Some(v) = args.breakdown {
        q.push(("sysparm_breakdown".into(), v));
    }
    if let Some(v) = args.breakdown_relation {
        q.push(("sysparm_breakdown_relation".into(), v));
    }
    if let Some(v) = args.elements_filter {
        q.push(("sysparm_elements_filter".into(), v));
    }
    if let Some(dv) = args.display {
        let dv: crate::query::DisplayValue = dv.into();
        q.push(("sysparm_display".into(), dv.as_str().to_string()));
    }
    push_flag(&mut q, "sysparm_favorites", args.favorites);
    push_flag(&mut q, "sysparm_key", args.key);
    push_flag(&mut q, "sysparm_target", args.target);
    if let Some(v) = args.contains {
        q.push(("sysparm_contains".into(), v));
    }
    if let Some(v) = args.tags {
        q.push(("sysparm_tags".into(), v));
    }
    q.push(("sysparm_per_page".into(), args.per_page.to_string()));
    q.push(("sysparm_page".into(), args.page.to_string()));
    if let Some(s) = args.sort_by {
        q.push(("sysparm_sortby".into(), s.as_str().to_string()));
    }
    if let Some(d) = args.sort_dir {
        q.push(("sysparm_sortdir".into(), d.as_str().to_string()));
    }
    if let Some(dv) = args.display_value {
        let dv: crate::query::DisplayValue = dv.into();
        q.push(("sysparm_display_value".into(), dv.as_str().to_string()));
    }
    push_flag(
        &mut q,
        "sysparm_exclude_reference_link",
        args.exclude_reference_link,
    );
    push_flag(&mut q, "sysparm_include_scores", args.include_scores);
    if let Some(v) = args.from {
        q.push(("sysparm_from".into(), v));
    }
    if let Some(v) = args.to {
        q.push(("sysparm_to".into(), v));
    }
    if let Some(v) = args.step {
        q.push(("sysparm_step".into(), v.to_string()));
    }
    if let Some(v) = args.limit {
        q.push(("sysparm_limit".into(), v.to_string()));
    }
    push_flag(
        &mut q,
        "sysparm_include_available_breakdowns",
        args.include_available_breakdowns,
    );
    push_flag(
        &mut q,
        "sysparm_include_available_aggregates",
        args.include_available_aggregates,
    );
    push_flag(&mut q, "sysparm_include_realtime", args.include_realtime);
    push_flag(
        &mut q,
        "sysparm_include_target_color_scheme",
        args.include_target_color_scheme,
    );
    push_flag(
        &mut q,
        "sysparm_include_forecast_scores",
        args.include_forecast_scores,
    );
    push_flag(
        &mut q,
        "sysparm_include_trendline_scores",
        args.include_trendline_scores,
    );
    push_flag(
        &mut q,
        "sysparm_include_prediction_interval",
        args.include_prediction_interval,
    );

    let resp = client.get(SCORECARDS_PATH, &q)?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(std::io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

pub fn favorite(global: &GlobalFlags, args: ScoresFavoriteArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;

    let q = vec![("sysparm_uuid".to_string(), args.uuid)];
    let resp = client.post(SCORECARDS_PATH, &q, &serde_json::json!({}))?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(std::io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

pub fn unfavorite(global: &GlobalFlags, args: ScoresFavoriteArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;

    let q = vec![("sysparm_uuid".to_string(), args.uuid)];
    client.delete(SCORECARDS_PATH, &q)
}
