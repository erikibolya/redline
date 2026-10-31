//! HTTP surface. Same routes as v0.1, plus `POST /api/sessions` and `POST /mcp`, all behind a
//! bearer token (or the editor cookie). The editor itself is unchanged.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Multipart, Path as RoutePath, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde_json::{json, Value};
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::agent::{AgentControl, AgentSettings, ClaimError, ClaimRequest, ClaimState};
use crate::auth::{self, Auth};
use crate::error::Error;
use crate::mcp::{self, McpContext};
use crate::model::{NewSession, Status, Submission};
use crate::store::Store;
use crate::MAX_BODY_BYTES;
use crate::MAX_VIDEO_BYTES;

#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    pub auth: Auth,
    /// Directory holding `index.html` / `list.html`. `None` in tests and in headless mode.
    pub ui: Option<PathBuf>,
    /// UI compiled into the binary (`include_str!`). The desktop app uses this so it never has
    /// to resolve a resource path at runtime; the files are small and self-contained.
    pub inline_ui: Option<&'static [(&'static str, &'static str)]>,
    /// Advertised base URL, e.g. `http://127.0.0.1:8788`.
    pub base_url: String,
    /// Where the desktop app stores its `config.json`. `None` in headless mode, which disables
    /// the setup endpoints (a server does not configure itself).
    pub config_path: Option<PathBuf>,
    /// `REDLINE_WEBHOOK_URL`: POSTed when the human submits (see `notify`).
    pub webhook: Option<String>,
    /// Server-authoritative agent limits and branch claims (v0.22).
    pub agent: Arc<Mutex<AgentControl>>,
}

impl AppState {
    pub fn new(store: Store, auth: Auth) -> Self {
        let agent = Arc::new(Mutex::new(AgentControl::load(store.data_dir())));
        Self {
            store,
            auth,
            ui: None,
            inline_ui: None,
            base_url: "http://127.0.0.1:8788".to_string(),
            config_path: None,
            webhook: None,
            agent,
        }
    }

    pub fn with_webhook(mut self, url: impl Into<String>) -> Self {
        self.webhook = Some(url.into());
        self
    }

    pub fn with_config_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.config_path = Some(path.into());
        self
    }

    pub fn with_inline_ui(mut self, files: &'static [(&'static str, &'static str)]) -> Self {
        self.inline_ui = Some(files);
        self
    }

    pub fn with_ui(mut self, ui: impl Into<PathBuf>) -> Self {
        self.ui = Some(ui.into());
        self
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    fn review_url(&self, id: &str) -> String {
        format!(
            "{}/s/{}?t={}",
            self.base_url.trim_end_matches('/'),
            id,
            self.auth.token()
        )
    }
}

pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/api/config", get(config))
        .route("/api/setup", get(setup_status).post(save_setup))
        .route("/api/setup/disconnect", post(disconnect_setup))
        .route("/api/servers", get(list_servers).post(upsert_server))
        .route("/api/servers/local/activate", post(activate_local))
        .route("/api/servers/{id}/activate", post(activate_server))
        .route("/api/servers/{id}/probe", get(probe_server))
        .route("/api/servers/{id}", delete(remove_server))
        .route("/api/sessions", get(list_sessions).post(create_session))
        .route(
            "/api/sessions/video",
            post(create_video_session).layer(DefaultBodyLimit::max(MAX_VIDEO_BYTES + 1024 * 1024)),
        )
        .route("/api/projects", get(list_projects).post(create_project))
        .route("/api/agent/repos", post(sync_agent_repos))
        .route(
            "/api/agent/settings",
            get(agent_settings).put(update_agent_settings),
        )
        .route("/api/agent/status", get(agent_status))
        .route("/api/agent/next", post(agent_next))
        .route("/api/agent/claims", post(claim_session))
        .route("/api/agent/claims/{id}/heartbeat", post(claim_heartbeat))
        .route("/api/agent/claims/{id}/state", post(claim_state))
        .route("/api/agent/claims/{id}/release", post(claim_release))
        .route(
            "/api/sessions/{id}",
            get(get_session).delete(delete_session),
        )
        .route("/api/sessions/{id}/branch", put(set_session_branch))
        .route("/api/sessions/{id}/annotations", post(submit))
        .route("/api/sessions/{id}/resolve", post(resolve))
        .route("/api/sessions/{id}/reopen", post(reopen))
        .route("/api/sessions/{id}/confirm-feature", post(confirm_feature))
        .route("/api/sessions/{id}/cancel-feature", post(cancel_feature))
        .route("/api/sessions/{id}/ship", post(request_ship))
        .route("/api/sessions/{id}/cancel", post(cancel))
        .route("/api/sessions/{id}/history", get(history))
        .route("/api/sessions/{id}/thread", get(agent_thread))
        .route("/api/sessions/{id}/events", get(agent_events))
        .route("/api/sessions/{id}/history/{stamp}/diff", get(history_diff))
        .route("/sessions/{id}/history/{file}", get(history_file))
        .route("/api/capture", post(capture_session))
        .route("/sessions/{id}/image", get(image))
        .route("/sessions/{id}/media", get(media))
        .route("/sessions/{id}/annotated.png", get(annotated))
        .route("/sessions/{id}/rounds/{name}", get(round_image))
        .route("/sessions/{id}/review.md", get(review_markdown_route))
        .route("/mcp", post(mcp_post))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_token))
        // The rail polls `/api/sessions`; a browser must never answer a decision with yesterday's
        // list. API/session responses are live data, not documents, so no-store is correct.
        .layer(middleware::from_fn(no_store));

    Router::new()
        .route("/health", get(health))
        .route("/", get(list_page))
        .route("/s/{id}", get(editor_page))
        .route("/setup", get(setup_page))
        .route("/servers", get(servers_page))
        .merge(protected)
        .fallback(static_file)
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(state)
}

// ---------------------------------------------------------------- auth middleware

async fn require_token(State(state): State<AppState>, request: Request, next: Next) -> Response {
    // Reject drive-by requests from a web page the human happens to have open. Browsers send this
    // header; non-browser agents do not, so absence is not treated as failure.
    if let Some(site) = request.headers().get("sec-fetch-site") {
        if site.as_bytes() == b"cross-site" {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({"error": "cross-site request"})),
            )
                .into_response();
        }
    }
    // A request that carries a body must declare its type; bodyless DELETEs need no header.
    let has_body = request
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .is_some_and(|length| length > 0)
        || request.headers().contains_key(header::TRANSFER_ENCODING);
    if has_body && !request.headers().contains_key(header::CONTENT_TYPE) {
        return (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Json(json!({"error": "content-type required"})),
        )
            .into_response();
    }

    let credential = auth::credential_from(request.headers(), request.uri().query());
    match credential {
        Some(token) if state.auth.verify(&token) => next.run(request).await,
        _ => (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "unauthorized", "hint": "Authorization: Bearer <token>" })),
        )
            .into_response(),
    }
}

/// Protected data (API JSON, images, history) is produced from mutable state and must not be
/// replayed by the browser cache. Images are cache-busted by `?v=<filename>`, so `no-store` costs
/// no correctness there either.
async fn no_store(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// The human-facing entry point: `GET /s/<id>?t=<token>` trades the token for a cookie so the
/// address bar, history and shared screenshots stay free of secrets.
fn trade_token_for_cookie(
    headers: &HeaderMap,
    query: Option<&str>,
    path: &str,
    state: &AppState,
) -> Option<Response> {
    let _ = headers;
    let token = query?
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == auth::QUERY_PARAM)
        .map(|(_, value)| value)?;
    if !state.auth.verify(token) {
        return None;
    }
    // Keep page-level parameters such as `new=1` or `unreachable=1`, but never echo the token.
    let mut target = path.to_string();
    if let Some(query) = query {
        let rest: Vec<&str> = query
            .split('&')
            .filter(|pair| !pair.is_empty() && !pair.starts_with("t="))
            .collect();
        if !rest.is_empty() {
            target.push('?');
            target.push_str(&rest.join("&"));
        }
    }
    let mut response = Redirect::temporary(&target).into_response();
    if let Ok(value) = HeaderValue::from_str(&auth::cookie_header(token)) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    Some(response)
}

