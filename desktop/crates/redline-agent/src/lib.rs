//! RedLine agent bridge.
//!
//! RedLine is pull-only for agents: a submission is state on disk, not a request to a running
//! model. This crate owns the loop instead — it watches submitted reviews, runs OpenCode against a
//! repository with the annotations in the prompt, and lets that run report back through RedLine
//! (`progress`, `reply`, `resolve`).
//!
//! It is deliberately small and dependency-light so the desktop app can embed it and a headless
//! host can run the `redline-agent` binary.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
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
    /// Repository the agent works in by default (legacy projects without a `repo`).
    pub repo: PathBuf,
    /// Root that project repo keys are relative to, on the agent's filesystem.
    pub repo_root: PathBuf,
    /// Prefix under which the OpenCode server reports the same repositories. `None` means the
    /// server and the agent see the same paths.
    pub opencode_root: Option<PathBuf>,
    /// Stable name sent with the repository sync; lets a future router tell agents apart.
    pub agent_id: String,
    /// How often the agent refreshes the repository list.
    pub repo_sync: Duration,
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
        let repo: PathBuf = repo.into();
        let repo_root = repo
            .parent()
            .map(Path::to_path_buf)
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| repo.clone());
        Self {
            url: url.into().trim_end_matches('/').to_string(),
            token: token.into(),
            repo,
            repo_root,
            opencode_root: None,
            agent_id: "default".to_string(),
            repo_sync: Duration::from_secs(60),
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct RepoEntry {
    /// Relative to the agent's repo root, always with `/` separators.
    key: String,
    name: String,
}

fn api_post(config: &AgentConfig, path: &str, body: &Value) -> Result<Value, String> {
    let response = http()
        .post(&format!("{}{path}", config.url))
        .set("Authorization", &format!("Bearer {}", config.token))
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(|err| err.to_string())?;
    let text = response.into_string().map_err(|err| err.to_string())?;
    serde_json::from_str(&text).map_err(|err| format!("invalid JSON from {path}: {err}"))
}

/// Strip one configured root from an OpenCode path. The OpenCode server and the agent can see the
/// same directory through different mounts, so both roots are accepted and the result is the
/// portable repo key stored in RedLine.
fn strip_repo_root(canonical: &str, root: &Path) -> Option<String> {
    let relative = Path::new(canonical).strip_prefix(root).ok()?;
    let key = relative.to_string_lossy().replace('\\', "/");
    let key = key.trim_matches('/').to_string();
    if key.is_empty()
        || key
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return None;
    }
    Some(key)
}

fn repo_key(config: &AgentConfig, canonical: &str) -> Option<String> {
    if let Some(root) = &config.opencode_root {
        if let Some(key) = strip_repo_root(canonical, root) {
            return Some(key);
        }
    }
    strip_repo_root(canonical, &config.repo_root)
}

/// Query the OpenCode server for its projects and keep the git repositories under the agent root.
fn opencode_repos(config: &AgentConfig) -> Result<Vec<RepoEntry>, String> {
    let server = config
        .opencode_server
        .as_deref()
        .ok_or_else(|| "no OpenCode server configured".to_string())?;
    let output = Command::new(&config.opencode_bin)
        .args(["api", "get", "/api/project", "--server", server])
        .output()
        .map_err(|err| format!("cannot run {}: {err}", config.opencode_bin))?;
    if !output.status.success() {
        return Err(format!("opencode api exited with {}", output.status));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let value: Value =
        serde_json::from_str(&text).map_err(|err| format!("invalid project JSON: {err}"))?;
    let projects = value
        .as_array()
        .ok_or_else(|| "OpenCode project list is not an array".to_string())?;
    let mut out = Vec::new();
    for project in projects {
        if project["vcs"].as_str() != Some("git") {
            continue;
        }
        let canonical = project["canonical"].as_str().unwrap_or_default();
        let Some(key) = repo_key(config, canonical) else {
            continue;
        };
        let name = project["name"]
            .as_str()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| key.rsplit('/').next().unwrap_or(&key).to_string());
        out.push(RepoEntry { key, name });
    }
    Ok(out)
}

