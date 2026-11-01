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
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
    /// Hard cap for one run, even if it keeps producing output. A runaway process must not hold
    /// a claim forever; the default is deliberately far above a normal review.
    pub timeout: Duration,
    /// Kill a run only after this long with no output. A long, active build is not a stuck run.
    pub idle_timeout: Duration,
    /// How many claim workers to run. The server enforces the real global/per-project limits;
    /// extra workers simply idle. `REDLINE_AGENT_WORKERS` overrides this.
    pub workers: usize,
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
            timeout: Duration::from_secs(6 * 60 * 60),
            idle_timeout: Duration::from_secs(15 * 60),
            workers: 4,
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

fn git(repo: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|err| format!("cannot run git: {err}"))
}

fn git_ok(repo: &Path, args: &[&str]) -> bool {
    git(repo, args)
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn path_segment(value: &str) -> String {
    let safe: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '-'
            }
        })
        .collect();
    let safe = safe.trim_matches('-');
    if safe.is_empty() {
        "work".to_string()
    } else {
        // Keep the path short: Git for Windows fails with "$GIT_DIR too big" once a worktree
        // path crosses the legacy MAX_PATH budget. The hash appended by `worktree_name` is what
        // makes it unique; the readable prefix is only a debugging aid.
        safe.chars().take(8).collect()
    }
}

fn worktree_name(work_key: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    work_key.hash(&mut hasher);
    format!("{}-{:016x}", path_segment(work_key), hasher.finish())
}

fn worktree_path(config: &AgentConfig, repo: &Path, work_key: &str) -> PathBuf {
    // Include the canonical repo path in the directory key: two projects can share a folder name.
    let repo_key = worktree_name(&repo.to_string_lossy());
    config
        .repo_root
        .join(".redline-worktrees")
        .join(repo_key)
        .join(worktree_name(work_key))
}

/// Pick the commit the feature branch starts from. Candidates are fully qualified on purpose: a
/// short name such as `dev` is ambiguous once a tag with the same name exists, and git then either
/// refuses it ("fatal: ambiguous object name") or silently picks the tag instead of the branch.
fn worktree_base(repo: &Path) -> &'static str {
    const CANDIDATES: [&str; 4] = [
        "refs/heads/dev",
        "refs/heads/main",
        "refs/remotes/origin/dev",
        "refs/remotes/origin/main",
    ];
    CANDIDATES
        .into_iter()
        .find(|candidate| {
            let peeled = format!("{candidate}^{{commit}}");
            git_ok(repo, &["rev-parse", "--verify", "--quiet", &peeled])
        })
        .unwrap_or("HEAD")
}

