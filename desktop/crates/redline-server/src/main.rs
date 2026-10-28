//! `redline` — headless server.
//!
//! This is the process the desktop app runs internally, and the thing CI and WSL can exercise
//! without a window. The Tauri shell adds a tray icon and a webview; it does not add behaviour.

use std::net::SocketAddr;
use std::path::PathBuf;

use axum::Router;
use redline_core::api::{router, AppState};
use redline_core::auth::Auth;
use redline_core::store::Store;

const USAGE: &str = "\
redline — local visual review queue for humans and AI agents

USAGE
  redline serve [options]        run the HTTP + MCP server
  redline mcp-config [options]   print the MCP client config (JSON)
  redline token                  print the stored token
  redline --version

OPTIONS
  --host ADDR       bind address (default 127.0.0.1; anything else needs care)
  --port PORT       port (default 8788)
  --data-dir DIR    where sessions/token live (default: OS app-data dir)
  --ui DIR          directory with index.html/list.html (default: bundled ui/ if present)
  --token TOKEN     override the stored token (useful in tests)
";

#[derive(Debug, Default)]
struct Options {
    host: Option<String>,
    port: Option<u16>,
    data_dir: Option<PathBuf>,
    ui: Option<PathBuf>,
    token: Option<String>,
}

fn parse_options(args: &[String]) -> Result<Options, String> {
    let mut options = Options::default();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let mut value = |name: &str| {
            iter.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match arg.as_str() {
            "--host" => options.host = Some(value("--host")?),
            "--port" => {
                options.port = Some(
                    value("--port")?
                        .parse()
                        .map_err(|_| "--port must be a number".to_string())?,
                )
            }
            "--data-dir" => options.data_dir = Some(PathBuf::from(value("--data-dir")?)),
            "--ui" => options.ui = Some(PathBuf::from(value("--ui")?)),
            "--token" => options.token = Some(value("--token")?),
            other => return Err(format!("unknown option: {other}")),
        }
    }
    Ok(options)
}

/// OS-appropriate data directory. Kept here rather than in core so core stays I/O-policy free.
fn default_data_dir() -> PathBuf {
    if let Ok(explicit) = std::env::var("REDLINE_DATA_DIR") {
        return PathBuf::from(explicit);
    }
    if cfg!(target_os = "windows") {
        if let Ok(base) = std::env::var("LOCALAPPDATA") {
            return PathBuf::from(base).join("redline");
        }
    } else if cfg!(target_os = "macos") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join("Library/Application Support/redline");
        }
    } else if let Ok(base) = std::env::var("XDG_DATA_HOME") {
        return PathBuf::from(base).join("redline");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".local/share/redline");
    }
    PathBuf::from("./redline-data")
}

/// Token persisted next to the sessions so a restart does not break the agent's config.
fn load_or_create_token(data_dir: &PathBuf, override_token: Option<String>) -> std::io::Result<Auth> {
    if let Some(token) = override_token {
        return Ok(Auth::new(token));
    }
    let path = data_dir.join("token");
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let trimmed = existing.trim();
        if !trimmed.is_empty() {
            return Ok(Auth::new(trimmed));
        }
    }
    std::fs::create_dir_all(data_dir)?;
    let auth = Auth::generate();
    std::fs::write(&path, format!("{}\n", auth.token()))?;
    restrict_permissions(&path);
    Ok(auth)
}

#[cfg(unix)]
fn restrict_permissions(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &std::path::Path) {}

fn mcp_config(base_url: &str, token: &str) -> serde_json::Value {
    serde_json::json!({
        "mcpServers": {
            "redline": {
                "type": "http",
                "url": format!("{base_url}/mcp"),
                "headers": { "Authorization": format!("Bearer {token}") }
            }
        }
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{USAGE}");
        return Ok(());
    }
    if args[0] == "--version" {
        println!("redline {}", redline_core::VERSION);
        return Ok(());
    }

    let command = args[0].clone();
    let options = parse_options(&args[1..])?;
    let data_dir = options.data_dir.clone().unwrap_or_else(default_data_dir);
    let host = options.host.clone().unwrap_or_else(|| "127.0.0.1".to_string());
    let port = options.port.unwrap_or(8788);
    let base_url = format!("http://{host}:{port}");
    let auth = load_or_create_token(&data_dir, options.token.clone())?;

    match command.as_str() {
        "serve" => {
            let store = Store::new(data_dir.join("sessions"));
            store.ensure()?;

            let mut state = AppState::new(store, auth.clone()).with_base_url(base_url.clone());
            let ui = options.ui.clone().or_else(|| {
                let bundled = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../ui");
                bundled.is_dir().then_some(bundled)
            });
            if let Some(ui) = ui {
                state = state.with_ui(ui);
            }

            if host != "127.0.0.1" && host != "localhost" && host != "::1" {
                eprintln!("warning: binding {host} exposes reviews beyond this machine; the token is the only guard");
            }

            let address: SocketAddr = format!("{host}:{port}").parse()?;
            let listener = tokio::net::TcpListener::bind(address).await?;
            println!("redline {} listening on {base_url}", redline_core::VERSION);
            println!("sessions: {}", data_dir.join("sessions").display());
            println!("open:     {base_url}/?t={}", auth.token());
            println!("mcp:      {base_url}/mcp  (Authorization: Bearer <token>)");
            println!();
            println!("{}", serde_json::to_string_pretty(&mcp_config(&base_url, auth.token()))?);

            axum::serve(listener, Router::from(router(state)))
                .with_graceful_shutdown(shutdown_signal())
                .await?;
            Ok(())
        }
        "mcp-config" => {
            println!("{}", serde_json::to_string_pretty(&mcp_config(&base_url, auth.token()))?);
            Ok(())
        }
        "token" => {
            println!("{}", auth.token());
            Ok(())
        }
        other => Err(format!("unknown command: {other}\n\n{USAGE}").into()),
    }
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    eprintln!("\nstopping");
}