// ---------------------------------------------------------------- pages

async fn list_page(State(state): State<AppState>, request: Request) -> Response {
    if let Some(response) =
        trade_token_for_cookie(request.headers(), request.uri().query(), "/", &state)
    {
        return response;
    }
    serve_ui(&state, "list.html")
}

async fn editor_page(
    State(state): State<AppState>,
    RoutePath(id): RoutePath<String>,
    request: Request,
) -> Response {
    if !crate::store::valid_id(&id) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let clean = format!("/s/{id}");
    if let Some(response) =
        trade_token_for_cookie(request.headers(), request.uri().query(), &clean, &state)
    {
        return response;
    }
    serve_ui(&state, "index.html")
}

async fn setup_page(State(state): State<AppState>, request: Request) -> Response {
    if let Some(response) =
        trade_token_for_cookie(request.headers(), request.uri().query(), "/setup", &state)
    {
        return response;
    }
    serve_ui(&state, "setup.html")
}

async fn servers_page(State(state): State<AppState>, request: Request) -> Response {
    if let Some(response) =
        trade_token_for_cookie(request.headers(), request.uri().query(), "/servers", &state)
    {
        return response;
    }
    serve_ui(&state, "servers.html")
}

fn serve_ui(state: &AppState, name: &str) -> Response {
    if let Some(files) = state.inline_ui {
        if let Some((_, body)) = files.iter().find(|(file, _)| *file == name) {
            return html_response(body);
        }
    }
    match &state.ui {
        Some(dir) => serve_file(&dir.join(name), None),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            "redline is running without its UI assets; use the JSON API or /mcp",
        )
            .into_response(),
    }
}

