//! `sn doctor` — "is this instance set up for what I am about to do?" in one
//! GraphQL round trip: connectivity, who the session is, whether it is admin,
//! and pass/fail checks for required roles, plugins and system properties.
//!
//! The checks ride three scripted GraphQL namespaces plus the generated
//! `GlideRecord_Query` one, all on `POST /api/now/graphql`. Everything below
//! was measured against a PDI (dev421992, 2026-09-28) as both an admin and a
//! plain `itil` account, and three of the findings shape the module:
//!
//! - **`now.sessionUser.getMatchingRoles` fails open for admin.** It is
//!   `roleNames.filter(gs.hasRole)`, and `gs.hasRole` answers true for *any*
//!   name under `admin` — `nonexistent_xyz` included (elevated roles such as
//!   `security_admin` are the exception: false until elevated). So a role
//!   only passes when `sys_user_role` proves it exists, and a pass under an
//!   admin session says so (`basis: "admin"`). Existence reads `_rowCount`,
//!   which counts matching rows *before* row ACLs: `itil` cannot read the
//!   `admin` role's row yet gets `_rowCount: 1` for it. `name=` is unique, so
//!   a count above one proves the filter was dropped (a mistyped term returns
//!   all 981 roles) and the check reports `unavailable`, never an answer.
//!   For a non-admin session `gs.hasRole` is membership, measured truthful.
//! - **`snWorkflowStudio.workflowStudio.isPluginInstalled` is trustworthy** —
//!   true for active plugins, false for fakes, no prefix matching, case
//!   sensitive — and the only route: `sys_plugins` is 403 even to admin and
//!   `v_plugin` times out server-side. Non-admins get `workflowStudio: null`.
//!   A canary asks about `com.glide.graphql`, the plugin this very request
//!   runs on, so a getter that says `false` for it is not believed.
//! - **`snDecisionTable.sysProperties.getSysPropertyValue` is `gs.getProperty`
//!   with a default**: an unset *and* an empty property both come back as the
//!   default (so the default is a sentinel and the two read as one state),
//!   password-type properties come back `null`, and for a non-admin every
//!   read is `null` while the namespace itself still resolves. So a null is
//!   "cannot read", never "unset", and the canary is a real read.
//!
//! Every check is `pass`, `fail` or `unavailable`; only `pass` passes. A
//! namespace the schema lacks (its product plugin is absent) fails validation
//! for the whole document, so the document is rebuilt without it and those
//! checks report `unavailable` with the instance's own message — one extra
//! round trip on a degraded instance, none on a healthy one.

use crate::cli::GlobalFlags;
use crate::cli::graphql::{errors_to_api_error, execute, graphql_errors};
use crate::cli::kernel::{build_client, build_profile, write_response};
use crate::error::{Error, NO_HTTP_STATUS, Result};
use serde_json::{Map, Value, json};
use std::time::Instant;

#[derive(clap::Args, Debug)]
pub struct DoctorArgs {
    /// Role the session must hold (repeatable or comma-separated). Checked
    /// against sys_user_role, so an admin session does not pass a name that
    /// is not a role.
    #[arg(long = "need-role", value_name = "ROLE", value_delimiter = ',')]
    pub need_role: Vec<String>,
    /// Plugin that must be installed and active, by plugin id (repeatable or
    /// comma-separated), e.g. com.snc.change_management.
    #[arg(long = "need-plugin", value_name = "PLUGIN_ID", value_delimiter = ',')]
    pub need_plugin: Vec<String>,
    /// System property that must be set (NAME) or equal a value
    /// (NAME=VALUE). Repeatable. Unset and empty read the same.
    #[arg(long = "need-property", value_name = "NAME[=VALUE]")]
    pub need_property: Vec<String>,
}

/// The default handed to the property getter. It answers with the default
/// for an unset *or* empty property, so a value no real property carries is
/// how "unset" is recognized.
const UNSET: &str = "sn-doctor:unset:6b1f0e2c";

/// The plugin the plugin getter is asked about as a canary: the GraphQL
/// framework itself, necessarily active on an instance answering this request.
const CANARY_PLUGIN: &str = "com.glide.graphql";

#[derive(Debug, Clone, PartialEq)]
struct PropertyNeed {
    name: String,
    expected: Option<String>,
}

#[derive(Debug, Default)]
struct Plan {
    roles: Vec<String>,
    plugins: Vec<String>,
    properties: Vec<PropertyNeed>,
}

/// The document's top-level fields. Each is independently optional: a missing
/// one disables its checks rather than the command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Root {
    /// `now.sessionUser`: the admin flag and the role check.
    Session,
    /// `GlideRecord_Query`: the caller's `sys_user` row and role existence.
    Records,
    /// `snWorkflowStudio.workflowStudio`: plugin checks.
    Plugins,
    /// `snDecisionTable.sysProperties`: property checks and the build tag.
    Properties,
}

