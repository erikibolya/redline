//! RedLine agent bridge.
//!
//! RedLine is pull-only for agents: a submission is state on disk, not a request to a running
//! model. This crate owns the loop instead — it watches submitted reviews, runs OpenCode against a
//! repository with the annotations in the prompt, and lets that run report back through RedLine
//! (`progress`, `reply`, `resolve`).
//!
//! It is deliberately small and dependency-light so the desktop app can embed it and a headless
//! host can run the `redline-agent` binary.

use std::collections::HashSet;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use wait_timeout::ChildExt;

#[derive(Clone, Debug)]
pub struct AgentConfig {
    /// RedLine base URL, e.g. `http://127.0.0.1:8788` or `https://redline.example`.
    pub url: String,
    pub token: String,
    /// Repository the agent works in.
    pub repo: PathBuf,
    /// OpenCode model id, e.g. `deepseek/deepseek-flash`.
    pub model: String,
    pub opencode_bin: String,
    /// Dispatch into an already running OpenCode instead of a private `--standalone` run.
    pub opencode_server: Option<String>,
    pub poll: Duration,
    pub timeout: Duration,
}

impl AgentConfig {
    pub fn new(url: impl Into<String>, token: impl Into<String>, repo: impl Into<PathBuf>) -> Self {
        Self {
            url: url.into().trim_end_matches('/').to_string(),
            token: token.into(),
            repo: repo.into(),
            model: "deepseek/deepseek-flash".to_string(),
            opencode_bin: "opencode".to_string(),
            opencode_server: None,
            poll: Duration::from_secs(10),
            timeout: Duration::from_secs(1800),
        }
    }
}

fn http() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(120))
        .build()
}

fn api_get(config: &AgentConfig, path: &str) -> Result<Value, String> {
    let response = http()
        .get(&format!("{}{path}", config.url))
        .set("Authorization", &format!("Bearer {}", config.token))
        .call()
        .map_err(|err| err.to_string())?;
    let text = response.into_string().map_err(|err| err.to_string())?;
    serde_json::from_str(&text).map_err(|err| format!("invalid JSON from {path}: {err}"))
}

fn mcp(config: &AgentConfig, tool: &str, arguments: Value) -> Result<Value, String> {
    let payload = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": tool, "arguments": arguments },
    });
    let response = http()
        .post(&format!("{}/mcp", config.url))
        .set("Authorization", &format!("Bearer {}", config.token))
        .set("Content-Type", "application/json")
        .send_string(&payload.to_string())
        .map_err(|err| err.to_string())?;
    let text = response.into_string().map_err(|err| err.to_string())?;
    let value: Value = serde_json::from_str(&text).map_err(|err| err.to_string())?;
    let content = value["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    match serde_json::from_str(&content) {
        Ok(value) => Ok(value),
        Err(_) => Ok(json!({ "text": content })),
    }
}

fn prompt_for(config: &AgentConfig, session: &Value, feedback: &Value) -> String {
    let id = session["id"].as_str().unwrap_or_default();
    let annotations = serde_json::to_string_pretty(&feedback["annotations"]).unwrap_or_default();
    let markdown = feedback["markdown"].as_str().unwrap_or("(no markdown)");
    let prompt = session["prompt"]
        .as_str()
        .or_else(|| session["note"].as_str())
        .unwrap_or("(none)");
    let review = feedback["review_url"].as_str().unwrap_or_default();
    format!(
        r#"You are working unattended in the repository at {repo} on a visual review from RedLine.

Review id: {id}
Review URL: {review}
Project: {project}
Prompt from the human: {prompt}

The human's annotations (JSON):
{annotations}

The rendered feedback:
{markdown}

Your job:
1. Read the repository's AGENTS.md and follow it.
2. Implement what the annotations ask for. Keep the change minimal and in the style of the
   codebase. Run the tests you can run. You may build and deploy with the repository's own
   scripts when the review needs a visual result — the human expects a screenshot of the change.
   Never rewrite published git history; leave the repository in a state a human can review.
3. Narrate as you work through the RedLine MCP endpoint with curl — one line per meaningful step,
   at most six:
   curl -s -H "Authorization: Bearer $REDLINE_TOKEN" -H 'content-type: application/json' \
     -d '{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"progress","arguments":{{"session_id":"{id}","message":"...","state":"tool"}}}}}}' \
     "$REDLINE_URL/mcp"
4. When done, send a short reply for the human in their language (tool `reply`, session `{id}`)
   and mark the review resolved (tool `resolve`). If you can produce a screenshot of the result,
   create a follow-up review with `capture_review` and put its review URL in the reply.
   The reply and the resolve are the last two tool calls of every run; the bridge resolves the
   review for you if you forget, but the human still needs your summary.

Do not ask questions: nobody is watching. If something is ambiguous, pick the smallest sensible
interpretation and say what you assumed in the reply."#,
        repo = config.repo.display(),
        project = session["project"].as_str().unwrap_or("Inbox"),
    )
}

