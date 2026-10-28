//! HTTP surface. Same routes as v0.1, plus `POST /api/sessions` and `POST /mcp`, all behind a
//! bearer token (or the editor cookie). The editor itself is unchanged.

use std::path::PathBuf;

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Path as RoutePath, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
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
    /// Directory holding `index.html` / `list.html` / `app.js`. `None` in tests and in
    /// headless mode, where only the JSON/MCP surface is exposed.
    pub ui: Option<PathBuf>,
    /// Advertised base URL, e.g. `http://127.0.0.1:8788`.
    pub base_url: String,
}

impl AppState {
    pub fn new(store: Store, auth: Auth) -> Self {
        Self { store, auth, ui: None, base_url: "http://127.0.0.1:8788".to_string() }
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
        format!("{}/s/{}?t={}", self.base_url.trim_end_matches('/'), id, self.auth.token())
    }
}

pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/api/sessions", get(list_sessions).post(create_session))
        .route("/api/sessions/{id}", get(get_session))
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
            return (StatusCode::FORBIDDEN, Json(json!({"error": "cross-site request"})))
                .into_response();
        }
    }
    if !matches!(*request.method(), Method::GET | Method::HEAD) && !request.headers().contains_key(header::CONTENT_TYPE)
    {
        return (StatusCode::UNSUPPORTED_MEDIA_TYPE, Json(json!({"error": "content-type required"})))
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
fn trade_token_for_cookie(headers: &HeaderMap, query: Option<&str>, path: &str, state: &AppState) -> Option<Response> {
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
    if let Some(response) = trade_token_for_cookie(request.headers(), request.uri().query(), "/", &state) {
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
    if let Some(response) = trade_token_for_cookie(request.headers(), request.uri().query(), &clean, &state) {
        return response;
    }
    serve_ui(&state, "index.html")
}

fn serve_ui(state: &AppState, name: &str) -> Response {
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
        HeaderValue::from_str(&guessed).unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

// ---------------------------------------------------------------- JSON API

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
            let raw = encoded.split_once(',').map(|(_, data)| data).unwrap_or(encoded);
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
    Ok(Json(json!({ "ok": true, "meta": meta, "annotations": annotations })))
}

async fn resolve(
    State(state): State<AppState>,
    RoutePath(id): RoutePath<String>,
) -> Result<Json<Value>, Error> {
    Ok(Json(json!({ "meta": state.store.resolve(&id)? })))
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