/// Create (or reuse) a git worktree for the claimed branch. A non-git legacy repo runs in place.
fn ensure_worktree(config: &AgentConfig, repo: &Path, work_key: &str) -> Result<PathBuf, String> {
    if !git_ok(repo, &["rev-parse", "--is-inside-work-tree"]) {
        eprintln!(
            "redline-agent: {} is not a git repository; running in place",
            repo.display()
        );
        return Ok(repo.to_path_buf());
    }
    let path = worktree_path(config, repo, work_key);
    if path.exists() {
        if git_ok(&path, &["rev-parse", "--is-inside-work-tree"]) {
            return Ok(path);
        }
        return Err(format!(
            "worktree path exists but is not a git worktree: {}",
            path.display()
        ));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("cannot create {}: {err}", parent.display()))?;
    }
    let path_arg = path
        .to_str()
        .ok_or_else(|| format!("worktree path is not UTF-8: {}", path.display()))?;
    let branch_ref = format!("refs/heads/{work_key}");
    let branch_exists = git_ok(repo, &["show-ref", "--verify", "--quiet", &branch_ref]);
    let remote_ref = format!("refs/remotes/origin/{work_key}");
    let remote_exists = git_ok(repo, &["show-ref", "--verify", "--quiet", &remote_ref]);
    let base = worktree_base(repo);
    let output = if branch_exists {
        git(repo, &["worktree", "add", path_arg, work_key])?
    } else if remote_exists {
        // The review may have been pushed from another clone; the handoff must start from those
        // commits, not from a fresh branch at dev's tip.
        git(
            repo,
            &[
                "worktree",
                "add",
                "-b",
                work_key,
                path_arg,
                &format!("origin/{work_key}"),
            ],
        )?
    } else {
        git(repo, &["worktree", "add", "-b", work_key, path_arg, base])?
    };
    if !output.status.success() {
        return Err(format!(
            "git worktree add failed for {work_key}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(path)
}

/// Remove a clean worktree after the feature is resolved. Dirty work is left for inspection.
fn release_worktree(config: &AgentConfig, repo: &Path, work_key: &str) {
    let path = worktree_path(config, repo, work_key);
    if path == repo || !path.exists() || !git_ok(&path, &["rev-parse", "--is-inside-work-tree"]) {
        return;
    }
    let dirty = git(&path, &["status", "--porcelain"])
        .map(|output| !output.stdout.is_empty())
        .unwrap_or(true);
    if dirty {
        eprintln!(
            "redline-agent: leaving dirty worktree {} for inspection",
            path.display()
        );
        return;
    }
    let Some(path_arg) = path.to_str() else {
        return;
    };
    match git(repo, &["worktree", "remove", path_arg]) {
        Ok(output) if output.status.success() => {
            eprintln!("redline-agent: removed worktree {}", path.display())
        }
        Ok(output) => eprintln!(
            "redline-agent: could not remove worktree {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ),
        Err(err) => eprintln!("redline-agent: could not remove worktree: {err}"),
    }
}

fn claim_next(config: &AgentConfig) -> Result<Option<(Value, Value, String)>, String> {
    let value = api_post(
        config,
        "/api/agent/next",
        // Declare the jobs this bridge can run. An old server ignores it; an old bridge sends no
        // jobs and is never handed a confirmed feature it would treat as another review round.
        &json!({ "agent_id": config.agent_id, "jobs": ["review", "handoff"] }),
    )?;
    if value["session"].is_null() {
        return Ok(None);
    }
    let job = value["job"].as_str().unwrap_or("review").to_string();
    Ok(Some((
        value["session"].clone(),
        value["claim"].clone(),
        job,
    )))
}

fn claim_heartbeat(config: &AgentConfig, id: &str) -> Result<(), String> {
    api_post(
        config,
        &format!("/api/agent/claims/{id}/heartbeat"),
        &json!({ "agent_id": config.agent_id }),
    )?;
    Ok(())
}

fn claim_waiting(config: &AgentConfig, id: &str) -> Result<(), String> {
    api_post(
        config,
        &format!("/api/agent/claims/{id}/state"),
        &json!({ "agent_id": config.agent_id, "state": "waiting" }),
    )?;
    Ok(())
}

fn claim_release(config: &AgentConfig, id: &str) -> Result<(), String> {
    api_post(
        config,
        &format!("/api/agent/claims/{id}/release"),
        &json!({ "agent_id": config.agent_id }),
    )?;
    Ok(())
}

fn prompt_for(session: &Value, feedback: &Value, repo: &Path, branch: &str) -> String {
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
Feature branch: {branch}
Work in this worktree only. Commit and push this branch; do not switch branches or rewrite
published history.

This is a review round, not a handoff. Do NOT open a pull request, run the repository's
feature-ready flow, merge, tag or publish a release. The human's ✓ Confirm feature queues a
separate handoff run; opening a PR now would hand off a feature they have not accepted. Pushing
the branch is allowed and expected.

Build and test scope: run focused checks for what you touched (cargo test for the affected
crates, a targeted e2e). Do NOT build or publish the full release/browser image and do NOT run
the release pipeline during a review — those are release/handoff steps and can take longer than
the whole review. Use a dev/preview deploy only when the review needs a visual result.
Prompt from the human: {prompt}

The human's annotations (JSON):
{annotations}

The rendered feedback:
{markdown}

Your job:
1. Read the repository's AGENTS.md and follow it.
2. Implement what the annotations ask for. Keep the change minimal and in the style of the
   codebase. Run focused tests, not the full release build. You may deploy to a dev/preview
   environment with the repository's own scripts when the review needs a visual result — the
   human expects a screenshot of the change. Never merge or deploy to production. Never rewrite
   published git history; leave the repository in a state a human can review.
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
        branch = branch,
    )
}

/// The second run a feature gets: the human already confirmed it, so the agent may perform the
/// integration's handoff (open the PR/MR and let the repository's merge flow finish). The
/// concrete forge commands live in the repository's AGENTS.md; RedLine only owns the
/// confirmation and the status report. This is deliberately separate from `prompt_for`: the
/// review run is forbidden to hand off, and only this prompt grants that permission.
fn handoff_prompt_for(session: &Value, repo: &Path, branch: &str) -> String {
    let id = session["id"].as_str().unwrap_or_default();
    let project = session["project"].as_str().unwrap_or("Inbox");
    let prompt = session["prompt"]
        .as_str()
        .or_else(|| session["note"].as_str())
        .unwrap_or("(none)");
    let handoff = &session["handoff"];
    let previous_url = handoff["url"].as_str().unwrap_or("");
    let previous = if let Some(message) = handoff["message"].as_str().filter(|m| !m.is_empty()) {
        let reuse = if previous_url.is_empty() {
            String::new()
        } else {
            format!(" Reuse the existing link: {previous_url}")
        };
        format!("A previous handoff attempt reported: {message}.{reuse}")
    } else if !previous_url.is_empty() {
        format!("A previous attempt left a link; reuse it: {previous_url}")
    } else {
        String::new()
    };
    format!(
        r#"You are performing the handoff for a feature the human has already confirmed in RedLine.
This is not a review round: do not redo the annotations, do not create a new review and do not
post a round.

Review id: {id}
Project: {project}
Feature branch: {branch}
Worktree: {repo}
The original request was: {prompt}
{previous}

The human confirmed this feature. Nobody is watching; do not ask questions.

Your job:
1. Read the repository's AGENTS.md and follow its shipping rules. It is the source of truth for
   the concrete flow: the remote, the base branch, how to open the PR/MR, which checks are
   required, and how the repository merges. Work only in this worktree and on this branch.
2. Make sure the branch is pushed. If the review left uncommitted changes, commit them on this
   branch first. Never rewrite published git history.
3. Open the pull request/merge request using the repository's documented flow. If a PR/MR for
   this branch already exists, reuse it; never open a second one. Do not mark it ready or enable
   auto-merge before the required checks are green. Do not merge locally.
4. Run the local preflight the repository asks for (build and affected tests); the full suite
   runs in CI. Do not replace it with a smoke test.
5. Wait for the required checks. Do not merge on red. Fix failures caused by this branch, push,
   and wait again. Do not merge into main when the repository integrates through dev.
6. Report the result through RedLine's MCP endpoint:
   curl -s -H "Authorization: Bearer $REDLINE_TOKEN" -H 'content-type: application/json' \
     -d '{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"handoff_status","arguments":{{"session_id":"{id}","state":"done","label":"merged","url":"https://<forge-host>/OWNER/REPO/pull/N","message":"merge commit ..."}}}}}}' \
     "$REDLINE_URL/mcp"
   - `done` + label "merged" only when the forge reports the PR/MR as merged.
   - `failed` with a short label and message when checks fail, the PR/MR is still open, or you
     cannot merge. Include the PR/MR URL in `url` so the human can click it.
7. Do not create a RedLine review or post a round for this handoff. Do not send a reply; the
   human sees the handoff state in the rail.

If the branch is already merged, report the existing PR/MR as done/merged. If there is nothing
to ship, report failed with the reason."#,
        id = id,
        project = project,
        branch = branch,
        repo = repo.display(),
        prompt = prompt,
        previous = previous,
    )
}

/// Why a run did not finish. The bridge reports `code` to RedLine so the human sees *timeout*
/// rather than a generic "failed", while the message keeps the exact limit.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RunError {
    code: &'static str,
    message: String,
}

impl RunError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// `15m`, `6h` — the timeout message is read by a human, not parsed.
fn human_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    if seconds >= 3600 && seconds % 3600 == 0 {
        format!("{}h", seconds / 3600)
    } else if seconds >= 60 && seconds % 60 == 0 {
        format!("{}m", seconds / 60)
    } else {
        format!("{seconds}s")
    }
}

