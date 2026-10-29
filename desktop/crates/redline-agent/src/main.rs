//! Headless entry point for the RedLine agent bridge.
//!
//! ```bash
//! REDLINE_URL=https://redline.example REDLINE_TOKEN=… REDLINE_AGENT_REPO=~/src/app redline-agent
//! ```

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use redline_agent::{run, AgentConfig};

fn env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn token() -> String {
    if let Some(value) = env("REDLINE_TOKEN") {
        return value;
    }
    let mut candidates = vec![PathBuf::from("/data/token")];
    candidates.extend(dirs_token());
    for candidate in candidates {
        if let Ok(value) = std::fs::read_to_string(&candidate) {
            let value = value.trim().to_string();
            if !value.is_empty() {
                return value;
            }
        }
    }
    eprintln!("redline-agent: REDLINE_TOKEN is not set and no token file was found");
    std::process::exit(2);
}

fn dirs_token() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".config/redline/token"))
}

fn main() {
    let repo = env("REDLINE_AGENT_REPO").unwrap_or_else(|| {
        std::env::current_dir()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| ".".to_string())
    });
    let mut config = AgentConfig::new(
        env("REDLINE_URL").unwrap_or_else(|| "http://127.0.0.1:8788".to_string()),
        token(),
        repo,
    );
    if let Some(model) = env("REDLINE_AGENT_MODEL") {
        config.model = model;
    }
    if let Some(bin) = env("OPENCODE_BIN") {
        config.opencode_bin = bin;
    }
    if let Some(server) = env("REDLINE_AGENT_OPENCODE_SERVER") {
        config.opencode_server = Some(server);
    }
    if let Some(poll) = env("REDLINE_AGENT_POLL").and_then(|value| value.parse().ok()) {
        config.poll = Duration::from_secs(poll);
    }
    if let Some(timeout) = env("REDLINE_AGENT_TIMEOUT").and_then(|value| value.parse().ok()) {
        config.timeout = Duration::from_secs(timeout);
    }

    if !config.repo.is_dir() {
        eprintln!(
            "redline-agent: repository does not exist: {}",
            config.repo.display()
        );
        std::process::exit(2);
    }

    // Ctrl+C terminates the process; the loop only needs the flag when it is embedded.
    run(config, Arc::new(AtomicBool::new(false)));
}
