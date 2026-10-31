use redline_core::mcp::{handle, McpContext};
use redline_core::model::Submission;
use redline_core::store::Store;
use serde_json::{json, Value};
use tempfile::TempDir;

const PNG_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
const TOKEN: &str = "mcp-token";

fn setup() -> (TempDir, Store) {
    let dir = TempDir::new().unwrap();
    let store = Store::new(dir.path().join("sessions"));
    store.ensure().unwrap();
    (dir, store)
}

/// MCP wraps tool payloads in a text block; this unwraps it back to JSON.
fn payload(result: &Value) -> Value {
    let text = result["content"][0]["text"].as_str().expect("text content");
    serde_json::from_str(text).unwrap_or_else(|_| json!(text))
}

async fn call(store: &Store, name: &str, arguments: Value) -> Value {
    let context = McpContext {
        store,
        base_url: "http://127.0.0.1:8788",
        token: TOKEN,
    };
    let request = json!({
        "jsonrpc": "2.0", "id": 7, "method": "tools/call",
        "params": { "name": name, "arguments": arguments }
    });
    let response = handle(&context, &request).await.expect("response");
    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 7);
    response["result"].clone()
}

#[tokio::test]
async fn initialize_and_tool_list_are_well_formed() {
    let (_dir, store) = setup();
    let context = McpContext {
        store: &store,
        base_url: "http://127.0.0.1:8788",
        token: TOKEN,
    };

    let init = handle(
        &context,
        &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize"}),
    )
    .await
    .unwrap();
    assert_eq!(
        init["result"]["protocolVersion"],
        redline_core::MCP_PROTOCOL_VERSION
    );
    assert_eq!(init["result"]["serverInfo"]["name"], "redline");
    assert_eq!(
        init["result"]["capabilities"]["tools"]["listChanged"],
        false
    );

    let tools = handle(
        &context,
        &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    )
    .await
    .unwrap();
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"review_screenshot"));
    assert!(names.contains(&"review_video"));
    assert!(names.contains(&"wait_for_feedback"));
    assert!(names.contains(&"get_feedback"));
    assert!(names.contains(&"list_reviews"));
    assert!(names.contains(&"resolve"));
    assert!(names.contains(&"ship_status"));

    // Notifications get no response at all.
    let notification = handle(
        &context,
        &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    )
    .await;
    assert!(notification.is_none());
}

#[tokio::test]
async fn review_screenshot_creates_a_session_the_human_can_open() {
    let (_dir, store) = setup();
    let result = call(
        &store,
        "review_screenshot",
        json!({
            "title": "Button / default",
            "note": "Check hover",
            "image_base64": format!("data:image/png;base64,{PNG_B64}")
        }),
    )
    .await;
    assert_eq!(result["isError"], false);

    let created = payload(&result);
    let id = created["session_id"].as_str().unwrap().to_string();
    assert_eq!(created["status"], "open");
    assert!(created["review_url"]
        .as_str()
        .unwrap()
        .contains(&format!("/s/{id}?t={TOKEN}")));

    let meta = store.require(&id).unwrap();
    assert_eq!(meta.title, "Button / default");
    assert_eq!(meta.agent.as_deref(), Some("mcp"));
    assert!(
        store.image_path(&meta).unwrap().is_file(),
        "screenshot written"
    );
}