impl Root {
    const ALL: [Root; 4] = [
        Root::Session,
        Root::Records,
        Root::Plugins,
        Root::Properties,
    ];

    fn field(self) -> &'static str {
        match self {
            Root::Session => "now",
            Root::Records => "GlideRecord_Query",
            Root::Plugins => "snWorkflowStudio",
            Root::Properties => "snDecisionTable",
        }
    }

    fn from_field(field: &str) -> Option<Root> {
        Root::ALL.into_iter().find(|r| r.field() == field)
    }
}

/// Roots dropped from the document, each with the instance's reason.
type Disabled = Vec<(Root, String)>;

pub fn run(global: &GlobalFlags, args: DoctorArgs) -> Result<()> {
    let plan = build_plan(&args)?;
    let profile = build_profile(global)?;
    let client = build_client(&profile, global.timeout)?;

    let mut disabled: Disabled = Vec::new();
    let mut latency_ms = None;
    let data = loop {
        let Some((doc, vars)) = build_document(&plan, &disabled) else {
            break Value::Null;
        };
        let started = Instant::now();
        let resp = execute(&client, &doc, vars, None)?;
        latency_ms.get_or_insert(started.elapsed().as_millis() as u64);
        let errors = graphql_errors(&resp);
        if errors.is_empty() {
            break resp.get("data").cloned().unwrap_or(Value::Null);
        }
        match attribute_errors(&errors, &disabled) {
            Some(newly) => disabled.extend(newly),
            None => return Err(errors_to_api_error(errors)),
        }
    };

    let mut report = evaluate(&plan, &data, &disabled);
    let obj = report.as_object_mut().expect("evaluate builds an object");
    obj.insert("profile".into(), Value::from(profile.name.clone()));
    obj.insert("instance".into(), Value::from(profile.instance.clone()));
    obj.insert(
        "latency_ms".into(),
        latency_ms.map_or(Value::Null, Value::from),
    );
    write_response(global, &report)?;

    match failure_summary(&report) {
        None => Ok(()),
        // The report is already on stdout; this is the exit code a CI step
        // branches on. No HTTP status failed — every request answered 200 —
        // so none is published.
        Some(message) => Err(Error::Api {
            status: NO_HTTP_STATUS,
            message,
            detail: None,
            transaction_id: None,
            sn_error: None,
        }),
    }
}

/// Validate and dedupe argv before anything touches the network. Role names
/// ride inside an encoded query (`name=<role>`), so a `^` or `=` would splice
/// in a term of the caller's own; the plugin id shares the charset because
/// both are platform identifiers.
fn build_plan(args: &DoctorArgs) -> Result<Plan> {
    let mut plan = Plan::default();
    for role in &args.need_role {
        let role = identifier(role, "--need-role", "role name")?;
        if !plan.roles.contains(&role) {
            plan.roles.push(role);
        }
    }
    for plugin in &args.need_plugin {
        let plugin = identifier(plugin, "--need-plugin", "plugin id")?;
        if !plan.plugins.contains(&plugin) {
            plan.plugins.push(plugin);
        }
    }
    for spec in &args.need_property {
        let (name, expected) = match spec.split_once('=') {
            Some((n, v)) => (n.trim(), Some(v.to_string())),
            None => (spec.trim(), None),
        };
        if name.is_empty() || name.chars().any(char::is_whitespace) {
            return Err(Error::Usage(format!(
                "--need-property '{spec}' needs a property name (NAME or NAME=VALUE)"
            )));
        }
        if expected.as_deref() == Some("") {
            return Err(Error::Usage(format!(
                "--need-property '{spec}': an empty value cannot be checked — the instance \
                 reports an empty property as unset"
            )));
        }
        let need = PropertyNeed {
            name: name.to_string(),
            expected,
        };
        if !plan.properties.contains(&need) {
            plan.properties.push(need);
        }
    }
    Ok(plan)
}

fn identifier(raw: &str, flag: &str, what: &str) -> Result<String> {
    let s = raw.trim();
    let ok = !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
    if ok {
        Ok(s.to_string())
    } else {
        Err(Error::Usage(format!(
            "{flag} '{raw}' is not a {what} (letters, digits, '_', '.', '-')"
        )))
    }
}

fn enabled(root: Root, disabled: &Disabled) -> bool {
    !disabled.iter().any(|(r, _)| *r == root)
}

