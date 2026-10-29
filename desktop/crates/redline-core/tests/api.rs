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
    harness_with(None)
}

fn harness_with(webhook: Option<&str>) -> Harness {
    let dir = TempDir::new().unwrap();
    let store = Store::new(dir.path().join("sessions"));
    store.ensure().unwrap();
    let state =
        AppState::new(store.clone(), Auth::new(TOKEN)).with_base_url("http://127.0.0.1:8788");
    let state = state.with_config_path(dir.path().join("config.json"));
    let state = match webhook {
        Some(url) => state.with_webhook(url),
        None => state,
    };
    Harness {
        app: router(state),
        store,
        _dir: dir,
    }
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
    assert!(
        review_url.contains(&format!("/s/{id}?t={TOKEN}")),
        "{review_url}"
    );

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
    assert_eq!(
        session["annotations"]["annotations"][0]["text"],
        "Move this up"
    );
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
        authed(
            "POST",
            "/api/sessions",
            json!({"title": "t", "image_base64": PNG_B64}),
        ),
    )
    .await;
    let id = created["meta"]["id"].as_str().unwrap().to_string();

    let (status, headers, _) = send(&h.app, get(&format!("/s/{id}?t={TOKEN}"))).await;
    assert_eq!(status, StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(
        headers["location"],
        format!("/s/{id}"),
        "token stripped from the URL"
    );
    let cookie = headers["set-cookie"].to_str().unwrap();
    assert!(cookie.contains(&format!("redline_token={TOKEN}")));
    assert!(
        cookie.contains("HttpOnly"),
        "cookie must not be readable by JS: {cookie}"
    );
}

