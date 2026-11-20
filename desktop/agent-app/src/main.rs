//! RedLine Agent — a small tray app around the headless `redline-agent` bridge.
//!
//! The app owns the bridge process (start, restart, stop), serves a local status/log UI on
//! loopback, and shows the same activity the server stores per device. It is deliberately
//! separate from the review client: a PC that only runs reviews does not need the editor.
//!
//! The UI is a plain local page (`http://127.0.0.1:<port>/`) so it is testable in a browser and
//! needs no Tauri IPC. The agent token is written to `agent-app.json` under the app config dir
//! and is never returned by the local API.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::path::BaseDirectory;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_autostart::ManagerExt as AutostartExt;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::UpdaterExt;

static EXITING: AtomicBool = AtomicBool::new(false);
const INDEX: &str = include_str!("../ui/index.html");
const UI_PORT: u16 = 8791;

#[derive(Clone, Serialize, Deserialize)]
struct Config {
    #[serde(default = "default_server")]
    server_url: String,
    #[serde(default)]
    token: String,
    #[serde(default = "default_agent")]
    agent_id: String,
    #[serde(default = "default_repo_root")]
    repo_root: String,
    #[serde(default)]
    repo: String,
    #[serde(default = "default_opencode")]
    opencode_server: String,
    #[serde(default = "default_model")]
    model: String,
}

fn default_server() -> String {
    "https://redline.ibolya.cz".to_string()
}
fn default_agent() -> String {
    "erik-pc".to_string()
}
fn default_repo_root() -> String {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    Path::new(&home)
        .join("source")
        .join("repos")
        .display()
        .to_string()
}
fn default_opencode() -> String {
    "http://127.0.0.1:4096".to_string()
}
fn default_model() -> String {
    "deepseek/deepseek-flash".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server_url: default_server(),
            token: String::new(),
            agent_id: default_agent(),
            repo_root: default_repo_root(),
            repo: String::new(),
            opencode_server: default_opencode(),
            model: default_model(),
        }
    }
}

#[derive(Clone, Serialize)]
struct LogLine {
    at: u64,
    line: String,
}

struct Ctx {
    app: AppHandle,
    config_path: PathBuf,
    config: Mutex<Config>,
    child_pid: Mutex<Option<u32>>,
    running: AtomicBool,
    stop: AtomicBool,
    log: Mutex<VecDeque<LogLine>>,
    activity: Mutex<Value>,
    update: Mutex<String>,
    autostart_item: Mutex<Option<CheckMenuItem<tauri::Wry>>>,
    /// No config file yet: a fresh install registers the tray app at login.
    first_run: bool,
}

impl Ctx {
    fn new(app: AppHandle) -> Self {
        let dir = app
            .path()
            .app_config_dir()
            .unwrap_or_else(|_| PathBuf::from("."));
        let config_path = dir.join("agent-app.json");
        let first_run = !config_path.exists();
        let config = std::fs::read_to_string(&config_path)
            .ok()
            .and_then(|text| serde_json::from_str::<Config>(&text).ok())
            .unwrap_or_default();
        Self {
            app,
            config_path,
            config: Mutex::new(config),
            child_pid: Mutex::new(None),
            running: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            log: Mutex::new(VecDeque::new()),
            activity: Mutex::new(Value::Null),
            update: Mutex::new(String::new()),
            autostart_item: Mutex::new(None),
            first_run,
        }
    }

    fn save_config(&self, config: &Config) -> Result<(), String> {
        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let text = serde_json::to_string_pretty(config).map_err(|error| error.to_string())?;
        std::fs::write(&self.config_path, format!("{text}\n")).map_err(|error| error.to_string())
    }

    fn push_log(&self, line: impl Into<String>) {
        let at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0);
        if let Ok(mut log) = self.log.lock() {
            log.push_back(LogLine {
                at,
                line: line.into(),
            });
            while log.len() > 400 {
                log.pop_front();
            }
        }
    }
}

// ---------------------------------------------------------------- agent process

fn agent_binary(app: &AppHandle) -> PathBuf {
    if let Ok(path) = app
        .path()
        .resolve("bin/redline-agent.exe", BaseDirectory::Resource)
    {
        if path.is_file() {
            return path;
        }
    }
    if let Ok(path) = app
        .path()
        .resolve("redline-agent.exe", BaseDirectory::Resource)
    {
        if path.is_file() {
            return path;
        }
    }
    let exe = std::env::current_exe().unwrap_or_default();
    let dir = exe.parent().unwrap_or_else(|| Path::new("."));
    for candidate in [
        dir.join("redline-agent.exe"),
        dir.join("bin").join("redline-agent.exe"),
    ] {
        if candidate.is_file() {
            return candidate;
        }
    }
    dir.join("redline-agent.exe")
}

