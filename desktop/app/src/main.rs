//! redline desktop shell: a menu bar, a tray icon, a window, and the same HTTP/MCP server the CLI
//! talks to.
//!
//! Two modes, decided by `config.json` in the data directory:
//! - **standalone** — this computer is the server (default, first run);
//! - **remote** — a server on another machine holds the sessions, this window is a thin client.
//!
//! Every action exists in three places — menu bar, tray, keyboard — and is implemented once in
//! [`handle_action`]. The setup wizard (`/setup`) is served by the local server in both modes, so
//! connecting to a server is a form, never a hand-edited file.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use axum::Router;
use redline_core::api::{router, AppState};
use redline_core::store::Store;
use serde_json::json;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_autostart::ManagerExt as AutostartExt;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

/// Set immediately before asking the process to exit. `RunEvent::ExitRequested` is vetoed while
/// this is false (that is what keeps the app alive with every window hidden), but a quit or a
/// post-update restart must be allowed through — otherwise the menu item does nothing at all.
static EXITING: AtomicBool = AtomicBool::new(false);

/// The port the CLI and agent configs assume (`redline` client default).
const PREFERRED_PORT: u16 = 8788;

/// The editor and the wizard, compiled in: no resource paths to resolve at runtime.
const UI_FILES: &[(&str, &str)] = &[
    ("index.html", include_str!("../../ui/index.html")),
    ("list.html", include_str!("../../ui/list.html")),
    ("login.html", include_str!("../../ui/login.html")),
    ("people.html", include_str!("../../ui/people.html")),
    ("audit.html", include_str!("../../ui/audit.html")),
    ("tokens.html", include_str!("../../ui/tokens.html")),
    ("invite.html", include_str!("../../ui/invite.html")),
    ("setup.html", include_str!("../../ui/setup.html")),
    ("servers.html", include_str!("../../ui/servers.html")),
    ("inbox.html", include_str!("../../ui/inbox.html")),
    ("app.css", include_str!("../../ui/app.css")),
    ("app.js", include_str!("../../ui/app.js")),
];

#[derive(Clone)]
struct Local {
    base_url: String,
    token: String,
}

#[derive(Clone, Debug)]
struct Profile {
    id: String,
    name: String,
    url: String,
    token: String,
}

/// The local agent runs in a thread next to the app; this is the only handle to it.
#[derive(Default)]
struct AgentState {
    cancel: Mutex<Option<Arc<AtomicBool>>>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

/// Everything the tray and menu handlers need; cloned into each closure.
#[derive(Clone)]
struct Ctx {
    data_dir: PathBuf,
    local: Local,
    agent: Arc<AgentState>,
    /// window label -> profile id (`local`, `manager`, or a profile id). One window per server;
    /// opening the same profile again focuses it instead of duplicating its cookies and state.
    windows: Arc<Mutex<HashMap<String, String>>>,
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_window(app);
        }))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            let data_dir = redline_core::paths::default_data_dir();

            // The local server always runs: it serves the wizard, and it is the app in standalone
            // mode. When a remote server is configured the window points there instead.
            let local = start_local_server(data_dir.clone())?;
            let ctx = Ctx {
                data_dir,
                local,
                agent: Arc::new(AgentState::default()),
                windows: Arc::new(Mutex::new(HashMap::new())),
            };
            let start_url = launch_url(&ctx.data_dir, &ctx.local);
            let start_profile = profile_for_url(&ctx.data_dir, &start_url);
            let start_is_manager = is_manager_url(&start_url);
            let main_role = if start_is_manager {
                "manager".to_string()
            } else {
                start_profile
                    .as_ref()
                    .map(|profile| profile.id.clone())
                    .unwrap_or_else(|| "local".to_string())
            };
            let title = window_title(start_profile.as_ref(), start_is_manager);
            let _window = build_window(
                app.handle(),
                &ctx.data_dir,
                &ctx.local,
                &ctx.windows,
                "main",
                &start_url,
                start_profile.as_ref(),
                &title,
                !start_is_manager,
                &main_role,
            )?;

            build_tray(app, &ctx)?;
            build_app_menu(app, &ctx)?;
            check_for_updates(app.handle().clone(), false);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build redline")
        .run(|_app, event| {
            // Keep the process (and therefore the MCP endpoint) alive when nothing is visible —
            // unless we asked for the exit ourselves.
            if let RunEvent::ExitRequested { api, .. } = event {
                if !EXITING.load(Ordering::SeqCst) {
                    api.prevent_exit();
                }
            }
        });
}

// ---------------------------------------------------------------- menus

/// The window menu bar. Same ids as the tray menu, same handler.
fn build_app_menu(app: &tauri::App, ctx: &Ctx) -> tauri::Result<()> {
    let sessions = MenuItem::with_id(app, "open", "Sessions", true, Some("CmdOrCtrl+1"))?;
    let servers = MenuItem::with_id(app, "servers", "Servers…", true, None::<&str>)?;
    let inbox = MenuItem::with_id(
        app,
        "inbox",
        "All reviews (combined inbox)…",
        true,
        Some("CmdOrCtrl+2"),
    )?;
    let copy_link = MenuItem::with_id(app, "copy-link", "Copy browser link", true, None::<&str>)?;
    let copy_mcp = MenuItem::with_id(app, "copy-mcp", "Copy MCP config", true, None::<&str>)?;
    let agent_start =
        MenuItem::with_id(app, "agent-start", "Start local agent", true, None::<&str>)?;
    let agent_stop = MenuItem::with_id(app, "agent-stop", "Stop local agent", true, None::<&str>)?;
    let agent_repo = MenuItem::with_id(
        app,
        "agent-repo",
        "Choose agent repository…",
        true,
        None::<&str>,
    )?;
    let check = MenuItem::with_id(
        app,
        "check-updates",
        "Check for updates…",
        true,
        None::<&str>,
    )?;
    let dev_update = MenuItem::with_id(
        app,
        "dev-update",
        "Install latest dev build…",
        true,
        None::<&str>,
    )?;
    let revert = MenuItem::with_id(
        app,
        "stable-revert",
        "Revert to last stable version…",
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "Quit RedLine", true, Some("CmdOrCtrl+Q"))?;
    let separator = PredefinedMenuItem::separator(app)?;
    let agent_separator = PredefinedMenuItem::separator(app)?;
    let docs = MenuItem::with_id(app, "docs", "Documentation", true, None::<&str>)?;
    let about = MenuItem::with_id(app, "about", "About RedLine", true, None::<&str>)?;
    let help_separator = PredefinedMenuItem::separator(app)?;

    let file = Submenu::with_items(
        app,
        "RedLine",
        true,
        &[
            &sessions,
            &servers,
            &inbox,
            &copy_link,
            &copy_mcp,
            &separator,
            &agent_start,
            &agent_stop,
            &agent_repo,
            &agent_separator,
            &quit,
        ],
    )?;
    let help = Submenu::with_items(
        app,
        "Help",
        true,
        &[&docs, &check, &dev_update, &revert, &help_separator, &about],
    )?;
    app.set_menu(Menu::with_items(app, &[&file, &help])?)?;

    let handler_ctx = ctx.clone();
    app.on_menu_event(move |app, event| {
        let id = event.id().as_ref().to_string();
        handle_action(&id, app, &handler_ctx);
    });
    Ok(())
}