/// Kill the run and everything it spawned. `opencode` launches shell commands and Docker builds;
/// killing only the direct child left those grandchildren running (a four-hour orphaned
/// `docker build` blocked the legacy builder in 2026-11-01). On Unix the child is its own process
/// group leader (`process_group(0)`), so a negative PID kills the whole tree.
fn kill_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let pid = child.id() as i32;
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Run one `opencode run` to completion. `Ok(true)` means the process exited successfully.
fn run_opencode(
    config: &AgentConfig,
    session_id: &str,
    prompt: &str,
    repo: &Path,
    handoff: bool,
) -> Result<(bool, String), RunError> {
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
        .env("PWD", repo)
        .env("REDLINE_URL", &config.url)
        .env("REDLINE_TOKEN", &config.token)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // A new process group lets the timeout/STOP path kill shell commands and Docker builds,
        // not just the `opencode` wrapper.
        command.process_group(0);
    }

    let mut child = command
        .spawn()
        .map_err(|err| RunError::new("spawn", format!("cannot run opencode: {err}")))?;
    // Poll the session while the agent works: STOP must actually stop it, not just unlock the UI.
    let started = Instant::now();
    let mut last_check = Instant::now();
    let mut last_heartbeat = Instant::now();
    // The agent's own output is the "regular opencode window" a human asked for: stream a live
    // tail into the review (throttled, ANSI stripped) while the run is going, instead of only
    // showing the first line when it fails (2026-10-31).
    let sink = Arc::new(Mutex::new(String::new()));
    let posted = Arc::new(AtomicBool::new(false));
    // A run is idle only when it has produced no output; `sleep 120; tail build.log` loops still
    // prove the agent is alive, so the hard cap stays a last resort (2026-10-31).
    let activity = Arc::new(AtomicU64::new(now_millis()));
    let mut readers = Vec::new();
    if let Some(stream) = child.stdout.take() {
        readers.push(stream_output(
            config,
            session_id,
            stream,
            sink.clone(),
            posted.clone(),
            activity.clone(),
        ));
    }
    if let Some(stream) = child.stderr.take() {
        readers.push(stream_output(
            config,
            session_id,
            stream,
            sink.clone(),
            posted.clone(),
            activity.clone(),
        ));
    }
    let idle_ms = config.idle_timeout.as_millis().max(1) as u64;
    let status = loop {
        match child
            .wait_timeout(Duration::from_millis(500))
            .map_err(|err| RunError::new("spawn", err.to_string()))?
        {
            Some(status) => break status,
            None => {}
        }
        if now_millis().saturating_sub(activity.load(Ordering::Relaxed)) >= idle_ms {
            kill_tree(&mut child);
            return Err(RunError::new(
                "timeout_idle",
                format!(
                    "opencode produced no output for {}",
                    human_duration(config.idle_timeout)
                ),
            ));
        }
        if started.elapsed() >= config.timeout {
            kill_tree(&mut child);
            return Err(RunError::new(
                "timeout_hard",
                format!(
                    "opencode exceeded the {} run limit",
                    human_duration(config.timeout)
                ),
            ));
        }
        if last_check.elapsed() >= Duration::from_secs(2) {
            last_check = Instant::now();
            if let Ok(value) = api_get(config, &format!("/api/sessions/{session_id}")) {
                // A review run lives while the round is submitted. A handoff run lives while
                // the feature is still confirmed; reopening or cancelling it kills the handoff.
                let alive = if handoff {
                    value["meta"]["feature_confirmed_at"].is_string()
                        && value["meta"]["handoff"].is_object()
                } else {
                    value["meta"]["status"].as_str() == Some("submitted")
                };
                if !alive {
                    kill_tree(&mut child);
                    return Err(RunError::new("stopped", "stopped by the human"));
                }
            }
        }
        if last_heartbeat.elapsed() >= Duration::from_secs(30) {
            last_heartbeat = Instant::now();
            let _ = claim_heartbeat(config, session_id);
        }
    };

    // The readers drain both pipes until the child closes them; joining them gives the complete
    // output the failure path quotes. The full transcript stays local except for the live tail.
    for reader in readers {
        let _ = reader.join();
    }
    let output = sink.lock().map(|text| text.clone()).unwrap_or_default();
    if !posted.load(Ordering::Relaxed) {
        // A run whose output never crossed the throttle (short runs, buffered pipes) still gets
        // one line into the review, so "no output" never means "the agent said nothing".
        if let Some(line) = output.lines().rev().find(|line| !log_line(line).is_empty()) {
            let _ = mcp(
                config,
                "progress",
                json!({ "session_id": session_id, "message": log_line(line), "state": "log" }),
            );
        }
    }
    Ok((status.success(), output))
}

/// How often the bridge forwards a line of the agent's own output. A live tail, not a transcript:
/// the point is seeing that something is happening and what it is.
const LOG_LINE_EVERY: Duration = Duration::from_secs(2);
const LOG_LINE_MAX_CHARS: usize = 400;