async fn static_file(State(state): State<AppState>, request: Request) -> Response {
    let path = request.uri().path();
    let Some(relative) = path.strip_prefix("/static/") else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if relative.is_empty() || relative.contains("..") || relative.contains('\\') {
        return StatusCode::NOT_FOUND.into_response();
    }
    // Assets compiled into the binary win; the directory is only for `--ui` overrides.
    if let Some(files) = state.inline_ui {
        if let Some((_, body)) = files.iter().find(|(name, _)| *name == relative) {
            return inline_asset(relative, body);
        }
    }
    match &state.ui {
        Some(dir) => serve_file(&dir.join(relative), None),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

fn inline_asset(name: &str, body: &'static str) -> Response {
    let content_type = mime_guess::from_path(name)
        .first_raw()
        .unwrap_or("application/octet-stream");
    let mut response = Response::new(Body::from(body));
    if let Ok(value) = HeaderValue::from_str(content_type) {
        response.headers_mut().insert(header::CONTENT_TYPE, value);
    }
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn html_response(body: &'static str) -> Response {
    let mut response = Response::new(Body::from(body));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn serve_file(path: &std::path::Path, content_type: Option<&str>) -> Response {
    let Ok(bytes) = std::fs::read(path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let guessed = content_type
        .map(str::to_string)
        .or_else(|| mime_guess::from_path(path).first_raw().map(str::to_string))
        .unwrap_or_else(|| "application/octet-stream".to_string());
    let mut response = Response::new(Body::from(bytes));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&guessed)
            .unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

// ---------------------------------------------------------------- JSON API

/// What a human needs to connect an agent: the MCP endpoint, a ready-to-paste config, and the
/// CLI equivalent. The desktop app renders this on its empty state so first run is self-explanatory.
async fn config(State(state): State<AppState>) -> Json<Value> {
    let base = state.base_url.trim_end_matches('/');
    let authorization = format!("Bearer {}", state.auth.token());
    Json(json!({
        "base_url": base,
        "mcp": {
            "url": format!("{base}/mcp"),
            "authorization": authorization,
        },
        "mcp_config": {
            "mcpServers": {
                "redline": {
                    "type": "http",
                    "url": format!("{base}/mcp"),
                    "headers": { "Authorization": authorization },
                }
            }
        },
        "cli": "redline open shot.png --title \"Button / default\"",
        "sessions_dir": state.store.root().display().to_string(),
    }))
}

async fn health() -> Json<Value> {
    Json(json!({ "ok": true, "version": crate::VERSION, "mcp": "/mcp" }))
}

async fn list_sessions(State(state): State<AppState>) -> Result<Json<Value>, Error> {
    // The rail wants a badge per session; computing it here saves N round-trips from the UI.
    let sessions = state.store.list()?;
    let items: Vec<Value> = sessions
        .into_iter()
        .map(|meta| {
            let count = state
                .store
                .annotations(&meta.id)
                .map(|set| set.annotations.len())
                .unwrap_or(0);
            let mut value = serde_json::to_value(&meta).unwrap_or(Value::Null);
            if let Some(object) = value.as_object_mut() {
                object.insert("annotation_count".to_string(), json!(count));
                object.insert("feature_state".to_string(), json!(meta.feature_state()));
            }
            value
        })
        .collect();
    Ok(Json(json!({ "sessions": items })))
}

async fn list_projects(State(state): State<AppState>) -> Result<Json<Value>, Error> {
    let projects = state.store.projects()?;
    let sessions = state.store.list()?;
    let default_project_max = state
        .agent
        .lock()
        .map(|agent| agent.settings().project_max_parallel)
        .unwrap_or(1);
    let items: Vec<Value> = projects
        .into_iter()
        .map(|project| {
            let mine: Vec<&crate::model::Meta> = sessions
                .iter()
                .filter(|meta| meta.project.as_deref() == Some(project.slug.as_str()))
                .collect();
            let waiting = mine
                .iter()
                .filter(|meta| meta.status.as_str() == "submitted")
                .count();
            let done = mine
                .iter()
                .filter(|meta| meta.status.as_str() == "resolved")
                .count();
            let last = mine
                .iter()
                .map(|meta| {
                    meta.submitted_at
                        .clone()
                        .unwrap_or_else(|| meta.created_at.clone())
                })
                .max();
            json!({
                "slug": project.slug,
                "name": project.name,
                "agent": project.agent,
                "repo": project.repo,
                "max_parallel": project.max_parallel,
                "effective_max_parallel": project.max_parallel.unwrap_or(default_project_max),
                "created_at": project.created_at,
                "sessions": mine.len(),
                "waiting": waiting,
                "done": done,
                "last_activity": last,
            })
        })
        .collect();
    Ok(Json(json!({ "projects": items })))
}

#[derive(Debug, serde::Deserialize)]
struct NewProject {
    name: String,
    #[serde(default)]
    agent: Option<String>,
}

async fn create_project(
    State(state): State<AppState>,
    Json(body): Json<NewProject>,
) -> Result<Json<Value>, Error> {
    let project = state.store.ensure_project(&body.name)?;
    Ok(Json(json!({ "project": project })))
}

#[derive(Debug, serde::Deserialize)]
struct RepoSync {
    agent_id: String,
    repos: Vec<RepoEntry>,
}

#[derive(Debug, serde::Deserialize)]
struct RepoEntry {
    key: String,
    #[serde(default)]
    name: String,
}

/// An agent reports the repositories it can see (OpenCode projects, filtered to git). RedLine
/// stores them as projects with a relative `repo` key so the same record works on every agent
/// mount. This endpoint only creates/updates; it never deletes reviews.
async fn sync_agent_repos(
    State(state): State<AppState>,
    Json(body): Json<RepoSync>,
) -> Result<Json<Value>, Error> {
    let agent = normalize_agent_id(&body.agent_id)?;
    // A repo sync is the bridge's periodic heartbeat (every 60 s), so it is the natural place to
    // record presence even when no review is claimed.
    state
        .agent
        .lock()
        .map_err(|_| Error::Conflict("agent state is locked".to_string()))?
        .touch(&agent);
    if body.repos.len() > 200 {
        return Err(Error::BadRequest(
            "too many repositories (max 200)".to_string(),
        ));
    }
    let mut seen = HashSet::new();
    let mut projects = Vec::new();
    for repo in body.repos {
        let key = normalize_repo_key(&repo.key)?;
        if !seen.insert(key.clone()) {
            continue;
        }
        let name: String = repo.name.trim().chars().take(120).collect::<String>();
        let project = state.store.sync_repo_project(&agent, &key, &name)?;
        projects.push(json!(project));
    }
    Ok(Json(json!({ "ok": true, "projects": projects })))
}

// ---------------------------------------------------------------- agent concurrency

#[derive(Debug, serde::Deserialize)]
struct AgentSettingsUpdate {
    #[serde(default)]
    max_parallel: Option<u32>,
    #[serde(default)]
    project_max_parallel: Option<u32>,
    #[serde(default)]
    lease_seconds: Option<u64>,
    #[serde(default)]
    projects: BTreeMap<String, Option<u32>>,
}

#[derive(Debug, serde::Deserialize)]
struct ClaimBody {
    session_id: String,
    agent_id: String,
    #[serde(default)]
    work_key: Option<String>,
    #[serde(default)]
    state: Option<ClaimState>,
}

#[derive(Debug, serde::Deserialize)]
struct AgentHeartbeat {
    agent_id: String,
}

#[derive(Debug, serde::Deserialize)]
struct ClaimStateBody {
    agent_id: String,
    state: ClaimState,
}

#[derive(Debug, serde::Deserialize)]
struct AgentRelease {
    agent_id: String,
}

#[derive(Debug, serde::Deserialize)]
struct NextBody {
    agent_id: String,
    /// Job kinds this bridge understands. Older bridges omit it and only ever receive review
    /// work; a new bridge sends `["review", "ship"]`, so a confirmed feature is not handed to a
    /// client that would treat it as another annotation round.
    #[serde(default)]
    jobs: Option<Vec<String>>,
}

fn claim_error(err: ClaimError) -> Error {
    match err {
        ClaimError::Invalid(message) => Error::BadRequest(message),
        other => Error::Conflict(other.to_string()),
    }
}

fn project_max(state: &AppState, project: &str, settings: &AgentSettings) -> u32 {
    state
        .store
        .project(project)
        .ok()
        .flatten()
        .and_then(|project| project.max_parallel)
        .unwrap_or(settings.project_max_parallel)
}

fn agent_projects(state: &AppState, settings: &AgentSettings) -> Result<Vec<Value>, Error> {
    Ok(state
        .store
        .projects()?
        .into_iter()
        .filter(|project| project.slug != "inbox")
        .map(|project| {
            json!({
                "slug": project.slug,
                "name": project.name,
                "repo": project.repo,
                "agent": project.agent,
                "max_parallel": project.max_parallel,
                "effective_max_parallel": project.max_parallel.unwrap_or(settings.project_max_parallel),
            })
        })
        .collect())
}

async fn agent_settings(State(state): State<AppState>) -> Result<Json<Value>, Error> {
    let (settings, status) = {
        let mut agent = state
            .agent
            .lock()
            .map_err(|_| Error::Conflict("agent state is locked".to_string()))?;
        (agent.settings().clone(), agent.status())
    };
    let projects = agent_projects(&state, &settings)?;
    Ok(Json(json!({
        "max_parallel": settings.max_parallel,
        "project_max_parallel": settings.project_max_parallel,
        "lease_seconds": settings.lease_seconds,
        "running": status.running,
        "waiting": status.waiting,
        "claims": status.claims,
        "projects": projects,
    })))
}

async fn update_agent_settings(
    State(state): State<AppState>,
    Json(body): Json<AgentSettingsUpdate>,
) -> Result<Json<Value>, Error> {
    for value in [body.max_parallel, body.project_max_parallel]
        .into_iter()
        .flatten()
    {
        if !(1..=32).contains(&value) {
            return Err(Error::BadRequest(
                "max_parallel and project_max_parallel must be between 1 and 32".to_string(),
            ));
        }
    }
    if let Some(value) = body.lease_seconds {
        if !(30..=3600).contains(&value) {
            return Err(Error::BadRequest(
                "lease_seconds must be between 30 and 3600".to_string(),
            ));
        }
    }
    for value in body.projects.values().flatten() {
        if !(1..=32).contains(value) {
            return Err(Error::BadRequest(
                "per-project max_parallel must be between 1 and 32".to_string(),
            ));
        }
    }

    for (slug, max_parallel) in &body.projects {
        if state.store.project(slug)?.is_none() {
            if slug == "inbox" {
                state.store.ensure_project("Inbox")?;
            } else {
                return Err(Error::NotFound(slug.clone()));
            }
        }
        state.store.set_project_max_parallel(slug, *max_parallel)?;
    }

    {
        let mut agent = state
            .agent
            .lock()
            .map_err(|_| Error::Conflict("agent state is locked".to_string()))?;
        let mut settings = agent.settings().clone();
        if let Some(value) = body.max_parallel {
            settings.max_parallel = value;
        }
        if let Some(value) = body.project_max_parallel {
            settings.project_max_parallel = value;
        }
        if let Some(value) = body.lease_seconds {
            settings.lease_seconds = value;
        }
        agent.set_settings(settings).map_err(Error::BadRequest)?;
    }

    agent_settings(State(state)).await
}

async fn agent_status(State(state): State<AppState>) -> Result<Json<Value>, Error> {
    let mut agent = state
        .agent
        .lock()
        .map_err(|_| Error::Conflict("agent state is locked".to_string()))?;
    Ok(Json(json!(agent.status())))
}

async fn claim_session(
    State(state): State<AppState>,
    Json(body): Json<ClaimBody>,
) -> Result<Json<Value>, Error> {
    let mut meta = state.store.require(&body.session_id)?;
    if meta.status != Status::Submitted {
        return Err(Error::Conflict(
            "only a submitted review can be claimed".to_string(),
        ));
    }
    let work_key = body
        .work_key
        .clone()
        .or_else(|| meta.branch.clone())
        .unwrap_or_else(|| meta.work_key());
    if meta.branch.as_deref() != Some(work_key.as_str()) {
        meta = state.store.set_branch(&meta.id, Some(&work_key))?;
    }
    let project = meta.project.clone().unwrap_or_else(|| "inbox".to_string());
    let mut agent = state
        .agent
        .lock()
        .map_err(|_| Error::Conflict("agent state is locked".to_string()))?;
    agent.touch(&body.agent_id);
    let settings = agent.settings().clone();
    let max_parallel = project_max(&state, &project, &settings);
    let claim = agent
        .try_claim(ClaimRequest {
            session_id: meta.id.clone(),
            agent_id: body.agent_id,
            project,
            work_key,
            state: body.state.unwrap_or(ClaimState::Running),
            global_max: settings.max_parallel,
            project_max: max_parallel,
            lease_seconds: settings.lease_seconds,
        })
        .map_err(claim_error)?;
    Ok(Json(json!({
        "claim": claim,
        "session": meta,
        "limits": { "max_parallel": settings.max_parallel, "project_max": max_parallel },
    })))
}

async fn claim_heartbeat(
    State(state): State<AppState>,
    RoutePath(id): RoutePath<String>,
    Json(body): Json<AgentHeartbeat>,
) -> Result<Json<Value>, Error> {
    let lease_seconds = {
        let agent = state
            .agent
            .lock()
            .map_err(|_| Error::Conflict("agent state is locked".to_string()))?;
        agent.settings().lease_seconds
    };
    let claim = {
        let mut agent = state
            .agent
            .lock()
            .map_err(|_| Error::Conflict("agent state is locked".to_string()))?;
        agent.touch(&body.agent_id);
        agent
            .heartbeat(&id, &body.agent_id, lease_seconds)
            .map_err(claim_error)?
    };
    Ok(Json(json!({ "claim": claim })))
}

async fn claim_state(
    State(state): State<AppState>,
    RoutePath(id): RoutePath<String>,
    Json(body): Json<ClaimStateBody>,
) -> Result<Json<Value>, Error> {
    let meta = state.store.require(&id)?;
    let project = meta.project.clone().unwrap_or_else(|| "inbox".to_string());
    let mut agent = state
        .agent
        .lock()
        .map_err(|_| Error::Conflict("agent state is locked".to_string()))?;
    let settings = agent.settings().clone();
    let max_parallel = project_max(&state, &project, &settings);
    agent.touch(&body.agent_id);
    let claim = agent
        .set_state(
            &id,
            &body.agent_id,
            body.state,
            settings.max_parallel,
            max_parallel,
            settings.lease_seconds,
        )
        .map_err(claim_error)?;
    Ok(Json(json!({ "claim": claim })))
}

async fn claim_release(
    State(state): State<AppState>,
    RoutePath(id): RoutePath<String>,
    Json(body): Json<AgentRelease>,
) -> Result<Json<Value>, Error> {
    let claim = {
        let mut agent = state
            .agent
            .lock()
            .map_err(|_| Error::Conflict("agent state is locked".to_string()))?;
        agent.touch(&body.agent_id);
        agent.release(&id, &body.agent_id).map_err(claim_error)?
    };
    Ok(Json(json!({ "ok": true, "claim": claim })))
}

async fn agent_next(
    State(state): State<AppState>,
    Json(body): Json<NextBody>,
) -> Result<Json<Value>, Error> {
    let mut sessions = state.store.list()?;
    sessions.retain(|meta| {
        meta.status == Status::Submitted
            && meta.agent_state.as_deref() != Some("working")
            && meta.agent_state.as_deref() != Some("replied")
    });
    sessions.sort_by_key(|meta| {
        meta.submitted_at
            .clone()
            .unwrap_or_else(|| meta.created_at.clone())
    });

    let mut agent = state
        .agent
        .lock()
        .map_err(|_| Error::Conflict("agent state is locked".to_string()))?;
    agent.touch(&body.agent_id);
    let settings = agent.settings().clone();
    let claims = agent.claims();
    let mut blocked = "empty";

    // Review work first: a human waiting for a fix should not queue behind a shipment.
    for mut meta in sessions {
        if let Some(claim) = claims.iter().find(|claim| claim.session_id == meta.id) {
            if claim.agent_id != body.agent_id {
                blocked = "claimed";
                continue;
            }
            if claim.state == ClaimState::Running {
                blocked = "running";
                continue;
            }
        }
        let project = meta.project.clone().unwrap_or_else(|| "inbox".to_string());
        let max_parallel = project_max(&state, &project, &settings);
        let work_key = meta.work_key();
        let request = ClaimRequest {
            session_id: meta.id.clone(),
            agent_id: body.agent_id.clone(),
            project,
            work_key: work_key.clone(),
            state: ClaimState::Running,
            global_max: settings.max_parallel,
            project_max: max_parallel,
            lease_seconds: settings.lease_seconds,
        };
        match agent.try_claim(request) {
            Ok(claim) => {
                if meta.branch.as_deref() != Some(claim.work_key.as_str()) {
                    meta = state.store.set_branch(&meta.id, Some(&claim.work_key))?;
                }
                return Ok(Json(json!({
                    "session": meta,
                    "claim": claim,
                    "job": "review",
                    "limits": { "max_parallel": settings.max_parallel, "project_max": max_parallel },
                })));
            }
            Err(ClaimError::AtCapacity { .. }) => blocked = "at_capacity",
            Err(ClaimError::BranchLocked { .. }) => blocked = "branch_locked",
            Err(ClaimError::SessionClaimed { .. }) => blocked = "claimed",
            Err(err) => return Err(claim_error(err)),
        }
    }

    let can_ship = body
        .jobs
        .as_ref()
        .is_some_and(|jobs| jobs.iter().any(|job| job == "ship"));
    if can_ship {
        let mut ships = state.store.list()?;
        ships.retain(|meta| {
            meta.feature_confirmed_at.is_some()
                && meta
                    .ship
                    .as_ref()
                    .is_some_and(|ship| ship.status.claimable())
        });
        ships.sort_by_key(|meta| {
            meta.ship
                .as_ref()
                .and_then(|ship| ship.requested_at.clone())
                .unwrap_or_else(|| meta.created_at.clone())
        });
        for meta in ships {
            let existing = claims.iter().find(|claim| claim.session_id == meta.id);
            if let Some(claim) = existing {
                if claim.agent_id != body.agent_id {
                    blocked = "claimed";
                    continue;
                }
                if claim.state == ClaimState::Running {
                    blocked = "running";
                    continue;
                }
            }
            let project = meta.project.clone().unwrap_or_else(|| "inbox".to_string());
            let max_parallel = project_max(&state, &project, &settings);
            // A waiting review claim carries the branch the agent actually worked on. It can be
            // a v0.21 `session/<id>` name even when the meta now reads as `feat/...`; shipping
            // must use the branch with the commits, not the derived label.
            let work_key = existing
                .map(|claim| claim.work_key.clone())
                .unwrap_or_else(|| meta.work_key());
            let request = ClaimRequest {
                session_id: meta.id.clone(),
                agent_id: body.agent_id.clone(),
                project,
                work_key,
                state: ClaimState::Running,
                global_max: settings.max_parallel,
                project_max: max_parallel,
                lease_seconds: settings.lease_seconds,
            };
            match agent.try_claim(request) {
                Ok(claim) => {
                    let meta = state.store.mark_ship_running(&meta.id).map_err(|err| {
                        let _ = agent.release(&meta.id, &body.agent_id);
                        err
                    })?;
                    return Ok(Json(json!({
                        "session": meta,
                        "claim": claim,
                        "job": "ship",
                        "limits": { "max_parallel": settings.max_parallel, "project_max": max_parallel },
                    })));
                }
                Err(ClaimError::AtCapacity { .. }) => blocked = "at_capacity",
                Err(ClaimError::BranchLocked { .. }) => blocked = "branch_locked",
                Err(ClaimError::SessionClaimed { .. }) => blocked = "claimed",
                Err(err) => return Err(claim_error(err)),
            }
        }
    }

    Ok(Json(json!({
        "session": null,
        "reason": blocked,
        "limits": { "max_parallel": settings.max_parallel },
    })))
}

fn normalize_agent_id(raw: &str) -> Result<String, Error> {
    let agent = raw.trim();
    if agent.is_empty()
        || agent.len() > 64
        || !agent
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(Error::BadRequest(
            "agent_id must be 1-64 characters of [a-z0-9_-]".to_string(),
        ));
    }
    Ok(agent.to_string())
}

fn normalize_repo_key(raw: &str) -> Result<String, Error> {
    let key = raw.trim();
    if key.is_empty()
        || key.len() > 200
        || key.starts_with('/')
        || key.contains('\\')
        || key.chars().any(char::is_control)
        || key
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(Error::BadRequest(format!(
            "invalid repo key {raw:?}: use a relative path without . or .."
        )));
    }
    Ok(key.to_string())
}

async fn get_session(
    State(state): State<AppState>,
    RoutePath(id): RoutePath<String>,
) -> Result<Json<Value>, Error> {
    let meta = state.store.require(&id)?;
    let annotations = state.store.annotations(&id)?;
    Ok(Json(json!({ "meta": meta, "annotations": annotations })))
}

#[derive(Debug, serde::Deserialize)]
struct BranchBody {
    #[serde(default)]
    branch: Option<String>,
}

/// Name the feature branch a review runs on.
///
/// A review created before v0.22 can still carry `session/<id>`, which is not a feature branch.
/// Renaming is refused while the round is submitted: the agent's worktree was already created for
/// the old name, so a rename would describe work that is happening somewhere else.
async fn set_session_branch(
    State(state): State<AppState>,
    RoutePath(id): RoutePath<String>,
    Json(body): Json<BranchBody>,
) -> Result<Json<Value>, Error> {
    let meta = state.store.require(&id)?;
    if meta.status == Status::Submitted {
        return Err(Error::Conflict(
            "stop the review before renaming its branch".to_string(),
        ));
    }
    if meta.ship.is_some() {
        return Err(Error::Conflict(
            "this feature already has a ship run; reopen it before renaming the branch".to_string(),
        ));
    }
    let branch = body
        .branch
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    Ok(Json(json!({
        "meta": state.store.set_branch(&id, branch)?,
    })))
}

async fn create_session(
    State(state): State<AppState>,
    Json(body): Json<NewSession>,
) -> Result<Json<Value>, Error> {
    let meta = create_from_request(&state, body)?;
    let review_url = state.review_url(&meta.id);
    Ok(Json(json!({
        "meta": meta,
        "review_url": review_url,
        "dir": state.store.dir(&meta.id)?.display().to_string(),
    })))
}

/// Browser video upload: multipart form with the video (`video`/`file`), optional `poster`, and
/// the same metadata fields as `POST /api/sessions`. The route has its own body limit; JSON
/// uploads stay bounded by `MAX_BODY_BYTES`.
async fn create_video_session(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<Value>, Error> {
    let mut fields = BTreeMap::<String, String>::new();
    let mut video: Option<Vec<u8>> = None;
    let mut poster: Option<Vec<u8>> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| Error::BadRequest(format!("invalid multipart upload: {err}")))?
    {
        let name = field.name().unwrap_or_default().to_string();
        if name == "video" || name == "file" {
            let bytes = field
                .bytes()
                .await
                .map_err(|err| Error::BadRequest(format!("cannot read video field: {err}")))?;
            if bytes.len() > MAX_VIDEO_BYTES {
                return Err(Error::BadRequest(format!(
                    "video is larger than {} MB",
                    MAX_VIDEO_BYTES / (1024 * 1024)
                )));
            }
            video = Some(bytes.to_vec());
        } else if name == "poster" {
            let bytes = field
                .bytes()
                .await
                .map_err(|err| Error::BadRequest(format!("cannot read poster field: {err}")))?;
            if bytes.len() > crate::MAX_POSTER_BYTES {
                return Err(Error::BadRequest(format!(
                    "poster is larger than {} MB",
                    crate::MAX_POSTER_BYTES / (1024 * 1024)
                )));
            }
            poster = Some(bytes.to_vec());
        } else {
            let value = field
                .text()
                .await
                .map_err(|err| Error::BadRequest(format!("cannot read {name}: {err}")))?;
            fields.insert(name, value);
        }
    }
    let video = video.ok_or_else(|| Error::BadRequest("provide a video file".to_string()))?;
    let field = |name: &str| {
        fields
            .get(name)
            .map(String::as_str)
            .filter(|v| !v.trim().is_empty())
    };
    let title = field("title").unwrap_or("Video review");
    let meta = state.store.create_video_from_bytes(
        &video,
        title,
        field("url").unwrap_or_default(),
        field("note").unwrap_or_default(),
        None,
        field("agent"),
        field("project"),
        poster.as_deref(),
        field("duration_ms").and_then(|value| value.parse::<u64>().ok()),
        field("width").and_then(|value| value.parse::<u32>().ok()),
        field("height").and_then(|value| value.parse::<u32>().ok()),
        field("fps").and_then(|value| value.parse::<f64>().ok()),
    )?;
    let meta = if let Some(branch) = field("branch") {
        state.store.set_branch(&meta.id, Some(branch))?
    } else {
        meta
    };
    let review_url = state.review_url(&meta.id);
    Ok(Json(json!({
        "meta": meta,
        "review_url": review_url,
        "dir": state.store.dir(&meta.id)?.display().to_string(),
    })))
}

/// Shared by `POST /api/sessions` and the MCP `review_screenshot` tool.
pub fn create_from_request(
    state: &AppState,
    body: NewSession,
) -> crate::Result<crate::model::Meta> {
    use base64::Engine as _;

    let has_video = body.video_base64.is_some() || body.video_path.is_some();
    if has_video {
        let bytes = match (body.video_base64.as_deref(), body.video_path.as_deref()) {
            (Some(encoded), _) => {
                let raw = encoded
                    .split_once(',')
                    .map(|(_, data)| data)
                    .unwrap_or(encoded);
                base64::engine::general_purpose::STANDARD
                    .decode(raw.trim())
                    .map_err(|err| Error::BadRequest(format!("invalid video_base64: {err}")))?
            }
            (None, Some(path)) => {
                let size = std::fs::metadata(path)?.len();
                if size as usize > crate::MAX_VIDEO_BYTES {
                    return Err(Error::BadRequest(format!(
                        "video is larger than {} MB",
                        crate::MAX_VIDEO_BYTES / (1024 * 1024)
                    )));
                }
                std::fs::read(path)?
            }
            (None, None) => unreachable!(),
        };
        if bytes.len() > crate::MAX_VIDEO_BYTES {
            return Err(Error::BadRequest(format!(
                "video is larger than {} MB",
                crate::MAX_VIDEO_BYTES / (1024 * 1024)
            )));
        }
        let poster = match body.poster_base64.as_deref() {
            Some(encoded) => {
                let raw = encoded
                    .split_once(',')
                    .map(|(_, data)| data)
                    .unwrap_or(encoded);
                Some(
                    base64::engine::general_purpose::STANDARD
                        .decode(raw.trim())
                        .map_err(|err| {
                            Error::BadRequest(format!("invalid poster_base64: {err}"))
                        })?,
                )
            }
            None => None,
        };
        let meta = state.store.create_video_from_bytes(
            &bytes,
            &body.title,
            &body.url,
            &body.note,
            body.id.as_deref(),
            body.agent.as_deref(),
            body.project.as_deref(),
            poster.as_deref(),
            body.duration_ms,
            body.width,
            body.height,
            body.fps,
        )?;
        return if let Some(branch) = body.branch.as_deref() {
            state.store.set_branch(&meta.id, Some(branch))
        } else {
            Ok(meta)
        };
    }

    let (bytes, name) = match (body.image_base64.as_deref(), body.image_path.as_deref()) {
        (Some(encoded), _) => {
            let raw = encoded
                .split_once(',')
                .map(|(_, data)| data)
                .unwrap_or(encoded);
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(raw.trim())
                .map_err(|err| Error::BadRequest(format!("invalid image_base64: {err}")))?;
            (bytes, "screenshot.png".to_string())
        }
        (None, Some(path)) => {
            let path = std::path::PathBuf::from(path);
            let bytes = std::fs::read(&path)?;
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("screenshot.png")
                .to_string();
            (bytes, name)
        }
        (None, None) => {
            return Err(Error::BadRequest(
                "provide image_base64 or image_path".to_string(),
            ))
        }
    };

    let meta = state.store.create_from_bytes(
        &bytes,
        &name,
        &body.title,
        &body.url,
        &body.note,
        body.id.as_deref(),
        body.agent.as_deref(),
        body.project.as_deref(),
    )?;
    if let Some(branch) = body.branch.as_deref() {
        state.store.set_branch(&meta.id, Some(branch))
    } else {
        Ok(meta)
    }
}

async fn submit(
    State(state): State<AppState>,
    RoutePath(id): RoutePath<String>,
    Json(body): Json<Submission>,
) -> Result<Json<Value>, Error> {
    let (meta, annotations) = state.store.submit(&id, &body)?;
    // Wake up whatever agent the host wired in; a dead endpoint must not fail the submission.
    if let Some(url) = state.webhook.clone() {
        let payload = json!({
            "event": "submitted",
            "session_id": meta.id,
            "title": meta.title,
            "project": meta.project,
            "prompt": meta.prompt,
            "annotations": annotations.annotations.len(),
            "submitted_at": meta.submitted_at,
            "review_url": state.review_url(&meta.id),
        });
        std::thread::spawn(move || crate::notify::post_json(&url, &payload));
    }
    Ok(Json(
        json!({ "ok": true, "meta": meta, "annotations": annotations }),
    ))
}

async fn resolve(
    State(state): State<AppState>,
    RoutePath(id): RoutePath<String>,
) -> Result<Json<Value>, Error> {
    Ok(Json(json!({ "meta": state.store.resolve(&id)? })))
}

async fn cancel(
    State(state): State<AppState>,
    RoutePath(id): RoutePath<String>,
) -> Result<Json<Value>, Error> {
    Ok(Json(json!({ "meta": state.store.cancel(&id)? })))
}

async fn reopen(
    State(state): State<AppState>,
    RoutePath(id): RoutePath<String>,
) -> Result<Json<Value>, Error> {
    Ok(Json(json!({ "meta": state.store.reopen(&id)? })))
}

async fn confirm_feature(
    State(state): State<AppState>,
    RoutePath(id): RoutePath<String>,
) -> Result<Json<Value>, Error> {
    Ok(Json(json!({ "meta": state.store.confirm_feature(&id)? })))
}

async fn cancel_feature(
    State(state): State<AppState>,
    RoutePath(id): RoutePath<String>,
) -> Result<Json<Value>, Error> {
    Ok(Json(json!({ "meta": state.store.cancel_feature(&id)? })))
}

/// Queue (or retry) a ship run for a feature the human already confirmed. Used by the rail's
/// retry action after a failed ship, and by scripts for features confirmed before v0.23.
async fn request_ship(
    State(state): State<AppState>,
    RoutePath(id): RoutePath<String>,
) -> Result<Json<Value>, Error> {
    Ok(Json(json!({ "meta": state.store.request_ship(&id)? })))
}

async fn history(
    State(state): State<AppState>,
    RoutePath(id): RoutePath<String>,
) -> Result<Json<Value>, Error> {
    state.store.require(&id)?;
    Ok(Json(json!({ "submissions": state.store.history(&id)? })))
}

/// What changed between an archived round and what is submitted now.
async fn history_diff(
    State(state): State<AppState>,
    RoutePath((id, stamp)): RoutePath<(String, String)>,
) -> Result<Json<Value>, Error> {
    state.store.require(&id)?;
    let path = state.store.history_file(&id, &format!("{stamp}.json"))?;
    let text = std::fs::read_to_string(&path)
        .map_err(|_| Error::NotFound(format!("{id}/history/{stamp}.json")))?;
    let previous: crate::model::AnnotationSet = serde_json::from_str(&text)
        .map_err(|err| Error::BadRequest(format!("archived round is unreadable: {err}")))?;
    let current = state.store.annotations(&id)?;
    let result = crate::diff::diff(&previous.annotations, &current.annotations);
    Ok(Json(json!({
        "from": { "stamp": stamp, "annotations": previous.annotations.len() },
        "to": { "stamp": "current", "annotations": current.annotations.len() },
        "added": result.added,
        "removed": result.removed,
        "changed": result.changed,
    })))
}

async fn history_file(
    State(state): State<AppState>,
    RoutePath((id, file)): RoutePath<(String, String)>,
) -> Result<Response, Error> {
    let path = state.store.history_file(&id, &file)?;
    if !path.is_file() {
        return Err(Error::NotFound(format!("{id}/history/{file}")));
    }
    let content_type = if file.ends_with(".json") {
        "application/json"
    } else {
        "image/png"
    };
    Ok(serve_file(&path, Some(content_type)))
}

/// Messages the agent attached with the MCP `reply` tool (`agent-thread.jsonl`).
///
/// Written by the agent, read by the human: without this route the file exists on disk and no
/// screen ever shows it, so a human who submitted a review had no way to see an answer.
async fn agent_thread(
    State(state): State<AppState>,
    RoutePath(id): RoutePath<String>,
) -> Result<Json<Value>, Error> {
    state.store.require(&id)?;
    let path = state.store.dir(&id)?.join("agent-thread.jsonl");
    let mut messages = Vec::new();
    if path.is_file() {
        let text = std::fs::read_to_string(&path)?;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            // A half-written line (the agent died mid-append) must not break the whole thread.
            if let Ok(entry) = serde_json::from_str::<Value>(line) {
                messages.push(entry);
            }
        }
    }
    Ok(Json(json!({ "messages": messages })))
}

/// What the agent said it is doing (`progress` → `agent-events.jsonl`). Newest last.
async fn agent_events(
    State(state): State<AppState>,
    RoutePath(id): RoutePath<String>,
) -> Result<Json<Value>, Error> {
    state.store.require(&id)?;
    let mut events = state.store.agent_events(&id)?;
    let total = events.len();
    if events.len() > 100 {
        events.drain(..total - 100);
    }
    Ok(Json(json!({ "events": events })))
}

#[derive(Debug, serde::Deserialize)]
struct CaptureRequest {
    url: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    note: String,
    #[serde(default)]
    viewport: Option<String>,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    branch: Option<String>,
}

/// Screenshot a URL server-side and open a review for it. Same engine as the MCP tool, so the
/// browser image (or REDLINE_CHROME) decides whether this works.
async fn capture_session(
    State(state): State<AppState>,
    Json(body): Json<CaptureRequest>,
) -> Result<Json<Value>, Error> {
    let url = body.url.trim().to_string();
    if url.is_empty() {
        return Err(Error::BadRequest("url is required".to_string()));
    }
    let (width, height) = match body.viewport.as_deref() {
        Some(viewport) if !viewport.trim().is_empty() => {
            crate::capture::parse_viewport(viewport).map_err(Error::BadRequest)?
        }
        _ => (1440, 900),
    };
    let request = crate::capture::CaptureRequest {
        url: url.clone(),
        width,
        height,
    };
    let png = tokio::task::spawn_blocking(move || crate::capture::capture(&request))
        .await
        .map_err(|err| Error::BadRequest(format!("capture task failed: {err}")))?
        .map_err(Error::BadRequest)?;

    let title = if body.title.trim().is_empty() {
        url.clone()
    } else {
        body.title.trim().to_string()
    };
    let meta = state.store.create_from_bytes(
        &png,
        "screenshot.png",
        &title,
        &url,
        &body.note,
        None,
        Some(body.agent.as_deref().unwrap_or("human")),
        body.project.as_deref(),
    )?;
    let meta = if let Some(branch) = body.branch.as_deref() {
        state.store.set_branch(&meta.id, Some(branch))?
    } else {
        meta
    };
    let review_url = state.review_url(&meta.id);
    Ok(Json(json!({ "meta": meta, "review_url": review_url })))
}

async fn delete_session(
    State(state): State<AppState>,
    RoutePath(id): RoutePath<String>,
) -> Result<Json<Value>, Error> {
    let dir = state.store.remove(&id)?;
    Ok(Json(
        json!({ "ok": true, "removed": dir.display().to_string() }),
    ))
}

async fn image(
    State(state): State<AppState>,
    RoutePath(id): RoutePath<String>,
) -> Result<Response, Error> {
    let meta = state.store.require(&id)?;
    let path =
        if meta.media.as_ref().map(|media| media.kind) == Some(crate::model::MediaKind::Video) {
            state.store.poster_path(&meta)?
        } else {
            state.store.image_path(&meta)?
        };
    Ok(serve_file(&path, None))
}

/// Stream the video with Range support so the browser can seek without downloading it all.
/// `ServeFile` also sets `Accept-Ranges`, `Content-Type` and the 206 `Content-Range`.
async fn media(
    State(state): State<AppState>,
    RoutePath(id): RoutePath<String>,
    request: Request,
) -> Result<Response, Error> {
    let meta = state.store.require(&id)?;
    if meta.media.as_ref().map(|media| media.kind) != Some(crate::model::MediaKind::Video) {
        return Err(Error::NotFound(format!("{id}/media")));
    }
    let path = state.store.image_path(&meta)?;
    if !path.is_file() {
        return Err(Error::NotFound(format!("{id}/media")));
    }
    let service = ServeFile::new(path);
    let mut proxy = Request::builder()
        .uri(request.uri())
        .body(Body::empty())
        .map_err(|err| Error::BadRequest(err.to_string()))?;
    *proxy.headers_mut() = request.headers().clone();
    let response = service
        .oneshot(proxy)
        .await
        .expect("ServeFile is infallible");
    let (parts, body) = response.into_parts();
    let mut response = Response::from_parts(parts, Body::new(body));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

async fn annotated(
    State(state): State<AppState>,
    RoutePath(id): RoutePath<String>,
) -> Result<Response, Error> {
    let path = state.store.file_path(&id, "annotated.png")?;
    if !path.is_file() {
        return Err(Error::NotFound(format!("{id}/annotated.png")));
    }
    Ok(serve_file(&path, Some("image/png")))
}

#[derive(Debug, Clone)]
struct ServerProfile {
    id: String,
    name: String,
    url: String,
    token: String,
}

fn normalize_url(raw: &str) -> String {
    raw.trim().trim_end_matches('/').to_string()
}

fn default_server_name(url: &str) -> String {
    let host = url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url);
    if host.is_empty() {
        url.to_string()
    } else {
        host.to_string()
    }
}

fn unique_server_id(base: &str, used: &HashSet<String>) -> String {
    let base = crate::store::slugify(base);
    let mut id = base.clone();
    let mut suffix = 2;
    while used.contains(&id) {
        id = format!("{base}-{suffix}");
        suffix += 1;
    }
    id
}

/// Read both the current profiles schema and the pre-profiles `remote_url`/`token` file.
fn servers_from_value(value: &Value) -> (Vec<ServerProfile>, String) {
    let mut servers = Vec::new();
    let mut used = HashSet::new();
    if let Some(items) = value.get("servers").and_then(Value::as_array) {
        for item in items {
            let url = normalize_url(item.get("url").and_then(Value::as_str).unwrap_or(""));
            if url.is_empty() {
                continue;
            }
            let name = item
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| default_server_name(&url));
            let token = item
                .get("token")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let base = item
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .unwrap_or(&name);
            let id = unique_server_id(base, &used);
            used.insert(id.clone());
            servers.push(ServerProfile {
                id,
                name,
                url,
                token,
            });
        }
    }
    let mut active = value
        .get("active")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    // Old clients wrote only these two fields. Keep those users connected on the first launch.
    if servers.is_empty() {
        if let Some(url) = value.get("remote_url").and_then(Value::as_str) {
            let url = normalize_url(url);
            if !url.is_empty() {
                let name = default_server_name(&url);
                let id = unique_server_id(&name, &used);
                servers.push(ServerProfile {
                    id: id.clone(),
                    name,
                    url,
                    token: value
                        .get("token")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                });
                active = id;
            }
        }
    }
    if active.is_empty()
        || (active != "local" && !servers.iter().any(|profile| profile.id == active))
    {
        active = servers
            .first()
            .map(|profile| profile.id.clone())
            .unwrap_or_else(|| "local".to_string());
    }
    (servers, active)
}

fn config_for_servers(servers: &[ServerProfile], active: &str) -> Value {
    let items: Vec<Value> = servers
        .iter()
        .map(|profile| {
            json!({
                "id": profile.id,
                "name": profile.name,
                "url": profile.url,
                "token": profile.token,
            })
        })
        .collect();
    let mut value = json!({ "servers": items, "active": active });
    if let Some(profile) = servers.iter().find(|profile| profile.id == active) {
        // Keep the old keys in sync so an older build can still connect.
        value["remote_url"] = json!(profile.url);
        value["token"] = json!(profile.token);
    }
    value
}

fn load_servers(state: &AppState) -> (Vec<ServerProfile>, String) {
    match read_config(state) {
        Some(value) => servers_from_value(&value),
        None => (Vec::new(), "local".to_string()),
    }
}

fn write_servers(state: &AppState, servers: &[ServerProfile], active: &str) -> Result<(), Error> {
    let path = state.config_path.clone().ok_or_else(|| {
        Error::BadRequest("server profiles are only available in the desktop app".to_string())
    })?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut text = serde_json::to_string_pretty(&config_for_servers(servers, active))?;
    text.push('\n');
    std::fs::write(path, text)?;
    Ok(())
}

fn server_public(profile: &ServerProfile) -> Value {
    json!({
        "id": profile.id,
        "name": profile.name,
        "url": profile.url,
        "has_token": !profile.token.is_empty(),
    })
}

fn active_open_url(state: &AppState, servers: &[ServerProfile], active: &str) -> String {
    servers
        .iter()
        .find(|profile| profile.id == active)
        .map(|profile| format!("{}/?t={}", profile.url, profile.token))
        .unwrap_or_else(|| format!("{}/?t={}", state.base_url, state.auth.token()))
}

async fn probe_http(url: &str, token: &str) -> Result<Value, String> {
    if url.starts_with("https://") {
        return Ok(json!({ "ok": null, "error": "https is trusted but not probed" }));
    }
    let url = url.to_string();
    let token = token.to_string();
    tokio::task::spawn_blocking(move || {
        let client =
            crate::client::Client::new(&format!("{url}/"), token).map_err(|err| err.to_string())?;
        let (status, health) = client.get("/health").map_err(|err| err.to_string())?;
        if status >= 400 {
            return Err(format!("HTTP {status}"));
        }
        Ok(json!({
            "ok": true,
            "version": health.get("version").and_then(Value::as_str).unwrap_or(""),
        }))
    })
    .await
    .map_err(|err| format!("probe task failed: {err}"))?
}

async fn put_server(state: &AppState, body: ServerRequest) -> Result<Value, Error> {
    if state.config_path.is_none() {
        return Err(Error::BadRequest(
            "server profiles are only available in the desktop app".to_string(),
        ));
    }
    let url = normalize_url(&body.url);
    if url.is_empty() {
        return Err(Error::BadRequest("server URL is required".to_string()));
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(Error::BadRequest(
            "use an http:// or https:// URL".to_string(),
        ));
    }
    let token = body.token.trim().to_string();
    if token.is_empty() {
        return Err(Error::BadRequest("token is required".to_string()));
    }
    probe_http(&url, &token)
        .await
        .map_err(|err| Error::BadRequest(format!("could not use that server: {err}")))?;

    let (mut servers, mut active) = load_servers(state);
    let name = body
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| default_server_name(&url));
    let requested_id = body
        .id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .unwrap_or(&name);
    // `local` is the synthetic "this computer" profile.
    let requested_id = if requested_id == "local" {
        "local-server"
    } else {
        requested_id
    };
    let id = if let Some(profile) = servers
        .iter_mut()
        .find(|profile| profile.id == requested_id)
    {
        profile.name = name;
        profile.url = url.clone();
        profile.token = token.clone();
        profile.id.clone()
    } else {
        let used: HashSet<String> = servers.iter().map(|profile| profile.id.clone()).collect();
        let id = unique_server_id(requested_id, &used);
        servers.push(ServerProfile {
            id: id.clone(),
            name,
            url: url.clone(),
            token: token.clone(),
        });
        id
    };
    if body.activate.unwrap_or(true) {
        active = id.clone();
    }
    let saved = servers
        .iter()
        .find(|profile| profile.id == id)
        .cloned()
        .ok_or_else(|| Error::NotFound(id.clone()))?;
    write_servers(state, &servers, &active)?;
    Ok(json!({
        "ok": true,
        "server": server_public(&saved),
        "active": active,
        "open_url": active_open_url(state, &servers, &active),
    }))
}

