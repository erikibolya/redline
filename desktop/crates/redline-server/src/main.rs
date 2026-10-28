//! `redline` — headless server plus the CLI the agent workflow uses.
//!
//! The desktop app runs this same code in-process; the CLI verbs talk HTTP to a running instance
//! so that "the app is open in the tray" and "an agent creates a review" use one code path.

use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use axum::Router;
use redline::http::{Client, ClientError};
use redline_core::api::{router, AppState};
use redline_core::auth::Auth;
use redline_core::paths::{default_data_dir, load_or_create_token, sessions_dir};
use redline_core::store::Store;

/// Editor assets compiled in: the headless server must run from a bare container with no
/// checkout next to it.
const UI_FILES: &[(&str, &str)] = &[
    ("index.html", include_str!("../../../ui/index.html")),
    ("list.html", include_str!("../../../ui/list.html")),
    ("setup.html", include_str!("../../../ui/setup.html")),
];

const USAGE: &str = "\
redline — local visual review queue for humans and AI agents

USAGE
  redline serve [options]                  run the HTTP + MCP server (foreground)
  redline open IMAGE [options]             create a review session from a screenshot
  redline list                             list sessions
  redline get ID                           print a session and its annotations
  redline wait ID [--timeout 600]          block until the human submits
  redline remove ID [--yes]                delete a session
  redline health                           exit 0 if the server answers /health
  redline token                            print the API token
  redline mcp-config                       print the MCP client config (JSON)

SERVER OPTIONS
  --host ADDR       bind address (default 127.0.0.1)
  --port PORT       port (default 8788)
  --data-dir DIR    where sessions and the token live
  --sessions DIR    override the sessions directory only
  --ui DIR          directory with index.html/list.html (default: built in)
  --public-url URL  URL humans open, when bound to 0.0.0.0 in a container
  --token TOKEN     override the stored token (env: REDLINE_TOKEN)

CLIENT OPTIONS
  --host ADDR       server address (default 127.0.0.1)
  --port PORT       server port (default 8788)
  --data-dir DIR    where the token is stored (must match the server)
  --token TOKEN     pass the token explicitly

OPEN OPTIONS
  --title T  --url URL  --note TEXT  --id ID  --json  --open-browser
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match run(args) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{message}");
            1
        }
    };
    std::process::exit(code);
}

/// Flags that consume the next argument. Everything else is a boolean switch.
const VALUE_FLAGS: &[&str] = &[
    "host",
    "port",
    "data-dir",
    "sessions",
    "ui",
    "public-url",
    "token",
    "title",
    "url",
    "note",
    "id",
    "timeout",
    "interval",
];

/// Boolean switches. Anything beginning with `--` that is neither here nor in VALUE_FLAGS is an
/// error: a typo used to be swallowed silently (which is how `--public-url` stopped working).
const SWITCHES: &[&str] = &["json", "open-browser", "yes", "help"];

#[derive(Default, Debug)]
struct Flags {
    values: Vec<(String, String)>,
    switches: Vec<String>,
    positional: Vec<String>,
}

impl Flags {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut flags = Flags::default();
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            if let Some(name) = arg.strip_prefix("--") {
                if VALUE_FLAGS.contains(&name) {
                    let value = iter
                        .next()
                        .ok_or_else(|| format!("--{name} needs a value"))?;
                    flags.values.push((name.to_string(), value.clone()));
                } else if SWITCHES.contains(&name) {
                    flags.switches.push(name.to_string());
                } else {
                    return Err(format!("unknown option: --{name}"));
                }
            } else {
                flags.positional.push(arg.clone());
            }
        }
        Ok(flags)
    }

    fn value(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .rev()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    fn switch(&self, name: &str) -> bool {
        self.switches.iter().any(|switch| switch == name)
    }

    fn number(&self, name: &str) -> Result<Option<u64>, String> {
        match self.value(name) {
            Some(value) => value
                .parse()
                .map(Some)
                .map_err(|_| format!("--{name} must be a number")),
            None => Ok(None),
        }
    }
}

