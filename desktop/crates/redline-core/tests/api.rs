use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use redline_core::api::{router, AppState};
use redline_core::auth::Auth;
use redline_core::store::Store;
use serde_json::{json, Value};
use tempfile::TempDir;
use tower::ServiceExt;

const PNG_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
const TOKEN: &str = "test-token";

struct Harness {
    app: axum::Router,
    store: Store,
    _dir: TempDir,
}

fn harness() -> Harness {
    let dir = TempDir::new().unwrap();
    let store = Store::new(dir.path().join("sessions"));
    store.ensure().unwrap();
    let state = AppState::new(store.clone(), Auth::new(TOKEN)).with_base_url("http://127.0.0.1:8788");
    Harness { app: router(state), store, _dir: dir }
}

async fn send(app: &axum::Router, request: Request<Body>) -> (StatusCode, axum::http::HeaderMap, Value) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, headers, json)
}

fn get(uri: &str) -> Request<Body> {
    Request::builder().uri(uri).body(Body::empty()).unwrap()
}

fn authed(method: &str, uri: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

#[tokio::test]
async fn health_is_public_but_sessions_are_not() {
    let h = harness();

    let (status, _, body) = send(&h.app, get("/health")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);

    let (status, _, body) = send(&h.app, get("/api/sessions")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "no token => 401");
    assert_eq!(body["error"], "unauthorized");

    let request = Request::builder()
        .uri("/api/sessions")
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let (status, _, body) = send(&h.app, request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["sessions"], json!([]));
}

#[tokio::test]
async fn create_submit_and_read_back_round_trip() {
    let h = harness();

    let (status, _, created) = send(
        &h.app,
        authed(
            "POST",
            "/api/sessions",
            json!({
                "title": "Button / default",
                "url": "http://localhost:5016/button",
                "note": "Check hover",
                "image_base64": PNG_B64,
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let id = created["meta"]["id"].as_str().expect("id").to_string();
    let review_url = created["review_url"].as_str().unwrap();
    assert!(review_url.contains(&format!("/s/{id}?t={TOKEN}")), "{review_url}");

    // Anonymous access to the image is refused; the token opens it.
    let (status, _, _) = send(&h.app, get(&format!("/sessions/{id}/image"))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let request = Request::builder()
        .uri(format!("/sessions/{id}/image"))
        .header("cookie", format!("redline_token={TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let response = h.app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&bytes[..4], b"\x89PNG");

    // Submit the way the editor does: annotations + composited PNG.
    let (status, _, submitted) = send(
        &h.app,
        authed(
            "POST",
            &format!("/api/sessions/{id}/annotations"),
            json!({
                "annotations": [
                    {"type": "text", "x": 5, "y": 6, "text": "Move this up", "color": "#e11d48"},
                    {"type": "rect", "start": {"x": 1, "y": 2}, "end": {"x": 3, "y": 4}, "color": "#e11d48", "width": 3}
                ],
                "width": 1440,
                "height": 900,
                "png": format!("data:image/png;base64,{PNG_B64}")
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{submitted}");
    assert_eq!(submitted["meta"]["status"], "submitted");

    let (status, _, session) = send(
        &h.app,
        Request::builder()
            .uri(format!("/api/sessions/{id}"))
            .header("authorization", format!("Bearer {TOKEN}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(session["meta"]["status"], "submitted");
    assert_eq!(session["annotations"]["annotations"][0]["text"], "Move this up");
    assert_eq!(session["annotations"]["image"]["width"], 1440);

    let folder = h.store.dir(&id).unwrap();
    for name in ["annotations.json", "annotated.png", "review.md"] {
        assert!(folder.join(name).is_file(), "{name} missing");
    }
    let review = std::fs::read_to_string(folder.join("review.md")).unwrap();
    assert!(review.contains("- (5, 6): Move this up"), "{review}");

    // The annotated PNG is served for the human and for MCP resources.
    let (status, _, _) = send(
        &h.app,
        Request::builder()
            .uri(format!("/sessions/{id}/annotated.png"))
            .header("authorization", format!("Bearer {TOKEN}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn editor_url_trades_the_token_for_a_cookie() {
    let h = harness();
    let (_, _, created) = send(
        &h.app,
        authed("POST", "/api/sessions", json!({"title": "t", "image_base64": PNG_B64})),
    )
    .await;
    let id = created["meta"]["id"].as_str().unwrap().to_string();

    let (status, headers, _) = send(&h.app, get(&format!("/s/{id}?t={TOKEN}"))).await;
    assert_eq!(status, StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(headers["location"], format!("/s/{id}"), "token stripped from the URL");
    let cookie = headers["set-cookie"].to_str().unwrap();
    assert!(cookie.contains(&format!("redline_token={TOKEN}")));
    assert!(cookie.contains("HttpOnly"), "cookie must not be readable by JS: {cookie}");
}

#[tokio::test]
async fn cross_site_and_bad_payloads_are_rejected() {
    let h = harness();
    let (_, _, created) = send(
        &h.app,
        authed("POST", "/api/sessions", json!({"title": "t", "image_base64": PNG_B64})),
    )
    .await;
    let id = created["meta"]["id"].as_str().unwrap().to_string();

    // A web page the human visits must not be able to post annotations to localhost.
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/sessions/{id}/annotations"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("content-type", "application/json")
        .header("sec-fetch-site", "cross-site")
        .body(Body::from(json!({"annotations": []}).to_string()))
        .unwrap();
    let (status, _, _) = send(&h.app, request).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // A POST without a content type is refused before parsing.
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/sessions/{id}/annotations"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::from("{}"))
        .unwrap();
    let (status, _, _) = send(&h.app, request).await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);

    // Unknown and traversal-shaped ids are 404, never a filesystem read.
    for path in ["/api/sessions/nope", "/api/sessions/..%2F..%2Fetc", "/sessions/nope/image"] {
        let request = Request::builder()
            .uri(path)
            .header("authorization", format!("Bearer {TOKEN}"))
            .body(Body::empty())
            .unwrap();
        let (status, _, _) = send(&h.app, request).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn delete_removes_the_session_and_404s_afterwards() {
    let h = harness();
    let (_, _, created) = send(
        &h.app,
        authed("POST", "/api/sessions", json!({"title": "gone", "image_base64": PNG_B64})),
    )
    .await;
    let id = created["meta"]["id"].as_str().unwrap().to_string();
    assert!(h.store.dir(&id).unwrap().is_dir());

    let request = Request::builder()
        .method("DELETE")
        .uri(format!("/api/sessions/{id}"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let (status, _, body) = send(&h.app, request).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!h.store.dir(&id).unwrap().exists());

    let request = Request::builder()
        .uri(format!("/api/sessions/{id}"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let (status, _, _) = send(&h.app, request).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn mcp_endpoint_requires_the_token_and_answers_notifications_with_202() {
    let h = harness();

    let (status, _, _) = send(
        &h.app,
        Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("content-type", "application/json")
            .body(Body::from(json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}).to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // A notification has no id and must not produce a JSON-RPC response.
    let (status, _, _) = send(
        &h.app,
        authed("POST", "/mcp", json!({"jsonrpc": "2.0", "method": "notifications/initialized"})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
}