/// Fallback that does not depend on OpenCode's experimental project API: every directory with a
/// `.git` entry under the configured root. Nested repos (`ActiUnit/Au.Net`) are found; we do not
/// descend into a repo once it is found.
fn scan_repos(root: &Path) -> Vec<RepoEntry> {
    fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Vec<RepoEntry>) {
        if depth > 4 || out.len() >= 200 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if !path.is_dir()
                || name.starts_with('.')
                || matches!(
                    name.as_str(),
                    "node_modules" | "target" | "dist" | "build" | "vendor"
                )
            {
                continue;
            }
            if path.join(".git").exists() {
                if let Ok(relative) = path.strip_prefix(root) {
                    let key = relative.to_string_lossy().replace('\\', "/");
                    if !key.is_empty() {
                        out.push(RepoEntry {
                            key,
                            name: name.clone(),
                        });
                    }
                }
                continue;
            }
            walk(root, &path, depth + 1, out);
        }
    }

    let mut out = Vec::new();
    walk(root, root, 0, &mut out);
    out.sort_by(|a, b| a.key.to_lowercase().cmp(&b.key.to_lowercase()));
    out
}

/// Server mode trusts OpenCode's project list — that is the set the user sees there. The
/// filesystem scan is only for standalone agents, where there is no OpenCode server to ask.
fn discover_repos(config: &AgentConfig) -> Result<Vec<RepoEntry>, String> {
    let mut out = if config.opencode_server.is_some() {
        opencode_repos(config)?
    } else {
        scan_repos(&config.repo_root)
    };
    out.sort_by(|a, b| a.key.to_lowercase().cmp(&b.key.to_lowercase()));
    out.dedup_by(|a, b| a.key == b.key);
    Ok(out)
}

fn sync_repos(config: &AgentConfig) -> Result<usize, String> {
    let repos = discover_repos(config)?;
    let payload = json!({
        "agent_id": config.agent_id,
        "repos": repos
            .iter()
            .map(|repo| json!({ "key": repo.key, "name": repo.name }))
            .collect::<Vec<_>>(),
    });
    let value = api_post(config, "/api/agent/repos", &payload)?;
    Ok(value["projects"]
        .as_array()
        .map(Vec::len)
        .unwrap_or(repos.len()))
}

/// Resolve a project's repo key against this agent's filesystem. A symlink or `..` cannot escape
/// the configured root; a missing repo is an error rather than a silent fallback.
fn resolve_repo(config: &AgentConfig, key: &str) -> Result<PathBuf, String> {
    let root = std::fs::canonicalize(&config.repo_root).map_err(|err| {
        format!(
            "repo root {} is not readable: {err}",
            config.repo_root.display()
        )
    })?;
    let candidate = root.join(key);
    let path = std::fs::canonicalize(&candidate)
        .map_err(|err| format!("repository {key} is not available: {err}"))?;
    if !path.starts_with(&root) {
        return Err(format!("repository {key} escapes {}", root.display()));
    }
    if !path.is_dir() {
        return Err(format!("repository {key} is not a directory"));
    }
    Ok(path)
}

fn project_repo(
    config: &AgentConfig,
    projects: &Value,
    session: &Value,
) -> Result<PathBuf, String> {
    let slug = session["project"].as_str().unwrap_or("inbox");
    let project = projects["projects"]
        .as_array()
        .and_then(|list| list.iter().find(|project| project["slug"] == slug));
    let Some(project) = project else {
        return Ok(config.repo.clone());
    };
    match project["repo"].as_str() {
        Some(key) => resolve_repo(config, key),
        None => Ok(config.repo.clone()),
    }
}

fn prompt_for(session: &Value, feedback: &Value, repo: &Path) -> String {
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
4. Finish the round in one of two ways:
   - If the human must look at a new screenshot: post it INTO THIS review with `capture_round`
     (session id `{id}`, a URL, a short note) or `post_round` (an image). Never create a separate
     review for a follow-up round — one task is one session. Do not resolve in this case: the
     review goes back to the human for the next annotation.
   - If no visual follow-up is needed: send a short `reply` in the human's language, then
     `resolve`.

Do not ask questions: nobody is watching. If something is ambiguous, pick the smallest sensible
interpretation and say what you assumed in the reply."#,
        repo = repo.display(),
        project = session["project"].as_str().unwrap_or("Inbox"),
    )
}

