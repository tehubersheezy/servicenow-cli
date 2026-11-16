//! `sn appmanager` — the Application Manager: which store apps and plugins are
//! installed, which have an update, and which could be installed.
//!
//! Reads the scripted REST API behind the Application Manager UI
//! (`/api/sn_appclient/appmanager`, also mounted at `/v1/appmanager`). It is
//! undocumented; everything below was measured on a Brazil instance or read
//! from the API's own script includes, and each trap answered HTTP 200.
//!
//! - **Two routes, one list.** `POST /apps` and `POST /plugins` (the body is a
//!   filter object, `{}` here) each need `tab_context` — `installed`,
//!   `updates` or `available_for_you` — and answer 400 without it. Apps come
//!   back as `result.apps`, plugins as a bare `result` array. The UI merges
//!   them, and so does `list`.
//! - **Neither route is paged or searched server-side.** The plugin route
//!   drops plugins that are really store apps by comparing against the *same
//!   request's* apps query, which obeys the same `sysparm_limit`: paged at
//!   200 it returned 963 rows for 961 plugins, one of them twice. And
//!   `search_key` on `/apps` is a text search, not a substring match (`flow`
//!   returned "Docker Spoke"). Both lists are fetched whole (252 apps and 961
//!   plugins took about 2.5 s) and filtered here.
//! - **Totals travel in an `x-total-count` header**, which is one more reason
//!   to fetch whole: the count is then simply the number of rows.
//! - **`GET /app/{id}` takes a sys_id, not a scope** (a scope is a 400 "No app
//!   found matching sourceAppId"), so a scope is resolved through the lists
//!   first. Every call mints tiny-URL rows for the links in its answer.
//! - **`GET /plugin/{id}` answers an unknown id with 200 and `{"plugin":{}}`**,
//!   matches case-sensitively, and knows nothing of store-app scopes. An
//!   empty object is "no such plugin", never "not installed".
//! - **The write routes are GETs** (`/app/install`, `/app/update`,
//!   `/app/repair`, `/plugin/activate`, `/plugin/repair`, `/plugin/rollback`).
//!   An id is a path segment, so those words are refused as ids before the
//!   network: `sn appmanager show activate` must never become a request.
//! - **`GET /plugin/activate?plugin_id=` answers in the CI/CD shape**:
//!   `status` `"0"` Pending with `links.progress.id`, pollable at
//!   `/api/sn_cicd/progress/{id}` (a small plugin took 45 s). An unknown id is
//!   refused *in band* — 200, `status: "3"`, `error: "Plugin ID is not valid"`,
//!   no link — so `install` reads the status itself rather than trusting 200.
//! - **`GET /app/install` accepts an id that names nothing**: 200, Pending,
//!   and the operation then retries in the instance's operation queue until
//!   someone cancels it. Nothing here sends an id it has not resolved first,
//!   and store-app install and update are not wired (the reference instance
//!   cannot reach the store, so neither could be measured).
//! - **`available_for_you` queues an instance event** on every call
//!   (`sn_appclient.update_last_installed_from`), as opening the UI does.
//! - **One REST ACL guards every route**: `admin`,
//!   `sn_appclient.app_client_user`, `sn_appclient.app_client_company_installer`
//!   or `unified_plugin_read_only`. Anyone else gets 403.
//! - Plugin list rows carry no version; `show` does. An app's "update
//!   available" is whatever the instance last synced from the store.

use crate::cli::get::common::identifier;
use crate::cli::kernel::{confirm_destructive, connect, unwrap_or_raw, write_response};
use crate::cli::record_ref::is_sys_id;
use crate::cli::{GlobalFlags, OutputMode, WaitArgs};
use crate::client::Client;
use crate::error::{Error, NO_HTTP_STATUS, Result};
use clap::{Subcommand, ValueEnum};
use serde_json::{Value, json};

const BASE: &str = "/api/sn_appclient/appmanager";
/// ServiceNow's reply for a scripted REST route that does not exist.
const NO_SUCH_RESOURCE: &str = "Requested URI does not represent any resource";
/// The roles the API's one REST ACL ("App Manager Security Rules") accepts.
const ROLES: &str = "admin, sn_appclient.app_client_user, \
                     sn_appclient.app_client_company_installer or unified_plugin_read_only";