/// The GraphQL document for every root still enabled, and its variables.
/// Only variables the document uses are declared — an unused declaration is
/// itself a validation error. `None` when every root has been disabled.
fn build_document(plan: &Plan, disabled: &Disabled) -> Option<(String, Option<Value>)> {
    let mut decls: Vec<String> = Vec::new();
    let mut vars = Map::new();
    let mut body = String::new();

    if enabled(Root::Session, disabled) {
        body.push_str("  now { sessionUser { admin: getMatchingRoles(roleNames: [\"admin\"])");
        if !plan.roles.is_empty() {
            decls.push("$roles: [String]!".into());
            vars.insert("roles".into(), json!(plan.roles));
            body.push_str(" held: getMatchingRoles(roleNames: $roles)");
        }
        body.push_str(" } }\n");
    }
    if enabled(Root::Records, disabled) {
        body.push_str(
            "  GlideRecord_Query {\n    me: sys_user(queryConditions: \
             \"sys_id=javascript:gs.getUserID()\", pagination: {limit: 2}) \
             { _rowCount _results { sys_id { value } user_name { value } } }\n",
        );
        for (i, role) in plan.roles.iter().enumerate() {
            decls.push(format!("$role{i}: String"));
            vars.insert(format!("role{i}"), Value::from(format!("name={role}")));
            body.push_str(&format!(
                "    role{i}: sys_user_role(queryConditions: $role{i}, pagination: {{limit: 2}}) \
                 {{ _rowCount }}\n"
            ));
        }
        body.push_str("  }\n");
    }
    if enabled(Root::Plugins, disabled) {
        body.push_str(&format!(
            "  snWorkflowStudio {{ workflowStudio {{ canary: isPluginInstalled(plugin_name: \
             \"{CANARY_PLUGIN}\")"
        ));
        for (i, plugin) in plan.plugins.iter().enumerate() {
            decls.push(format!("$plugin{i}: String!"));
            vars.insert(format!("plugin{i}"), Value::from(plugin.clone()));
            body.push_str(&format!(
                " plugin{i}: isPluginInstalled(plugin_name: $plugin{i})"
            ));
        }
        body.push_str(" } }\n");
    }
    if enabled(Root::Properties, disabled) {
        decls.push("$unset: String".into());
        vars.insert("unset".into(), Value::from(UNSET));
        body.push_str(
            "  snDecisionTable { sysProperties { build: getSysPropertyValue(sys_property_name: \
             \"glide.buildtag\", default_value: $unset)",
        );
        for (i, need) in plan.properties.iter().enumerate() {
            decls.push(format!("$property{i}: String!"));
            vars.insert(format!("property{i}"), Value::from(need.name.clone()));
            body.push_str(&format!(
                " property{i}: getSysPropertyValue(sys_property_name: $property{i}, \
                 default_value: $unset)"
            ));
        }
        body.push_str(" } }\n");
    }

    if body.is_empty() {
        return None;
    }
    let doc = if decls.is_empty() {
        format!("query {{\n{body}}}")
    } else {
        format!("query ({}) {{\n{body}}}", decls.join(", "))
    };
    Some((doc, (!vars.is_empty()).then_some(Value::Object(vars))))
}

/// The roots every error belongs to, when each one belongs to a root still in
/// the document. `None` when any error cannot be attributed — that is a real
/// failure and is reported as one, not absorbed as an unavailable check.
///
/// Validation errors name their path in the message
/// (`FieldUndefined@[snDecisionTable/…]`); execution errors carry a `path`
/// array. Either locates the root.
fn attribute_errors(errors: &[Value], disabled: &Disabled) -> Option<Disabled> {
    let mut newly: Disabled = Vec::new();
    for err in errors {
        let message = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("GraphQL error");
        let root = error_root(err).and_then(Root::from_field)?;
        if !enabled(root, disabled) {
            return None;
        }
        if !newly.iter().any(|(r, _)| *r == root) {
            newly.push((root, message.to_string()));
        }
    }
    (!newly.is_empty()).then_some(newly)
}

fn error_root(err: &Value) -> Option<&str> {
    if let Some(first) = err
        .get("path")
        .and_then(Value::as_array)
        .and_then(|p| p.first())
        .and_then(Value::as_str)
    {
        return Some(first);
    }
    let message = err.get("message")?.as_str()?;
    let start = message.find("@[")? + 2;
    let rest = &message[start..];
    let end = rest.find([']', '/'])?;
    Some(&rest[..end])
}

/// Why a root's checks cannot run: dropped from the document, or present but
/// answering `null` at `path` (row/role ACLs answer that way, not with an
/// error). `Ok` carries the section when it answered.
fn section<'a>(
    data: &'a Value,
    root: Root,
    path: &str,
    disabled: &Disabled,
) -> std::result::Result<&'a Value, String> {
    if let Some((_, reason)) = disabled.iter().find(|(r, _)| *r == root) {
        return Err(format!(
            "{} is not available on this instance: {reason}",
            root.field()
        ));
    }
    match data.pointer(path) {
        Some(v) if !v.is_null() => Ok(v),
        _ => Err(format!(
            "{} answered null: not readable by this account",
            path.trim_start_matches('/').replace('/', ".")
        )),
    }
}