fn build_tray(app: &tauri::App, ctx: &Ctx) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "tray:open", "Open RedLine", true, None::<&str>)?;
    let servers = MenuItem::with_id(app, "tray:servers", "Servers…", true, None::<&str>)?;
    let inbox = MenuItem::with_id(
        app,
        "tray:inbox",
        "All reviews (combined inbox)…",
        true,
        None::<&str>,
    )?;
    let login = tauri::menu::CheckMenuItem::with_id(
        app,
        "tray:autostart",
        "Start at login",
        true,
        app.autolaunch().is_enabled().unwrap_or(false),
        None::<&str>,
    )?;
    let copy_link = MenuItem::with_id(
        app,
        "tray:copy-link",
        "Copy browser link",
        true,
        None::<&str>,
    )?;
    let copy_config =
        MenuItem::with_id(app, "tray:copy-mcp", "Copy MCP config", true, None::<&str>)?;
    let agent_start = MenuItem::with_id(
        app,
        "tray:agent-start",
        "Start local agent",
        true,
        None::<&str>,
    )?;
    let agent_stop = MenuItem::with_id(
        app,
        "tray:agent-stop",
        "Stop local agent",
        true,
        None::<&str>,
    )?;
    let agent_repo = MenuItem::with_id(
        app,
        "tray:agent-repo",
        "Choose agent repository…",
        true,
        None::<&str>,
    )?;
    let check = MenuItem::with_id(
        app,
        "tray:check-updates",
        "Check for updates…",
        true,
        None::<&str>,
    )?;
    let dev_update = MenuItem::with_id(
        app,
        "tray:dev-update",
        "Install latest dev build…",
        true,
        None::<&str>,
    )?;
    let revert = MenuItem::with_id(
        app,
        "tray:stable-revert",
        "Revert to last stable version…",
        true,
        None::<&str>,
    )?;
    let status = match read_remote(&ctx.data_dir) {
        Some(remote) => format!("Configured: {}", remote.base_url),
        None => format!("Standalone: {}", ctx.local.base_url),
    };
    let address = MenuItem::with_id(app, "tray:address", &status, false, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let agent_separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "tray:quit", "Quit RedLine", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &open,
            &servers,
            &inbox,
            &login,
            &copy_link,
            &copy_config,
            &separator,
            &agent_start,
            &agent_stop,
            &agent_repo,
            &agent_separator,
            &check,
            &dev_update,
            &revert,
            &address,
            &separator,
            &quit,
        ],
    )?;

    let handler_ctx = ctx.clone();
    TrayIconBuilder::with_id("main")
        .tooltip("RedLine — mark the screenshot, the agent writes the fix")
        .icon(
            app.default_window_icon()
                .cloned()
                .ok_or(tauri::Error::UnknownPath)?,
        )
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| {
            let raw = event.id().as_ref().to_string();
            let id = raw.strip_prefix("tray:").unwrap_or(&raw);
            handle_action(id, app, &handler_ctx);
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_window(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

/// Every menu, tray and keyboard action funnels through here.
/// A menu click can be delivered to both the app-menu handler and the tray handler. Ignoring the
/// same action within a few hundred milliseconds keeps dialogs and file pickers from opening twice.
static LAST_ACTION: Mutex<Option<(String, std::time::Instant)>> = Mutex::new(None);

fn handle_action(id: &str, app: &AppHandle, ctx: &Ctx) {
    if let Ok(mut last) = LAST_ACTION.lock() {
        if let Some((previous, at)) = last.as_ref() {
            if previous == id && at.elapsed() < std::time::Duration::from_millis(400) {
                return;
            }
        }
        *last = Some((id.to_string(), std::time::Instant::now()));
    }
    match id {
        "open" => open_window(app, ctx),
        "servers" => open_manager_window(app, ctx),
        "inbox" => open_inbox_window(app, ctx),
        "copy-link" => {
            let target = match read_remote(&ctx.data_dir) {
                Some(remote) => format!("{}/?t={}", remote.base_url, remote.token),
                None => format!("{}/?t={}", ctx.local.base_url, ctx.local.token),
            };
            copy_or_complain(app, &target, "browser link");
        }
        "copy-mcp" => {
            copy_or_complain(app, &mcp_config(&ctx.data_dir, &ctx.local), "MCP config");
        }
        "agent-start" => start_agent(app, ctx),
        "agent-stop" => stop_agent(ctx),
        "agent-repo" => choose_agent_repo(app, ctx),
        "check-updates" => check_for_updates(app.clone(), true),
        "dev-update" => check_dev_update(app.clone()),
        "stable-revert" => revert_to_stable(app.clone()),
        "autostart" => {
            let manager = app.autolaunch();
            let result = if manager.is_enabled().unwrap_or(false) {
                manager.disable()
            } else {
                manager.enable()
            };
            if let Err(err) = result {
                eprintln!("redline: cannot change autostart: {err}");
            }
        }
        "docs" => open_url("https://github.com/erikibolya/redline#readme"),
        "about" => {
            app.dialog()
                .message(format!(
                    "RedLine {}\n\nMark the screenshot. The agent writes the fix.",
                    app.package_info().version
                ))
                .title("About RedLine")
                .kind(MessageDialogKind::Info)
                .show(|_| {});
        }
        "quit" => quit(app),
        _ => {}
    }
}

fn copy_or_complain(app: &AppHandle, text: &str, what: &str) {
    match copy_to_clipboard(text) {
        Ok(()) => {}
        Err(err) => {
            eprintln!("redline: cannot copy {what}: {err}");
            app.dialog()
                .message(format!("Could not copy the {what}: {err}"))
                .title("RedLine")
                .kind(MessageDialogKind::Error)
                .show(|_| {});
        }
    }
}

// ---------------------------------------------------------------- server URL

/// Standalone → the local UI; remote → the server's UI, or the wizard if it is not answering.
fn launch_url(data_dir: &Path, local: &Local) -> String {
    match read_remote(data_dir) {
        Some(remote) => {
            if reachable(&remote.base_url, &remote.token) {
                format!("{}/?t={}", remote.base_url, remote.token)
            } else {
                format!("{}/servers?t={}&unreachable=1", local.base_url, local.token)
            }
        }
        None => format!("{}/?t={}", local.base_url, local.token),
    }
}

fn reachable(base_url: &str, token: &str) -> bool {
    // The dependency-free client speaks plain HTTP only. For an https server we cannot probe,
    // so trust it: the webview shows the TLS/connection error if it is actually down. Probing
    // (and failing) here is what sent people back to the Connect form on every launch.
    if base_url.starts_with("https://") {
        return true;
    }
    redline_core::client::Client::new(&format!("{}/", base_url.trim_end_matches('/')), token)
        .and_then(|client| client.get("/api/config"))
        .is_ok()
}

/// Repo/model/server for the local agent, stored beside `config.json`.
#[derive(Default)]
struct AgentSettings {
    repo: Option<PathBuf>,
    model: Option<String>,
    opencode_server: Option<String>,
}

fn agent_config_path(data_dir: &Path) -> PathBuf {
    data_dir.join("agent.json")
}

fn read_agent_settings(data_dir: &Path) -> AgentSettings {
    let Ok(text) = std::fs::read_to_string(agent_config_path(data_dir)) else {
        return AgentSettings::default();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return AgentSettings::default();
    };
    AgentSettings {
        repo: value["repo"].as_str().map(PathBuf::from),
        model: value["model"].as_str().map(str::to_string),
        opencode_server: value["opencode_server"].as_str().map(str::to_string),
    }
}

fn write_agent_settings(data_dir: &Path, settings: &AgentSettings) {
    let mut value = serde_json::Map::new();
    if let Some(repo) = &settings.repo {
        value.insert("repo".into(), serde_json::json!(repo.display().to_string()));
    }
    if let Some(model) = &settings.model {
        value.insert("model".into(), serde_json::json!(model));
    }
    if let Some(server) = &settings.opencode_server {
        value.insert("opencode_server".into(), serde_json::json!(server));
    }
    let text = serde_json::to_string_pretty(&serde_json::Value::Object(value)).unwrap_or_default();
    if let Err(err) = std::fs::write(agent_config_path(data_dir), text) {
        eprintln!("redline: cannot write agent.json: {err}");
    }
}

fn pick_folder(app: &AppHandle) -> Option<PathBuf> {
    let picked = app.dialog().file().blocking_pick_folder()?;
    match picked {
        tauri_plugin_dialog::FilePath::Path(path) => Some(path),
        tauri_plugin_dialog::FilePath::Url(url) => url.to_file_path().ok(),
    }
}

fn start_agent(app: &AppHandle, ctx: &Ctx) {
    if ctx
        .agent
        .thread
        .lock()
        .map(|slot| slot.is_some())
        .unwrap_or(false)
    {
        app.dialog()
            .message("The local agent is already running.")
            .title("RedLine")
            .kind(MessageDialogKind::Info)
            .show(|_| {});
        return;
    }
    let mut settings = read_agent_settings(&ctx.data_dir);
    let Some(repo) = settings.repo.clone().or_else(|| pick_folder(app)) else {
        return;
    };
    settings.repo = Some(repo.clone());
    write_agent_settings(&ctx.data_dir, &settings);

    // The agent talks to whichever server this app is showing: the remote one if configured,
    // otherwise the embedded one.
    let (url, token) = match read_remote(&ctx.data_dir) {
        Some(remote) => (remote.base_url, remote.token),
        None => (ctx.local.base_url.clone(), ctx.local.token.clone()),
    };
    let mut config = redline_agent::AgentConfig::new(url, token, repo);
    config.agent_id = "local".to_string();
    if let Some(model) = settings
        .model
        .clone()
        .filter(|value| !value.trim().is_empty())
    {
        config.model = model;
    }
    if let Some(server) = settings
        .opencode_server
        .clone()
        .filter(|value| !value.trim().is_empty())
    {
        config.opencode_server = Some(server);
    }

    let cancel = Arc::new(AtomicBool::new(false));
    let signal = cancel.clone();
    let handle = std::thread::spawn(move || redline_agent::run(config, signal));
    if let Ok(mut slot) = ctx.agent.cancel.lock() {
        *slot = Some(cancel);
    }
    if let Ok(mut slot) = ctx.agent.thread.lock() {
        *slot = Some(handle);
    }
}

fn stop_agent(ctx: &Ctx) {
    if let Ok(mut slot) = ctx.agent.cancel.lock() {
        if let Some(cancel) = slot.take() {
            cancel.store(true, Ordering::Relaxed);
        }
    }
    // Detach: the loop stops at the next poll, a run in flight finishes on its own.
    if let Ok(mut slot) = ctx.agent.thread.lock() {
        *slot = None;
    }
}

fn choose_agent_repo(app: &AppHandle, ctx: &Ctx) {
    let Some(repo) = pick_folder(app) else {
        return;
    };
    let mut settings = read_agent_settings(&ctx.data_dir);
    settings.repo = Some(repo);
    write_agent_settings(&ctx.data_dir, &settings);
}

fn read_config(data_dir: &Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(data_dir.join("config.json")).ok()?;
    serde_json::from_str(&text).ok()
}

fn normalize_url(raw: &str) -> String {
    raw.trim().trim_end_matches('/').to_string()
}

fn default_server_name(url: &str) -> String {
    let host = url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url);
    if host.is_empty() {
        url.to_string()
    } else {
        host.to_string()
    }
}

/// Window labels and profile ids share one safe alphabet: Tauri rejects `/`, `:` and spaces.
fn safe_label(raw: &str) -> String {
    let mut out = String::new();
    let mut separated = false;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            separated = false;
        } else if !separated {
            out.push('-');
            separated = true;
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "server".to_string()
    } else {
        out
    }
}