/// Fixed segments under `/app/` and `/plugin/`. Each is a GET that changes the
/// instance, so none may ever be sent as an id.
const ROUTE_WORDS: [&str; 5] = ["activate", "install", "repair", "rollback", "update"];

#[derive(Subcommand, Debug)]
pub enum AppManagerSub {
    /// List store apps and plugins: installed ones by default, or those with
    /// an update (--updates) or not yet installed (--available).
    ///
    /// One row per app or plugin, sorted by name. `id` is what `sn appmanager show`
    /// takes: the scope for an app, the plugin id for a plugin. Plugin rows
    /// carry no version; `sn appmanager show` reports it. `--output raw` prints the
    /// instance's own response for one `--kind`.
    List(AppListArgs),
    /// Find apps and plugins by substring across id, name, scope and description.
    ///
    /// Same rows and flags as `sn appmanager list`. `--output raw` prints the
    /// unfiltered response instead, as `sn appmanager list` does.
    Search(AppSearchArgs),
    /// One app or plugin: version, whether it is installed, whether an update
    /// is available, dependencies.
    ///
    /// An id nothing answers to is an error (exit 2), not `installed: false`.
    Show(AppShowArgs),
    /// Install (activate) a plugin by its plugin id. Cannot be undone: an
    /// activated plugin cannot be deactivated.
    ///
    /// Runs in the background on the instance; `--wait` blocks until it
    /// finishes, otherwise poll `links.progress.id` with `sn cicd progress`.
    /// Store apps are installed with `sn cicd app install`.
    Install(AppInstallPluginArgs),
}

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
#[value(rename_all = "lowercase")]
pub enum Kind {
    /// Store applications (scoped apps from the ServiceNow Store).
    App,
    /// Platform plugins.
    Plugin,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::App => "app",
            Kind::Plugin => "plugin",
        }
    }
}

#[derive(clap::Args, Debug, Default)]
pub struct AppFilterArgs {
    /// Only installed apps and plugins that have an update available, as of
    /// the instance's last sync with the store.
    #[arg(long, conflicts_with = "available")]
    pub updates: bool,
    /// What is not installed yet instead of what is.
    #[arg(long)]
    pub available: bool,
    /// Only store apps or only plugins. Default: both.
    #[arg(long, short = 'k', value_enum, value_name = "KIND")]
    pub kind: Option<Kind>,
    /// Maximum rows to print. Default: all of them. A cut is reported on stderr.
    #[arg(long, short = 'l', value_name = "N",
          value_parser = clap::value_parser!(u32).range(1..))]
    pub limit: Option<u32>,
    /// Rows to skip before the first one printed.
    #[arg(long, value_name = "N", default_value_t = 0)]
    pub offset: u32,
}

#[derive(clap::Args, Debug, Default)]
pub struct AppListArgs {
    #[command(flatten)]
    pub filter: AppFilterArgs,
}

#[derive(clap::Args, Debug)]
pub struct AppSearchArgs {
    /// Case-insensitive substring, matched against id, name, scope and description.
    #[arg(value_name = "TERM")]
    pub term: String,
    #[command(flatten)]
    pub filter: AppFilterArgs,
}

#[derive(clap::Args, Debug)]
pub struct AppShowArgs {
    /// The `id` `sn appmanager list` prints: an app's scope (`sn_flow_designer`) or a
    /// plugin id (`com.snc.incident`). An app's sys_id works too.
    #[arg(value_name = "ID")]
    pub id: String,
    /// Look only among store apps or only among plugins. Needed only when an
    /// id answers to both.
    #[arg(long, short = 'k', value_enum, value_name = "KIND")]
    pub kind: Option<Kind>,
}

#[derive(clap::Args, Debug)]
pub struct AppInstallPluginArgs {
    /// The plugin id, as `sn appmanager list --available --kind plugin` prints it
    /// (e.g. `com.snc.document_management`).
    #[arg(value_name = "PLUGIN_ID")]
    pub id: String,
    /// Also load the plugin's demo data.
    #[arg(long)]
    pub demo_data: bool,
    /// Skip confirmation prompt (required for non-interactive use).
    #[arg(long, short = 'y')]
    pub yes: bool,
    #[command(flatten)]
    pub wait: WaitArgs,
}