/// Forward one stream of the agent's own output (stdout or stderr) to the review while it runs.
/// The stream is also collected locally, so the failure message and the bridge log keep their
/// `first_line`/`tail` behaviour.
fn stream_output<R: Read + Send + 'static>(
    config: &AgentConfig,
    session_id: &str,
    stream: R,
    sink: Arc<Mutex<String>>,
    posted: Arc<AtomicBool>,
    activity: Arc<AtomicU64>,
) -> std::thread::JoinHandle<()> {
    let config = config.clone();
    let id = session_id.to_string();
    std::thread::spawn(move || {
        use std::io::BufRead as _;
        let mut reader = std::io::BufReader::new(stream);
        let mut next = Instant::now();
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            // Any line — even one the throttle suppresses — is proof of life for the idle check.
            activity.store(now_millis(), Ordering::Relaxed);
            if let Ok(mut text) = sink.lock() {
                text.push_str(&line);
                // Keep the tail only: an agent can print megabytes and the bridge only quotes the
                // first line and the last 400 characters.
                if text.len() > 64_000 {
                    let cut = text.len() - 32_000;
                    let keep = text[cut..].to_string();
                    text.clear();
                    text.push_str(&keep);
                }
            }
            if Instant::now() >= next {
                let message = log_line(&line);
                if !message.is_empty()
                    && mcp(
                        &config,
                        "progress",
                        json!({
                            "session_id": id,
                            "message": message,
                            "state": "log",
                        }),
                    )
                    .is_ok()
                {
                    posted.store(true, Ordering::Relaxed);
                    next = Instant::now() + LOG_LINE_EVERY;
                }
            }
            line.clear();
        }
    })
}

/// One line of agent output as it should appear in the review: terminal colour codes removed, the
/// length bounded, and nothing at all for blank lines.
fn log_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            // CSI sequence: ESC [ … final byte in @..~
            if chars.peek() == Some(&'[') {
                chars.next();
                for next in chars.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            continue;
        }
        if ch == '\r' || ch == '\n' {
            continue;
        }
        out.push(ch);
    }
    let trimmed = out.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    trimmed.chars().take(LOG_LINE_MAX_CHARS).collect()
}

#[cfg(test)]
mod live_output_tests {
    use super::*;

    #[test]
    fn agent_output_lines_lose_terminal_colours_and_stay_short() {
        assert_eq!(
            log_line("\u{1b}[32m✓ edited src/lib.rs\u{1b}[0m\r\n"),
            "✓ edited src/lib.rs"
        );
        assert_eq!(log_line("   \n"), "");
        assert_eq!(
            log_line(&"x".repeat(900)).chars().count(),
            LOG_LINE_MAX_CHARS
        );
    }
}

fn fail_claim(
    config: &AgentConfig,
    id: &str,
    owned: &Arc<Mutex<HashSet<String>>>,
    code: Option<&str>,
    message: String,
) {
    // `state: failed` is not an answer: the server hands the round back to the human, unlocked and
    // labelled, instead of leaving it submitted behind a run that is already gone (2026-10-31).
    // The code lets the UI say "timed out" instead of a generic "failed".
    let mut arguments = json!({ "session_id": id, "message": message, "state": "failed" });
    if let Some(code) = code {
        arguments["code"] = json!(code);
    }
    let _ = mcp(config, "reply", arguments);
    let _ = claim_release(config, id);
    if let Ok(mut set) = owned.lock() {
        set.remove(id);
    }
}

fn handle_claimed(
    config: &AgentConfig,
    session: &Value,
    repo: &Path,
    work_key: &str,
    owned: &Arc<Mutex<HashSet<String>>>,
    job: &str,
) {
    if job == "handoff" {
        handle_handoff_claimed(config, session, repo, work_key, owned);
        return;
    }
    let id = session["id"].as_str().unwrap_or_default().to_string();
    if let Ok(mut set) = owned.lock() {
        set.insert(id.clone());
    }
    eprintln!("redline-agent: picked up {id} on {work_key}");

    let worktree = match ensure_worktree(config, repo, work_key) {
        Ok(path) => path,
        Err(err) => {
            fail_claim(
                config,
                &id,
                owned,
                None,
                format!("I cannot start this review: {err}."),
            );
            return;
        }
    };
    let feedback = match mcp(config, "get_feedback", json!({ "session_id": id })) {
        Ok(value) => value,
        Err(err) => {
            fail_claim(
                config,
                &id,
                owned,
                None,
                format!("I cannot read the review: {err}."),
            );
            return;
        }
    };
    let _ = mcp(
        config,
        "progress",
        json!({ "session_id": id, "message": "Picked up by the agent bridge", "state": "thinking" }),
    );

    let prompt = prompt_for(session, &feedback, &worktree, work_key);
    let output = match run_opencode(config, &id, &prompt, &worktree, false) {
        Ok((true, output)) => {
            eprintln!("redline-agent: {id} finished ({} bytes)", output.len());
            output
        }
        Ok((false, output)) => {
            eprintln!("redline-agent: {id} failed");
            fail_claim(
                config,
                &id,
                owned,
                None,
                format!("The agent run failed: {}", first_line(&output)),
            );
            return;
        }
        Err(err) if err.code == "stopped" => {
            eprintln!("redline-agent: {id} stopped by the human");
            String::new()
        }
        Err(err) => {
            eprintln!("redline-agent: {id}: {err}");
            fail_claim(
                config,
                &id,
                owned,
                Some(err.code),
                format!("Agent bridge error: {}", err.message),
            );
            return;
        }
    };

    if !output.is_empty() {
        eprintln!("redline-agent: {id} output tail: {}", tail(&output, 400));
    }
    // The CLI can exit a moment before the server finishes the last tool call; judge the session
    // after a short grace period, and never invent a message for the human.
    std::thread::sleep(Duration::from_secs(2));
    let status = api_get(config, &format!("/api/sessions/{id}"))
        .ok()
        .and_then(|value| value["meta"]["status"].as_str().map(str::to_string));
    match status.as_deref() {
        Some("resolved") => {
            eprintln!("redline-agent: {id} resolved by the agent");
            let _ = claim_release(config, &id);
            if let Ok(mut set) = owned.lock() {
                set.remove(&id);
            }
            release_worktree(config, repo, work_key);
        }
        Some("open") => {
            eprintln!("redline-agent: {id} handed back to the human; keeping {work_key} owned");
            if let Err(err) = claim_waiting(config, &id) {
                eprintln!("redline-agent: {id} could not keep the branch waiting: {err}");
                if let Ok(mut set) = owned.lock() {
                    set.remove(&id);
                }
            }
        }
        _ => {
            eprintln!("redline-agent: {id} finished without a reply or a round");
            let _ = mcp(
                config,
                "reply",
                json!({
                    "session_id": id,
                    "message": "The agent finished without a reply or a new round; the review is still waiting for you."
                }),
            );
            let _ = claim_release(config, &id);
            if let Ok(mut set) = owned.lock() {
                set.remove(&id);
            }
        }
    }
}