fn profiles_from_value(value: &serde_json::Value) -> Vec<Profile> {
    let mut profiles = Vec::new();
    let mut used = HashSet::new();
    if let Some(items) = value.get("servers").and_then(|value| value.as_array()) {
        for item in items {
            let url = normalize_url(
                item.get("url")
                    .and_then(|value| value.as_str())
                    .unwrap_or(""),
            );
            if url.is_empty() {
                continue;
            }
            let name = item
                .get("name")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| default_server_name(&url));
            let base = item
                .get("id")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| name.clone());
            let base = if base.is_empty() {
                safe_label(&name)
            } else {
                base
            };
            let mut id = base.clone();
            let mut suffix = 2;
            while used.contains(&id) {
                id = format!("{base}-{suffix}");
                suffix += 1;
            }
            used.insert(id.clone());
            profiles.push(Profile {
                id,
                name,
                url,
                token: item
                    .get("token")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .to_string(),
            });
        }
    }
    if profiles.is_empty() {
        if let Some(url) = value
            .get("remote_url")
            .and_then(|value| value.as_str())
            .map(normalize_url)
            .filter(|url| !url.is_empty())
        {
            let name = default_server_name(&url);
            let id = safe_label(&name);
            used.insert(id.clone());
            profiles.push(Profile {
                id,
                name,
                url,
                token: value
                    .get("token")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .to_string(),
            });
        }
    }
    profiles
}