#[derive(Debug, serde::Deserialize)]
struct ServerRequest {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    url: String,
    token: String,
    #[serde(default)]
    activate: Option<bool>,
}

/// What the local Servers page reads. Tokens never leave the config file.
async fn list_servers(State(state): State<AppState>) -> Result<Json<Value>, Error> {
    if state.config_path.is_none() {
        return Err(Error::BadRequest(
            "server profiles are only available in the desktop app".to_string(),
        ));
    }
    let (servers, active) = load_servers(&state);
    let items: Vec<Value> = servers
        .iter()
        .map(|profile| {
            let mut item = server_public(profile);
            item["active"] = json!(profile.id == active);
            item
        })
        .collect();
    Ok(Json(json!({
        "servers": items,
        "active": active,
        "local_url": state.base_url,
        "active_name": servers.iter().find(|profile| profile.id == active).map(|profile| profile.name.clone()),
        // The local Servers page's "Back to sessions" must return to the *active* origin, not
        // to this local server: the local server has no reviews and no agent presence.
        "active_url": active_open_url(&state, &servers, &active),
    })))
}

async fn activate_local(State(state): State<AppState>) -> Result<Json<Value>, Error> {
    let (servers, _) = load_servers(&state);
    write_servers(&state, &servers, "local")?;
    Ok(Json(json!({
        "ok": true,
        "active": "local",
        "open_url": format!("{}/?t={}", state.base_url, state.auth.token()),
    })))
}