/// Dispatch one `sn appmanager` verb.
pub fn run(global: &GlobalFlags, sub: AppManagerSub) -> Result<()> {
    match sub {
        AppManagerSub::List(args) => list(global, "list", &args.filter, None),
        AppManagerSub::Search(args) => {
            let term = args.term.trim().to_lowercase();
            if term.is_empty() {
                return Err(Error::Usage("the search term must not be empty".into()));
            }
            list(global, "search", &args.filter, Some(&term))
        }
        AppManagerSub::Show(args) => show(global, args),
        AppManagerSub::Install(args) => install(global, args),
    }
}

/// The UI's three tabs, which are also the API's only way to say which rows
/// are wanted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Installed,
    Updates,
    Available,
}

impl Tab {
    fn context(self) -> &'static str {
        match self {
            Tab::Installed => "installed",
            Tab::Updates => "updates",
            Tab::Available => "available_for_you",
        }
    }
}

impl AppFilterArgs {
    fn tab(&self) -> Tab {
        if self.updates {
            Tab::Updates
        } else if self.available {
            Tab::Available
        } else {
            Tab::Installed
        }
    }
}

fn list(
    global: &GlobalFlags,
    verb: &str,
    filter: &AppFilterArgs,
    term: Option<&str>,
) -> Result<()> {
    let raw = global.output == OutputMode::Raw;
    if raw {
        check_raw(verb, filter)?;
    }
    let client = connect(global)?;
    let tab = filter.tab();

    if raw {
        let route = match filter.kind {
            Some(Kind::Plugin) => "plugins",
            _ => "apps",
        };
        return write_response(global, &fetch(&client, route, tab)?);
    }

    let mut rows: Vec<Value> = Vec::new();
    if filter.kind != Some(Kind::Plugin) {
        let resp = fetch(&client, "apps", tab)?;
        rows.extend(app_rows(&resp)?.iter().map(|r| app_row(r, tab)));
    }
    if filter.kind != Some(Kind::App) {
        let resp = fetch(&client, "plugins", tab)?;
        rows.extend(plugin_rows(&resp)?.iter().map(plugin_row));
    }
    if let Some(term) = term {
        rows.retain(|row| matches(row, term));
    }
    rows.sort_by_cached_key(|row| (lower(row, "name"), lower(row, "id")));

    let total = rows.len();
    let page: Vec<Value> = rows
        .into_iter()
        .skip(filter.offset as usize)
        .take(filter.limit.map_or(usize::MAX, |n| n as usize))
        .collect();
    if page.len() < total {
        eprintln!("sn: showing {} of {total} rows", page.len());
    }
    write_response(global, &Value::Array(page))
}

/// `--output raw` promises the instance's own envelope, and a merged, cut list
/// has none: two requests, rows reordered, a page taken here rather than there.
fn check_raw(verb: &str, filter: &AppFilterArgs) -> Result<()> {
    if filter.kind.is_none() {
        return Err(Error::Usage(format!(
            "--output raw cannot render sn appmanager {verb}'s merged result (two requests, no single \
             envelope to keep); add --kind app or --kind plugin, or use the default output"
        )));
    }
    if filter.limit.is_some() || filter.offset != 0 {
        return Err(Error::Usage(
            "--limit and --offset cut the merged rows, which --output raw does not print; \
             drop them or use the default output"
                .into(),
        ));
    }
    Ok(())
}

/// One whole tab from one route. Never paged and never searched: see the
/// module notes for what the instance does with either.
fn fetch(client: &Client, route: &str, tab: Tab) -> Result<Value> {
    client
        .post(
            &format!("{BASE}/{route}"),
            &[("tab_context".to_string(), tab.context().to_string())],
            &json!({}),
        )
        .map_err(explain)
}

fn app_rows(resp: &Value) -> Result<&Vec<Value>> {
    resp.pointer("/result/apps")
        .and_then(Value::as_array)
        .ok_or_else(|| unexpected("apps", "no `result.apps` array"))
}

fn plugin_rows(resp: &Value) -> Result<&Vec<Value>> {
    resp.get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| unexpected("plugins", "`result` is not an array"))
}

fn unexpected(route: &str, what: &str) -> Error {
    Error::Instance {
        message: format!(
            "the Application Manager API's {route} answer is not the list it should be ({what})"
        ),
        detail: None,
    }
}

