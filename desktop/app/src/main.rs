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

use std::path::{Path, PathBuf};

use axum::Router;
use redline_core::api::{router, AppState};
use redline_core::store::Store;
use serde_json::json;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_autostart::ManagerExt as AutostartExt;
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

/// The port the CLI and agent configs assume (`redline` client default).
const PREFERRED_PORT: u16 = 8788;

/// The editor and the wizard, compiled in: no resource paths to resolve at runtime.
const UI_FILES: &[(&str, &str)] = &[
    ("index.html", include_str!("../../ui/index.html")),
    ("list.html", include_str!("../../ui/list.html")),
    ("setup.html", include_str!("../../ui/setup.html")),
];

#[derive(Clone)]
struct Local {
    base_url: String,
    token: String,
}

/// Everything the tray and menu handlers need; cloned into each closure.
#[derive(Clone)]
struct Ctx {
    data_dir: PathBuf,
    local: Local,
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
            let start_url = launch_url(&data_dir, &local);

            let window = WebviewWindowBuilder::new(
                app,
                "main",
                WebviewUrl::External(start_url.parse()?),
            )
            .title("redline")
            .inner_size(1280.0, 860.0)
            .min_inner_size(720.0, 480.0)
            .build()?;

            // Closing the window should not stop the reviewer: agents keep connecting while it
            // sits in the tray.
            let handle = window.clone();
            window.on_window_event(move |event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = handle.hide();
                }
            });

            let ctx = Ctx { data_dir, local };
            build_tray(app, &ctx)?;
            build_app_menu(app, &ctx)?;
            check_for_updates(app.handle().clone(), false);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build redline")
        .run(|_app, event| {
            // Keep the process (and therefore the MCP endpoint) alive when nothing is visible.
            if let RunEvent::ExitRequested { api, .. } = event {
                api.prevent_exit();
            }
        });
}

// ---------------------------------------------------------------- menus

/// The window menu bar. Same ids as the tray menu, same handler.
fn build_app_menu(app: &tauri::App, ctx: &Ctx) -> tauri::Result<()> {
    let sessions = MenuItem::with_id(app, "open", "Sessions", true, Some("CmdOrCtrl+1"))?;
    let connect = MenuItem::with_id(app, "connect", "Connect to a server…", true, None::<&str>)?;
    let copy_link = MenuItem::with_id(app, "copy-link", "Copy browser link", true, None::<&str>)?;
    let copy_mcp = MenuItem::with_id(app, "copy-mcp", "Copy MCP config", true, None::<&str>)?;
    let check = MenuItem::with_id(app, "check-updates", "Check for updates…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit redline", true, Some("CmdOrCtrl+Q"))?;
    let separator = PredefinedMenuItem::separator(app)?;
    let docs = MenuItem::with_id(app, "docs", "Documentation", true, None::<&str>)?;
    let about = MenuItem::with_id(app, "about", "About redline", true, None::<&str>)?;
    let help_separator = PredefinedMenuItem::separator(app)?;

    let file = Submenu::with_items(
        app,
        "redline",
        true,
        &[&sessions, &connect, &copy_link, &copy_mcp, &check, &separator, &quit],
    )?;
    let help = Submenu::with_items(app, "Help", true, &[&docs, &help_separator, &about])?;
    app.set_menu(Menu::with_items(app, &[&file, &help])?)?;

    let handler_ctx = ctx.clone();
    app.on_menu_event(move |app, event| {
        let id = event.id().as_ref().to_string();
        handle_action(&id, app, &handler_ctx);
    });
    Ok(())
}