fn read_profiles(data_dir: &Path) -> Vec<Profile> {
    read_config(data_dir)
        .map(|value| profiles_from_value(&value))
        .unwrap_or_default()
}

/// The last focused server, used by the local agent and Copy link. `active: "local"` means no
/// remote profile; a missing active falls back to the first profile like the server does.
fn active_profile(data_dir: &Path) -> Option<Profile> {
    let value = read_config(data_dir)?;
    if value.get("active").and_then(|value| value.as_str()) == Some("local") {
        return None;
    }
    let profiles = profiles_from_value(&value);
    value
        .get("active")
        .and_then(|value| value.as_str())
        .and_then(|id| profiles.iter().find(|profile| profile.id == id).cloned())
        .or_else(|| profiles.first().cloned())
}

fn profile_for_url(data_dir: &Path, url: &str) -> Option<Profile> {
    read_profiles(data_dir)
        .into_iter()
        .filter(|profile| {
            url == profile.url
                || url.starts_with(&format!("{}/", profile.url))
                || url.starts_with(&format!("{}?", profile.url))
        })
        .max_by_key(|profile| profile.url.len())
}

fn read_remote(data_dir: &Path) -> Option<Local> {
    active_profile(data_dir).map(|profile| Local {
        base_url: profile.url,
        token: profile.token,
    })
}

fn is_manager_url(url: &str) -> bool {
    tauri::Url::parse(url)
        .map(|url| url.path().starts_with("/servers") || url.path().starts_with("/setup"))
        .unwrap_or(false)
}

fn window_title(profile: Option<&Profile>, manager: bool) -> String {
    if manager {
        "RedLine — Servers".to_string()
    } else if let Some(profile) = profile {
        format!("RedLine — {}", profile.name)
    } else {
        "RedLine — This computer".to_string()
    }
}

/// A local window is a fresh cookie jar. A navigation may carry no `?t=` (the Servers page links
/// `/inbox` and relies on its own cookie), so the new window must be signed in explicitly.
fn sign_local_url(target: &tauri::Url, token: &str) -> String {
    if target.query_pairs().any(|(key, _)| key == "t") {
        return target.to_string();
    }
    let mut signed = target.clone();
    signed.query_pairs_mut().append_pair("t", token);
    signed.to_string()
}

/// Which native window a *local* page belongs in. `/servers` is the launcher, `/inbox` is the
/// combined inbox, and every other local page is the "This computer" server. A local URL that
/// lands in the wrong window looks connected while showing the wrong list (2026-11-20).
fn local_window(path: &str) -> (&'static str, &'static str, &'static str) {
    if path.starts_with("/servers") || path.starts_with("/setup") {
        ("server-manager", "manager", "RedLine — Servers")
    } else if path.starts_with("/inbox") {
        ("inbox", "inbox", "RedLine — All servers")
    } else {
        ("local-server", "local", "RedLine — This computer")
    }
}

