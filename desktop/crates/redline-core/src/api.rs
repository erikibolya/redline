//! HTTP surface. Same routes as v0.1, plus `POST /api/sessions` and `POST /mcp`, all behind a
//! bearer token (or the editor cookie). The editor itself is unchanged.

use std::path::PathBuf;

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Path as RoutePath, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::auth::{self, Auth};
use crate::error::Error;
use crate::mcp::{self, McpContext};
use crate::model::{NewSession, Submission};
use crate::store::Store;
use crate::MAX_BODY_BYTES;

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
}

impl AppState {
    pub fn new(store: Store, auth: Auth) -> Self {
        Self {
            store,
            auth,
            ui: None,
            inline_ui: None,
            base_url: "http://127.0.0.1:8788".to_string(),
            config_path: None,
        }
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
        .route("/api/sessions", get(list_sessions).post(create_session))
        .route(
            "/api/sessions/{id}",
            get(get_session).delete(delete_session),
        )
        .route("/api/sessions/{id}/annotations", post(submit))
        .route("/api/sessions/{id}/resolve", post(resolve))
        .route("/sessions/{id}/image", get(image))
        .route("/sessions/{id}/annotated.png", get(annotated))
        .route("/mcp", post(mcp_post))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_token));

    Router::new()
        .route("/health", get(health))
        .route("/", get(list_page))
        .route("/s/{id}", get(editor_page))
        .route("/setup", get(setup_page))
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
    let mut response = Redirect::temporary(path).into_response();
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
    let Some(dir) = &state.ui else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(relative) = path.strip_prefix("/static/") else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if relative.is_empty() || relative.contains("..") || relative.contains('\\') {
        return StatusCode::NOT_FOUND.into_response();
    }
    serve_file(&dir.join(relative), None)
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
    let sessions = state.store.list()?;
    Ok(Json(json!({ "sessions": sessions })))
}

async fn get_session(
    State(state): State<AppState>,
    RoutePath(id): RoutePath<String>,
) -> Result<Json<Value>, Error> {
    let meta = state.store.require(&id)?;
    let annotations = state.store.annotations(&id)?;
    Ok(Json(json!({ "meta": meta, "annotations": annotations })))
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

/// Shared by `POST /api/sessions` and the MCP `review_screenshot` tool.
pub fn create_from_request(
    state: &AppState,
    body: NewSession,
) -> crate::Result<crate::model::Meta> {
    use base64::Engine as _;

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

    state.store.create_from_bytes(
        &bytes,
        &name,
        &body.title,
        &body.url,
        &body.note,
        body.id.as_deref(),
        body.agent.as_deref(),
    )
}

async fn submit(
    State(state): State<AppState>,
    RoutePath(id): RoutePath<String>,
    Json(body): Json<Submission>,
) -> Result<Json<Value>, Error> {
    let (meta, annotations) = state.store.submit(&id, &body)?;
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
    Ok(serve_file(&state.store.image_path(&meta)?, None))
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

/// What the wizard shows: am I standalone or pointed at a server?
async fn setup_status(State(state): State<AppState>) -> Json<Value> {
    let config = read_config(&state);
    let remote = config
        .as_ref()
        .and_then(|value| value.get("remote_url"))
        .and_then(Value::as_str)
        .map(str::to_string);
    Json(json!({
        "mode": if remote.is_some() { "remote" } else { "local" },
        "remote_url": remote,
        "local_url": state.base_url,
        "sessions_dir": state.store.root().display().to_string(),
    }))
}

#[derive(Debug, serde::Deserialize)]
struct SetupRequest {
    remote_url: String,
    token: String,
}

/// Persist "use that server instead", but only after proving the credentials work — a saved
/// config that 401s on next launch is the worst possible first-run experience.
async fn save_setup(
    State(state): State<AppState>,
    Json(body): Json<SetupRequest>,
) -> Result<Json<Value>, Error> {
    let path = state.config_path.clone().ok_or_else(|| {
        Error::BadRequest("setup is only available in the desktop app".to_string())
    })?;

    let remote_url = body.remote_url.trim().trim_end_matches('/').to_string();
    if remote_url.is_empty() {
        return Err(Error::BadRequest("server URL is required".to_string()));
    }
    if !remote_url.starts_with("http://") && !remote_url.starts_with("https://") {
        return Err(Error::BadRequest(
            "use an http:// or https:// URL".to_string(),
        ));
    }
    // A plain-HTTP probe cannot validate a TLS endpoint; for https we let the webview be the
    // judge (a bad certificate shows up immediately) instead of refusing the address.
    let is_https = remote_url.starts_with("https://");
    let token = body.token.trim().to_string();
    if token.is_empty() {
        return Err(Error::BadRequest("token is required".to_string()));
    }

    let probe_url = remote_url.clone();
    let probe_token = token.clone();
    let reachable = if is_https {
        Ok(())
    } else {
        tokio::task::spawn_blocking(move || {
            crate::client::Client::new(&format!("{probe_url}/"), probe_token)
                .and_then(|client| client.get("/api/config"))
                .map(|_| ())
        })
        .await
        .map_err(|err| Error::BadRequest(format!("check failed: {err}")))?
    };

    if let Err(err) = reachable {
        return Err(Error::BadRequest(format!(
            "could not use that server: {err}"
        )));
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let config = json!({ "remote_url": remote_url, "token": token });
    let mut text = serde_json::to_string_pretty(&config)?;
    text.push('\n');
    std::fs::write(&path, text)?;

    Ok(Json(json!({
        "ok": true,
        "remote_url": remote_url,
        "open_url": format!("{remote_url}/?t={token}"),
    })))
}

async fn disconnect_setup(State(state): State<AppState>) -> Result<Json<Value>, Error> {
    let path = state.config_path.clone().ok_or_else(|| {
        Error::BadRequest("setup is only available in the desktop app".to_string())
    })?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(Json(
            json!({ "ok": true, "mode": "local", "local_url": state.base_url }),
        )),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Json(
            json!({ "ok": true, "mode": "local", "local_url": state.base_url }),
        )),
        Err(err) => Err(Error::Io(err)),
    }
}

fn read_config(state: &AppState) -> Option<Value> {
    let path = state.config_path.as_ref()?;
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
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