/// A handoff job failed before or during the agent run. Report it on the rail and release the
/// claim; keep the worktree so a retry starts from the same commits.
fn fail_handoff_claim(
    config: &AgentConfig,
    id: &str,
    owned: &Arc<Mutex<HashSet<String>>>,
    message: String,
) {
    let _ = mcp(
        config,
        "handoff_status",
        json!({ "session_id": id, "state": "failed", "message": message }),
    );
    let _ = claim_release(config, id);
    if let Ok(mut set) = owned.lock() {
        set.remove(id);
    }
}

/// The branch a confirmed feature should hand off. A v0.21 review can carry `session/<id>` in
/// git while its meta was later renamed to `feat/...`; the commits live on the session branch,
/// so prefer it when the fancy name has no commits.
fn handoff_branch(repo: &Path, work_key: &str, id: &str) -> String {
    let branch_ref = format!("refs/heads/{work_key}");
    if git_ok(repo, &["show-ref", "--verify", "--quiet", &branch_ref]) {
        return work_key.to_string();
    }
    let legacy = format!("session/{id}");
    let legacy_ref = format!("refs/heads/{legacy}");
    if git_ok(repo, &["show-ref", "--verify", "--quiet", &legacy_ref]) {
        eprintln!("redline-agent: handing off {id} on the legacy branch {legacy}");
        return legacy;
    }
    work_key.to_string()
}

fn branch_exists(repo: &Path, branch: &str) -> bool {
    git_ok(
        repo,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
    ) || git_ok(
        repo,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/remotes/origin/{branch}"),
        ],
    )
}

fn handle_handoff_claimed(
    config: &AgentConfig,
    session: &Value,
    repo: &Path,
    work_key: &str,
    owned: &Arc<Mutex<HashSet<String>>>,
) {
    let id = session["id"].as_str().unwrap_or_default().to_string();
    if let Ok(mut set) = owned.lock() {
        set.insert(id.clone());
    }
    let branch = handoff_branch(repo, work_key, &id);
    eprintln!("redline-agent: handing off {id} on {branch}");
    if !branch_exists(repo, &branch) {
        fail_handoff_claim(
            config,
            &id,
            owned,
            format!("The branch {branch} has no commits to hand off; nothing was opened."),
        );
        return;
    }
    let worktree = match ensure_worktree(config, repo, &branch) {
        Ok(path) => path,
        Err(err) => {
            fail_handoff_claim(
                config,
                &id,
                owned,
                format!("I cannot start the handoff: {err}."),
            );
            return;
        }
    };
    let _ = mcp(
        config,
        "progress",
        json!({
            "session_id": id,
            "message": "Performing the confirmed feature's handoff: opening the pull request",
            "state": "step"
        }),
    );
    let prompt = handoff_prompt_for(session, &worktree, &branch);
    let output = match run_opencode(config, &id, &prompt, &worktree, true) {
        Ok((true, output)) => {
            eprintln!(
                "redline-agent: {id} handoff run finished ({} bytes)",
                output.len()
            );
            output
        }
        Ok((false, output)) => {
            eprintln!("redline-agent: {id} handoff run failed");
            fail_handoff_claim(
                config,
                &id,
                owned,
                format!("The handoff run failed: {}", first_line(&output)),
            );
            return;
        }
        Err(err) if err.code == "stopped" => {
            eprintln!("redline-agent: {id} handoff stopped by the human");
            // A stopped handoff must not leave the rail saying "handing off" forever. Report a
            // failure when the feature is still confirmed; if the confirmation was cleared the
            // server refuses the report and the UI derives a stale handoff from the dead claim.
            fail_handoff_claim(
                config,
                &id,
                owned,
                "Handoff stopped before it reported a result (the feature is no longer confirmed or the run was stopped).".to_string(),
            );
            return;
        }
        Err(err) => {
            eprintln!("redline-agent: {id} handoff bridge error: {err}");
            fail_handoff_claim(
                config,
                &id,
                owned,
                format!("Agent bridge error: {}", err.message),
            );
            return;
        }
    };
    if !output.is_empty() {
        eprintln!(
            "redline-agent: {id} handoff output tail: {}",
            tail(&output, 400)
        );
    }
    std::thread::sleep(Duration::from_secs(2));
    let meta = api_get(config, &format!("/api/sessions/{id}")).ok();
    let status = meta
        .as_ref()
        .and_then(|value| value["meta"]["handoff"]["status"].as_str())
        .unwrap_or_default()
        .to_string();
    match status.as_str() {
        "done" => {
            eprintln!("redline-agent: {id} handoff done");
            let _ = claim_release(config, &id);
            if let Ok(mut set) = owned.lock() {
                set.remove(&id);
            }
            release_worktree(config, repo, &branch);
        }
        "failed" => {
            eprintln!("redline-agent: {id} handoff failed; keeping {work_key} for a retry");
            let _ = claim_release(config, &id);
            if let Ok(mut set) = owned.lock() {
                set.remove(&id);
            }
        }
        _ => {
            eprintln!("redline-agent: {id} finished without a handoff result");
            fail_handoff_claim(
                config,
                &id,
                owned,
                "The handoff run finished without reporting done or failed.".to_string(),
            );
        }
    }
}