/// Start the embedded server on a free loopback port and return where it landed.
fn start_local_server(data_dir: PathBuf) -> Result<Local, Box<dyn std::error::Error>> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        runtime.block_on(async move {
            let auth =
                redline_core::paths::load_or_create_token(&data_dir, None, true).expect("token");
            let store = Store::new(redline_core::paths::sessions_dir(&data_dir));
            store.ensure().expect("sessions dir");

            // Prefer the documented port so `redline open`, MCP configs and bookmarks keep
            // working across restarts. Fall back to an ephemeral port only if it is taken.
            let preferred: u16 = std::env::var("REDLINE_PORT")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(PREFERRED_PORT);
            let mut bound = None;
            for candidate in [preferred, 0] {
                match tokio::net::TcpListener::bind(("127.0.0.1", candidate)).await {
                    Ok(listener) => {
                        bound = Some(listener);
                        break;
                    }
                    Err(err) if candidate == 0 => panic!("cannot bind a loopback port: {err}"),
                    Err(_) => eprintln!("redline: port {candidate} is busy, trying another"),
                }
            }
            let listener = bound.expect("listener");
            let address = listener.local_addr().expect("local addr");
            let base_url = format!("http://{address}");
            let state = AppState::new(store, auth.clone())
                .with_base_url(base_url.clone())
                .with_inline_ui(UI_FILES)
                .with_config_path(data_dir.join("config.json"));
            let _ = sender.send(Local {
                base_url,
                token: auth.token().to_string(),
            });

            axum::serve(listener, Router::from(router(state)))
                .await
                .expect("serve");
        });
    });

    receiver
        .recv_timeout(std::time::Duration::from_secs(10))
        .map_err(|err| format!("server did not start: {err}").into())
}

/// Copy-paste config for agents. Reads the current mode at click time, so switching servers in
/// the wizard does not require a restart.
fn mcp_config(data_dir: &Path, local: &Local) -> String {
    let (base, token) = match read_remote(data_dir) {
        Some(remote) => (remote.base_url, remote.token),
        None => (local.base_url.clone(), local.token.clone()),
    };
    let config = json!({
        "mcpServers": {
            "redline": {
                "type": "http",
                "url": format!("{base}/mcp"),
                "headers": { "Authorization": format!("Bearer {token}") }
            }
        }
    });
    serde_json::to_string_pretty(&config).unwrap_or_default()
}

// ---------------------------------------------------------------- window helpers

/// Exit for real. The flag is what distinguishes "the human asked" from "the last window hid".
fn quit(app: &AppHandle) {
    // The webview prefixes the window title with "• " while it holds unsaved drafts; no Tauri
    // globals are exposed to the page, so this is the whole channel. Every server window can
    // hold its own draft, so check all of them, not only the first one.
    let dirty = app.webview_windows().values().any(|window| {
        window
            .title()
            .ok()
            .is_some_and(|title| title.starts_with('•'))
    });
    if dirty {
        let handle = app.clone();
        app.dialog()
            .message("Unsaved changes will be lost. Quit anyway?")
            .title("RedLine")
            .buttons(MessageDialogButtons::OkCancelCustom(
                "Quit".to_string(),
                "Cancel".to_string(),
            ))
            .show(move |confirmed| {
                if confirmed {
                    EXITING.store(true, Ordering::SeqCst);
                    handle.exit(0);
                }
            });
        return;
    }
    EXITING.store(true, Ordering::SeqCst);
    app.exit(0);
}

fn restart(app: &AppHandle) {
    EXITING.store(true, Ordering::SeqCst);
    app.restart();
}

/// Show the active server in its own window. Each server keeps one window (and one cookie jar);
/// opening the same profile again focuses that window instead of creating a second one.
fn open_window(app: &AppHandle, ctx: &Ctx) {
    let active = active_profile(&ctx.data_dir);
    let (role, url, title, profile) = match active {
        Some(profile) => {
            let role = profile.id.clone();
            let url = format!("{}/?t={}", profile.url, profile.token);
            let title = window_title(Some(&profile), false);
            (role, url, title, Some(profile))
        }
        None => (
            "local".to_string(),
            format!("{}/?t={}", ctx.local.base_url, ctx.local.token),
            "RedLine — This computer".to_string(),
            None,
        ),
    };
    if let Some(label) = window_for_profile(&ctx.windows, &role) {
        if focus_window(app, &label) {
            return;
        }
    }
    let label = if profile.is_some() {
        format!("server-{}", safe_label(&role))
    } else {
        "local-server".to_string()
    };
    let _ = build_window(
        app,
        &ctx.data_dir,
        &ctx.local,
        &ctx.windows,
        &label,
        &url,
        profile.as_ref(),
        &title,
        true,
        &role,
    );
}

/// The server manager is a local window. It is not a server: opening profiles from it creates
/// server windows, and focusing it never changes which server the local agent targets.
fn open_manager_window(app: &AppHandle, ctx: &Ctx) {
    let url = format!("{}/servers?t={}", ctx.local.base_url, ctx.local.token);
    if let Some(main) = app.get_webview_window("main") {
        if let Ok(current) = main.url() {
            if origin(&current) == origin_of_base(&ctx.local.base_url)
                && (current.path().starts_with("/servers") || current.path().starts_with("/setup"))
            {
                let _ = main.show();
                let _ = main.unminimize();
                let _ = main.set_focus();
                if current.path().starts_with("/setup") {
                    navigate_window(&main, &url);
                }
                return;
            }
        }
    }
    if let Some(label) = window_for_profile(&ctx.windows, "manager") {
        if let Some(window) = app.get_webview_window(&label) {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
            if let Ok(current) = window.url() {
                if current.path().starts_with("/setup") {
                    navigate_window(&window, &url);
                }
            }
            return;
        }
    }
    let _ = build_window(
        app,
        &ctx.data_dir,
        &ctx.local,
        &ctx.windows,
        "server-manager",
        &url,
        None,
        "RedLine — Servers",
        false,
        "manager",
    );
}

/// The combined inbox is a local window over every profile. It is not a server: focusing it
/// never changes which server the local agent targets, and it can sit beside each server's own
/// window.
fn open_inbox_window(app: &AppHandle, ctx: &Ctx) {
    let url = format!("{}/inbox?t={}", ctx.local.base_url, ctx.local.token);
    if let Some(label) = window_for_profile(&ctx.windows, "inbox") {
        if let Some(window) = app.get_webview_window(&label) {
            // A window label is not a page: never focus an inbox window that was navigated
            // somewhere else and call it the inbox.
            if let Ok(current) = window.url() {
                if !current.path().starts_with("/inbox") {
                    navigate_window(&window, &url);
                }
            }
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
            return;
        }
    }
    let _ = build_window(
        app,
        &ctx.data_dir,
        &ctx.local,
        &ctx.windows,
        "inbox",
        &url,
        None,
        "RedLine — All servers",
        false,
        "inbox",
    );
}

