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

fn build_config(require_token: bool) -> AgentConfig {
    let repo = env("REDLINE_AGENT_REPO").unwrap_or_else(|| {
        std::env::current_dir()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| ".".to_string())
    });
    let mut config = AgentConfig::new(
        env("REDLINE_URL").unwrap_or_else(|| "http://127.0.0.1:8788".to_string()),
        if require_token {
            token()
        } else {
            String::new()
        },
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
    if let Some(root) = env("REDLINE_AGENT_REPO_ROOT") {
        config.repo_root = PathBuf::from(root);
    }
    if let Some(root) = env("REDLINE_AGENT_OPENCODE_ROOT") {
        config.opencode_root = Some(PathBuf::from(root));
    }
    if let Some(agent) = env("REDLINE_AGENT_ID") {
        config.agent_id = agent;
    }
    if let Some(sync) = env("REDLINE_AGENT_SYNC").and_then(|value| value.parse().ok()) {
        config.repo_sync = Duration::from_secs(sync);
    }
    if let Some(poll) = env("REDLINE_AGENT_POLL").and_then(|value| value.parse().ok()) {
        config.poll = Duration::from_secs(poll);
    }
    if let Some(timeout) = env("REDLINE_AGENT_TIMEOUT").and_then(|value| value.parse().ok()) {
        config.timeout = Duration::from_secs(timeout);
    }
    if let Some(idle) = env("REDLINE_AGENT_IDLE_TIMEOUT").and_then(|value| value.parse().ok()) {
        config.idle_timeout = Duration::from_secs(idle);
    }
    if let Some(workers) =
        env("REDLINE_AGENT_WORKERS").and_then(|value| value.parse::<usize>().ok())
    {
        config.workers = workers.clamp(1, 32);
    }
    config
}

fn main() {
    let mut args = std::env::args().skip(1);
    let command = args.next();
    if command.as_deref() == Some("prune") {
        let apply = args.any(|arg| arg == "--yes" || arg == "-y");
        let config = build_config(false);
        if !config.repo.is_dir() {
            eprintln!(
                "redline-agent: repository does not exist: {}",
                config.repo.display()
            );
            std::process::exit(2);
        }
        match redline_agent::prune_worktrees(&config, apply) {
            Ok(report) => {
                println!(
                    "redline-agent: {} candidate(s), {} removed, {} failed",
                    report.candidates, report.removed, report.failed
                );
                if report.failed > 0 {
                    std::process::exit(1);
                }
            }
            Err(err) => {
                eprintln!("redline-agent: {err}");
                std::process::exit(1);
            }
        }
        return;
    }

    let config = build_config(true);
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