/// A store-app list row in the shape both kinds share. `id` is the scope, the
/// handle a person recognizes; the sys_id the detail route needs rides along.
fn app_row(row: &Value, tab: Tab) -> Value {
    let scope = text(row, "scope");
    json!({
        "kind": "app",
        "id": if scope.is_null() { text(row, "sys_id") } else { scope.clone() },
        "name": text(row, "name"),
        "scope": scope,
        "version": text(row, "version"),
        "latest_version": text(row, "latest_version"),
        "installed": tab != Tab::Available,
        "update_available": flag(row.get("update_available")),
        "vendor": text(row, "vendor"),
        "description": text(row, "short_description"),
        "sys_id": text(row, "sys_id"),
    })
}

/// A plugin list row in the same shape. The route returns no version, and its
/// `sys_id` is the plugin id again, so neither is passed off as the real thing.
fn plugin_row(row: &Value) -> Value {
    json!({
        "kind": "plugin",
        "id": text(row, "app_id"),
        "name": text(row, "app_name"),
        "scope": text(row, "scope"),
        "version": Value::Null,
        "latest_version": Value::Null,
        "installed": flag(row.get("active")),
        "update_available": flag(row.get("update_available")),
        "vendor": Value::Null,
        "description": text(row, "description"),
        "sys_id": Value::Null,
    })
}

/// A non-empty string field, or null. The instance sends `""` and `null`
/// interchangeably for "nothing here".
fn text(row: &Value, key: &str) -> Value {
    match row.get(key).and_then(Value::as_str) {
        Some(s) if !s.is_empty() => Value::from(s),
        _ => Value::Null,
    }
}

/// The instance's booleans: `"0"`/`"1"` in list rows, real booleans in
/// details, and absent for "no".
fn flag(v: Option<&Value>) -> bool {
    match v {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => matches!(s.as_str(), "1" | "true"),
        Some(Value::Number(n)) => n.as_f64().is_some_and(|n| n != 0.0),
        _ => false,
    }
}

fn lower(row: &Value, key: &str) -> String {
    row.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_lowercase()
}

/// `term` is already lowercased.
fn matches(row: &Value, term: &str) -> bool {
    ["id", "name", "scope", "description"]
        .iter()
        .any(|key| lower(row, key).contains(term))
}

/// What the caller named: an app's sys_id, or a scope / plugin id.
#[derive(Debug, PartialEq, Eq)]
enum AppRef {
    SysId(String),
    Id(String),
}

/// Validate an id before it can become a path segment.
fn parse_ref(raw: &str, kind: Option<Kind>) -> Result<AppRef> {
    let id = identifier(raw, "plugin id, app scope or app sys_id")?;
    if id.chars().all(|c| c == '.') {
        return Err(Error::Usage(format!(
            "'{id}' is not an app scope or plugin id"
        )));
    }
    if ROUTE_WORDS.iter().any(|word| word.eq_ignore_ascii_case(id)) {
        return Err(Error::Usage(format!(
            "'{id}' is not an app scope or plugin id: it is an Application Manager route \
             that changes the instance, so it is never sent as an id"
        )));
    }
    if !is_sys_id(id) {
        return Ok(AppRef::Id(id.to_string()));
    }
    if kind == Some(Kind::Plugin) {
        return Err(Error::Usage(format!(
            "'{id}' is a sys_id, and a plugin is named by its plugin id (e.g. com.snc.incident)"
        )));
    }
    Ok(AppRef::SysId(id.to_ascii_lowercase()))
}

/// The record a reference resolved to, with the envelope of the request that
/// read it (what `--output raw` prints).
enum Found {
    App(Value),
    Plugin(Value),
}

fn show(global: &GlobalFlags, args: AppShowArgs) -> Result<()> {
    let target = parse_ref(&args.id, args.kind)?;
    let client = connect(global)?;
    let found = resolve(&client, &target, args.kind)?;
    if global.output == OutputMode::Raw {
        let (Found::App(resp) | Found::Plugin(resp)) = &found;
        return write_response(global, resp);
    }
    let summary = match &found {
        Found::App(resp) => app_summary(&resp["result"]["app_info_on_instance"]),
        Found::Plugin(resp) => plugin_summary(&resp["result"]["plugin"]),
    };
    write_response(global, &summary)
}