async fn activate_server(
    State(state): State<AppState>,
    RoutePath(id): RoutePath<String>,
) -> Result<Json<Value>, Error> {
    if id == "local" {
        return activate_local(State(state)).await;
    }
    let (servers, _) = load_servers(&state);
    let profile = servers
        .iter()
        .find(|profile| profile.id == id)
        .cloned()
        .ok_or_else(|| Error::NotFound(id.clone()))?;
    write_servers(&state, &servers, &id)?;
    Ok(Json(json!({
        "ok": true,
        "active": id,
        "open_url": format!("{}/?t={}", profile.url, profile.token),
    })))
}

async fn remove_server(
    State(state): State<AppState>,
    RoutePath(id): RoutePath<String>,
) -> Result<Json<Value>, Error> {
    if id == "local" {
        return Err(Error::BadRequest(
            "the local profile cannot be removed".to_string(),
        ));
    }
    let (mut servers, active) = load_servers(&state);
    let before = servers.len();
    servers.retain(|profile| profile.id != id);
    if servers.len() == before {
        return Err(Error::NotFound(id));
    }
    let next = if active == id {
        "local".to_string()
    } else {
        active
    };
    write_servers(&state, &servers, &next)?;
    Ok(Json(json!({ "ok": true, "active": next })))
}