fn read_lines(ctx: Arc<Ctx>, stream: impl std::io::Read + Send + 'static) {
    std::thread::spawn(move || {
        for line in BufReader::new(stream).lines() {
            match line {
                Ok(line) if !line.trim().is_empty() => ctx.push_log(line),
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });
}

fn start_agent(ctx: Arc<Ctx>) {
    if ctx.running.swap(true, Ordering::SeqCst) {
        return;
    }
    ctx.stop.store(false, Ordering::SeqCst);
    std::thread::spawn(move || {
        loop {
            if ctx.stop.load(Ordering::SeqCst) {
                break;
            }
            let config = ctx
                .config
                .lock()
                .map(|config| config.clone())
                .unwrap_or_default();
            if config.token.trim().is_empty() {
                ctx.push_log("No agent token yet — open Settings and paste one.");
                break;
            }
            let binary = agent_binary(&ctx.app);
            if !binary.is_file() {
                ctx.push_log(format!("Agent binary not found: {}", binary.display()));
                break;
            }
            let repo = if config.repo.trim().is_empty() {
                Path::new(&config.repo_root)
                    .join("redline")
                    .display()
                    .to_string()
            } else {
                config.repo.trim().to_string()
            };
            let mut command = Command::new(&binary);
            command
                .env("REDLINE_URL", config.server_url.trim_end_matches('/'))
                .env("REDLINE_TOKEN", &config.token)
                .env("REDLINE_AGENT_ID", &config.agent_id)
                .env("REDLINE_AGENT_REPO_ROOT", &config.repo_root)
                .env("REDLINE_AGENT_REPO", repo)
                .env("REDLINE_AGENT_OPENCODE_SERVER", &config.opencode_server)
                .env("REDLINE_AGENT_MODEL", &config.model)
                .env("GIT_CONFIG_COUNT", "1")
                .env("GIT_CONFIG_KEY_0", "safe.directory")
                .env("GIT_CONFIG_VALUE_0", "*")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
            }
            match command.spawn() {
                Ok(mut child) => {
                    let pid = child.id();
                    if let Ok(mut slot) = ctx.child_pid.lock() {
                        *slot = Some(pid);
                    }
                    ctx.push_log(format!("Agent started (pid {pid})"));
                    if let Some(stdout) = child.stdout.take() {
                        read_lines(ctx.clone(), stdout);
                    }
                    if let Some(stderr) = child.stderr.take() {
                        read_lines(ctx.clone(), stderr);
                    }
                    let status = child.wait();
                    if let Ok(mut slot) = ctx.child_pid.lock() {
                        *slot = None;
                    }
                    ctx.push_log(match status {
                        Ok(status) => format!("Agent exited ({status})"),
                        Err(error) => format!("Agent wait failed: {error}"),
                    });
                }
                Err(error) => {
                    ctx.push_log(format!("Could not start {}: {error}", binary.display()));
                    ctx.stop.store(true, Ordering::SeqCst);
                }
            }
            if ctx.stop.load(Ordering::SeqCst) {
                break;
            }
            ctx.push_log("Restarting in 5 seconds…");
            std::thread::sleep(Duration::from_secs(5));
        }
        ctx.running.store(false, Ordering::SeqCst);
    });
}

fn stop_agent(ctx: &Ctx) {
    ctx.stop.store(true, Ordering::SeqCst);
    let pid = ctx.child_pid.lock().ok().and_then(|mut slot| slot.take());
    if let Some(pid) = pid {
        #[cfg(windows)]
        {
            let _ = Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        #[cfg(not(windows))]
        {
            let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
        }
        ctx.push_log(format!("Stopped agent (pid {pid})"));
    }
}

// ---------------------------------------------------------------- RedLine activity

fn poll_activity(ctx: Arc<Ctx>) {
    std::thread::spawn(move || loop {
        let config = ctx
            .config
            .lock()
            .map(|config| config.clone())
            .unwrap_or_default();
        if !config.token.trim().is_empty() && !config.server_url.trim().is_empty() {
            let url = format!(
                "{}/api/agent/activity",
                config.server_url.trim_end_matches('/')
            );
            let agent = ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(2))
                .timeout(Duration::from_secs(5))
                .build();
            let value = match agent
                .get(&url)
                .set("Authorization", &format!("Bearer {}", config.token))
                .call()
            {
                Ok(response) => response
                    .into_string()
                    .ok()
                    .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                    .unwrap_or_else(|| json!({ "error": "the server did not answer JSON" })),
                Err(ureq::Error::Status(status, _)) => json!({ "error": format!("HTTP {status}") }),
                Err(error) => json!({ "error": error.to_string() }),
            };
            if let Ok(mut activity) = ctx.activity.lock() {
                *activity = value;
            }
            update_tray(&ctx);
        }
        std::thread::sleep(Duration::from_secs(3));
    });
}

fn activity_summary(activity: &Value) -> String {
    if let Some(error) = activity.get("error").and_then(Value::as_str) {
        return format!("RedLine Agent — offline ({error})");
    }
    let Some(agent) = activity
        .get("agents")
        .and_then(Value::as_array)
        .and_then(|agents| agents.first())
    else {
        return "RedLine Agent — waiting".to_string();
    };
    if let Some(claim) = agent
        .get("claims")
        .and_then(Value::as_array)
        .and_then(|claims| claims.first())
    {
        let title = claim
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("a review");
        let step = claim
            .get("step")
            .and_then(|step| step.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("working");
        return format!("RedLine Agent — {title}: {step}");
    }
    if agent.get("online").and_then(Value::as_bool) == Some(true) {
        "RedLine Agent — waiting for work".to_string()
    } else {
        "RedLine Agent — offline".to_string()
    }
}

fn update_tray(ctx: &Ctx) {
    let Some(tray) = ctx.app.tray_by_id("main") else {
        return;
    };
    let activity = ctx
        .activity
        .lock()
        .map(|activity| activity.clone())
        .unwrap_or(Value::Null);
    let text = if ctx.running.load(Ordering::SeqCst) {
        activity_summary(&activity)
    } else {
        "RedLine Agent — stopped".to_string()
    };
    let _ = tray.set_tooltip(Some(&text));
}

// ---------------------------------------------------------------- local UI

#[derive(Deserialize)]
struct ConfigBody {
    server_url: String,
    #[serde(default)]
    token: Option<String>,
    agent_id: String,
    repo_root: String,
    #[serde(default)]
    repo: String,
    opencode_server: String,
    model: String,
}

async fn index() -> Html<&'static str> {
    Html(INDEX)
}

async fn status(State(ctx): State<Arc<Ctx>>) -> Json<Value> {
    let config = ctx
        .config
        .lock()
        .map(|config| config.clone())
        .unwrap_or_default();
    let log: Vec<LogLine> = ctx
        .log
        .lock()
        .map(|log| log.iter().rev().take(200).rev().cloned().collect())
        .unwrap_or_default();
    let activity = ctx
        .activity
        .lock()
        .map(|activity| activity.clone())
        .unwrap_or(Value::Null);
    let update = ctx
        .update
        .lock()
        .map(|value| value.clone())
        .unwrap_or_default();
    Json(json!({
        "config": {
            "server_url": config.server_url,
            "agent_id": config.agent_id,
            "repo_root": config.repo_root,
            "repo": config.repo,
            "opencode_server": config.opencode_server,
            "model": config.model,
            "token_set": !config.token.is_empty(),
        },
        "running": ctx.running.load(Ordering::SeqCst),
        "pid": ctx.child_pid.lock().ok().and_then(|pid| *pid),
        "activity": activity,
        "log": log,
        "update": update,
    }))
}

async fn save_config(State(ctx): State<Arc<Ctx>>, Json(body): Json<ConfigBody>) -> Response {
    let current = ctx
        .config
        .lock()
        .map(|config| config.clone())
        .unwrap_or_default();
    let token = match body.token {
        Some(token) if !token.trim().is_empty() => token.trim().to_string(),
        _ => current.token,
    };
    let config = Config {
        server_url: body.server_url.trim().trim_end_matches('/').to_string(),
        token,
        agent_id: body.agent_id.trim().to_string(),
        repo_root: body.repo_root.trim().to_string(),
        repo: body.repo.trim().to_string(),
        opencode_server: body.opencode_server.trim().to_string(),
        model: body.model.trim().to_string(),
    };
    if config.server_url.is_empty() || config.agent_id.is_empty() || config.repo_root.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "server URL, agent id and repository root are required" })),
        )
            .into_response();
    }
    if let Err(error) = ctx.save_config(&config) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error })),
        )
            .into_response();
    }
    if let Ok(mut slot) = ctx.config.lock() {
        *slot = config;
    }
    stop_agent(&ctx);
    ctx.push_log("Configuration saved.");
    start_agent(ctx.clone());
    Json(json!({ "ok": true })).into_response()
}