#[allow(clippy::too_many_arguments)]
fn build_window(
    app: &AppHandle,
    data_dir: &Path,
    local: &Local,
    windows: &Arc<Mutex<HashMap<String, String>>>,
    label: &str,
    url: &str,
    profile: Option<&Profile>,
    title: &str,
    activate_on_focus: bool,
    role: &str,
) -> Result<tauri::WebviewWindow, Box<dyn std::error::Error>> {
    let parsed = url.parse::<tauri::Url>()?;
    let app_handle = app.clone();
    let nav_data = data_dir.to_path_buf();
    let nav_local = local.clone();
    let nav_windows = windows.clone();
    let nav_profile = profile.cloned();
    let window = WebviewWindowBuilder::new(app, label, WebviewUrl::External(parsed))
        .title(title)
        .inner_size(1280.0, 860.0)
        .min_inner_size(720.0, 480.0)
        .on_navigation(move |target| {
            navigation_allowed(
                &app_handle,
                &nav_data,
                &nav_local,
                &nav_windows,
                nav_profile.as_ref(),
                target,
            )
        })
        .build()?;
    if let Ok(mut map) = windows.lock() {
        map.insert(label.to_string(), role.to_string());
    }
    let handle = window.clone();
    let focus_local = local.clone();
    let focus_profile = profile.cloned();
    window.on_window_event(move |event| match event {
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            let _ = handle.hide();
        }
        // The local agent follows the last focused server. A manager window is not a server, so
        // it is deliberately excluded by `activate_on_focus`.
        WindowEvent::Focused(true) if activate_on_focus => {
            set_active(&focus_local, focus_profile.as_ref());
        }
        _ => {}
    });
    let _ = window.show();
    let _ = window.set_focus();
    Ok(window)
}

fn navigation_allowed(
    app: &AppHandle,
    data_dir: &Path,
    local: &Local,
    windows: &Arc<Mutex<HashMap<String, String>>>,
    source_profile: Option<&Profile>,
    target: &tauri::Url,
) -> bool {
    if !matches!(target.scheme(), "http" | "https") {
        return true;
    }
    let flagged = target.query_pairs().any(|(key, _)| key == "rl_window");
    let clean = strip_window_flag(target);
    let source_origin = source_profile
        .and_then(|profile| tauri::Url::parse(&profile.url).ok())
        .map(|url| origin(&url))
        .unwrap_or_else(|| origin_of_base(&local.base_url));
    if flagged || origin(&clean) != source_origin {
        if open_target_window(app, data_dir, local, windows, &clean) {
            return false;
        }
    }
    true
}

fn strip_window_flag(url: &tauri::Url) -> tauri::Url {
    let mut clean = url.clone();
    let pairs: Vec<(String, String)> = clean
        .query_pairs()
        .filter(|(key, _)| key != "rl_window")
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    clean.set_query(None);
    if !pairs.is_empty() {
        clean.query_pairs_mut().extend_pairs(pairs);
    }
    clean
}

fn open_target_window(
    app: &AppHandle,
    data_dir: &Path,
    local: &Local,
    windows: &Arc<Mutex<HashMap<String, String>>>,
    target: &tauri::Url,
) -> bool {
    let mut url = target.as_str().to_string();
    let profile = profile_for_url(data_dir, &url);
    let local_origin = origin_of_base(&local.base_url);
    let (label, role, title) = if let Some(profile) = &profile {
        (
            format!("server-{}", safe_label(&profile.id)),
            profile.id.clone(),
            format!("RedLine — {}", profile.name),
        )
    } else if origin(target) == local_origin {
        // A local page is not automatically "This computer": `/servers` is the manager and
        // `/inbox` is the combined inbox. Getting this wrong would focus the empty local
        // dashboard instead of the window the human asked for.
        let (label, role, title) = local_window(target.path());
        // A local window is a fresh webview with its own cookie jar, so a navigation that
        // carries no `?t=` (the Servers page links `/inbox` and relies on its own cookie) would
        // open an unauthenticated window. Sign the new window in explicitly.
        url = sign_local_url(target, &local.token);
        (label.to_string(), role.to_string(), title.to_string())
    } else {
        // A remote URL that is not a configured profile belongs in the system browser, not in a
        // mystery webview: add it in Servers… first if it should be a RedLine window.
        open_url(&url);
        return true;
    };
    if let Some(existing) = window_for_profile(windows, &role) {
        if focus_window(app, &existing) {
            return true;
        }
    }
    if app.get_webview_window(&label).is_some() {
        if let Ok(mut map) = windows.lock() {
            map.insert(label.clone(), role.clone());
        }
        return focus_window(app, &label);
    }
    build_window(
        app,
        data_dir,
        local,
        windows,
        &label,
        &url,
        profile.as_ref(),
        &title,
        true,
        &role,
    )
    .is_ok()
}

fn window_for_profile(windows: &Arc<Mutex<HashMap<String, String>>>, role: &str) -> Option<String> {
    windows
        .lock()
        .ok()?
        .iter()
        .find(|(_, value)| value.as_str() == role)
        .map(|(label, _)| label.clone())
}

fn focus_window(app: &AppHandle, label: &str) -> bool {
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        true
    } else {
        false
    }
}

/// The active profile is the local agent's target and the default for Copy link. Focus is the
/// only signal that changes it; switching a window's URL must not silently move the agent.
fn set_active(local: &Local, profile: Option<&Profile>) {
    let path = profile
        .map(|profile| format!("/api/servers/{}/activate", profile.id))
        .unwrap_or_else(|| "/api/servers/local/activate".to_string());
    let base = local.base_url.clone();
    let token = local.token.clone();
    std::thread::spawn(move || {
        match redline_core::client::Client::new(&format!("{}/", base.trim_end_matches('/')), token)
        {
            Ok(client) => {
                if let Err(error) = client.post(&path, &json!({})) {
                    eprintln!("redline: cannot remember the active server: {error}");
                }
            }
            Err(error) => eprintln!("redline: cannot reach the local server: {error}"),
        }
    });
}

