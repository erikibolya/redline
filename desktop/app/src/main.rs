//! redline desktop shell: a tray icon, a hidden main window, and the same HTTP/MCP server the
//! CLI talks to.
//!
//! The shell is deliberately thin — every behaviour lives in `redline-core`, so this file only
//! starts the server, creates the window, and wires the tray menu.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

use axum::Router;
use redline_core::api::{router, AppState};
use redline_core::store::Store;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent};

/// The editor, compiled in. Self-contained HTML (inline CSS/JS), so no resource paths to resolve.
const UI_FILES: &[(&str, &str)] = &[
    ("index.html", include_str!("../../ui/index.html")),
    ("list.html", include_str!("../../ui/list.html")),
];

struct ServerHandle {
    base_url: String,
    token: String,
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_window(app);
        }))
        .setup(|app| {
            let data_dir = redline_core::paths::default_data_dir();
            let server = start_server(data_dir)?;
            let url = format!("{}/?t={}", server.base_url, server.token);

            let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url.parse()?))
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

            build_tray(app, server)?;
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

/// Start the embedded server on a free loopback port and return where it landed.
fn start_server(data_dir: PathBuf) -> Result<ServerHandle, Box<dyn std::error::Error>> {
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

            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind loopback");
            let address = listener.local_addr().expect("local addr");
            let base_url = format!("http://{address}");
            let state = AppState::new(store, auth.clone())
                .with_base_url(base_url.clone())
                .with_inline_ui(UI_FILES);
            let _ = sender.send(ServerHandle {
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

fn mcp_config(server: &ServerHandle) -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "mcpServers": {
            "redline": {
                "type": "http",
                "url": format!("{}/mcp", server.base_url),
                "headers": { "Authorization": format!("Bearer {}", server.token) }
            }
        }
    }))
    .unwrap_or_default()
}

fn build_tray(app: &tauri::App, server: ServerHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open redline", true, None::<&str>)?;
    let copy_config = MenuItem::with_id(app, "copy-mcp", "Copy MCP config", true, None::<&str>)?;
    let address = MenuItem::with_id(
        app,
        "address",
        &format!("Listening on {}", server.base_url),
        false,
        None::<&str>,
    )?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit redline", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &copy_config, &address, &separator, &quit])?;

    let config = mcp_config(&server);
    TrayIconBuilder::with_id("main")
        .tooltip("redline — redline the screenshot, the agent writes the fix")
        .icon(
            app.default_window_icon()
                .cloned()
                .ok_or(tauri::Error::UnknownPath)?,
        )
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "open" => show_window(app),
            "copy-mcp" => {
                if let Err(err) = copy_to_clipboard(&config) {
                    eprintln!("redline: cannot copy MCP config: {err}");
                }
            }
            "quit" => app.exit(0),
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

fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
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