/// Find the one app or plugin a reference names. Apps and plugins share the
/// id namespace in principle, so with no `--kind` both sides are asked and an
/// id that answers to both is the caller's to settle.
fn resolve(client: &Client, target: &AppRef, kind: Option<Kind>) -> Result<Found> {
    let id = match target {
        AppRef::SysId(sys_id) => return app_detail(client, sys_id, None).map(Found::App),
        AppRef::Id(id) => id,
    };
    let app = match kind {
        Some(Kind::Plugin) => None,
        _ => find_app(client, id)?,
    };
    let plugin = match kind {
        Some(Kind::App) => None,
        _ => plugin_detail(client, id)?,
    };
    match (app, plugin) {
        (Some(sys_id), None) => app_detail(client, &sys_id, Some(id)).map(Found::App),
        (None, Some(resp)) => Ok(Found::Plugin(resp)),
        (Some(sys_id), Some(_)) => Err(Error::Usage(format!(
            "'{id}' is both a store app's scope (sys_id {sys_id}) and a plugin id; \
             pass --kind app or --kind plugin"
        ))),
        (None, None) => Err(Error::Api {
            status: NO_HTTP_STATUS,
            message: format!(
                "no {} with id '{id}' on this instance",
                kind.map_or("store app or plugin", Kind::as_str)
            ),
            detail: Some(
                "ids are exact and case-sensitive; find one with `sn appmanager search <TERM>` \
                 (add --available for what is not installed)"
                    .into(),
            ),
            transaction_id: None,
            sn_error: None,
        }),
    }
}

/// The sys_id of the store app with exactly this scope: installed first, then
/// available. The detail route takes nothing else.
fn find_app(client: &Client, scope: &str) -> Result<Option<String>> {
    for tab in [Tab::Installed, Tab::Available] {
        let resp = fetch(client, "apps", tab)?;
        let ids: Vec<&str> = app_rows(&resp)?
            .iter()
            .filter(|row| row.get("scope").and_then(Value::as_str) == Some(scope))
            .filter_map(|row| row.get("sys_id").and_then(Value::as_str))
            .collect();
        match ids.as_slice() {
            [] => {}
            [one] => return Ok(Some(one.to_string())),
            many => {
                return Err(Error::Usage(format!(
                    "{} store apps have the scope '{scope}'; pass a sys_id: {}",
                    many.len(),
                    many.join(", ")
                )));
            }
        }
    }
    Ok(None)
}

/// `GET /app/{sys_id}`, checked to be the app that was asked for.
fn app_detail(client: &Client, sys_id: &str, scope: Option<&str>) -> Result<Value> {
    let resp = client
        .get(&format!("{BASE}/app/{sys_id}"), &[])
        .map_err(explain)?;
    let info = &resp["result"]["app_info_on_instance"];
    let same_id = info
        .get("sys_id")
        .and_then(Value::as_str)
        .is_some_and(|got| got.eq_ignore_ascii_case(sys_id));
    let same_scope =
        scope.is_none_or(|want| info.get("scope").and_then(Value::as_str) == Some(want));
    if !same_id || !same_scope {
        return Err(Error::Instance {
            message: format!(
                "asked the Application Manager for app {sys_id} and got a different record back"
            ),
            detail: Some(format!(
                "returned sys_id {}, scope {}",
                info.get("sys_id").unwrap_or(&Value::Null),
                info.get("scope").unwrap_or(&Value::Null)
            )),
        });
    }
    Ok(resp)
}

/// `GET /plugin/{id}`: `None` for the empty object an unknown id gets, and an
/// error for a plugin that is not the one asked for.
fn plugin_detail(client: &Client, id: &str) -> Result<Option<Value>> {
    let resp = client
        .get(&format!("{BASE}/plugin/{id}"), &[])
        .map_err(explain)?;
    let Some(plugin) = resp.pointer("/result/plugin").and_then(Value::as_object) else {
        return Err(Error::Instance {
            message: format!(
                "asked the Application Manager for plugin '{id}' and the answer holds no \
                 `result.plugin` object"
            ),
            detail: None,
        });
    };
    if plugin.is_empty() {
        return Ok(None);
    }
    let got = plugin
        .get("id")
        .or_else(|| plugin.get("app_id"))
        .and_then(Value::as_str);
    if got != Some(id) {
        return Err(Error::Instance {
            message: format!(
                "asked the Application Manager for plugin '{id}' and got a different record back"
            ),
            detail: Some(format!("returned id {}", got.unwrap_or("(none)"))),
        });
    }
    Ok(Some(resp))
}