fn origin_of_base(base: &str) -> String {
    tauri::Url::parse(base)
        .map(|url| origin(&url))
        .unwrap_or_default()
}

fn origin(url: &tauri::Url) -> String {
    let port = url
        .port_or_known_default()
        .map(|port| port.to_string())
        .unwrap_or_default();
    format!(
        "{}://{}:{}",
        url.scheme(),
        url.host_str().unwrap_or(""),
        port
    )
}

fn show_window(app: &AppHandle) {
    if let Some(window) = app
        .get_webview_window("main")
        .or_else(|| app.get_webview_window("server-manager"))
        .or_else(|| app.get_webview_window("inbox"))
        .or_else(|| app.webview_windows().into_values().next())
    {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn navigate_window(window: &tauri::WebviewWindow, url: &str) {
    let _ = window.show();
    let _ = window.set_focus();
    let escaped = serde_json::to_string(url).unwrap_or_else(|_| "\"/\"".to_string());
    let _ = window.eval(&format!("window.location.replace({escaped});"));
}

/// Stable releases come from the rolling `latest` manifest; dev builds from the rolling
/// `dev-build` prerelease (never `dev`: that collides with the branch of the same name). Both are
/// GitHub release assets, so no extra infrastructure.
const STABLE_MANIFEST: &str =
    "https://github.com/erikibolya/redline/releases/latest/download/latest.json";
const DEV_MANIFEST: &str =
    "https://github.com/erikibolya/redline/releases/download/dev-build/latest.json";

#[derive(Clone, Copy)]
enum UpdateChannel {
    Stable,
    Dev,
    Revert,
}

/// Check for updates. At launch nothing is in flight, so install silently; from the menu the
/// human asked, so report either way.
fn check_for_updates(app: AppHandle, notify: bool) {
    tauri::async_runtime::spawn(async move {
        use tauri_plugin_updater::UpdaterExt;
        let updater = match app.updater() {
            Ok(updater) => updater,
            Err(err) => {
                eprintln!("redline: updater unavailable: {err}");
                if notify {
                    app.dialog()
                        .message(format!("Updater unavailable: {err}"))
                        .title("RedLine")
                        .kind(MessageDialogKind::Error)
                        .show(|_| {});
                }
                return;
            }
        };
        offer_update(app, updater, UpdateChannel::Stable, notify).await;
    });
}

fn check_dev_update(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        use tauri_plugin_updater::UpdaterExt;
        let Some(url) = manifest_url(DEV_MANIFEST) else {
            return;
        };
        let updater = match app
            .updater_builder()
            .endpoints(vec![url])
            .and_then(|builder| builder.build())
        {
            Ok(updater) => updater,
            Err(err) => {
                report(&app, "Dev build unavailable", format!("{err}"));
                return;
            }
        };
        offer_update(app, updater, UpdateChannel::Dev, true).await;
    });
}

fn revert_to_stable(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        use tauri_plugin_updater::UpdaterExt;
        let Some(url) = manifest_url(STABLE_MANIFEST) else {
            return;
        };
        // The default comparator only accepts newer versions; a revert is exactly the opposite.
        let updater = match app
            .updater_builder()
            .endpoints(vec![url])
            .and_then(|builder| {
                builder
                    .version_comparator(|current, remote| remote.version != current)
                    .build()
            }) {
            Ok(updater) => updater,
            Err(err) => {
                report(&app, "Revert unavailable", format!("{err}"));
                return;
            }
        };
        offer_update(app, updater, UpdateChannel::Revert, true).await;
    });
}

fn manifest_url(url: &str) -> Option<tauri::Url> {
    match tauri::Url::parse(url) {
        Ok(url) => Some(url),
        Err(err) => {
            eprintln!("redline: bad manifest url {url}: {err}");
            None
        }
    }
}

fn report(app: &AppHandle, title: &str, message: String) {
    app.dialog()
        .message(message)
        .title(title)
        .kind(MessageDialogKind::Error)
        .show(|_| {});
}

async fn offer_update(
    app: AppHandle,
    updater: tauri_plugin_updater::Updater,
    channel: UpdateChannel,
    notify: bool,
) {
    let current = app.package_info().version.to_string();
    match updater.check().await {
        Ok(Some(update)) => {
            let version = update.version.clone();
            let message = match channel {
                UpdateChannel::Stable => format!(
                    "RedLine {version} is available.\n\nUpdate now? The app restarts when the \
                     installer finishes."
                ),
                UpdateChannel::Dev => format!(
                    "RedLine dev build {version} is available (you are on {current}).\n\nDev \
                     builds are unreleased snapshots; install it anyway?"
                ),
                UpdateChannel::Revert => format!(
                    "Revert to the last stable version {version}?\n\nYou are on {current}; the \
                     app will restart on {version}."
                ),
            };
            let title = match channel {
                UpdateChannel::Stable => "Update available",
                UpdateChannel::Dev => "Dev build available",
                UpdateChannel::Revert => "Revert to stable",
            };
            let confirm = match channel {
                UpdateChannel::Revert => "Revert",
                _ => "Install",
            };
            let interactive = app.clone();
            // Never restart the app behind the human's back: ask, with the version named.
            app.dialog()
                .message(message)
                .title(title)
                .buttons(MessageDialogButtons::OkCancelCustom(
                    confirm.to_string(),
                    "Later".to_string(),
                ))
                .show(move |confirmed| {
                    if !confirmed {
                        return;
                    }
                    let app = interactive.clone();
                    let update = update.clone();
                    tauri::async_runtime::spawn(async move {
                        match update.download_and_install(|_, _| {}, || {}).await {
                            // On Windows the installer takes over and exits this process.
                            Ok(()) => restart(&app),
                            Err(err) => {
                                eprintln!("redline: update failed: {err}");
                                app.dialog()
                                    .message(format!("Update to {version} failed: {err}"))
                                    .title("RedLine")
                                    .kind(MessageDialogKind::Error)
                                    .show(|_| {});
                            }
                        }
                    });
                });
        }
        Ok(None) => {
            if notify {
                let text = match channel {
                    UpdateChannel::Stable => format!("You are on the latest version ({current})."),
                    UpdateChannel::Dev => {
                        format!("No dev build newer than {current}. Nothing to install.")
                    }
                    UpdateChannel::Revert => {
                        format!("You are already on the latest stable version ({current}).")
                    }
                };
                app.dialog()
                    .message(text)
                    .title("RedLine")
                    .kind(MessageDialogKind::Info)
                    .show(|_| {});
            }
        }
        Err(err) => {
            eprintln!("redline: update check failed: {err}");
            if notify {
                app.dialog()
                    .message(format!("Could not check for updates: {err}"))
                    .title("RedLine")
                    .kind(MessageDialogKind::Error)
                    .show(|_| {});
            }
        }
    }
}