async fn probe_server(
    State(state): State<AppState>,
    RoutePath(id): RoutePath<String>,
) -> Result<Json<Value>, Error> {
    let (servers, _) = load_servers(&state);
    let profile = servers
        .iter()
        .find(|profile| profile.id == id)
        .cloned()
        .ok_or_else(|| Error::NotFound(id))?;
    let probed = probe_http(&profile.url, &profile.token).await;
    match probed {
        Ok(value) => Ok(Json(value)),
        Err(err) => Ok(Json(json!({ "ok": false, "error": err }))),
    }
}

/// Kept for the old single-server wizard. It now appends to the profile list and activates it.
async fn setup_status(State(state): State<AppState>) -> Json<Value> {
    let (servers, active) = load_servers(&state);
    let current = servers.iter().find(|profile| profile.id == active);
    Json(json!({
        "mode": if current.is_some() { "remote" } else { "local" },
        "remote_url": current.map(|profile| profile.url.clone()),
        "active_name": current.map(|profile| profile.name.clone()),
        "servers": servers.len(),
        "local_url": state.base_url,
        "active_url": active_open_url(&state, &servers, &active),
        "sessions_dir": state.store.root().display().to_string(),
    }))
}

#[derive(Debug, serde::Deserialize)]
struct SetupRequest {
    remote_url: String,
    token: String,
    #[serde(default)]
    name: Option<String>,
}