/// Activate one plugin. The id is resolved first and the answer's own status
/// read afterwards: the instance accepts ids that name nothing and reports
/// refusals under HTTP 200.
fn install(global: &GlobalFlags, args: AppInstallPluginArgs) -> Result<()> {
    let AppRef::Id(id) = parse_ref(&args.id, Some(Kind::Plugin))? else {
        unreachable!("parse_ref refuses a sys_id for a plugin");
    };
    // An activated plugin cannot be deactivated, and its dependencies come
    // with it: instance-wide, asynchronous, no second confirmation downstream.
    let what = if args.demo_data {
        format!("plugin {id} with demo data")
    } else {
        format!("plugin {id}")
    };
    confirm_destructive(args.yes, "install", &what)?;

    let client = connect(global)?;
    let Some(found) = plugin_detail(&client, &id)? else {
        return Err(Error::Api {
            status: NO_HTTP_STATUS,
            message: format!("no plugin with id '{id}' on this instance"),
            detail: Some(
                "ids are exact and case-sensitive; find one with `sn appmanager search <TERM> \
                 --available --kind plugin`. A store app is installed with `sn cicd app install`"
                    .into(),
            ),
            transaction_id: None,
            sn_error: None,
        });
    };
    let plugin = &found["result"]["plugin"];
    if flag(plugin.get("isInstalled")) {
        return Err(Error::Usage(format!(
            "plugin {id} is already installed{}",
            plugin
                .get("version")
                .and_then(Value::as_str)
                .map(|v| format!(" (version {v})"))
                .unwrap_or_default()
        )));
    }

    let resp = client
        .get(
            &format!("{BASE}/plugin/activate"),
            &[
                ("plugin_id".to_string(), id.clone()),
                ("load_demo_data".to_string(), args.demo_data.to_string()),
            ],
        )
        .map_err(|err| may_have_started(explain(err), &id))?;
    check_started(&resp, &id)?;
    let out = unwrap_or_raw(resp, global.output);
    crate::cli::progress::finish_cicd(global, &client, out, args.wait)
}

/// A write that never answered may still have been received.
fn may_have_started(err: Error, id: &str) -> Error {
    match err {
        Error::Transport(msg) => Error::Transport(format!(
            "{msg}; the install of {id} may have started anyway — check with \
             `sn appmanager show {id}` before retrying"
        )),
        other => other,
    }
}

/// The activate route's refusals arrive under 200 as `status: "3"` with the
/// reason in `error`, and a started operation always carries a progress link.
fn check_started(resp: &Value, id: &str) -> Result<()> {
    let body = &resp["result"];
    let said = |key: &str| {
        body.get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
    };
    if matches!(said("status"), Some("3" | "4")) {
        return Err(Error::Api {
            status: NO_HTTP_STATUS,
            message: format!(
                "the instance refused to install plugin {id}: {}",
                said("error")
                    .or(said("status_message"))
                    .unwrap_or("no reason given")
            ),
            detail: said("status_detail").map(String::from),
            transaction_id: None,
            sn_error: Some(body.clone()),
        });
    }
    if body
        .pointer("/links/progress/id")
        .and_then(Value::as_str)
        .is_none()
    {
        return Err(Error::Instance {
            message: format!(
                "asked the instance to install plugin {id} and its answer names no operation \
                 to follow, so whether anything started is unknown"
            ),
            detail: Some(format!(
                "check with `sn appmanager show {id}`; answer: {body}"
            )),
        });
    }
    Ok(())
}

fn app_summary(info: &Value) -> Value {
    json!({
        "kind": "app",
        "id": text(info, "scope"),
        "name": text(info, "name"),
        "scope": text(info, "scope"),
        "version": text(info, "version"),
        "latest_version": text(info, "latest_version"),
        "installed": flag(info.get("isInstalled")),
        "update_available": flag(info.get("isInstalledAndUpdateAvailable")),
        "vendor": text(info, "vendor"),
        "description": text(info, "short_description"),
        "sys_id": text(info, "sys_id"),
        "plugin_id": text(info, "sys_code"),
        "install_date": text(info, "install_date"),
        "demo_data": text(info, "demo_data"),
        "dependencies": dependencies(info.get("dependencies")),
        "indicators": info.get("indicators").cloned().unwrap_or(Value::Null),
    })
}

