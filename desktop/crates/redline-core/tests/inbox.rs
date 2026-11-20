//! The combined inbox: the desktop app's profile list merged into one read-only queue.
//!
//! Two things must stay true or the feature is worse than separate windows:
//! - every row keeps the server that owns it, and an unreachable server never breaks the list;
//! - a click goes back to the owning server (with *its* token), never to a URL the page supplied.

use axum::body::Body;
use axum::extract::State as AxumState;
use axum::http::{header, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get as route_get;
use http_body_util::BodyExt;
use redline_core::api::{router, AppState};
use redline_core::auth::Auth;
use redline_core::store::Store;
use serde_json::{json, Value};
use tempfile::TempDir;
use tower::ServiceExt;

const TOKEN: &str = "test-token";
const REMOTE_TOKEN: &str = "remote-token";
const PNG_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

#[derive(Clone)]
struct Stub {
    sessions: Value,
}

async fn stub_sessions(AxumState(stub): AxumState<Stub>, request: Request<Body>) -> Response {
    let auth = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if auth != format!("Bearer {REMOTE_TOKEN}") {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({ "error": "unauthorized" })),
        )
            .into_response();
    }
    axum::Json(json!({ "sessions": stub.sessions })).into_response()
}

/// A stand-in for one RedLine server. It answers `/api/sessions` only with the profile's token.
async fn start_stub(sessions: Value) -> String {
    let app = axum::Router::new()
        .route("/api/sessions", route_get(stub_sessions))
        .with_state(Stub { sessions });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

struct Harness {
    app: axum::Router,
    dir: TempDir,
}

fn harness() -> Harness {
    let dir = TempDir::new().unwrap();
    let store = Store::new(dir.path().join("sessions"));
    store.ensure().unwrap();
    let state = AppState::new(store, Auth::new(TOKEN))
        .with_base_url("http://127.0.0.1:8788")
        .with_config_path(dir.path().join("config.json"));
    Harness {
        app: router(state),
        dir,
    }
}

fn headless() -> Harness {
    let dir = TempDir::new().unwrap();
    let store = Store::new(dir.path().join("sessions"));
    store.ensure().unwrap();
    let state = AppState::new(store, Auth::new(TOKEN)).with_base_url("http://127.0.0.1:8788");
    Harness {
        app: router(state),
        dir,
    }
}

fn get(uri: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap()
}

async fn send(
    app: &axum::Router,
    request: Request<Body>,
) -> (StatusCode, axum::http::HeaderMap, Value) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, headers, json)
}

fn write_config(dir: &TempDir, stub_url: &str) {
    let down = format!("http://127.0.0.1:{}", free_port());
    let value = json!({
        "servers": [
            { "id": "alpha", "name": "Alpha", "url": stub_url, "token": REMOTE_TOKEN },
            { "id": "down", "name": "Down", "url": down, "token": "stale" }
        ],
        "active": "alpha"
    });
    std::fs::write(dir.path().join("config.json"), value.to_string()).unwrap();
}

fn remote_sessions() -> Value {
    json!([
        {
            "id": "20261120-100000-remote-one-aaaa",
            "title": "Remote one",
            "status": "submitted",
            "project": "alpha",
            "created_at": "2026-11-20T09:50:00+00:00",
            "submitted_at": "2026-11-20T10:00:00+00:00",
            "updated_at": "2026-11-20T10:00:00+00:00",
            "annotation_count": 2,
            "assigned_agent": "holly",
            "assigned_agent_online": true,
            "agent_running": true
        },
        {
            "id": "20261120-080000-remote-two-bbbb",
            "title": "Remote two",
            "status": "open",
            "project": "alpha",
            "created_at": "2026-11-20T08:00:00+00:00",
            "updated_at": "2026-11-20T09:00:00+00:00",
            "annotation_count": 0,
            "assigned_agent": "holly",
            "assigned_agent_online": true
        }
    ])
}