struct Check {
    kind: &'static str,
    name: String,
    status: &'static str,
    reason: Option<String>,
    extra: Map<String, Value>,
}

impl Check {
    fn new(kind: &'static str, name: &str) -> Self {
        Check {
            kind,
            name: name.to_string(),
            status: "unavailable",
            reason: None,
            extra: Map::new(),
        }
    }

    fn pass(mut self) -> Self {
        self.status = "pass";
        self
    }

    fn fail(mut self, reason: impl Into<String>) -> Self {
        self.status = "fail";
        self.reason = Some(reason.into());
        self
    }

    fn unavailable(mut self, reason: impl Into<String>) -> Self {
        self.status = "unavailable";
        self.reason = Some(reason.into());
        self
    }

    fn with(mut self, key: &str, value: Value) -> Self {
        self.extra.insert(key.into(), value);
        self
    }

    fn to_json(&self) -> Value {
        let mut out = Map::new();
        out.insert("kind".into(), Value::from(self.kind));
        out.insert("name".into(), Value::from(self.name.clone()));
        out.insert("status".into(), Value::from(self.status));
        if let Some(r) = &self.reason {
            out.insert("reason".into(), Value::from(r.clone()));
        }
        out.extend(self.extra.clone());
        Value::Object(out)
    }
}

const NO_ROLE_ANSWER: &str = "the session's role check returned no answer";

fn capability<T>(r: &std::result::Result<T, String>) -> Value {
    match r {
        Ok(_) => json!({ "available": true }),
        Err(reason) => json!({ "available": false, "reason": reason }),
    }
}

/// Turn the response into the report. Pure, so every verdict is testable
/// without a server.
fn evaluate(plan: &Plan, data: &Value, disabled: &Disabled) -> Value {
    let mut checks: Vec<Check> = Vec::new();

    // --- session: admin flag + the role check's `held` list -----------------
    let session = section(data, Root::Session, "/now/sessionUser", disabled);
    let admin: Option<bool> = session
        .as_ref()
        .ok()
        .and_then(|s| s.get("admin"))
        .and_then(Value::as_array)
        .map(|a| a.iter().any(|r| r.as_str() == Some("admin")));
    let records = section(data, Root::Records, "/GlideRecord_Query", disabled);

    // --- roles --------------------------------------------------------------
    // Both halves are needed: the session's role check says "held", and
    // sys_user_role says the name is a role at all.
    let roles = session.clone().and_then(|s| {
        let recs = records.clone()?;
        match admin {
            Some(_) => Ok((s, recs)),
            None => Err(NO_ROLE_ANSWER.to_string()),
        }
    });
    for (i, role) in plan.roles.iter().enumerate() {
        let check = Check::new("role", role);
        let (held, recs) = match &roles {
            Ok((s, recs)) => match s.get("held").and_then(Value::as_array) {
                Some(held) => (held, *recs),
                None => {
                    checks.push(check.unavailable(NO_ROLE_ANSWER));
                    continue;
                }
            },
            Err(reason) => {
                checks.push(check.unavailable(reason.clone()));
                continue;
            }
        };
        let in_held = held.iter().any(|r| r.as_str() == Some(role));
        let count = recs
            .pointer(&format!("/role{i}/_rowCount"))
            .and_then(Value::as_u64);
        checks.push(match count {
            None => check.unavailable("sys_user_role reported no row count"),
            Some(n) if n > 1 => check.unavailable(format!(
                "sys_user_role matched {n} rows for name={role}; the filter was not applied, \
                 so the role's existence cannot be established"
            )),
            Some(0) if in_held => check.with("exists", Value::Bool(false)).fail(
                "no such role on this instance (the session's role check said yes anyway: \
                 admin answers every role check)",
            ),
            Some(0) => check
                .with("exists", Value::Bool(false))
                .fail("no such role on this instance"),
            Some(_) if in_held => {
                let basis = if admin == Some(true) && role != "admin" {
                    "admin"
                } else {
                    "session"
                };
                check
                    .with("exists", Value::Bool(true))
                    .with("basis", Value::from(basis))
                    .pass()
            }
            Some(_) => check
                .with("exists", Value::Bool(true))
                .fail("the session does not hold this role"),
        });
    }

    // --- plugins ------------------------------------------------------------
    let plugins = section(
        data,
        Root::Plugins,
        "/snWorkflowStudio/workflowStudio",
        disabled,
    )
    .and_then(|ws| match ws.get("canary") {
        Some(Value::Bool(true)) => Ok(ws),
        Some(Value::Bool(false)) => Err(format!(
            "the plugin check denies {CANARY_PLUGIN}, which this request ran on, so its \
             answers cannot be trusted"
        )),
        _ => Err("the plugin check answered null: not readable by this account".to_string()),
    });
    for (i, plugin) in plan.plugins.iter().enumerate() {
        let check = Check::new("plugin", plugin);
        checks.push(match &plugins {
            Err(reason) => check.unavailable(reason.clone()),
            Ok(ws) => match ws.get(format!("plugin{i}")) {
                Some(Value::Bool(true)) => check.pass(),
                Some(Value::Bool(false)) => check.fail("not installed, or not active"),
                _ => check.unavailable("the plugin check returned no answer"),
            },
        });
    }

    // --- properties ---------------------------------------------------------
    let properties = section(
        data,
        Root::Properties,
        "/snDecisionTable/sysProperties",
        disabled,
    )
    .and_then(|sp| match sp.get("build") {
        Some(Value::String(_)) => Ok(sp),
        _ => Err("the property getter answered null: not readable by this account".to_string()),
    });
    let build_tag = properties
        .as_ref()
        .ok()
        .and_then(|sp| sp.get("build"))
        .and_then(Value::as_str)
        .filter(|v| *v != UNSET)
        .map_or(Value::Null, Value::from);
    for (i, need) in plan.properties.iter().enumerate() {
        let mut check = Check::new("property", &need.name);
        if let Some(e) = &need.expected {
            check = check.with("expected", Value::from(e.clone()));
        }
        checks.push(match &properties {
            Err(reason) => check.unavailable(reason.clone()),
            Ok(sp) => match sp.get(format!("property{i}")) {
                Some(Value::String(v)) if v == UNSET => check
                    .with("value", Value::Null)
                    .fail("not set (or set to an empty value)"),
                Some(Value::String(v)) => {
                    let check = check.with("value", Value::from(v.clone()));
                    match &need.expected {
                        Some(e) if e != v => check.fail(format!("value is '{v}', not '{e}'")),
                        _ => check.pass(),
                    }
                }
                _ => check.unavailable(
                    "the property getter returned no value (it never returns password-type \
                     properties)",
                ),
            },
        });
    }

    let ok = checks.iter().all(|c| c.status == "pass");
    json!({
        "ok": ok,
        "user": user_of(records.as_ref().ok().copied()),
        "admin": admin.map_or(Value::Null, Value::Bool),
        "build_tag": build_tag,
        "capabilities": {
            "roles": capability(&roles),
            "plugins": capability(&plugins),
            "properties": capability(&properties),
        },
        "checks": checks.iter().map(Check::to_json).collect::<Vec<_>>(),
    })
}