fn run(args: Vec<String>) -> Result<i32, String> {
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" || args[0] == "help" {
        print!("{USAGE}");
        return Ok(0);
    }
    if args[0] == "--version" || args[0] == "version" {
        println!("redline {}", redline_core::VERSION);
        return Ok(0);
    }

    let command = args[0].clone();
    let flags = Flags::parse(&args[1..])?;
    let data_dir = flags
        .value("data-dir")
        .map(PathBuf::from)
        .unwrap_or_else(default_data_dir);
    let host = flags.value("host").unwrap_or("127.0.0.1").to_string();
    let port = flags.number("port")?.unwrap_or(8788) as u16;
    let base_url = format!("http://{host}:{port}");
    // The server persists an explicit token so the CLI (which reads the same file) still
    // authenticates; client commands must not rewrite it.
    let auth = load_or_create_token(
        &data_dir,
        flags.value("token").map(str::to_string).or_else(|| {
            std::env::var("REDLINE_TOKEN")
                .ok()
                .filter(|v| !v.trim().is_empty())
        }),
        command == "serve",
    )
    .map_err(|err| format!("cannot read or create the token: {err}"))?;

    match command.as_str() {
        "serve" => serve(&flags, &data_dir, auth),
        "open" => open(&flags, &base_url, &auth),
        "list" => list(&flags, &base_url, &auth),
        "get" => get(&flags, &base_url, &auth),
        "wait" => wait(&flags, &base_url, &auth),
        "remove" => remove(&flags, &base_url, &auth),
        "health" => {
            let client = Client::new(&format!("http://127.0.0.1:{port}/"), "")
                .map_err(|err| err.to_string())?;
            match client.get("/health") {
                Ok(_) => Ok(0),
                Err(err) => {
                    eprintln!("unhealthy: {err}");
                    Ok(1)
                }
            }
        }
        "token" => {
            println!("{}", auth.token());
            Ok(0)
        }
        "mcp-config" => {
            println!(
                "{}",
                serde_json::to_string_pretty(&mcp_config(&base_url, auth.token()))
                    .map_err(|err| err.to_string())?
            );
            Ok(0)
        }
        other => Err(format!("unknown command: {other}\n\n{USAGE}")),
    }
}

// ---------------------------------------------------------------- server

fn serve(flags: &Flags, data_dir: &Path, auth: Auth) -> Result<i32, String> {
    let host = flags.value("host").unwrap_or("127.0.0.1").to_string();
    let port = flags.number("port")?.unwrap_or(8788) as u16;
    let sessions_root = flags
        .value("sessions")
        .map(PathBuf::from)
        .unwrap_or_else(|| sessions_dir(data_dir));

    let runtime = tokio::runtime::Runtime::new().map_err(|err| err.to_string())?;
    runtime.block_on(async move {
        let store = Store::new(&sessions_root);
        store.ensure().map_err(|err| err.to_string())?;

        // `--public-url` is what humans click; agents always talk to the server directly, so the
        // default stays loopback even when the socket is bound to 0.0.0.0 in a container.
        let advertised = flags
            .value("public-url")
            .map(str::to_string)
            .unwrap_or_else(|| format!("http://127.0.0.1:{port}"));
        let mut state = AppState::new(store, auth.clone()).with_base_url(advertised);
        match flags.value("ui").map(PathBuf::from) {
            Some(ui) => state = state.with_ui(ui),
            None => state = state.with_inline_ui(UI_FILES),
        }

        if !matches!(host.as_str(), "127.0.0.1" | "localhost" | "::1") {
            eprintln!(
                "warning: binding {host} exposes reviews beyond this machine; the token is the only guard"
            );
        }

        let address: SocketAddr = format!("{host}:{port}")
            .parse()
            .map_err(|_| format!("invalid address: {host}:{port}"))?;
        let listener = tokio::net::TcpListener::bind(address)
            .await
            .map_err(|err| format!("cannot bind {address}: {err} (is redline already running?)"))?;
        let bound = listener.local_addr().map_err(|err| err.to_string())?;

        println!("redline {} listening on http://{bound}", redline_core::VERSION);
        println!("sessions: {}", sessions_root.display());
        println!("open (human):   http://{bound}/?t={}", auth.token());
        println!("agents (local): http://127.0.0.1:{port}/mcp");
        println!("token:          {}", auth.token());
        println!();
        println!(
            "{}",
            serde_json::to_string_pretty(&mcp_config(&format!("http://127.0.0.1:{port}"), auth.token()))
                .map_err(|err| err.to_string())?
        );

        axum::serve(listener, Router::from(router(state)))
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
                eprintln!("\nstopping");
            })
            .await
            .map_err(|err| err.to_string())?;
        Ok(0)
    })
}