#[tokio::test]
async fn cross_site_and_bad_payloads_are_rejected() {
    let h = harness();
    let (_, _, created) = send(
        &h.app,
        authed(
            "POST",
            "/api/sessions",
            json!({"title": "t", "image_base64": PNG_B64}),
        ),
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
    for path in [
        "/api/sessions/nope",
        "/api/sessions/..%2F..%2Fetc",
        "/sessions/nope/image",
    ] {
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
        authed(
            "POST",
            "/api/sessions",
            json!({"title": "gone", "image_base64": PNG_B64}),
        ),
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
async fn sessions_can_be_resolved_and_reopened() {
    let h = harness();
    let (_, _, created) = send(
        &h.app,
        authed(
            "POST",
            "/api/sessions",
            json!({"title": "lifecycle", "image_base64": PNG_B64}),
        ),
    )
    .await;
    let id = created["meta"]["id"].as_str().unwrap().to_string();

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/sessions/{id}/resolve"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let (status, _, body) = send(&h.app, request).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["meta"]["status"], "resolved");
    assert!(body["meta"]["resolved_at"].is_string());

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/sessions/{id}/reopen"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let (status, _, body) = send(&h.app, request).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["meta"]["status"], "open");
    assert_eq!(body["meta"]["resolved_at"], serde_json::Value::Null);
}

#[tokio::test]
async fn history_diff_reports_what_changed_between_rounds() {
    let h = harness();
    let (_, _, created) = send(
        &h.app,
        authed(
            "POST",
            "/api/sessions",
            json!({"title": "diff", "image_base64": PNG_B64}),
        ),
    )
    .await;
    let id = created["meta"]["id"].as_str().unwrap().to_string();

    send(
        &h.app,
        authed(
            "POST",
            &format!("/api/sessions/{id}/annotations"),
            json!({"annotations": [
                {"type": "text", "id": "t1", "x": 1, "y": 1, "text": "before"},
                {"type": "rect", "id": "r1", "start": {"x": 0, "y": 0}, "end": {"x": 4, "y": 4}}
            ]}),
        ),
    )
    .await;
    send(
        &h.app,
        authed(
            "POST",
            &format!("/api/sessions/{id}/annotations"),
            json!({"annotations": [
                {"type": "text", "id": "t1", "x": 1, "y": 1, "text": "after"},
                {"type": "arrow", "id": "a1", "start": {"x": 9, "y": 9}, "end": {"x": 12, "y": 12}}
            ]}),
        ),
    )
    .await;

    let request = Request::builder()
        .uri(format!("/api/sessions/{id}/history"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let (_, _, body) = send(&h.app, request).await;
    let stamp = body["submissions"][0]["stamp"]
        .as_str()
        .unwrap()
        .to_string();

    let request = Request::builder()
        .uri(format!("/api/sessions/{id}/history/{stamp}/diff"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let (status, _, body) = send(&h.app, request).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["added"][0]["kind"], "arrow");
    assert_eq!(body["removed"][0]["kind"], "rect");
    assert_eq!(body["changed"][0]["kind"], "text");
    assert!(body["changed"][0]["after"]
        .as_str()
        .unwrap()
        .contains("after"));
    assert!(body["changed"][0]["before"]
        .as_str()
        .unwrap()
        .contains("before"));
}

#[tokio::test]
async fn resubmitting_archives_the_previous_round() {
    let h = harness();
    let (_, _, created) = send(
        &h.app,
        authed(
            "POST",
            "/api/sessions",
            json!({"title": "rounds", "image_base64": PNG_B64}),
        ),
    )
    .await;
    let id = created["meta"]["id"].as_str().unwrap().to_string();

    let submit = |annotations: serde_json::Value| {
        authed(
            "POST",
            &format!("/api/sessions/{id}/annotations"),
            json!({"annotations": annotations, "png": format!("data:image/png;base64,{PNG_B64}")}),
        )
    };

    send(
        &h.app,
        submit(json!([{"type": "text", "x": 1, "y": 1, "text": "first"}])),
    )
    .await;
    send(
        &h.app,
        submit(json!([
            {"type": "text", "x": 1, "y": 1, "text": "first"},
            {"type": "rect", "start": {"x": 0, "y": 0}, "end": {"x": 4, "y": 4}}
        ])),
    )
    .await;

    let request = Request::builder()
        .uri(format!("/api/sessions/{id}/history"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let (status, _, body) = send(&h.app, request).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let rounds = body["submissions"].as_array().unwrap();
    assert_eq!(rounds.len(), 1, "one previous round archived: {body}");
    assert_eq!(rounds[0]["annotations"], 1);
    assert_eq!(rounds[0]["png"], true);

    let stamp = rounds[0]["stamp"].as_str().unwrap().to_string();
    let request = Request::builder()
        .uri(format!("/sessions/{id}/history/{stamp}.json"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let response = h.app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(String::from_utf8_lossy(&body).contains("first"));
}

#[tokio::test]
async fn capture_without_a_browser_is_an_actionable_error() {
    let h = harness();
    let (status, _, body) = send(
        &h.app,
        authed(
            "POST",
            "/api/capture",
            json!({"url": "https://example.com"}),
        ),
    )
    .await;
    // On a host with Chromium this would create a session; without one it must explain itself.
    if status == StatusCode::BAD_REQUEST {
        let message = body["error"].as_str().unwrap_or_default();
        assert!(
            message.contains("Chromium") || message.contains("browser"),
            "{message}"
        );
    } else {
        assert_eq!(status, StatusCode::OK, "{body}");
    }
}

#[tokio::test]
async fn review_markdown_is_readable_over_http() {
    let h = harness();
    let (_, _, created) = send(
        &h.app,
        authed(
            "POST",
            "/api/sessions",
            json!({"title": "md", "image_base64": PNG_B64}),
        ),
    )
    .await;
    let id = created["meta"]["id"].as_str().unwrap().to_string();

    // Nothing to read before a submission.
    let request = Request::builder()
        .uri(format!("/sessions/{id}/review.md"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let (status, _, _) = send(&h.app, request).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    send(
        &h.app,
        authed(
            "POST",
            &format!("/api/sessions/{id}/annotations"),
            json!({"annotations": [{"type": "text", "x": 4, "y": 5, "text": "tighten this"}]}),
        ),
    )
    .await;

    let request = Request::builder()
        .uri(format!("/sessions/{id}/review.md"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let response = h.app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(content_type.contains("markdown"), "{content_type}");
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(String::from_utf8_lossy(&body).contains("- (4, 5): tighten this"));
}

#[tokio::test]
async fn session_list_carries_annotation_counts_for_the_rail() {
    let h = harness();
    let (_, _, created) = send(
        &h.app,
        authed(
            "POST",
            "/api/sessions",
            json!({"title": "counted", "image_base64": PNG_B64}),
        ),
    )
    .await;
    let id = created["meta"]["id"].as_str().unwrap().to_string();

    let request = Request::builder()
        .uri("/api/sessions")
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let (_, _, body) = send(&h.app, request).await;
    assert_eq!(body["sessions"][0]["annotation_count"], 0);

    send(
        &h.app,
        authed(
            "POST",
            &format!("/api/sessions/{id}/annotations"),
            json!({"annotations": [
                {"type": "text", "x": 1, "y": 2, "text": "one"},
                {"type": "rect", "start": {"x": 1, "y": 1}, "end": {"x": 5, "y": 5}}
            ]}),
        ),
    )
    .await;

    let request = Request::builder()
        .uri("/api/sessions")
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let (_, _, body) = send(&h.app, request).await;
    assert_eq!(body["sessions"][0]["annotation_count"], 2);
}

#[tokio::test]
async fn inline_css_and_js_are_served() {
    let dir = tempfile::TempDir::new().unwrap();
    let store = Store::new(dir.path().join("sessions"));
    store.ensure().unwrap();
    static FILES: &[(&str, &str)] = &[
        ("index.html", "<h1>x</h1>"),
        ("list.html", "<h1>y</h1>"),
        ("app.css", "body{color:red}"),
        ("app.js", "console.log(1)"),
    ];
    let app = router(
        AppState::new(store, Auth::new(TOKEN))
            .with_inline_ui(FILES)
            .with_base_url("http://127.0.0.1:8788"),
    );
    for (path, expected_type) in [
        ("/static/app.css", "text/css"),
        ("/static/app.js", "javascript"),
    ] {
        let response = app.clone().oneshot(get(path)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(
            content_type.contains(expected_type),
            "{path}: {content_type}"
        );
    }
}

#[tokio::test]
async fn config_endpoint_explains_how_to_connect_an_agent() {
    let h = harness();

    let (status, _, _) = send(&h.app, get("/api/config")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let request = Request::builder()
        .uri("/api/config")
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let (status, _, body) = send(&h.app, request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["mcp"]["url"], "http://127.0.0.1:8788/mcp");
    assert_eq!(
        body["mcp_config"]["mcpServers"]["redline"]["headers"]["Authorization"],
        format!("Bearer {TOKEN}")
    );
    assert!(body["cli"].as_str().unwrap().starts_with("redline open"));
}

#[tokio::test]
async fn setup_refuses_an_unreachable_or_wrong_token_before_saving() {
    let h = harness();
    let config = h._dir.path().join("config.json");

    // Wrong token: the probe must fail and nothing may be written.
    let (status, _, body) = send(
        &h.app,
        authed(
            "POST",
            "/api/setup",
            json!({"remote_url": "http://127.0.0.1:9", "token": "nope"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("could not use that server"));
    assert!(!config.exists(), "a failed probe must not persist a config");

    // A scheme the client cannot speak at all is rejected with an actionable message.
    let (status, _, body) = send(
        &h.app,
        authed(
            "POST",
            "/api/setup",
            json!({"remote_url": "ftp://holly", "token": "x"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("http:// or https://"));
}

#[tokio::test]
async fn setup_saves_a_server_that_answers_and_can_disconnect() {
    // A second redline standing in for "the server on holly".
    let dir = tempfile::TempDir::new().unwrap();
    let store = Store::new(dir.path().join("sessions"));
    store.ensure().unwrap();
    let remote_state =
        AppState::new(store, Auth::new("remote-token")).with_base_url("http://127.0.0.1:0");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let remote_url = format!("http://{}", listener.local_addr().unwrap());
    let remote_app = router(remote_state);
    tokio::spawn(async move {
        axum::serve(listener, remote_app).await.unwrap();
    });

    let h = harness();
    let config = h._dir.path().join("config.json");

    let (status, _, body) = send(
        &h.app,
        authed(
            "POST",
            "/api/setup",
            json!({"remote_url": remote_url, "token": "remote-token"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ok"], true);
    assert!(body["open_url"]
        .as_str()
        .unwrap()
        .contains("/?t=remote-token"));

    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
    assert_eq!(saved["remote_url"], remote_url.as_str());
    assert_eq!(saved["token"], "remote-token");

    let request = Request::builder()
        .uri("/api/setup")
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let (_, _, body) = send(&h.app, request).await;
    assert_eq!(body["mode"], "remote");
    assert_eq!(body["remote_url"], remote_url.as_str());

    let request = Request::builder()
        .method("POST")
        .uri("/api/setup/disconnect")
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let (status, _, body) = send(&h.app, request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["mode"], "local");
    assert!(!config.exists());
}

#[tokio::test]
async fn https_servers_are_accepted_without_a_plain_http_probe() {
    let h = harness();
    let config = h._dir.path().join("config.json");

    // No TLS client here, so the address is trusted and the webview is the judge.
    let (status, _, body) = send(
        &h.app,
        authed(
            "POST",
            "/api/setup",
            json!({"remote_url": "https://redline.ibolya.cz", "token": "t"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["open_url"], "https://redline.ibolya.cz/?t=t");
    assert!(config.exists());

    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
    assert_eq!(saved["remote_url"], "https://redline.ibolya.cz");
}

#[tokio::test]
async fn setup_is_unavailable_without_a_config_path() {
    let dir = tempfile::TempDir::new().unwrap();
    let store = Store::new(dir.path().join("sessions"));
    store.ensure().unwrap();
    let state = AppState::new(store, Auth::new(TOKEN)); // headless: no config file
    let app = router(state);
    let (status, _, body) = send(
        &app,
        authed(
            "POST",
            "/api/setup",
            json!({"remote_url": "http://x", "token": "y"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("desktop app"));
}

#[tokio::test]
async fn inline_ui_is_served_without_touching_the_filesystem() {
    let dir = tempfile::TempDir::new().unwrap();
    let store = Store::new(dir.path().join("sessions"));
    store.ensure().unwrap();
    static FILES: &[(&str, &str)] = &[
        ("index.html", "<h1>editor</h1>"),
        ("list.html", "<h1>sessions</h1>"),
    ];
    let state = AppState::new(store, Auth::new(TOKEN))
        .with_inline_ui(FILES)
        .with_base_url("http://127.0.0.1:8788");
    let app = router(state);

    for (path, expected) in [("/", "sessions"), ("/s/anything", "editor")] {
        let response = app.clone().oneshot(get(path)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert!(String::from_utf8_lossy(&bytes).contains(expected), "{path}");
    }
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
            .body(Body::from(
                json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // A notification has no id and must not produce a JSON-RPC response.
    let (status, _, _) = send(
        &h.app,
        authed(
            "POST",
            "/mcp",
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
}

/// The agent's `reply` used to land in `agent-thread.jsonl` with no route and no UI reading it,
/// so a human who submitted a review never saw an answer. The thread is now a first-class read.
#[tokio::test]
async fn agent_replies_are_readable_by_the_human() {
    let h = harness();
    let (_, _, created) = send(
        &h.app,
        authed(
            "POST",
            "/api/sessions",
            json!({"title": "Self review", "image_base64": PNG_B64}),
        ),
    )
    .await;
    let id = created["meta"]["id"].as_str().unwrap().to_string();

    let (status, _, replied) = send(
        &h.app,
        authed(
            "POST",
            "/mcp",
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {
                    "name": "reply",
                    "arguments": { "session_id": id, "message": "Looking at it now." }
                }
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{replied}");
    assert_eq!(replied["result"]["isError"], false);

    let request = Request::builder()
        .uri(format!("/api/sessions/{id}/thread"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let (status, _, thread) = send(&h.app, request).await;
    assert_eq!(status, StatusCode::OK, "{thread}");
    assert_eq!(thread["messages"][0]["message"], "Looking at it now.");

    // Same protection as every other data route.
    let (status, _, _) = send(&h.app, get(&format!("/api/sessions/{id}/thread"))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// W3: the human must be able to see that the agent read the review and what it is doing.
#[tokio::test]
async fn feedback_marks_received_and_progress_shows_activity() {
    let h = harness();
    let (_, _, created) = send(
        &h.app,
        authed(
            "POST",
            "/api/sessions",
            json!({"title": "Status", "image_base64": PNG_B64}),
        ),
    )
    .await;
    let id = created["meta"]["id"].as_str().unwrap().to_string();

    let (status, _, submitted) = send(
        &h.app,
        authed(
            "POST",
            &format!("/api/sessions/{id}/annotations"),
            json!({"annotations": [], "note": "make the button green"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{submitted}");
    assert_eq!(submitted["meta"]["prompt"], "make the button green");

    let (status, _, _) = send(
        &h.app,
        authed(
            "POST",
            "/mcp",
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": { "name": "get_feedback", "arguments": { "session_id": id } }
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let request = Request::builder()
        .uri(format!("/api/sessions/{id}"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let (_, _, session) = send(&h.app, request).await;
    assert_eq!(session["meta"]["agent_state"], "received");
    assert!(session["meta"]["seen_by_agent_at"].is_string());

    let (status, _, _) = send(
        &h.app,
        authed(
            "POST",
            "/mcp",
            json!({
                "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": { "name": "progress", "arguments": {
                    "session_id": id,
                    "message": "Editing app.css",
                    "state": "tool",
                    "tool": "edit"
                }}
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let request = Request::builder()
        .uri(format!("/api/sessions/{id}/events"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let (status, _, events) = send(&h.app, request).await;
    assert_eq!(status, StatusCode::OK, "{events}");
    assert_eq!(events["events"][0]["message"], "Editing app.css");
    assert_eq!(events["events"][0]["tool"], "edit");

    let request = Request::builder()
        .uri(format!("/api/sessions/{id}"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let (_, _, session) = send(&h.app, request).await;
    assert_eq!(session["meta"]["agent_state"], "working");
}

/// W5: a session can live in a project folder; sessions without one stay in the virtual Inbox.
#[tokio::test]
async fn sessions_can_live_in_a_project_folder() {
    let h = harness();
    let (status, _, created) = send(
        &h.app,
        authed(
            "POST",
            "/api/sessions",
            json!({"title": "In project", "image_base64": PNG_B64, "project": "RedLine UI"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let id = created["meta"]["id"].as_str().unwrap().to_string();
    assert_eq!(created["meta"]["project"], "redline-ui");
    assert!(h
        .store
        .projects_root()
        .join("redline-ui")
        .join(&id)
        .join("meta.json")
        .is_file());

    // A session without a project is still the Inbox.
    let (_, _, flat) = send(
        &h.app,
        authed(
            "POST",
            "/api/sessions",
            json!({"title": "Inbox", "image_base64": PNG_B64}),
        ),
    )
    .await;
    assert_eq!(flat["meta"]["project"], Value::Null);

    let request = Request::builder()
        .uri("/api/projects")
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let (status, _, projects) = send(&h.app, request).await;
    assert_eq!(status, StatusCode::OK, "{projects}");
    let items = projects["projects"].as_array().unwrap();
    let project = items
        .iter()
        .find(|item| item["slug"] == "redline-ui")
        .expect("project listed");
    assert_eq!(project["sessions"], 1);
    let inbox = items
        .iter()
        .find(|item| item["slug"] == "inbox")
        .expect("inbox listed");
    assert_eq!(inbox["sessions"], 1);

    // The project session is reachable by id even though it is nested one level deeper.
    let request = Request::builder()
        .uri(format!("/api/sessions/{id}"))
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let (status, _, session) = send(&h.app, request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(session["meta"]["project"], "redline-ui");
}

/// W6: a host can wake an agent up when the human submits.
#[tokio::test]
async fn submitting_fires_the_webhook() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            // The body can arrive in a second packet: read until Content-Length is satisfied.
            let mut buffer = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let read = stream.read(&mut chunk).unwrap_or(0);
                if read == 0 {
                    break;
                }
                buffer.extend_from_slice(&chunk[..read]);
                let Some(header_end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") else {
                    continue;
                };
                let head = String::from_utf8_lossy(&buffer[..header_end]);
                let length = head
                    .lines()
                    .find_map(|line| line.strip_prefix("Content-Length: "))
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if buffer.len() >= header_end + 4 + length {
                    break;
                }
            }
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
            let _ = sender.send(String::from_utf8_lossy(&buffer).to_string());
        }
    });

    let h = harness_with(Some(&format!("http://{address}/hook")));
    let (_, _, created) = send(
        &h.app,
        authed(
            "POST",
            "/api/sessions",
            json!({"title": "Wake the agent", "image_base64": PNG_B64}),
        ),
    )
    .await;
    let id = created["meta"]["id"].as_str().unwrap().to_string();

    let (status, _, _) = send(
        &h.app,
        authed(
            "POST",
            &format!("/api/sessions/{id}/annotations"),
            json!({"annotations": [], "note": "look here"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let request = receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("webhook called");
    assert!(request.contains("POST /hook"), "{request}");
    assert!(request.contains("\"event\":\"submitted\""), "{request}");
    assert!(request.contains(&id), "{request}");
}