#[tokio::test]
async fn the_inbox_merges_every_profile_and_keeps_the_owning_server() {
    let stub = start_stub(remote_sessions()).await;
    let h = harness();
    write_config(&h.dir, &stub);

    // A local review proves the "This computer" queue is part of the same list.
    let (status, _, created) = send(
        &h.app,
        Request::builder()
            .method("POST")
            .uri("/api/sessions")
            .header("authorization", format!("Bearer {TOKEN}"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "title": "Local one",
                    "image_base64": PNG_B64,
                    "project": "local-project",
                })
                .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");

    let (status, _, body) = send(&h.app, get("/api/inbox")).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let servers = body["servers"].as_array().unwrap();
    assert_eq!(servers.len(), 3, "{body}");
    assert_eq!(servers[0]["id"], "local");
    assert_eq!(servers[0]["name"], "This computer");
    assert_eq!(servers[0]["online"], true);
    assert_eq!(servers[0]["sessions"], 1);

    let alpha = servers
        .iter()
        .find(|server| server["id"] == "alpha")
        .unwrap();
    assert_eq!(alpha["name"], "Alpha");
    assert_eq!(alpha["online"], true);
    assert_eq!(alpha["open"], 2);
    assert_eq!(alpha["waiting"], 1);

    let down = servers
        .iter()
        .find(|server| server["id"] == "down")
        .unwrap();
    assert_eq!(down["online"], false);
    assert!(
        down["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty()),
        "an offline server must say why: {down}"
    );

    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 3, "{body}");
    let remote = items
        .iter()
        .find(|item| item["session"]["id"] == "20261120-100000-remote-one-aaaa")
        .expect("the remote review is in the merged list");
    assert_eq!(remote["server"], "alpha");
    assert_eq!(remote["server_name"], "Alpha");
    assert_eq!(remote["session"]["title"], "Remote one");
    assert_eq!(remote["session"]["agent_running"], true);
    assert_eq!(
        remote["open_url"],
        "/api/inbox/open?server=alpha&session=20261120-100000-remote-one-aaaa"
    );
    assert!(
        !remote["open_url"].as_str().unwrap().contains(REMOTE_TOKEN),
        "the row URL must not carry the profile token"
    );
    let local = items
        .iter()
        .find(|item| item["server"] == "local")
        .expect("the local review is in the merged list");
    assert_eq!(local["session"]["title"], "Local one");

    // Newest first, by the same stamp the row prints.
    let stamps: Vec<i64> = items
        .iter()
        .map(|item| {
            let session = &item["session"];
            ["updated_at", "submitted_at", "created_at"]
                .iter()
                .find_map(|key| session.get(*key).and_then(Value::as_str))
                .and_then(|stamp| chrono::DateTime::parse_from_rfc3339(stamp).ok())
                .map(|stamp| stamp.timestamp_millis())
                .unwrap_or(0)
        })
        .collect();
    assert!(
        stamps.windows(2).all(|pair| pair[0] >= pair[1]),
        "the merged list must be newest first: {stamps:?}"
    );
}

#[tokio::test]
async fn a_click_goes_back_to_the_owning_server_with_its_own_token() {
    let stub = start_stub(remote_sessions()).await;
    let h = harness();
    write_config(&h.dir, &stub);

    let (status, headers, _) = send(
        &h.app,
        get("/api/inbox/open?server=alpha&session=20261120-100000-remote-one-aaaa"),
    )
    .await;
    assert_eq!(status, StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(
        headers.get(header::LOCATION).unwrap().to_str().unwrap(),
        format!("{stub}/s/20261120-100000-remote-one-aaaa?t={REMOTE_TOKEN}&rl_window=1")
    );

    let (status, headers, _) = send(
        &h.app,
        get("/api/inbox/open?server=local&session=20261120-100000-remote-one-aaaa"),
    )
    .await;
    assert_eq!(status, StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(
        headers.get(header::LOCATION).unwrap().to_str().unwrap(),
        format!("http://127.0.0.1:8788/s/20261120-100000-remote-one-aaaa?t={TOKEN}&rl_window=1")
    );

    let (status, _, body) = send(&h.app, get("/api/inbox/open?server=nope&session=x")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, _, body) = send(
        &h.app,
        get("/api/inbox/open?server=alpha&session=..%2F..%2Fetc"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn the_inbox_requires_the_owner_and_is_not_available_headless() {
    let h = harness();
    let (status, _, body) = send(
        &h.app,
        Request::builder()
            .uri("/api/inbox")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");

    let h = headless();
    let (status, _, body) = send(&h.app, get("/api/inbox")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("only available in the desktop app"));
    let (status, _, body) = send(
        &h.app,
        get("/api/inbox/open?server=local&session=20261120-100000-remote-one-aaaa"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}