// ---------------------------------------------------------------- client verbs

fn client(base_url: &str, auth: &Auth) -> Result<Client, String> {
    Client::new(base_url, auth.token()).map_err(|err| err.to_string())
}

fn open(flags: &Flags, base_url: &str, auth: &Auth) -> Result<i32, String> {
    let image = flags
        .positional
        .first()
        .ok_or("usage: redline open IMAGE [--title T] [--note TEXT]")?;
    let path = PathBuf::from(image);
    if !path.is_file() {
        return Err(format!("no such file: {}", path.display()));
    }
    let absolute = std::fs::canonicalize(&path).map_err(|err| err.to_string())?;
    let title = flags.value("title").map(str::to_string).unwrap_or_else(|| {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("review")
            .to_string()
    });

    let mut body = serde_json::json!({
        "title": title,
        "url": flags.value("url").unwrap_or_default(),
        "note": flags.value("note").unwrap_or_default(),
        "agent": "cli",
        "image_path": absolute.display().to_string(),
    });
    if let Some(id) = flags.value("id") {
        body["id"] = serde_json::Value::String(id.to_string());
    }

    let client = client(base_url, auth)?;
    let created = match client.post("/api/sessions", &body) {
        Ok((_, value)) => value,
        Err(ClientError::Http(_, _)) => {
            // The server may live on another machine (or another filesystem namespace):
            // fall back to uploading the bytes.
            use base64::Engine as _;
            let bytes = std::fs::read(&absolute).map_err(|err| err.to_string())?;
            body["image_path"] = serde_json::Value::Null;
            body["image_base64"] =
                serde_json::Value::String(base64::engine::general_purpose::STANDARD.encode(bytes));
            client
                .post("/api/sessions", &body)
                .map(|(_, value)| value)
                .map_err(|err| err.to_string())?
        }
        Err(err) => return Err(err.to_string()),
    };

    let meta = &created["meta"];
    let url = created["review_url"].as_str().unwrap_or_default();
    if flags.switch("json") {
        println!(
            "{}",
            serde_json::to_string_pretty(&created).map_err(|err| err.to_string())?
        );
    } else {
        println!("session: {}", meta["id"].as_str().unwrap_or("-"));
        println!("dir:     {}", created["dir"].as_str().unwrap_or("-"));
        println!("url:     {url}");
    }
    if flags.switch("open-browser") {
        open_in_browser(url);
    }
    Ok(0)
}

fn list(flags: &Flags, base_url: &str, auth: &Auth) -> Result<i32, String> {
    let (_, value) = client(base_url, auth)?
        .get("/api/sessions")
        .map_err(|err| err.to_string())?;
    if flags.switch("json") {
        println!(
            "{}",
            serde_json::to_string_pretty(&value["sessions"]).map_err(|err| err.to_string())?
        );
        return Ok(0);
    }
    for session in value["sessions"].as_array().cloned().unwrap_or_default() {
        println!(
            "{:45} {:10} {}",
            session["id"].as_str().unwrap_or("-"),
            session["status"].as_str().unwrap_or("-"),
            session["title"].as_str().unwrap_or("")
        );
    }
    Ok(0)
}