fn open_url(url: &str) {
    let (program, args): (&str, Vec<&str>) = if cfg!(target_os = "windows") {
        ("cmd", vec!["/C", "start", "", url])
    } else if cfg!(target_os = "macos") {
        ("open", vec![url])
    } else {
        ("xdg-open", vec![url])
    };
    let _ = std::process::Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// Clipboard through the OS API. Shelling out to `cmd /C clip` used to flash a console window
/// and then wait forever on stdin — the "freeze" when copying a token.
fn copy_to_clipboard(text: &str) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(text.to_string()))
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_labels_are_safe_and_stable() {
        assert_eq!(safe_label("Production"), "production");
        assert_eq!(safe_label("Produkce / Praha"), "produkce-praha");
        assert_eq!(safe_label(""), "server");
    }

    #[test]
    fn profiles_keep_ids_names_and_tokens() {
        let profiles = profiles_from_value(&serde_json::json!({
            "servers": [
                { "id": "production", "name": "Holly", "url": "https://redline.example/", "token": "p" },
                { "id": "dev", "name": "Dev", "url": "http://192.168.1.2:8788", "token": "d" }
            ],
            "active": "dev"
        }));
        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0].id, "production");
        assert_eq!(profiles[0].name, "Holly");
        assert_eq!(profiles[0].url, "https://redline.example");
        assert_eq!(profiles[0].token, "p");
        assert_eq!(profiles[1].id, "dev");
        assert_eq!(profiles[1].url, "http://192.168.1.2:8788");
    }

    #[test]
    fn a_legacy_config_becomes_one_profile() {
        let profiles = profiles_from_value(&serde_json::json!({
            "remote_url": "https://redline.example/",
            "token": "secret"
        }));
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].url, "https://redline.example");
        assert_eq!(profiles[0].token, "secret");
    }

    #[test]
    fn duplicate_names_get_distinct_window_roles() {
        let profiles = profiles_from_value(&serde_json::json!({
            "servers": [
                { "id": "prod", "name": "Production", "url": "https://a.example", "token": "a" },
                { "id": "prod", "name": "Production", "url": "https://b.example", "token": "b" }
            ]
        }));
        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0].id, "prod");
        assert_eq!(profiles[1].id, "prod-2");
    }

    #[test]
    fn the_window_flag_is_removed_from_the_server_url() {
        let url = tauri::Url::parse("https://redline.example/?t=secret&rl_window=1&x=2").unwrap();
        let clean = strip_window_flag(&url);
        assert_eq!(clean.as_str(), "https://redline.example/?t=secret&x=2");
    }

    #[test]
    fn manager_urls_are_recognised() {
        assert!(is_manager_url("http://127.0.0.1:8788/servers?t=x"));
        assert!(is_manager_url("http://127.0.0.1:8788/setup?new=1"));
        assert!(!is_manager_url("http://127.0.0.1:8788/"));
        // `is_manager_url` is a path check; launch_url only produces these paths for the local
        // manager, while a remote server is opened at `/`.
        assert!(is_manager_url("https://redline.example/servers"));
    }

    #[test]
    fn a_local_window_opened_without_a_token_is_signed_in() {
        let url = tauri::Url::parse("http://127.0.0.1:8788/inbox").unwrap();
        assert_eq!(
            sign_local_url(&url, "secret"),
            "http://127.0.0.1:8788/inbox?t=secret"
        );
        let already = tauri::Url::parse("http://127.0.0.1:8788/inbox?t=own").unwrap();
        assert_eq!(sign_local_url(&already, "secret"), already.as_str());
    }

    #[test]
    fn every_local_page_lands_in_its_own_window() {
        assert_eq!(local_window("/inbox").0, "inbox");
        assert_eq!(local_window("/inbox?t=x").1, "inbox");
        assert_eq!(local_window("/servers?t=x").0, "server-manager");
        assert_eq!(local_window("/setup?new=1").1, "manager");
        assert_eq!(local_window("/s/20261120-100000-review").0, "local-server");
        assert_eq!(local_window("/").2, "RedLine — This computer");
    }

    #[test]
    fn origins_ignore_a_trailing_slash_and_compare_default_ports() {
        let a = tauri::Url::parse("https://redline.example/").unwrap();
        let b = tauri::Url::parse("https://redline.example/s/1?t=x").unwrap();
        assert_eq!(origin(&a), origin(&b));
        assert_eq!(
            origin_of_base("http://127.0.0.1:8788/"),
            "http://127.0.0.1:8788"
        );
    }

    #[test]
    fn a_server_url_maps_back_to_its_profile() {
        let dir =
            std::env::temp_dir().join(format!("redline-app-profile-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.json"),
            r#"{"servers":[{"id":"prod","name":"Production","url":"https://redline.example","token":"t"}],"active":"prod"}"#,
        )
        .unwrap();
        assert_eq!(active_profile(&dir).unwrap().id, "prod");
        assert_eq!(
            profile_for_url(&dir, "https://redline.example/?t=t")
                .unwrap()
                .id,
            "prod"
        );
        assert!(profile_for_url(&dir, "https://other.example/?t=t").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