fn build_tray(app: &tauri::App, ctx: &Ctx) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open redline", true, None::<&str>)?;
    let connect = MenuItem::with_id(app, "connect", "Connect to a server…", true, None::<&str>)?;
    let login = tauri::menu::CheckMenuItem::with_id(
        app,
        "autostart",
        "Start at login",
        true,
        app.autolaunch().is_enabled().unwrap_or(false),
        None::<&str>,
    )?;
    let copy_link = MenuItem::with_id(app, "copy-link", "Copy browser link", true, None::<&str>)?;
    let copy_config = MenuItem::with_id(app, "copy-mcp", "Copy MCP config", true, None::<&str>)?;
    let check = MenuItem::with_id(app, "check-updates", "Check for updates…", true, None::<&str>)?;
    let status = match read_remote(&ctx.data_dir) {
        Some(remote) => format!("Server: {}", remote.base_url),
        None => format!("Standalone: {}", ctx.local.base_url),
    };
    let address = MenuItem::with_id(app, "address", &status, false, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit redline", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &open, &connect, &login, &copy_link, &copy_config, &check, &address, &separator, &quit,
        ],
    )?;

    let handler_ctx = ctx.clone();
    TrayIconBuilder::with_id("main")
        .tooltip("redline — redline the screenshot, the agent writes the fix")
        .icon(
            app.default_window_icon()
                .cloned()
                .ok_or(tauri::Error::UnknownPath)?,
        )
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| {
            let id = event.id().as_ref().to_string();
            handle_action(&id, app, &handler_ctx);
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
fn handle_action(id: &str, app: &AppHandle, ctx: &Ctx) {
    match id {
        "open" => show_window(app),
        "connect" => {
            let url = format!("{}/setup?t={}", ctx.local.base_url, ctx.local.token);
            navigate(app, &url);
        }
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
        "check-updates" => check_for_updates(app.clone(), true),
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
                    "redline {}\n\nRedline the screenshot. The agent writes the fix.",
                    app.package_info().version
                ))
                .title("About redline")
                .kind(MessageDialogKind::Info)
                .show(|_| {});
        }
        "quit" => app.exit(0),
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
                .title("redline")
                .kind(MessageDialogKind::Error)
                .show(|_| {});
        }
    }
}

// ---------------------------------------------------------------- server URL

/// Standalone → the local UI; remote → the server's UI, or the wizard if it is not answering.
fn launch_url(data_dir: &Path, local: &Local) -> String {
    let setup = format!("{}/setup?t={}", local.base_url, local.token);
    match read_remote(data_dir) {
        Some(remote) => {
            if reachable(&remote.base_url, &remote.token) {
                format!("{}/?t={}", remote.base_url, remote.token)
            } else {
                format!("{setup}&unreachable=1")
            }
        }
        None => format!("{}/?t={}", local.base_url, local.token),
    }
}

fn reachable(base_url: &str, token: &str) -> bool {
    redline_core::client::Client::new(&format!("{}/", base_url.trim_end_matches('/')), token)
        .and_then(|client| client.get("/api/config"))
        .is_ok()
}

fn read_remote(data_dir: &Path) -> Option<Local> {
    let text = std::fs::read_to_string(data_dir.join("config.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let base_url = value
        .get("remote_url")?
        .as_str()?
        .trim()
        .trim_end_matches('/')
        .to_string();
    if base_url.is_empty() {
        return None;
    }
    let token = value
        .get("token")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    Some(Local { base_url, token })
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
            let auth = redline_core::paths::load_or_create_token(&data_dir, None, true)
                .expect("token");
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

fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn navigate(app: &AppHandle, url: &str) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
        let escaped = serde_json::to_string(url).unwrap_or_else(|_| "\"/\"".to_string());
        let _ = window.eval(&format!("window.location.replace({escaped});"));
    }
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
                        .title("redline")
                        .kind(MessageDialogKind::Error)
                        .show(|_| {});
                }
                return;
            }
        };

        match updater.check().await {
            Ok(Some(update)) => {
                let version = update.version.clone();
                match update.download_and_install(|_, _| {}, || {}).await {
                    Ok(()) => {
                        app.dialog()
                            .message(format!("redline {version} installed — restarting."))
                            .title("redline")
                            .kind(MessageDialogKind::Info)
                            .show(|_| {});
                        app.restart();
                    }
                    Err(err) => {
                        eprintln!("redline: update failed: {err}");
                        if notify {
                            app.dialog()
                                .message(format!("Update {version} failed: {err}"))
                                .title("redline")
                                .kind(MessageDialogKind::Error)
                                .show(|_| {});
                        }
                    }
                }
            }
            Ok(None) => {
                if notify {
                    app.dialog()
                        .message(format!(
                            "You are on the latest version ({}).",
                            app.package_info().version
                        ))
                        .title("redline")
                        .kind(MessageDialogKind::Info)
                        .show(|_| {});
                }
            }
            Err(err) => {
                eprintln!("redline: update check failed: {err}");
                if notify {
                    app.dialog()
                        .message(format!("Could not check for updates: {err}"))
                        .title("redline")
                        .kind(MessageDialogKind::Error)
                        .show(|_| {});
                }
            }
        }
    });
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

/// Clipboard without a plugin: one platform command per OS, no capabilities to configure.
fn copy_to_clipboard(text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut child = if cfg!(target_os = "windows") {
        std::process::Command::new("cmd")
            .args(["/C", "clip"])
            .stdin(std::process::Stdio::piped())
            .spawn()?
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()?
    } else {
        std::process::Command::new("xclip")
            .args(["-selection", "clipboard"])
            .stdin(std::process::Stdio::piped())
            .spawn()?
    };
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(text.as_bytes())?;
    }
    let _ = child.wait();
    Ok(())
}