async fn start(State(ctx): State<Arc<Ctx>>) -> Json<Value> {
    start_agent(ctx.clone());
    Json(json!({ "ok": true }))
}

async fn stop(State(ctx): State<Arc<Ctx>>) -> Json<Value> {
    stop_agent(&ctx);
    Json(json!({ "ok": true }))
}

async fn update(State(ctx): State<Arc<Ctx>>) -> Json<Value> {
    check_updates(ctx, true);
    Json(json!({ "ok": true }))
}

async fn open_redline(State(ctx): State<Arc<Ctx>>) -> Json<Value> {
    let config = ctx
        .config
        .lock()
        .map(|config| config.clone())
        .unwrap_or_default();
    open_url(&config.server_url);
    Json(json!({ "ok": true }))
}

fn start_ui(ctx: Arc<Ctx>) -> std::io::Result<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", UI_PORT))
        .or_else(|_| std::net::TcpListener::bind(("127.0.0.1", 0)))
        .map_err(|error| {
            std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                format!("cannot bind the local UI: {error}"),
            )
        })?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                eprintln!("redline-agent-app: runtime failed: {error}");
                return;
            }
        };
        runtime.block_on(async move {
            let listener = match tokio::net::TcpListener::from_std(listener) {
                Ok(listener) => listener,
                Err(error) => {
                    eprintln!("redline-agent-app: listener failed: {error}");
                    return;
                }
            };
            let app = Router::new()
                .route("/", get(index))
                .route("/api/status", get(status))
                .route("/api/config", post(save_config))
                .route("/api/agent/start", post(start))
                .route("/api/agent/stop", post(stop))
                .route("/api/update", post(update))
                .route("/api/open", post(open_redline))
                .with_state(ctx);
            if let Err(error) = axum::serve(listener, app).await {
                eprintln!("redline-agent-app: UI server failed: {error}");
            }
        });
    });
    Ok(port)
}