fn plugin_summary(plugin: &Value) -> Value {
    json!({
        "kind": "plugin",
        "id": text(plugin, "id"),
        "name": text(plugin, "name"),
        "scope": text(plugin, "scope"),
        "version": text(plugin, "version"),
        "latest_version": Value::Null,
        "installed": flag(plugin.get("isInstalled")),
        "update_available": flag(plugin.get("update_available")),
        "vendor": Value::Null,
        "description": text(plugin, "description"),
        "sys_id": Value::Null,
        "product": text(plugin, "lineage_name"),
        "has_demo_data": flag(plugin.get("has_demo_data")),
        "can_rollback": flag(plugin.get("can_rollback")),
        "indicators": plugin.get("indicators").cloned().unwrap_or(Value::Null),
    })
}

/// An app's dependencies arrive as one string, `id:version,id:version`, where
/// the version of a platform plugin is the word `sys`.
fn dependencies(v: Option<&Value>) -> Value {
    let Some(list) = v.and_then(Value::as_str) else {
        return json!([]);
    };
    list.split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| match entry.rsplit_once(':') {
            Some((id, version)) => json!({"id": id, "version": version}),
            None => json!({"id": entry, "version": Value::Null}),
        })
        .collect()
}

/// Say what a refusal from this API means: who it answers to, and what a
/// missing route is. Every other error passes through untouched.
fn explain(err: Error) -> Error {
    match err {
        Error::Auth {
            status: 403,
            message,
            transaction_id,
        } => Error::Auth {
            status: 403,
            message: format!("{message}: the Application Manager API answers only to {ROLES}"),
            transaction_id,
        },
        Error::Api {
            status: 400,
            message,
            detail,
            transaction_id,
            sn_error,
        } if message.trim() == NO_SUCH_RESOURCE => Error::Api {
            status: 400,
            message: format!(
                "the Application Manager API is not available on this instance ({message}): \
                 its REST API ({BASE}) is absent or inactive"
            ),
            detail,
            transaction_id,
            sn_error,
        },
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_words_and_path_tricks_are_never_ids() {
        for bad in [
            "activate", "Install", "update", "repair", "rollback", ".", "..", "a/b", "",
        ] {
            assert!(
                matches!(parse_ref(bad, None), Err(Error::Usage(_))),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_sys_id_names_an_app_and_never_a_plugin() {
        let hex = "012FA9AD7367AD6393AE5DEA97AF6F65";
        assert_eq!(
            parse_ref(hex, None).unwrap(),
            AppRef::SysId(hex.to_ascii_lowercase())
        );
        assert!(parse_ref(hex, Some(Kind::Plugin)).is_err());
        assert_eq!(
            parse_ref("com.snc.incident", None).unwrap(),
            AppRef::Id("com.snc.incident".into())
        );
    }

    #[test]
    fn the_instances_booleans() {
        for yes in [json!("1"), json!("true"), json!(true), json!(1)] {
            assert!(flag(Some(&yes)), "{yes}");
        }
        for no in [
            json!("0"),
            json!("false"),
            json!(false),
            json!(null),
            json!(""),
        ] {
            assert!(!flag(Some(&no)), "{no}");
        }
        assert!(!flag(None));
    }

    #[test]
    fn dependencies_split_on_the_last_colon() {
        assert_eq!(
            dependencies(Some(&json!("com.glide.hub:sys, sn_workflow_studio:30.0.4"))),
            json!([
                {"id": "com.glide.hub", "version": "sys"},
                {"id": "sn_workflow_studio", "version": "30.0.4"}
            ])
        );
        assert_eq!(dependencies(Some(&Value::Null)), json!([]));
    }

    #[test]
    fn only_the_403_and_the_missing_route_are_rewritten() {
        let Error::Auth { message, .. } = explain(Error::Auth {
            status: 403,
            message: "User Not Authorized".into(),
            transaction_id: None,
        }) else {
            panic!("expected Auth");
        };
        assert!(message.contains("unified_plugin_read_only"), "{message}");

        let api = |message: &str| Error::Api {
            status: 400,
            message: message.into(),
            detail: None,
            transaction_id: None,
            sn_error: None,
        };
        let Error::Api { message, .. } = explain(api(NO_SUCH_RESOURCE)) else {
            panic!("expected Api");
        };
        assert!(message.contains("not available"), "{message}");
        let Error::Api { message, .. } = explain(api("No app found")) else {
            panic!("expected Api");
        };
        assert_eq!(message, "No app found");
    }
}
