use crate::body::{build_body, BodyInput};
use crate::cli::table::{build_client, build_profile, format_from_flags, unwrap_or_raw};
use crate::cli::{
    CatalogCartEmptyArgs, CatalogCartItemArgs, CatalogCartUpdateArgs, CatalogCategoriesArgs,
    CatalogCategoryArgs, CatalogGetArgs, CatalogItemArgs, CatalogItemsArgs, CatalogListArgs,
    CatalogOrderArgs, GlobalFlags,
};
use crate::error::{Error, Result};
use crate::output::emit_value;
use std::io;

const BASE: &str = "/api/sn_sc/servicecatalog";

pub fn list(global: &GlobalFlags, args: CatalogListArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let mut query: Vec<(String, String)> = Vec::new();
    if let Some(v) = args.text {
        query.push(("sysparm_text".into(), v));
    }
    let resp = client.get(&format!("{BASE}/catalogs"), &query)?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

pub fn get(global: &GlobalFlags, args: CatalogGetArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("{BASE}/catalogs/{}", args.sys_id);
    let resp = client.get(&path, &[])?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

pub fn categories(global: &GlobalFlags, args: CatalogCategoriesArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("{BASE}/catalogs/{}/categories", args.catalog_sys_id);
    let mut query: Vec<(String, String)> = Vec::new();
    query.push(("sysparm_limit".into(), args.setlimit.to_string()));
    if let Some(v) = args.offset {
        query.push(("sysparm_offset".into(), v.to_string()));
    }
    if args.top_level_only {
        query.push(("sysparm_top_level_only".into(), "true".into()));
    }
    let resp = client.get(&path, &query)?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

pub fn category(global: &GlobalFlags, args: CatalogCategoryArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("{BASE}/categories/{}", args.sys_id);
    let resp = client.get(&path, &[])?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

pub fn items(global: &GlobalFlags, args: CatalogItemsArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let mut query: Vec<(String, String)> = Vec::new();
    if let Some(v) = args.text {
        query.push(("sysparm_text".into(), v));
    }
    if let Some(v) = args.category {
        query.push(("sysparm_category".into(), v));
    }
    if let Some(v) = args.catalog {
        query.push(("sysparm_catalog".into(), v));
    }
    if let Some(v) = args.item_type {
        query.push(("sysparm_type".into(), v));
    }
    query.push(("sysparm_limit".into(), args.setlimit.to_string()));
    if let Some(v) = args.offset {
        query.push(("sysparm_offset".into(), v.to_string()));
    }
    let resp = client.get(&format!("{BASE}/items"), &query)?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

pub fn item(global: &GlobalFlags, args: CatalogItemArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("{BASE}/items/{}", args.sys_id);
    let resp = client.get(&path, &[])?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

pub fn item_variables(global: &GlobalFlags, args: CatalogItemArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("{BASE}/items/{}/variables", args.sys_id);
    let resp = client.get(&path, &[])?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

pub fn order(global: &GlobalFlags, args: CatalogOrderArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("{BASE}/items/{}/order_now", args.sys_id);
    let body_input = if let Some(d) = args.data {
        BodyInput::Data(d)
    } else if !args.field.is_empty() {
        BodyInput::Fields(args.field)
    } else {
        BodyInput::Data("{}".into())
    };
    let body = build_body(body_input)?;
    let resp = client.post(&path, &[], &body)?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

pub fn add_to_cart(global: &GlobalFlags, args: CatalogOrderArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("{BASE}/items/{}/add_to_cart", args.sys_id);
    let body_input = if let Some(d) = args.data {
        BodyInput::Data(d)
    } else if !args.field.is_empty() {
        BodyInput::Fields(args.field)
    } else {
        BodyInput::Data("{}".into())
    };
    let body = build_body(body_input)?;
    let resp = client.post(&path, &[], &body)?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

pub fn cart(global: &GlobalFlags) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let resp = client.get(&format!("{BASE}/cart"), &[])?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

pub fn cart_update(global: &GlobalFlags, args: CatalogCartUpdateArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("{BASE}/cart/{}", args.cart_item_id);
    let body_input = if let Some(d) = args.data {
        BodyInput::Data(d)
    } else if !args.field.is_empty() {
        BodyInput::Fields(args.field)
    } else {
        BodyInput::None
    };
    let body = build_body(body_input)?;
    let resp = client.put(&path, &[], &body)?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

pub fn cart_remove(global: &GlobalFlags, args: CatalogCartItemArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("{BASE}/cart/{}", args.cart_item_id);
    client.delete(&path, &[])?;
    Ok(())
}

pub fn cart_empty(global: &GlobalFlags, args: CatalogCartEmptyArgs) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let path = format!("{BASE}/cart/{}/empty", args.sys_id);
    client.delete(&path, &[])?;
    Ok(())
}

pub fn checkout(global: &GlobalFlags) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let resp = client.post(
        &format!("{BASE}/cart/checkout"),
        &[],
        &serde_json::json!({}),
    )?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

pub fn submit_order(global: &GlobalFlags) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let resp = client.post(
        &format!("{BASE}/cart/submit_order"),
        &[],
        &serde_json::json!({}),
    )?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}

pub fn wishlist(global: &GlobalFlags) -> Result<()> {
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;
    let resp = client.get(&format!("{BASE}/wishlist"), &[])?;
    let out = unwrap_or_raw(resp, global.output);
    emit_value(io::stdout().lock(), &out, format_from_flags(global))
        .map_err(|e| Error::Usage(format!("stdout: {e}")))
}