// ---------------------------------------------------------------- window, tray, updates

fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn build_window(
    app: &tauri::App,
    ctx: &Arc<Ctx>,
    port: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    let url = format!("http://127.0.0.1:{port}/");
    let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url.parse()?))
        .title("RedLine Agent")
        .inner_size(780.0, 640.0)
        .min_inner_size(560.0, 440.0)
        .build()?;
    let handle = window.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = handle.hide();
        }
    });
    let configured = ctx
        .config
        .lock()
        .map(|config| !config.token.is_empty())
        .unwrap_or(false);
    if configured {
        let _ = window.hide();
    } else {
        let _ = window.show();
        let _ = window.set_focus();
    }
    Ok(())
}

fn build_tray(app: &tauri::App, ctx: Arc<Ctx>) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open RedLine Agent", true, None::<&str>)?;
    let browse = MenuItem::with_id(app, "browse", "Open RedLine", true, None::<&str>)?;
    let start = MenuItem::with_id(app, "start", "Start agent", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop", "Stop agent", true, None::<&str>)?;
    let updates = MenuItem::with_id(
        app,
        "check-updates",
        "Check for updates…",
        true,
        None::<&str>,
    )?;
    let autostart = CheckMenuItem::with_id(
        app,
        "autostart",
        "Start at login",
        true,
        app.autolaunch().is_enabled().unwrap_or(false),
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "Quit RedLine Agent", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let separator2 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &open,
            &browse,
            &separator,
            &start,
            &stop,
            &separator2,
            &autostart,
            &updates,
            &quit,
        ],
    )?;
    if let Ok(mut slot) = ctx.autostart_item.lock() {
        *slot = Some(autostart);
    }
    let handler_ctx = ctx.clone();
    TrayIconBuilder::with_id("main")
        .tooltip("RedLine Agent")
        .icon(
            app.default_window_icon()
                .cloned()
                .ok_or(tauri::Error::UnknownPath)?,
        )
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "open" => show_window(app),
            "browse" => {
                let config = handler_ctx
                    .config
                    .lock()
                    .map(|config| config.clone())
                    .unwrap_or_default();
                open_url(&config.server_url);
            }
            "start" => start_agent(handler_ctx.clone()),
            "stop" => stop_agent(&handler_ctx),
            "check-updates" => check_updates(handler_ctx.clone(), true),
            "autostart" => toggle_autostart(app, &handler_ctx),
            "quit" => {
                EXITING.store(true, Ordering::SeqCst);
                stop_agent(&handler_ctx);
                app.exit(0);
            }
            _ => {}
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