/// Run one `opencode run` to completion. `Ok(true)` means the process exited successfully.
fn run_opencode(config: &AgentConfig, prompt: &str) -> Result<(bool, String), String> {
    let mut command = Command::new(&config.opencode_bin);
    command.arg("run");
    match &config.opencode_server {
        Some(server) => {
            command.args(["--server", server]);
        }
        None => {
            command.arg("--standalone");
        }
    }
    command.args(["--model", &config.model]);
    command.args(["--title", "redline review"]);
    command.arg(prompt);
    command
        .current_dir(&config.repo)
        .env("REDLINE_URL", &config.url)
        .env("REDLINE_TOKEN", &config.token)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|err| format!("cannot run opencode: {err}"))?;
    let status = child
        .wait_timeout(config.timeout)
        .map_err(|err| err.to_string())?;
    let status = match status {
        Some(status) => status,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("opencode timed out after {:?}", config.timeout));
        }
    };

    let mut output = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_string(&mut output);
    }
    if let Some(mut stderr) = child.stderr.take() {
        let mut errors = String::new();
        let _ = stderr.read_to_string(&mut errors);
        if !errors.trim().is_empty() {
            output.push('\n');
            output.push_str(&errors);
        }
    }
    Ok((status.success(), output))
}

fn handle(config: &AgentConfig, session: &Value) {
    let id = session["id"].as_str().unwrap_or_default().to_string();
    eprintln!("redline-agent: picked up {id}");
    let feedback = match mcp(config, "get_feedback", json!({ "session_id": id })) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("redline-agent: cannot read feedback for {id}: {err}");
            return;
        }
    };
    let _ = mcp(
        config,
        "progress",
        json!({ "session_id": id, "message": "Picked up by the agent bridge", "state": "thinking" }),
    );

    let prompt = prompt_for(config, session, &feedback);
    let outcome = match run_opencode(config, &prompt) {
        Ok((true, output)) => {
            eprintln!("redline-agent: {id} finished ({} bytes)", output.len());
            output
        }
        Ok((false, output)) => {
            eprintln!("redline-agent: {id} failed");
            let _ = mcp(
                config,
                "reply",
                json!({ "session_id": id, "message": format!("The agent run failed: {}", first_line(&output)) }),
            );
            return;
        }
        Err(err) => {
            eprintln!("redline-agent: {id}: {err}");
            let _ = mcp(
                config,
                "reply",
                json!({ "session_id": id, "message": format!("Agent bridge error: {err}") }),
            );
            return;
        }
    };

    // The agent should have replied and resolved itself. Keep the transcript in the server log;
    // the human only ever sees a short note, never a wall of stdout.
    let mut replied = false;
    if let Ok(value) = api_get(config, &format!("/api/sessions/{id}")) {
        if value["meta"]["status"] == "resolved" {
            eprintln!("redline-agent: {id} resolved by the agent");
            return;
        }
        replied = value["meta"]["agent_state"] == "replied";
    }
    eprintln!("redline-agent: {id} output tail: {}", tail(&outcome, 400));
    if !replied {
        let _ = mcp(
            config,
            "reply",
            json!({ "session_id": id, "message": "The agent finished without a summary. Its changes are in the repository's working tree — check `git status` there." }),
        );
    }
    let _ = mcp(
        config,
        "resolve",
        json!({ "session_id": id, "summary": "Resolved by the agent bridge" }),
    );
}

fn first_line(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("no output");
    if line.len() <= 240 {
        return line.to_string();
    }
    format!("{}…", &line[..240])
}

fn tail(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    if trimmed.len() <= limit {
        return trimmed.to_string();
    }
    trimmed[trimmed.len() - limit..].to_string()
}

/// Watch the server until `cancel` is set. One review at a time, no queue.
pub fn run(config: AgentConfig, cancel: Arc<AtomicBool>) {
    let mut seen: HashSet<String> = HashSet::new();
    eprintln!(
        "redline-agent: watching {} for submitted reviews; repo {}",
        config.url,
        config.repo.display()
    );
    while !cancel.load(Ordering::Relaxed) {
        match api_get(&config, "/api/sessions") {
            Ok(value) => {
                let mut sessions = value["sessions"].as_array().cloned().unwrap_or_default();
                sessions.sort_by_key(|session| {
                    session["created_at"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string()
                });
                for session in sessions {
                    if session["status"] != "submitted" {
                        continue;
                    }
                    let state = session["agent_state"].as_str().unwrap_or_default();
                    if state == "working" || state == "replied" {
                        continue;
                    }
                    let id = session["id"].as_str().unwrap_or_default().to_string();
                    if id.is_empty() || seen.contains(&id) {
                        continue;
                    }
                    seen.insert(id);
                    handle(&config, &session);
                    if cancel.load(Ordering::Relaxed) {
                        return;
                    }
                }
            }
            Err(err) => eprintln!("redline-agent: cannot reach RedLine: {err}"),
        }
        let deadline = Instant::now() + config.poll;
        while Instant::now() < deadline {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_keeps_the_end_of_a_long_output() {
        let text = "x".repeat(1000);
        assert_eq!(tail(&text, 10).len(), 10);
        assert_eq!(tail("  short  ", 100), "short");
    }

    #[test]
    fn prompt_carries_the_session_repository_and_notes() {
        let config = AgentConfig::new("http://127.0.0.1:8788", "t", "/tmp/repo");
        let session = json!({
            "id": "20260101-120000-demo",
            "project": "RedLine UI",
            "prompt": "make it green",
            "note": "look at the button",
        });
        let feedback = json!({
            "review_url": "http://127.0.0.1:8788/s/20260101-120000-demo?t=t",
            "annotations": [{ "type": "rect", "note": "alignment" }],
            "markdown": "# Visual review: demo",
        });
        let prompt = prompt_for(&config, &session, &feedback);
        assert!(prompt.contains("20260101-120000-demo"));
        assert!(prompt.contains("RedLine UI"));
        assert!(prompt.contains("make it green"));
        assert!(prompt.contains("alignment"));
        assert!(prompt.contains("/tmp/repo"));
    }
}