fn get(flags: &Flags, base_url: &str, auth: &Auth) -> Result<i32, String> {
    let id = flags.positional.first().ok_or("usage: redline get ID")?;
    match client(base_url, auth)?.get(&format!("/api/sessions/{id}")) {
        Ok((_, value)) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&value).map_err(|err| err.to_string())?
            );
            Ok(0)
        }
        Err(ClientError::Http(404, _)) => {
            eprintln!("session not found");
            Ok(1)
        }
        Err(err) => Err(err.to_string()),
    }
}

fn wait(flags: &Flags, base_url: &str, auth: &Auth) -> Result<i32, String> {
    let id = flags
        .positional
        .first()
        .ok_or("usage: redline wait ID")?
        .to_string();
    let timeout = flags.number("timeout")?.unwrap_or(600);
    let interval = Duration::from_secs(flags.number("interval")?.unwrap_or(2).max(1));
    let client = client(base_url, auth)?;
    let deadline = Instant::now() + Duration::from_secs(timeout);

    loop {
        match client.get(&format!("/api/sessions/{id}")) {
            Ok((_, value)) => {
                let status = value["meta"]["status"].as_str().unwrap_or("open");
                if status != "open" {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&value).map_err(|err| err.to_string())?
                    );
                    return Ok(0);
                }
            }
            Err(ClientError::Http(404, _)) => {
                eprintln!("session not found");
                return Ok(1);
            }
            Err(err) => {
                eprintln!("{err}");
                return Ok(1);
            }
        }
        if Instant::now() >= deadline {
            eprintln!("timeout waiting for {id}");
            return Ok(2);
        }
        std::thread::sleep(interval);
    }
}

fn remove(flags: &Flags, base_url: &str, auth: &Auth) -> Result<i32, String> {
    let id = flags.positional.first().ok_or("usage: redline remove ID")?;
    let client = client(base_url, auth)?;
    if !flags.switch("yes") {
        print!("remove session {id}? [y/N] ");
        std::io::stdout().flush().ok();
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer).ok();
        if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            return Ok(1);
        }
    }
    match client.delete(&format!("/api/sessions/{id}")) {
        Ok((_, value)) => {
            println!("removed {}", value["removed"].as_str().unwrap_or(id));
            Ok(0)
        }
        Err(ClientError::Http(404, _)) => {
            eprintln!("session not found");
            Ok(1)
        }
        Err(err) => Err(err.to_string()),
    }
}

fn open_in_browser(url: &str) {
    let command = if cfg!(target_os = "windows") {
        ("cmd", vec!["/C", "start", "", url])
    } else if cfg!(target_os = "macos") {
        ("open", vec![url])
    } else {
        ("xdg-open", vec![url])
    };
    let _ = std::process::Command::new(command.0)
        .args(command.1)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

// ---------------------------------------------------------------- config

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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Flags, String> {
        Flags::parse(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn public_url_is_parsed_as_a_value_flag() {
        let flags = parse(&["--host", "0.0.0.0", "--public-url", "https://x.test"]).unwrap();
        assert_eq!(flags.value("public-url"), Some("https://x.test"));
        assert_eq!(flags.value("host"), Some("0.0.0.0"));
    }

    #[test]
    fn unknown_options_fail_loudly() {
        assert!(parse(&["--public-urll", "https://x.test"]).is_err());
        assert!(parse(&["--bogus"]).is_err());
        assert!(parse(&["--json", "--yes"]).is_ok());
    }

    #[test]
    fn last_value_wins_for_repeated_flags() {
        // The container appends its command after the image entrypoint, so flags repeat.
        let flags = parse(&["--data-dir", "/a", "--data-dir", "/data"]).unwrap();
        assert_eq!(flags.value("data-dir"), Some("/data"));
    }
}