fn toggle_autostart(app: &AppHandle, ctx: &Ctx) {
    let enabled = !app.autolaunch().is_enabled().unwrap_or(false);
    let result = if enabled {
        app.autolaunch().enable()
    } else {
        app.autolaunch().disable()
    };
    match result {
        Ok(()) => {
            if let Ok(item) = ctx.autostart_item.lock() {
                if let Some(item) = item.as_ref() {
                    let _ = item.set_checked(enabled);
                }
            }
            ctx.push_log(format!(
                "Start at login {}",
                if enabled { "enabled" } else { "disabled" }
            ));
        }
        Err(error) => ctx.push_log(format!("Could not change autostart: {error}")),
    }
}

fn check_updates(ctx: Arc<Ctx>, interactive: bool) {
    let app = ctx.app.clone();
    tauri::async_runtime::spawn(async move {
        let updater = match app.updater() {
            Ok(updater) => updater,
            Err(error) => {
                set_update(&ctx, format!("Updater unavailable: {error}"));
                return;
            }
        };
        let current = app.package_info().version.to_string();
        match updater.check().await {
            Ok(Some(update)) => {
                let version = update.version.clone();
                set_update(&ctx, format!("Update {version} available"));
                if !interactive {
                    return;
                }
                let install_ctx = ctx.clone();
                let install_app = app.clone();
                app.dialog()
                    .message(format!(
                        "RedLine Agent {version} is available.\n\nInstall it now? The app restarts \
                         when the installer finishes."
                    ))
                    .title("Update available")
                    .buttons(MessageDialogButtons::OkCancelCustom(
                        "Install".to_string(),
                        "Later".to_string(),
                    ))
                    .show(move |confirmed| {
                        if !confirmed {
                            return;
                        }
                        let ctx = install_ctx.clone();
                        let app = install_app.clone();
                        tauri::async_runtime::spawn(async move {
                            match update.download_and_install(|_, _| {}, || {}).await {
                                Ok(()) => {
                                    EXITING.store(true, Ordering::SeqCst);
                                    stop_agent(&ctx);
                                    set_update(&ctx, format!("Installed {version}; restarting…"));
                                    app.restart();
                                }
                                Err(error) => {
                                    set_update(&ctx, format!("Update failed: {error}"));
                                    app.dialog()
                                        .message(format!("Update to {version} failed: {error}"))
                                        .title("RedLine Agent")
                                        .kind(MessageDialogKind::Error)
                                        .show(|_| {});
                                }
                            }
                        });
                    });
            }
            Ok(None) => set_update(&ctx, format!("Up to date ({current})")),
            Err(error) => set_update(&ctx, format!("Update check failed: {error}")),
        }
    });
}

fn set_update(ctx: &Ctx, text: String) {
    if let Ok(mut slot) = ctx.update.lock() {
        *slot = text;
    }
}

fn open_url(url: &str) {
    if url.trim().is_empty() {
        return;
    }
    #[cfg(target_os = "windows")]
    let (program, args) = ("cmd", vec!["/C", "start", "", url]);
    #[cfg(target_os = "macos")]
    let (program, args) = ("open", vec![url]);
    #[cfg(all(unix, not(target_os = "macos")))]
    let (program, args) = ("xdg-open", vec![url]);
    let _ = Command::new(program)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
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
            let ctx = Arc::new(Ctx::new(app.handle().clone()));
            let port = start_ui(ctx.clone())?;
            build_tray(app, ctx.clone())?;
            // A device agent should be there after a reboot; the menu still lets the human turn
            // it off, and a later launch does not re-enable a deliberate choice.
            if ctx.first_run {
                if let Ok(slot) = ctx.autostart_item.lock() {
                    if let Some(item) = slot.as_ref() {
                        let _ = app.autolaunch().enable();
                        let _ = item.set_checked(true);
                    }
                }
            }
            build_window(app, &ctx, port)?;
            if ctx
                .config
                .lock()
                .map(|config| !config.token.is_empty())
                .unwrap_or(false)
            {
                start_agent(ctx.clone());
            }
            poll_activity(ctx.clone());
            check_updates(ctx.clone(), false);
            ctx.push_log("RedLine Agent started.");
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building RedLine Agent")
        .run(|_app, event| {
            if let RunEvent::ExitRequested { api, .. } = event {
                if !EXITING.load(Ordering::SeqCst) {
                    api.prevent_exit();
                }
            }
        });
}