fn first_line(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("no output");
    if line.chars().count() <= 240 {
        return line.to_string();
    }
    format!("{}…", line.chars().take(240).collect::<String>())
}

fn tail(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    let length = trimmed.chars().count();
    if length <= limit {
        return trimmed.to_string();
    }
    trimmed.chars().skip(length - limit).collect()
}

/// Watch the server until `cancel` is set. The server hands out at most its global/per-project
/// limits; extra workers idle. Each claim runs in its own branch worktree.
pub fn run(config: AgentConfig, cancel: Arc<AtomicBool>) {
    let worker_count = config.workers.max(1);
    let owned = Arc::new(Mutex::new(HashSet::new()));
    let lease = api_get(&config, "/api/agent/settings")
        .ok()
        .and_then(|value| value["lease_seconds"].as_u64())
        .unwrap_or(120);
    let heartbeat_every = Duration::from_secs((lease / 3).clamp(5, 30));
    eprintln!(
        "redline-agent: watching {} with {worker_count} workers; agent {}, repo root {}",
        config.url,
        config.agent_id,
        config.repo_root.display()
    );

    let heartbeat_config = config.clone();
    let heartbeat_owned = owned.clone();
    let heartbeat_cancel = cancel.clone();
    let heartbeat = std::thread::spawn(move || {
        heartbeat_loop(
            heartbeat_config,
            heartbeat_owned,
            heartbeat_cancel,
            heartbeat_every,
        )
    });

    let mut handles = Vec::new();
    for index in 0..worker_count {
        let worker_config = config.clone();
        let worker_cancel = cancel.clone();
        let worker_owned = owned.clone();
        handles.push(std::thread::spawn(move || {
            worker_loop(worker_config, worker_cancel, worker_owned, index)
        }));
    }
    for handle in handles {
        let _ = handle.join();
    }
    let _ = heartbeat.join();
}

fn worker_loop(
    config: AgentConfig,
    cancel: Arc<AtomicBool>,
    owned: Arc<Mutex<HashSet<String>>>,
    index: usize,
) {
    let mut last_sync: Option<Instant> = None;
    while !cancel.load(Ordering::Relaxed) {
        if last_sync
            .map(|at| at.elapsed() >= config.repo_sync)
            .unwrap_or(true)
        {
            match sync_repos(&config) {
                Ok(count) => eprintln!("redline-agent: worker {index} synced {count} repositories"),
                Err(err) => {
                    eprintln!("redline-agent: worker {index} repository sync failed: {err}")
                }
            }
            last_sync = Some(Instant::now());
        }

        match claim_next(&config) {
            Ok(Some((session, claim, job))) => {
                let work_key = claim["work_key"].as_str().unwrap_or_default().to_string();
                let repo = match api_get(&config, "/api/projects")
                    .and_then(|projects| project_repo(&config, &projects, &session))
                {
                    Ok(repo) => repo,
                    Err(err) => {
                        let id = session["id"].as_str().unwrap_or_default();
                        if job == "handoff" {
                            fail_handoff_claim(
                                &config,
                                id,
                                &owned,
                                format!("I cannot resolve the project repository: {err}."),
                            );
                        } else {
                            fail_claim(
                                &config,
                                id,
                                &owned,
                                None,
                                format!("I cannot resolve the project repository: {err}."),
                            );
                        }
                        if wait(&cancel, config.poll) {
                            return;
                        }
                        continue;
                    }
                };
                handle_claimed(&config, &session, &repo, &work_key, &owned, &job);
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
            }
            Ok(None) => {}
            Err(err) => eprintln!("redline-agent: worker {index} cannot ask for work: {err}"),
        }

        if wait(&cancel, config.poll) {
            return;
        }
    }
}