#[tokio::test]
async fn ship_status_records_the_pr_only_for_a_confirmed_feature() {
    let (_dir, store) = setup();
    let created = payload(
        &call(
            &store,
            "review_screenshot",
            json!({
                "title": "ship me",
                "image_base64": format!("data:image/png;base64,{PNG_B64}")
            }),
        )
        .await,
    );
    let id = created["session_id"].as_str().unwrap().to_string();

    let refused = call(
        &store,
        "ship_status",
        json!({
            "session_id": id,
            "state": "merged",
            "pr_url": "https://github.com/o/r/pull/7"
        }),
    )
    .await;
    assert_eq!(refused["isError"], true, "{refused}");
    assert!(refused["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("not confirmed"));

    store.confirm_feature(&id).unwrap();
    let opened = call(
        &store,
        "ship_status",
        json!({
            "session_id": id,
            "state": "opened",
            "pr_url": "https://github.com/o/r/pull/7",
            "pr_number": 7
        }),
    )
    .await;
    assert_eq!(opened["isError"], false, "{opened}");
    assert_eq!(payload(&opened)["ship"]["status"], "opened");

    // A merged report keeps the PR and adds the merge commit.
    let merged = call(
        &store,
        "ship_status",
        json!({
            "session_id": id,
            "state": "merged",
            "merge_commit": "abc123"
        }),
    )
    .await;
    assert_eq!(merged["isError"], false, "{merged}");
    let ship = payload(&merged)["ship"].clone();
    assert_eq!(ship["status"], "merged");
    assert_eq!(ship["pr_number"], 7);
    assert_eq!(ship["merge_commit"], "abc123");
    assert_eq!(
        store.require(&id).unwrap().ship.unwrap().status,
        redline_core::model::ShipStatus::Merged
    );

    // The URL is required when the agent says the PR exists.
    let no_url = call(
        &store,
        "ship_status",
        json!({"session_id": id, "state": "opened"}),
    )
    .await;
    assert_eq!(no_url["isError"], true, "{no_url}");
}

#[tokio::test]
async fn review_video_creates_a_session_with_media_and_a_video_url() {
    use base64::Engine as _;
    const MP4: &[u8] = b"\x00\x00\x00\x18ftypmp42\x00\x00\x00\x00mp42isom";
    let (_dir, store) = setup();
    let result = call(
        &store,
        "review_video",
        json!({
            "title": "Checkout flow",
            "note": "watch the transition",
            "video_base64": base64::engine::general_purpose::STANDARD.encode(MP4),
            "poster_base64": PNG_B64,
            "duration_ms": 5000,
            "width": 1280,
            "height": 720,
        }),
    )
    .await;
    assert_eq!(result["isError"], false, "{result}");
    let created = payload(&result);
    let id = created["session_id"].as_str().unwrap().to_string();
    let meta = store.require(&id).unwrap();
    assert_eq!(
        meta.media.as_ref().map(|media| media.kind),
        Some(redline_core::model::MediaKind::Video)
    );
    assert!(store.image_path(&meta).unwrap().is_file(), "video written");
    assert!(
        store.poster_path(&meta).unwrap().is_file(),
        "poster written"
    );

    let feedback = payload(&call(&store, "get_feedback", json!({"session_id": id})).await);
    assert_eq!(feedback["media"]["kind"], "video");
    assert!(feedback["video_url"]
        .as_str()
        .unwrap()
        .contains(&format!("/sessions/{id}/media?t={TOKEN}")));
}

#[tokio::test]
async fn feedback_includes_annotations_markdown_and_the_png_uri() {
    let (_dir, store) = setup();
    let created = payload(
        &call(
            &store,
            "review_screenshot",
            json!({"title": "Form", "image_base64": PNG_B64}),
        )
        .await,
    );
    let id = created["session_id"].as_str().unwrap().to_string();

    store
        .submit(
            &id,
            &Submission {
                note: None,
                annotations: vec![redline_core::model::Annotation {
                    kind: "text".into(),
                    fields: serde_json::from_value(
                        json!({"x": 2, "y": 3, "text": "Tighten this", "color": "#e11d48"}),
                    )
                    .unwrap(),
                }],
                png: Some(format!("data:image/png;base64,{PNG_B64}")),
                width: Some(800),
                height: Some(600),
            },
        )
        .unwrap();

    let feedback = payload(&call(&store, "get_feedback", json!({"session_id": id})).await);
    assert_eq!(feedback["status"], "submitted");
    assert_eq!(feedback["feature_state"], "pending");
    assert_eq!(feedback["annotations"][0]["text"], "Tighten this");
    assert!(feedback["markdown"]
        .as_str()
        .unwrap()
        .contains("- (2, 3): Tighten this"));
    assert_eq!(
        feedback["annotated_png"],
        format!("redline://session/{id}/annotated.png")
    );

    // The human's feature decision reaches the agent through the same payload.
    store.confirm_feature(&id).unwrap();
    let confirmed = payload(&call(&store, "get_feedback", json!({"session_id": id})).await);
    assert_eq!(confirmed["status"], "resolved");
    assert_eq!(confirmed["feature_state"], "confirmed");
    assert!(confirmed["feature_confirmed_at"].is_string());
    assert_eq!(confirmed["feature_cancelled_at"], Value::Null);

    // Resources let an image-capable agent look at the redlines.
    let context = McpContext {
        store: &store,
        base_url: "http://127.0.0.1:8788",
        token: TOKEN,
    };
    let listed = handle(
        &context,
        &json!({"jsonrpc": "2.0", "id": 3, "method": "resources/list"}),
    )
    .await
    .unwrap();
    let uris: Vec<String> = listed["result"]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["uri"].as_str().unwrap().to_string())
        .collect();
    assert!(uris.contains(&format!("redline://session/{id}/review.md")));

    let read = handle(
        &context,
        &json!({
            "jsonrpc": "2.0", "id": 4, "method": "resources/read",
            "params": {"uri": format!("redline://session/{id}/review.md")}
        }),
    )
    .await
    .unwrap();
    assert!(read["result"]["contents"][0]["text"]
        .as_str()
        .unwrap()
        .contains("Tighten this"));
}

#[tokio::test]
async fn waiting_times_out_without_losing_the_session() {
    let (_dir, store) = setup();
    let created = payload(
        &call(
            &store,
            "review_screenshot",
            json!({"title": "Wait", "image_base64": PNG_B64}),
        )
        .await,
    );
    let id = created["session_id"].as_str().unwrap().to_string();

    let waited = payload(
        &call(
            &store,
            "wait_for_feedback",
            json!({"session_id": id, "timeout_seconds": 1}),
        )
        .await,
    );
    assert_eq!(waited["status"], "open");
    assert_eq!(waited["timed_out"], true);
    assert!(
        store.require(&id).is_ok(),
        "session still there after a timeout"
    );
}

#[tokio::test]
async fn tool_errors_are_results_not_transport_failures() {
    let (_dir, store) = setup();

    let unknown = call(&store, "nope", json!({})).await;
    assert_eq!(unknown["isError"], true);
    assert!(unknown["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("unknown tool"));

    let missing = call(
        &store,
        "get_feedback",
        json!({"session_id": "does-not-exist"}),
    )
    .await;
    assert_eq!(missing["isError"], true);

    let no_image = call(&store, "review_screenshot", json!({"title": "x"})).await;
    assert_eq!(no_image["isError"], true);
    assert!(no_image["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("image_base64"));

    // capture_review without a browser must explain how to enable it (this test host has none;
    // if CI images ever ship Chromium the tool would succeed and this assertion is skipped).
    let capture = call(
        &store,
        "capture_review",
        json!({"url": "http://127.0.0.1:9"}),
    )
    .await;
    if capture["isError"] == true {
        let message = capture["content"][0]["text"].as_str().unwrap();
        assert!(
            message.contains("Chromium") || message.contains("browser"),
            "{message}"
        );
    }
}

#[tokio::test]
async fn resolve_and_list_reflect_status_changes() {
    let (_dir, store) = setup();
    let created = payload(
        &call(
            &store,
            "review_screenshot",
            json!({"title": "Resolvable", "image_base64": PNG_B64}),
        )
        .await,
    );
    let id = created["session_id"].as_str().unwrap().to_string();

    let resolved = payload(&call(&store, "resolve", json!({"session_id": id})).await);
    assert_eq!(resolved["status"], "resolved");
    assert!(resolved["resolved_at"].is_string());

    let listed = payload(&call(&store, "list_reviews", json!({"status": "resolved"})).await);
    assert_eq!(listed["sessions"].as_array().unwrap().len(), 1);

    let open = payload(&call(&store, "list_reviews", json!({"status": "open"})).await);
    assert!(open["sessions"].as_array().unwrap().is_empty());

    // Agent messages land beside the human's data, never inside annotations.json.
    let (_dir, store) = setup();
    let created = payload(
        &call(
            &store,
            "review_screenshot",
            json!({"title": "Thread", "image_base64": PNG_B64}),
        )
        .await,
    );
    let id = created["session_id"].as_str().unwrap().to_string();
    payload(
        &call(
            &store,
            "reply",
            json!({"session_id": id, "message": "which button?"}),
        )
        .await,
    );
    let thread =
        std::fs::read_to_string(store.dir(&id).unwrap().join("agent-thread.jsonl")).unwrap();
    assert!(thread.contains("which button?"));
}