async fn save_setup(
    State(state): State<AppState>,
    Json(body): Json<SetupRequest>,
) -> Result<Json<Value>, Error> {
    let result = put_server(
        &state,
        ServerRequest {
            id: None,
            name: body.name,
            url: body.remote_url,
            token: body.token,
            activate: Some(true),
        },
    )
    .await?;
    Ok(Json(result))
}

async fn disconnect_setup(State(state): State<AppState>) -> Result<Json<Value>, Error> {
    let (servers, _) = load_servers(&state);
    write_servers(&state, &servers, "local")?;
    Ok(Json(json!({
        "ok": true,
        "mode": "local",
        "local_url": state.base_url,
    })))
}

async fn upsert_server(
    State(state): State<AppState>,
    Json(body): Json<ServerRequest>,
) -> Result<Json<Value>, Error> {
    Ok(Json(put_server(&state, body).await?))
}

fn read_config(state: &AppState) -> Option<Value> {
    let path = state.config_path.as_ref()?;
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Exactly what the agent reads, for the human to check before/after submitting.
/// Screenshots posted by the agent as later rounds of the same review.
async fn round_image(
    State(state): State<AppState>,
    RoutePath((id, name)): RoutePath<(String, String)>,
) -> Result<Response, Error> {
    if name.contains('/') || name.contains('\\') || name.contains("..") || !name.ends_with(".png") {
        return Err(Error::BadRequest("invalid round image".to_string()));
    }
    let path = state.store.dir(&id)?.join(&name);
    if !path.is_file() {
        return Err(Error::NotFound(format!("{id}/{name}")));
    }
    Ok(serve_file(&path, Some("image/png")))
}

async fn review_markdown_route(
    State(state): State<AppState>,
    RoutePath(id): RoutePath<String>,
) -> Result<Response, Error> {
    let path = state.store.file_path(&id, "review.md")?;
    if !path.is_file() {
        return Err(Error::NotFound(format!(
            "{id}/review.md (not submitted yet)"
        )));
    }
    Ok(serve_file(&path, Some("text/markdown; charset=utf-8")))
}

async fn mcp_post(State(state): State<AppState>, Json(request): Json<Value>) -> Response {
    let context = McpContext {
        store: &state.store,
        base_url: &state.base_url,
        token: state.auth.token(),
    };
    match mcp::handle(&context, &request).await {
        Some(response) => Json(response).into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

#[cfg(test)]
mod server_profile_tests {
    use super::*;

    #[test]
    fn legacy_config_becomes_one_profile() {
        let (servers, active) = servers_from_value(&json!({
            "remote_url": "https://redline.example/",
            "token": "secret",
        }));
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].url, "https://redline.example");
        assert_eq!(servers[0].token, "secret");
        assert_eq!(active, servers[0].id);
    }

    #[test]
    fn profiles_and_all_tokens_survive_a_switch() {
        let value = json!({
            "servers": [
                { "id": "production", "name": "Production", "url": "https://prod.example", "token": "p" },
                { "id": "dev", "name": "Dev", "url": "http://dev.example", "token": "d" }
            ],
            "active": "dev"
        });
        let (servers, active) = servers_from_value(&value);
        assert_eq!(servers.len(), 2);
        assert_eq!(active, "dev");
        assert_eq!(servers[0].token, "p");
        assert_eq!(servers[1].token, "d");

        let saved = config_for_servers(&servers, "local");
        assert_eq!(saved["active"], "local");
        assert!(saved.get("remote_url").is_none());
        assert_eq!(saved["servers"].as_array().unwrap().len(), 2);

        let saved = config_for_servers(&servers, "production");
        assert_eq!(saved["remote_url"], "https://prod.example");
        assert_eq!(saved["token"], "p");
    }

    #[test]
    fn duplicate_names_get_distinct_ids() {
        let used = HashSet::new();
        let first = unique_server_id("Production", &used);
        let mut used = used;
        used.insert(first.clone());
        let second = unique_server_id("Production", &used);
        assert_ne!(first, second);
        assert_eq!(first, "production");
        assert_eq!(second, "production-2");
    }
}