fn heartbeat_loop(
    config: AgentConfig,
    owned: Arc<Mutex<HashSet<String>>>,
    cancel: Arc<AtomicBool>,
    every: Duration,
) {
    let mut last = Instant::now();
    while !cancel.load(Ordering::Relaxed) {
        if last.elapsed() >= every {
            last = Instant::now();
            let ids: Vec<String> = owned
                .lock()
                .map(|set| set.iter().cloned().collect())
                .unwrap_or_default();
            for id in ids {
                if let Err(err) = claim_heartbeat(&config, &id) {
                    // A transient HTTP error must not drop the only heartbeat for a long run.
                    // The worker removes the id when it releases the claim.
                    eprintln!("redline-agent: heartbeat failed for {id}: {err}");
                }
            }
        }
        std::thread::sleep(Duration::from_millis(500));
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

    #[cfg(unix)]
    fn fake_opencode(root: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let script = root.join(format!("fake-opencode-{name}"));
        std::fs::write(&script, format!("#!/bin/sh\n{body}\n")).unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).unwrap();
        script
    }

    #[cfg(unix)]
    #[test]
    fn a_run_is_killed_when_it_goes_silent_or_exceeds_the_hard_cap() {
        let root = temp_root("idle-timeout");

        let mut config = AgentConfig::new("http://127.0.0.1:1", "t", &root);
        config.idle_timeout = Duration::from_secs(2);
        config.timeout = Duration::from_secs(30);
        let pid_file = root.join("grandchild.pid");
        let idle_body = format!("sleep 30 & echo $! > {}; wait", pid_file.display());
        config.opencode_bin = fake_opencode(&root, "idle", &idle_body)
            .to_string_lossy()
            .to_string();
        let idle = run_opencode(&config, "idle", "prompt", &root, false).unwrap_err();
        assert_eq!(idle.code, "timeout_idle");
        assert!(idle.message.contains("no output"), "{}", idle.message);
        #[cfg(target_os = "linux")]
        {
            // The shell spawned `sleep` as a grandchild. It must die with the process group, not
            // survive the bridge and hold the Docker builder (the 2026-11-01 orphan). A killed
            // grandchild may remain in /proc as a zombie until the container's init reaps it;
            // that is dead, not running.
            let pid = std::fs::read_to_string(&pid_file)
                .expect("grandchild pid")
                .trim()
                .to_string();
            let dead = |pid: &str| {
                let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
                if stat.is_empty() {
                    return true;
                }
                let state = stat
                    .rsplit_once(") ")
                    .map(|(_, rest)| rest)
                    .unwrap_or("")
                    .split_whitespace()
                    .next()
                    .unwrap_or("");
                state == "Z"
            };
            let mut gone = false;
            for _ in 0..30 {
                if dead(&pid) {
                    gone = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            assert!(gone, "grandchild {pid} survived the process-group kill");
        }

        config.opencode_bin = fake_opencode(
            &root,
            "active",
            "for i in 1 2 3 4 5 6 7 8 9 10; do echo tick; sleep 0.4; done",
        )
        .to_string_lossy()
        .to_string();
        let active = run_opencode(&config, "active", "prompt", &root, false);
        assert!(active.is_ok(), "active output must keep the run alive");

        config.idle_timeout = Duration::from_secs(30);
        config.timeout = Duration::from_secs(1);
        config.opencode_bin = fake_opencode(
            &root,
            "hard",
            "for i in 1 2 3 4 5 6 7 8 9 10; do echo tick; sleep 0.3; done",
        )
        .to_string_lossy()
        .to_string();
        let hard = run_opencode(&config, "hard", "prompt", &root, false).unwrap_err();
        assert_eq!(hard.code, "timeout_hard");
        assert!(hard.message.contains("run limit"), "{}", hard.message);

        let _ = std::fs::remove_dir_all(&root);
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
        let prompt = prompt_for(
            &session,
            &feedback,
            Path::new("/tmp/repo"),
            "feat/prompt-test",
        );
        assert!(prompt.contains("20260101-120000-demo"));
        assert!(prompt.contains("RedLine UI"));
        assert!(prompt.contains("make it green"));
        assert!(prompt.contains("alignment"));
        assert!(prompt.contains("/tmp/repo"));
        assert!(prompt.contains("feat/prompt-test"));
        // The review run must never hand off a feature the human has not confirmed.
        assert!(prompt.contains("Do NOT open a pull request"));
        assert!(prompt.contains("separate handoff run"));
    }

    #[test]
    fn a_handoff_prompt_is_forge_agnostic_and_reports_through_mcp() {
        let session = json!({
            "id": "20260101-120000-handoff",
            "project": "RedLine UI",
            "prompt": "make it green",
            "handoff": {
                "status": "failed",
                "message": "checks are red",
                "url": "https://github.com/o/r/pull/7",
            },
        });
        let prompt = handoff_prompt_for(&session, Path::new("/tmp/repo"), "feat/handoff-test");
        assert!(prompt.contains("already confirmed"));
        assert!(prompt.contains("feat/handoff-test"));
        assert!(prompt.contains("/tmp/repo"));
        assert!(prompt.contains("Read the repository's AGENTS.md"));
        assert!(prompt.contains("documented flow"));
        assert!(prompt.contains("never open a second one"));
        assert!(prompt.contains("Do not merge on red"));
        assert!(prompt.contains("handoff_status"));
        assert!(prompt.contains("\"state\":\"done\""));
        assert!(prompt.contains("\"label\":\"merged\""));
        assert!(prompt.contains("checks are red"));
        assert!(prompt.contains("https://github.com/o/r/pull/7"));
        // The prompt must not prescribe a forge; the repository's AGENTS.md owns the commands.
        assert!(!prompt.contains("feature-ready"));
        assert!(!prompt.contains("gh pr create"));
        assert!(!prompt.contains("REDLINE_BIN"));
        assert!(!prompt.contains("as a **draft** first"));
        assert!(!prompt.contains("GitHub Actions"));
    }

    #[test]
    fn a_handoff_prefers_the_legacy_branch_that_holds_the_commits() {
        if !git_available() {
            eprintln!("git not installed; skipping handoff branch test");
            return;
        }
        let root = temp_root("handoff-branch");
        let repo = init_repo(&root);
        let id = "20260101-120000-demo";
        git_in(&repo, &["branch", &format!("session/{id}")]);
        assert_eq!(
            handoff_branch(&repo, "feat/handoff-test", id),
            format!("session/{id}")
        );
        git_in(&repo, &["branch", "feat/handoff-test"]);
        assert_eq!(
            handoff_branch(&repo, "feat/handoff-test", id),
            "feat/handoff-test"
        );
        let _ = std::fs::remove_dir_all(&root);
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

    fn git_available() -> bool {
        Command::new("git")
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    fn init_repo(root: &Path) -> PathBuf {
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        assert!(Command::new("git")
            .args(["init", "-q"])
            .arg(&repo)
            .status()
            .unwrap()
            .success());
        std::fs::write(repo.join("README.md"), "test\n").unwrap();
        assert!(Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["add", "."])
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args([
                "-c",
                "user.name=RedLine Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-qm",
                "init",
            ])
            .status()
            .unwrap()
            .success());
        repo
    }

    #[test]
    fn a_claim_gets_its_own_worktree_on_the_feature_branch() {
        if !git_available() {
            eprintln!("git not installed; skipping worktree test");
            return;
        }
        let root = temp_root("worktree");
        let repo = init_repo(&root);
        let mut config = AgentConfig::new("http://127.0.0.1:8788", "t", &repo);
        config.repo_root = root.clone();

        let path = ensure_worktree(&config, &repo, "feat/worktree-test").unwrap();
        assert_ne!(path, repo);
        assert!(path.join("README.md").is_file());
        let head = Command::new("git")
            .arg("-C")
            .arg(&path)
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&head.stdout).trim(),
            "feat/worktree-test"
        );

        let reused = ensure_worktree(&config, &repo, "feat/worktree-test").unwrap();
        assert_eq!(reused, path);
        let other = ensure_worktree(&config, &repo, "feat/other").unwrap();
        assert_ne!(other, path);

        release_worktree(&config, &repo, "feat/worktree-test");
        assert!(!path.exists());
        let branch = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["rev-parse", "--verify", "feat/worktree-test"])
            .output()
            .unwrap();
        assert!(branch.status.success());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Run git in `repo` the way the bridge does, failing the test with git's own stderr.
    fn git_in(repo: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args([
                "-c",
                "user.name=RedLine Test",
                "-c",
                "user.email=test@example.com",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn commit_of(repo: &Path) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// A tag that shares its name with the base branch (`dev` exists as a tag in this repo) must not
    /// hijack the feature branch: git cannot resolve the short name and points at the wrong commit.
    #[test]
    fn a_tag_named_like_the_base_branch_does_not_hijack_the_worktree() {
        if !git_available() {
            eprintln!("git not installed; skipping worktree test");
            return;
        }
        let root = temp_root("ambiguous-base");
        let repo = init_repo(&root);
        git_in(&repo, &["checkout", "-q", "-b", "dev"]);
        std::fs::write(repo.join("dev.txt"), "dev\n").unwrap();
        git_in(&repo, &["add", "."]);
        git_in(&repo, &["commit", "-qm", "dev work"]);
        let dev_tip = commit_of(&repo);
        git_in(&repo, &["tag", "dev", "HEAD~1"]);
        git_in(&repo, &["checkout", "-q", "--detach", "HEAD~1"]);

        let mut config = AgentConfig::new("http://127.0.0.1:8788", "t", &repo);
        config.repo_root = root.clone();
        let path = ensure_worktree(&config, &repo, "feat/ambiguous")
            .expect("an extra tag named dev must not break the worktree");
        assert_eq!(
            commit_of(&path),
            dev_tip,
            "the feature must start from dev's tip, not from the tag that shares its name"
        );
        let _ = std::fs::remove_dir_all(&root);
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
            std::fs::canonicalize(root.join("other")).unwrap()
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
            std::fs::canonicalize(root.join("redline")).unwrap()
        );
        assert!(resolve_repo(&config, "../escape").is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }

    fn count_markers(dir: &Path) -> usize {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return 0;
        };
        entries
            .flatten()
            .map(|entry| {
                let path = entry.path();
                if path.is_dir() {
                    count_markers(&path)
                } else if path.file_name().and_then(|name| name.to_str()) == Some(".redline-marker")
                {
                    1
                } else {
                    0
                }
            })
            .sum()
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn two_claims_run_in_separate_worktrees() {
        use redline_core::agent::AgentSettings;
        use redline_core::api::{router, AppState};
        use redline_core::auth::Auth;
        use redline_core::model::Submission;
        use redline_core::store::Store;

        if !git_available() {
            eprintln!("git not installed; skipping bridge integration test");
            return;
        }
        let root = temp_root("parallel-bridge");
        let repo = init_repo(&root);
        let store = Store::new(root.join("sessions"));
        store.ensure().unwrap();

        let png = b"\x89PNG\r\n\x1a\n";
        let submission = Submission {
            note: None,
            annotations: Vec::new(),
            png: None,
            width: None,
            height: None,
        };
        let mut session_ids = Vec::new();
        for branch in ["feat/parallel-a", "feat/parallel-b"] {
            let meta = store
                .create_from_bytes(png, "shot.png", branch, "", "", None, None, Some("p1"))
                .unwrap();
            store.set_branch(&meta.id, Some(branch)).unwrap();
            store.submit(&meta.id, &submission).unwrap();
            session_ids.push(meta.id);
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let url = format!("http://{address}");
        let state = AppState::new(store.clone(), Auth::new("test-token")).with_base_url(&url);
        state
            .agent
            .lock()
            .unwrap()
            .set_settings(AgentSettings {
                max_parallel: 2,
                project_max_parallel: 2,
                lease_seconds: 120,
            })
            .unwrap();
        let agent = state.agent.clone();
        let app = router(state);
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        // The fake agent proves it ran in a directory of its own; dirty worktrees are kept. It
        // also prints a line, which must reach the review as live output the human can open.
        let script = root.join("fake-opencode");
        std::fs::write(
            &script,
            "#!/bin/sh\npwd > .redline-marker\necho 'edited src/lib.rs'\nsleep 1\nexit 0\n",
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&script).unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&script, permissions).unwrap();
        }

        let mut config = AgentConfig::new(url, "test-token", &repo);
        config.repo_root = root.clone();
        config.opencode_bin = script.to_string_lossy().to_string();
        config.workers = 2;
        config.poll = Duration::from_millis(200);
        config.repo_sync = Duration::from_secs(1);
        config.timeout = Duration::from_secs(10);
        let cancel = Arc::new(AtomicBool::new(false));
        let bridge_cancel = cancel.clone();
        let bridge = std::thread::spawn(move || crate::run(config, bridge_cancel));

        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let claims = agent.lock().unwrap().claims();
            let replied = session_ids.iter().all(|id| {
                store
                    .get(id)
                    .unwrap()
                    .and_then(|meta| meta.agent_state)
                    .as_deref()
                    == Some("replied")
            });
            if claims.is_empty() && replied {
                break;
            }
            if Instant::now() >= deadline {
                panic!("bridge did not finish; claims={claims:?}");
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        cancel.store(true, Ordering::Relaxed);
        bridge.join().unwrap();
        assert_eq!(
            count_markers(&root.join(".redline-worktrees")),
            2,
            "each claim must get its own worktree"
        );
        // The agent's own output is visible in the review while it runs, not only on failure.
        let logged = session_ids.iter().any(|id| {
            store
                .agent_events(id)
                .unwrap_or_default()
                .iter()
                .any(|event| event["state"] == "log" && event["message"] == "edited src/lib.rs")
        });
        assert!(
            logged,
            "the run's output must reach the review as a log event"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