/// The caller's own `sys_user` row, or `null`. The row is found through a
/// `javascript:gs.getUserID()` term, and an instance that cannot evaluate it
/// drops it silently — leaving every user. `sys_id` is unique, so a second
/// row is proof of that and reports nobody rather than a stranger.
fn user_of(records: Option<&Value>) -> Value {
    let Some(me) = records.and_then(|r| r.get("me")) else {
        return Value::Null;
    };
    if me.get("_rowCount").and_then(Value::as_u64) != Some(1) {
        return Value::Null;
    }
    let Some(row) = me
        .get("_results")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
    else {
        return Value::Null;
    };
    let field = |name: &str| {
        row.pointer(&format!("/{name}/value"))
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .map(str::to_string)
    };
    match (field("user_name"), field("sys_id")) {
        (Some(user_name), Some(sys_id)) => json!({ "user_name": user_name, "sys_id": sys_id }),
        _ => Value::Null,
    }
}

/// `Some(message)` naming every check that did not pass, `None` when all did.
fn failure_summary(report: &Value) -> Option<String> {
    let checks = report.get("checks").and_then(Value::as_array)?;
    let failed: Vec<String> = checks
        .iter()
        .filter(|c| c.get("status").and_then(Value::as_str) != Some("pass"))
        .map(|c| {
            format!(
                "{} '{}' ({})",
                c.get("kind").and_then(Value::as_str).unwrap_or("check"),
                c.get("name").and_then(Value::as_str).unwrap_or(""),
                c.get("status").and_then(Value::as_str).unwrap_or("unknown"),
            )
        })
        .collect();
    (!failed.is_empty()).then(|| {
        format!(
            "{} of {} doctor checks did not pass: {}",
            failed.len(),
            checks.len(),
            failed.join(", ")
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(roles: &[&str], plugins: &[&str], properties: &[&str]) -> DoctorArgs {
        let own = |v: &[&str]| v.iter().map(|s| s.to_string()).collect();
        DoctorArgs {
            need_role: own(roles),
            need_plugin: own(plugins),
            need_property: own(properties),
        }
    }

    fn plan(roles: &[&str], plugins: &[&str], properties: &[&str]) -> Plan {
        build_plan(&args(roles, plugins, properties)).unwrap()
    }

    /// A healthy response for a plan, as an admin or not: `held` is what
    /// `getMatchingRoles` answered, `counts` the per-role `_rowCount`s.
    fn data(
        admin: bool,
        held: &[&str],
        counts: &[u64],
        plugins: &[Value],
        props: &[Value],
    ) -> Value {
        let mut recs = json!({
            "me": {"_rowCount": 1, "_results": [
                {"sys_id": {"value": "u1"}, "user_name": {"value": "beth"}}
            ]}
        });
        for (i, n) in counts.iter().enumerate() {
            recs[format!("role{i}")] = json!({ "_rowCount": n });
        }
        let mut ws = json!({ "canary": true });
        for (i, p) in plugins.iter().enumerate() {
            ws[format!("plugin{i}")] = p.clone();
        }
        let mut sp = json!({ "build": "glide-brazil" });
        for (i, p) in props.iter().enumerate() {
            sp[format!("property{i}")] = p.clone();
        }
        json!({
            "now": {"sessionUser": {
                "admin": if admin { json!(["admin"]) } else { json!([]) },
                "held": held,
            }},
            "GlideRecord_Query": recs,
            "snWorkflowStudio": {"workflowStudio": ws},
            "snDecisionTable": {"sysProperties": sp},
        })
    }

    fn check<'a>(report: &'a Value, kind: &str, name: &str) -> &'a Value {
        report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["kind"] == kind && c["name"] == name)
            .unwrap_or_else(|| panic!("no {kind} check for {name} in {report}"))
    }

    #[test]
    fn plan_dedupes_and_splits_property_expectations() {
        let p = plan(
            &["itil", "admin", "itil"],
            &["com.snc.incident", "com.snc.incident"],
            &["a.b", "c.d=x=y", "a.b"],
        );
        assert_eq!(p.roles, ["itil", "admin"]);
        assert_eq!(p.plugins, ["com.snc.incident"]);
        assert_eq!(p.properties.len(), 2);
        // Only the first '=' splits: a value may itself contain one.
        assert_eq!(p.properties[1].expected.as_deref(), Some("x=y"));
    }

    #[test]
    fn plan_refuses_names_that_would_splice_a_query_term() {
        for bad in ["itil^ORname=admin", "a=b", "", "two words"] {
            let err = build_plan(&args(&[bad], &[], &[])).unwrap_err();
            assert!(matches!(err, Error::Usage(_)), "{bad:?} must be refused");
        }
        assert!(build_plan(&args(&[], &["com.x;y"], &[])).is_err());
        assert!(build_plan(&args(&["sn_change_write", "x_acme.app-user"], &[], &[])).is_ok());
    }

    #[test]
    fn plan_refuses_uncheckable_properties() {
        for bad in ["=v", "", "has space", "name="] {
            let err = build_plan(&args(&[], &[], &[bad])).unwrap_err();
            assert!(matches!(err, Error::Usage(_)), "{bad:?} must be refused");
        }
    }

    /// Every declared variable is used, and every used one declared: GraphQL
    /// rejects both an unused declaration and an undeclared reference.
    fn assert_variables_consistent(doc: &str, vars: &Option<Value>) {
        let declared: Vec<&str> = doc
            .strip_prefix("query (")
            .map(|rest| rest.split_once(')').unwrap().0)
            .map(|d| {
                d.split(", ")
                    .map(|v| v.split(':').next().unwrap())
                    .collect()
            })
            .unwrap_or_default();
        let keys: Vec<String> = vars
            .as_ref()
            .map(|v| v.as_object().unwrap().keys().cloned().collect())
            .unwrap_or_default();
        assert_eq!(declared.len(), keys.len(), "{doc}\n{vars:?}");
        for d in &declared {
            assert!(keys.contains(&d.trim_start_matches('$').to_string()), "{d}");
            assert!(
                doc.matches(d).count() >= 2,
                "{d} declared but unused:\n{doc}"
            );
        }
    }

    #[test]
    fn document_declares_exactly_the_variables_it_uses() {
        let p = plan(
            &["itil", "admin"],
            &["com.snc.incident"],
            &["glide.servlet.uri"],
        );
        let (doc, vars) = build_document(&p, &Vec::new()).unwrap();
        assert_variables_consistent(&doc, &vars);
        let vars = vars.unwrap();
        // Role names ride as variables, never interpolated into the document.
        assert_eq!(vars["role0"], "name=itil");
        assert_eq!(vars["roles"], json!(["itil", "admin"]));
        assert_eq!(vars["plugin0"], "com.snc.incident");
        assert_eq!(vars["unset"], UNSET);
        assert!(!doc.contains("name=itil"), "{doc}");

        // No roles: no `$roles`, but the admin probe is still asked.
        let (doc, vars) = build_document(&Plan::default(), &Vec::new()).unwrap();
        assert_variables_consistent(&doc, &vars);
        assert!(doc.contains("admin: getMatchingRoles"), "{doc}");
        assert!(!doc.contains("$roles"), "{doc}");

        // A disabled root takes its variables with it.
        let off = vec![(Root::Properties, "gone".to_string())];
        let (doc, vars) = build_document(&p, &off).unwrap();
        assert_variables_consistent(&doc, &vars);
        assert!(!doc.contains("snDecisionTable"), "{doc}");
    }

    #[test]
    fn document_is_none_once_every_root_is_disabled() {
        let off: Disabled = Root::ALL.iter().map(|r| (*r, "x".to_string())).collect();
        assert!(build_document(&Plan::default(), &off).is_none());
    }

    #[test]
    fn errors_are_attributed_to_their_root_or_not_at_all() {
        let missing_ns = json!({"message": "Validation error (FieldUndefined@[snDecisionTable]) : Field 'snDecisionTable' in type 'QueryType' is undefined"});
        let nested = json!({"message": "Validation error (FieldUndefined@[snWorkflowStudio/workflowStudio/isPluginInstalled]) : x"});
        let exec = json!({"message": "boom", "path": ["now", "sessionUser", "held"]});
        let got = attribute_errors(&[missing_ns.clone(), nested, exec], &Vec::new()).unwrap();
        let roots: Vec<Root> = got.iter().map(|(r, _)| *r).collect();
        assert_eq!(roots, [Root::Properties, Root::Plugins, Root::Session]);

        // Unattributable: a real failure, never absorbed as "unavailable".
        assert!(attribute_errors(&[json!({"message": "Syntax error"})], &Vec::new()).is_none());
        let foreign = json!({"message": "Validation error (FieldUndefined@[bogus]) : x"});
        assert!(attribute_errors(&[foreign], &Vec::new()).is_none());
        // One unattributable error sinks the lot.
        let mixed = [missing_ns.clone(), json!({"message": "x"})];
        assert!(attribute_errors(&mixed, &Vec::new()).is_none());
        // A root already dropped cannot fail again; if it seems to, stop.
        let off = vec![(Root::Properties, "gone".to_string())];
        assert!(attribute_errors(&[missing_ns], &off).is_none());
    }

    /// The finding that shaped this module: for an admin, `getMatchingRoles`
    /// echoes back a name that is not a role. It must not pass.
    #[test]
    fn admin_does_not_pass_a_role_that_does_not_exist() {
        let p = plan(&["itil", "definitely_not_a_role"], &[], &[]);
        let d = data(true, &["itil", "definitely_not_a_role"], &[1, 0], &[], &[]);
        let r = evaluate(&p, &d, &Vec::new());
        let fake = check(&r, "role", "definitely_not_a_role");
        assert_eq!(fake["status"], "fail");
        assert_eq!(fake["exists"], false);
        assert!(fake["reason"].as_str().unwrap().contains("admin"), "{fake}");
        // A real role passes, and says it passed on admin's say-so.
        let itil = check(&r, "role", "itil");
        assert_eq!(itil["status"], "pass");
        assert_eq!(itil["basis"], "admin");
        assert_eq!(r["admin"], true);
        assert_eq!(r["ok"], false);
    }

    #[test]
    fn non_admin_roles_rest_on_the_session() {
        let p = plan(&["itil", "admin"], &[], &[]);
        let d = data(false, &["itil"], &[1, 1], &[], &[]);
        let r = evaluate(&p, &d, &Vec::new());
        assert_eq!(check(&r, "role", "itil")["basis"], "session");
        let admin = check(&r, "role", "admin");
        assert_eq!(admin["status"], "fail");
        assert_eq!(admin["exists"], true);
        assert_eq!(r["admin"], false);
        assert_eq!(r["user"], json!({"user_name": "beth", "sys_id": "u1"}));
    }

    #[test]
    fn a_dropped_role_filter_is_unavailable_not_an_answer() {
        let p = plan(&["itil"], &[], &[]);
        let d = data(false, &["itil"], &[981], &[], &[]);
        let r = evaluate(&p, &d, &Vec::new());
        let c = check(&r, "role", "itil");
        assert_eq!(c["status"], "unavailable");
        assert!(c["reason"].as_str().unwrap().contains("981"), "{c}");
        assert_eq!(r["ok"], false);
    }

    #[test]
    fn plugins_pass_fail_and_refuse_a_lying_getter() {
        let p = plan(&[], &["com.snc.incident", "fake.plugin", "odd"], &[]);
        let d = data(
            false,
            &[],
            &[],
            &[json!(true), json!(false), Value::Null],
            &[],
        );
        let r = evaluate(&p, &d, &Vec::new());
        assert_eq!(check(&r, "plugin", "com.snc.incident")["status"], "pass");
        assert_eq!(check(&r, "plugin", "fake.plugin")["status"], "fail");
        assert_eq!(check(&r, "plugin", "odd")["status"], "unavailable");

        // The getter denies the plugin this very request ran on: believe none.
        let one = plan(&[], &["com.snc.incident"], &[]);
        let mut d = data(false, &[], &[], &[json!(true)], &[]);
        d["snWorkflowStudio"]["workflowStudio"]["canary"] = json!(false);
        let r = evaluate(&one, &d, &Vec::new());
        assert_eq!(
            check(&r, "plugin", "com.snc.incident")["status"],
            "unavailable"
        );
        assert_eq!(r["capabilities"]["plugins"]["available"], false);

        // Non-admin: the whole namespace answers null.
        let mut d = data(false, &[], &[], &[], &[]);
        d["snWorkflowStudio"]["workflowStudio"] = Value::Null;
        let r = evaluate(&one, &d, &Vec::new());
        assert_eq!(
            check(&r, "plugin", "com.snc.incident")["status"],
            "unavailable"
        );
    }

    #[test]
    fn properties_distinguish_unset_unreadable_and_mismatched() {
        let p = plan(
            &[],
            &[],
            &["set", "unset", "secret", "want=true", "want2=true"],
        );
        let values = [
            json!("https://x/"),
            json!(UNSET),
            Value::Null,
            json!("false"),
            json!("true"),
        ];
        let r = evaluate(&p, &data(false, &[], &[], &[], &values), &Vec::new());
        let set = check(&r, "property", "set");
        assert_eq!(set["status"], "pass");
        assert_eq!(set["value"], "https://x/");
        let unset = check(&r, "property", "unset");
        assert_eq!(unset["status"], "fail");
        // The sentinel never leaks out as if it were the value.
        assert_eq!(unset["value"], Value::Null);
        assert_eq!(check(&r, "property", "secret")["status"], "unavailable");
        let want = check(&r, "property", "want");
        assert_eq!(want["status"], "fail");
        assert_eq!(want["expected"], "true");
        assert_eq!(check(&r, "property", "want2")["status"], "pass");
        assert_eq!(r["build_tag"], "glide-brazil");
    }

    #[test]
    fn a_non_admin_property_getter_is_unavailable_not_unset() {
        // Measured as itil: the namespace resolves, every read is null.
        let mut d = data(false, &[], &[], &[], &[Value::Null]);
        d["snDecisionTable"]["sysProperties"]["build"] = Value::Null;
        let r = evaluate(&plan(&[], &[], &["glide.servlet.uri"]), &d, &Vec::new());
        assert_eq!(
            check(&r, "property", "glide.servlet.uri")["status"],
            "unavailable"
        );
        assert_eq!(r["capabilities"]["properties"]["available"], false);
        assert_eq!(r["build_tag"], Value::Null);
    }

    #[test]
    fn a_disabled_root_reports_the_instance_reason() {
        let off = vec![(
            Root::Properties,
            "Field 'snDecisionTable' is undefined".into(),
        )];
        let mut d = data(false, &[], &[], &[], &[]);
        d.as_object_mut().unwrap().remove("snDecisionTable");
        let r = evaluate(&plan(&[], &[], &["x.y"]), &d, &off);
        let c = check(&r, "property", "x.y");
        assert_eq!(c["status"], "unavailable");
        assert!(c["reason"].as_str().unwrap().contains("undefined"), "{c}");
    }

    #[test]
    fn identity_fails_closed_on_a_dropped_term() {
        let mut d = data(false, &[], &[], &[], &[]);
        d["GlideRecord_Query"]["me"]["_rowCount"] = json!(2);
        assert_eq!(
            evaluate(&Plan::default(), &d, &Vec::new())["user"],
            Value::Null
        );
    }

    #[test]
    fn no_checks_is_ok_and_summarizes_nothing() {
        let r = evaluate(
            &Plan::default(),
            &data(true, &[], &[], &[], &[]),
            &Vec::new(),
        );
        assert_eq!(r["ok"], true);
        assert_eq!(r["checks"], json!([]));
        assert!(failure_summary(&r).is_none());
    }

    #[test]
    fn summary_names_every_check_that_did_not_pass() {
        let p = plan(&["itil", "nope"], &["x.y"], &[]);
        let mut d = data(false, &["itil"], &[1, 0], &[], &[]);
        d["snWorkflowStudio"]["workflowStudio"] = Value::Null;
        let msg = failure_summary(&evaluate(&p, &d, &Vec::new())).unwrap();
        assert_eq!(
            msg,
            "2 of 3 doctor checks did not pass: role 'nope' (fail), plugin 'x.y' (unavailable)"
        );
    }
}