/// Run one `opencode run` to completion. `Ok(true)` means the process exited successfully.
fn run_opencode(
    config: &AgentConfig,
    session_id: &str,
    prompt: &str,
    repo: &Path,
) -> Result<(bool, String), String> {
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
        .current_dir(repo)
        .env("REDLINE_URL", &config.url)
        .env("REDLINE_TOKEN", &config.token)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|err| format!("cannot run opencode: {err}"))?;
    // Poll the session while the agent works: STOP must actually stop it, not just unlock the UI.
    let started = Instant::now();
    let mut last_check = Instant::now();
    let status = loop {
        match child
            .wait_timeout(Duration::from_millis(500))
            .map_err(|err| err.to_string())?
        {
            Some(status) => break status,
            None => {}
        }
        if started.elapsed() >= config.timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("opencode timed out after {:?}", config.timeout));
        }
        if last_check.elapsed() >= Duration::from_secs(2) {
            last_check = Instant::now();
            if let Ok(value) = api_get(config, &format!("/api/sessions/{session_id}")) {
                if value["meta"]["status"].as_str() != Some("submitted") {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("stopped by the human".to_string());
                }
            }
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

fn handle(config: &AgentConfig, session: &Value, repo: &Path) {
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

    let prompt = prompt_for(session, &feedback, repo);
    let outcome = match run_opencode(config, &id, &prompt, repo) {
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
        Err(err) if err == "stopped by the human" => {
            eprintln!("redline-agent: {id} stopped by the human");
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

    eprintln!("redline-agent: {id} output tail: {}", tail(&outcome, 400));
    // The CLI can exit a moment before the server finishes the last tool call; judge the session
    // after a short grace period, and never invent a message for the human.
    std::thread::sleep(Duration::from_secs(2));
    if let Ok(value) = api_get(config, &format!("/api/sessions/{id}")) {
        match value["meta"]["status"].as_str().unwrap_or_default() {
            "resolved" => eprintln!("redline-agent: {id} resolved by the agent"),
            "open" => eprintln!("redline-agent: {id} handed back to the human with a new round"),
            _ => eprintln!("redline-agent: {id} finished without a reply or a round"),
        }
    }
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
    // Keyed by submission, not by session: a round-based review is submitted many times.
    let mut seen: HashMap<String, String> = HashMap::new();
    eprintln!(
        "redline-agent: watching {} for submitted reviews; agent {}, repo root {}",
        config.url,
        config.agent_id,
        config.repo_root.display()
    );

    let mut last_sync: Option<Instant> = None;
    while !cancel.load(Ordering::Relaxed) {
        if last_sync
            .map(|at| at.elapsed() >= config.repo_sync)
            .unwrap_or(true)
        {
            match sync_repos(&config) {
                Ok(count) => eprintln!(
                    "redline-agent: synced {count} repositories as {}",
                    config.agent_id
                ),
                Err(err) => eprintln!("redline-agent: repository sync failed: {err}"),
            }
            last_sync = Some(Instant::now());
        }

        // Projects carry the repo binding. If this endpoint is down we do not guess: running a
        // review in the wrong repository is worse than waiting for the next poll.
        let projects = match api_get(&config, "/api/projects") {
            Ok(projects) => Some(projects),
            Err(err) => {
                eprintln!("redline-agent: cannot read projects: {err}");
                None
            }
        };

        if let Some(projects) = projects {
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
                        let submitted = session["submitted_at"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string();
                        if id.is_empty() || seen.get(&id) == Some(&submitted) {
                            continue;
                        }
                        seen.insert(id.clone(), submitted);

                        let repo = match project_repo(&config, &projects, &session) {
                            Ok(repo) => repo,
                            Err(err) => {
                                eprintln!("redline-agent: {id} cannot resolve its repo: {err}");
                                let _ = mcp(
                                    &config,
                                    "reply",
                                    json!({
                                        "session_id": id,
                                        "message": format!(
                                            "I cannot start this review: {err}. The project points at a repository this agent cannot see."
                                        ),
                                    }),
                                );
                                let _ = mcp(&config, "resolve", json!({ "session_id": id }));
                                continue;
                            }
                        };
                        handle(&config, &session, &repo);
                        if cancel.load(Ordering::Relaxed) {
                            return;
                        }
                    }
                }
                Err(err) => eprintln!("redline-agent: cannot reach RedLine: {err}"),
            }
        }

        if wait(&cancel, config.poll) {
            return;
        }
    }
}

fn wait(cancel: &AtomicBool, duration: Duration) -> bool {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        if cancel.load(Ordering::Relaxed) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    false
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
        let prompt = prompt_for(&session, &feedback, Path::new("/tmp/repo"));
        assert!(prompt.contains("20260101-120000-demo"));
        assert!(prompt.contains("RedLine UI"));
        assert!(prompt.contains("make it green"));
        assert!(prompt.contains("alignment"));
        assert!(prompt.contains("/tmp/repo"));
    }

    fn temp_root(name: &str) -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "redline-agent-{name}-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn repo_keys_work_across_the_two_mounts() {
        let mut config = AgentConfig::new("http://127.0.0.1:8788", "t", "/agent/repos/redline");
        config.repo_root = PathBuf::from("/agent/repos");
        config.opencode_root = Some(PathBuf::from("/server/repos"));
        assert_eq!(
            repo_key(&config, "/server/repos/ActiUnit/Au.Net").as_deref(),
            Some("ActiUnit/Au.Net")
        );
        assert_eq!(
            repo_key(&config, "/agent/repos/redline").as_deref(),
            Some("redline")
        );
        assert_eq!(repo_key(&config, "/server/repos"), None);
        assert_eq!(repo_key(&config, "/elsewhere/redline"), None);
    }

    #[test]
    fn scan_finds_git_repos_but_does_not_enter_them() {
        let root = temp_root("scan");
        std::fs::create_dir_all(root.join("one/.git")).unwrap();
        std::fs::create_dir_all(root.join("nested/two/.git")).unwrap();
        std::fs::create_dir_all(root.join("one/inside-another/.git")).unwrap();
        std::fs::create_dir_all(root.join("plain")).unwrap();
        let found = scan_repos(&root);
        let keys: Vec<&str> = found.iter().map(|repo| repo.key.as_str()).collect();
        assert_eq!(keys, vec!["nested/two", "one"]);
        assert_eq!(found[0].name, "two");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_review_uses_its_projects_repo_or_the_legacy_default() {
        let root = temp_root("project-repo");
        std::fs::create_dir_all(root.join("redline")).unwrap();
        std::fs::create_dir_all(root.join("other")).unwrap();
        let mut config = AgentConfig::new("http://127.0.0.1:8788", "t", root.join("redline"));
        config.repo_root = root.clone();

        let projects = json!({
            "projects": [
                { "slug": "other", "repo": "other" },
                { "slug": "broken", "repo": "missing" }
            ]
        });
        assert_eq!(
            project_repo(&config, &projects, &json!({ "project": "other" })).unwrap(),
            root.join("other")
        );
        assert_eq!(
            project_repo(&config, &projects, &json!({ "project": "legacy" })).unwrap(),
            config.repo
        );
        assert!(project_repo(&config, &projects, &json!({ "project": "broken" })).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn resolving_a_repo_cannot_escape_the_root() {
        let root = temp_root("resolve");
        std::fs::create_dir_all(root.join("redline")).unwrap();
        let mut config = AgentConfig::new("http://127.0.0.1:8788", "t", root.join("redline"));
        config.repo_root = root.clone();
        assert_eq!(
            resolve_repo(&config, "redline").unwrap(),
            root.join("redline")
        );
        assert!(resolve_repo(&config, "../escape").is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
