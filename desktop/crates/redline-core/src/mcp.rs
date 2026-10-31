//! MCP surface: JSON-RPC 2.0 over the same HTTP port (`POST /mcp`).
//!
//! Hand-rolled instead of pulling in an SDK: the protocol surface we need is four methods and a
//! dozen fields, and keeping it dependency-free means the crate still builds on any OS with no
//! GUI libraries. Transport is streamable HTTP with plain JSON responses (no SSE), which every
//! MCP client accepts.

use std::time::Duration;

use serde_json::{json, Value};

use crate::model::{AnnotationSet, Meta, ShipStatus, Status};
use crate::store::{ShipReport, Store};
use crate::MCP_PROTOCOL_VERSION;

pub struct McpContext<'a> {
    pub store: &'a Store,
    pub base_url: &'a str,
    pub token: &'a str,
}

/// Returns `None` for notifications (no `id`), which the HTTP layer answers with `202 Accepted`.
pub async fn handle(ctx: &McpContext<'_>, request: &Value) -> Option<Value> {
    let method = request.get("method").and_then(Value::as_str)?;
    let id = request.get("id").cloned()?;
    let params = request.get("params").cloned().unwrap_or(Value::Null);

    let outcome = match method {
        "initialize" => Ok(json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {
                "tools": { "listChanged": false },
                "resources": { "subscribe": false, "listChanged": false }
            },
            "serverInfo": { "name": "redline", "version": crate::VERSION },
            "instructions": "Create a review session from a screenshot, wait for the human to redline it, \
        then read the annotations. Prefer `review_screenshot` + `wait_for_feedback`.",
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tool_definitions() })),
        "tools/call" => call_tool(ctx, &params).await,
        "resources/list" => resources_list(ctx),
        "resources/read" => resources_read(ctx, &params),
        other => Err(rpc_error(-32601, format!("method not found: {other}"))),
    };

    Some(match outcome {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err(error) => json!({ "jsonrpc": "2.0", "id": id, "error": error }),
    })
}

fn rpc_error(code: i64, message: impl Into<String>) -> Value {
    json!({ "code": code, "message": message.into() })
}

/// Tool errors are results with `isError: true` (per MCP), not JSON-RPC errors — the model should
/// see the message and retry, not treat the transport as broken.
fn tool_error(message: impl Into<String>) -> Value {
    json!({
        "content": [{ "type": "text", "text": message.into() }],
        "isError": true
    })
}

fn tool_text(value: &Value) -> Value {
    let text = serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string());
    json!({ "content": [{ "type": "text", "text": text }], "isError": false })
}

pub fn tool_definitions() -> Value {
    json!([
        {
            "name": "review_screenshot",
            "description": "Upload a screenshot and open a review session for the human. Returns the session id and a review URL. Set wait_seconds to block until they submit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "image_base64": { "type": "string", "description": "Base64 PNG/JPEG, optionally a data: URL." },
                    "image_path": { "type": "string", "description": "Absolute path on the machine running redline." },
                    "title": { "type": "string", "description": "What is being reviewed, e.g. 'Button / default'." },
                    "url": { "type": "string", "description": "Page URL the screenshot came from." },
                    "note": { "type": "string", "description": "What the human should look at." },
                    "project": { "type": "string", "description": "Project name or slug; created on first use. Default: Inbox." },
                    "branch": { "type": "string", "description": "Feature branch this review belongs to. RedLine uses it as the branch-lock key." },
                    "wait_seconds": { "type": "number", "description": "Block up to this long for a submission (default 0)." }
                },
                "required": ["title"],
                "oneOf": [
                    { "required": ["image_base64"] },
                    { "required": ["image_path"] }
                ]
            }
        },
        {
            "name": "review_video",
            "description": "Upload a video (MP4 or WebM) and open a review session for the human. Returns the session id and a review URL. Set wait_seconds to block until they submit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "video_base64": { "type": "string", "description": "Base64 MP4/WebM, optionally a data: URL. Prefer video_path for large clips." },
                    "video_path": { "type": "string", "description": "Absolute path on the machine running redline." },
                    "poster_base64": { "type": "string", "description": "Optional first-frame PNG/JPEG (base64) used for the rail/gallery thumbnail." },
                    "duration_ms": { "type": "number", "description": "Video duration in milliseconds, when known." },
                    "width": { "type": "number" }, "height": { "type": "number" }, "fps": { "type": "number" },
                    "title": { "type": "string", "description": "What is being reviewed, e.g. 'Checkout flow'." },
                    "url": { "type": "string", "description": "Page URL the video came from." },
                    "note": { "type": "string", "description": "What the human should look at." },
                    "project": { "type": "string", "description": "Project name or slug; created on first use. Default: Inbox." },
                    "branch": { "type": "string", "description": "Feature branch this review belongs to. RedLine uses it as the branch-lock key." },
                    "wait_seconds": { "type": "number", "description": "Block up to this long for a submission (default 0)." }
                },
                "required": ["title"],
                "oneOf": [
                    { "required": ["video_base64"] },
                    { "required": ["video_path"] }
                ]
            }
        },
        {
            "name": "capture_review",
            "description": "Screenshot a URL with headless Chromium, then open a review session for it. Requires the :browser image or REDLINE_CHROME on the server.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "Page to capture." },
                    "title": { "type": "string", "description": "What is being reviewed." },
                    "note": { "type": "string", "description": "What the human should look at." },
                    "project": { "type": "string", "description": "Project name or slug; created on first use. Default: Inbox." },
                    "branch": { "type": "string", "description": "Feature branch this review belongs to. RedLine uses it as the branch-lock key." },
                    "viewport": { "type": "string", "description": "e.g. 1440x900 (default)." },
                    "wait_seconds": { "type": "number", "description": "Block up to this long for a submission." }
                },
                "required": ["url"]
            }
        },
        {
            "name": "get_feedback",
            "description": "Read the human's annotations, review.md and annotated screenshot for a session. Non-blocking. feature_state is pending until the human confirms or cancels the feature.",
            "inputSchema": {
                "type": "object",
                "properties": { "session_id": { "type": "string" } },
                "required": ["session_id"]
            }
        },
        {
            "name": "wait_for_feedback",
            "description": "Block until the human submits the review, confirms/cancels the feature, or the timeout elapses, then return the feedback (including feature_state).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "timeout_seconds": { "type": "number", "description": "Default 600, max 3600." }
                },
                "required": ["session_id"]
            }
        },
        {
            "name": "list_reviews",
            "description": "List review sessions, newest first, optionally filtered by status or project. Each item includes feature_state (pending, confirmed, cancelled).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "status": { "type": "string", "enum": ["open", "submitted", "resolved"] },
                    "project": { "type": "string", "description": "Project name or slug." },
                    "limit": { "type": "number", "description": "Default 50." }
                }
            }
        },
        {
            "name": "list_projects",
            "description": "List projects (conversation areas) with their session counts.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "progress",
            "description": "Report what you are doing right now so the human sees activity instead of silence. One event per meaningful step, not per token.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "message": { "type": "string", "description": "One short line, e.g. \"Editing app.css\"." },
                    "state": { "type": "string", "enum": ["thinking", "tool", "step"], "description": "Default: step." },
                    "tool": { "type": "string", "description": "Tool name when relevant." }
                },
                "required": ["session_id", "message"]
            }
        },
        {
            "name": "post_round",
            "description": "Post a new round into an existing review instead of creating a new one: the agent's answer, optionally with a fresh screenshot. Archives the previous round and hands the session back to the human.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "note": { "type": "string", "description": "Short message for the human." },
                    "image_base64": { "type": "string" },
                    "image_path": { "type": "string" },
                    "video_base64": { "type": "string", "description": "MP4/WebM base64; posts a new video round." },
                    "video_path": { "type": "string", "description": "Absolute path on the server; posts a new video round." },
                    "poster_base64": { "type": "string", "description": "Optional first-frame image for a video round." },
                    "duration_ms": { "type": "number" },
                    "width": { "type": "number" },
                    "height": { "type": "number" },
                    "fps": { "type": "number" }
                },
                "required": ["session_id"]
            }
        },
        {
            "name": "capture_round",
            "description": "Capture a URL and post it as the next round of an existing review (no new session).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "url": { "type": "string" },
                    "note": { "type": "string" },
                    "viewport": { "type": "string", "description": "e.g. 1440x900 (default)." }
                },
                "required": ["session_id", "url"]
            }
        },
        {
            "name": "reply",
            "description": "Attach an agent message or question to a session so the human sees it in the editor.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "message": { "type": "string" }
                },
                "required": ["session_id", "message"]
            }
        },
        {
            "name": "resolve",
            "description": "Mark a session resolved once its annotations have been implemented. This closes the review round; it is not the human's feature confirmation (feature_state stays pending).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "summary": { "type": "string" }
                },
                "required": ["session_id"]
            }
        },
        {
            "name": "ship_status",
            "description": "Report the result of a confirmed feature's ship run: the pull request is open, merged, closed or could not be shipped. The human's rail shows this state. Only call it for a feature the human confirmed.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "state": {
                        "type": "string",
                        "enum": ["opened", "merged", "failed", "closed"],
                        "description": "opened = PR created but not merged yet; merged = merged into the integration branch; failed/closed = no merge."
                    },
                    "pr_url": { "type": "string", "description": "Full pull-request URL (required for opened/merged)." },
                    "pr_number": { "type": "number" },
                    "merge_commit": { "type": "string" },
                    "message": { "type": "string", "description": "The failing check or reason when state=failed." }
                },
                "required": ["session_id", "state"]
            }
        }
    ])
}

async fn call_tool(ctx: &McpContext<'_>, params: &Value) -> Result<Value, Value> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    let outcome = match name {
        "review_screenshot" => review_screenshot(ctx, &args).await,
        "review_video" => review_video(ctx, &args).await,
        "get_feedback" => get_feedback(ctx, &args),
        "wait_for_feedback" => wait_for_feedback(ctx, &args).await,
        "list_reviews" => list_reviews(ctx, &args),
        "list_projects" => list_projects(ctx),
        "post_round" => post_round(ctx, &args),
        "capture_round" => capture_round(ctx, &args).await,
        "progress" => progress(ctx, &args),
        "reply" => reply(ctx, &args),
        "resolve" => resolve(ctx, &args),
        "ship_status" => ship_status(ctx, &args),
        "capture_review" => capture_review(ctx, &args).await,
        other => Err(format!("unknown tool: {other}")),
    };

    Ok(match outcome {
        Ok(value) => tool_text(&value),
        Err(message) => tool_error(message),
    })
}

fn string_arg(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

async fn review_screenshot(ctx: &McpContext<'_>, args: &Value) -> Result<Value, String> {
    use base64::Engine as _;

    let title = string_arg(args, "title").unwrap_or_else(|| "Screenshot review".to_string());
    let (bytes, name) = match (
        string_arg(args, "image_base64"),
        string_arg(args, "image_path"),
    ) {
        (Some(encoded), _) => {
            let raw = encoded
                .split_once(',')
                .map(|(_, data)| data)
                .unwrap_or(&encoded);
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(raw.trim())
                .map_err(|err| format!("image_base64 is not valid base64: {err}"))?;
            (bytes, "screenshot.png".to_string())
        }
        (None, Some(path)) => {
            let path = std::path::PathBuf::from(&path);
            let bytes = std::fs::read(&path)
                .map_err(|err| format!("cannot read image_path {path:?}: {err}"))?;
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("screenshot.png")
                .to_string();
            (bytes, name)
        }
        (None, None) => {
            return Err("provide image_base64 or image_path".to_string());
        }
    };

    let meta = ctx
        .store
        .create_from_bytes(
            &bytes,
            &name,
            &title,
            args.get("url").and_then(Value::as_str).unwrap_or_default(),
            args.get("note").and_then(Value::as_str).unwrap_or_default(),
            None,
            Some("mcp"),
            args.get("project").and_then(Value::as_str),
        )
        .map_err(|err| err.to_string())?;
    let meta = if let Some(branch) = string_arg(args, "branch") {
        ctx.store
            .set_branch(&meta.id, Some(&branch))
            .map_err(|err| err.to_string())?
    } else {
        meta
    };

    let wait = args
        .get("wait_seconds")
        .and_then(Value::as_f64)
        .unwrap_or(0.0) as u64;
    let meta = if wait > 0 {
        wait_for_status(ctx.store, &meta.id, wait.min(3600))
            .await
            .unwrap_or(meta)
    } else {
        meta
    };

    let feedback = if meta.status == Status::Submitted {
        Some(feedback_payload(ctx, &meta))
    } else {
        None
    };

    Ok(json!({
        "session_id": meta.id,
        "status": meta.status.as_str(),
        "title": meta.title,
        "review_url": review_url(ctx, &meta.id),
        "dir": ctx.store.dir(&meta.id).map(|p| p.display().to_string()).unwrap_or_default(),
        "feedback": feedback,
    }))
}

async fn review_video(ctx: &McpContext<'_>, args: &Value) -> Result<Value, String> {
    use base64::Engine as _;

    let title = string_arg(args, "title").unwrap_or_else(|| "Video review".to_string());
    let bytes = match (
        string_arg(args, "video_base64"),
        string_arg(args, "video_path"),
    ) {
        (Some(encoded), _) => {
            let raw = encoded
                .split_once(',')
                .map(|(_, data)| data)
                .unwrap_or(&encoded);
            base64::engine::general_purpose::STANDARD
                .decode(raw.trim())
                .map_err(|err| format!("video_base64 is not valid base64: {err}"))?
        }
        (None, Some(path)) => {
            let path = std::path::PathBuf::from(&path);
            let size = std::fs::metadata(&path)
                .map_err(|err| format!("cannot read video_path {path:?}: {err}"))?
                .len();
            if size as usize > crate::MAX_VIDEO_BYTES {
                return Err(format!(
                    "video is larger than {} MB",
                    crate::MAX_VIDEO_BYTES / (1024 * 1024)
                ));
            }
            std::fs::read(&path).map_err(|err| format!("cannot read video_path {path:?}: {err}"))?
        }
        (None, None) => return Err("provide video_base64 or video_path".to_string()),
    };
    let poster = match string_arg(args, "poster_base64") {
        Some(encoded) => {
            let raw = encoded
                .split_once(',')
                .map(|(_, data)| data)
                .unwrap_or(&encoded);
            Some(
                base64::engine::general_purpose::STANDARD
                    .decode(raw.trim())
                    .map_err(|err| format!("poster_base64 is not valid base64: {err}"))?,
            )
        }
        None => None,
    };
    let meta = ctx
        .store
        .create_video_from_bytes(
            &bytes,
            &title,
            args.get("url").and_then(Value::as_str).unwrap_or_default(),
            args.get("note").and_then(Value::as_str).unwrap_or_default(),
            None,
            Some("mcp"),
            args.get("project").and_then(Value::as_str),
            poster.as_deref(),
            args.get("duration_ms").and_then(Value::as_u64),
            args.get("width").and_then(Value::as_u64).map(|v| v as u32),
            args.get("height").and_then(Value::as_u64).map(|v| v as u32),
            args.get("fps").and_then(Value::as_f64),
        )
        .map_err(|err| err.to_string())?;
    let meta = if let Some(branch) = string_arg(args, "branch") {
        ctx.store
            .set_branch(&meta.id, Some(&branch))
            .map_err(|err| err.to_string())?
    } else {
        meta
    };
    let wait = args
        .get("wait_seconds")
        .and_then(Value::as_f64)
        .unwrap_or(0.0) as u64;
    let meta = if wait > 0 {
        wait_for_status(ctx.store, &meta.id, wait.min(3600))
            .await
            .unwrap_or(meta)
    } else {
        meta
    };
    let feedback = if meta.status == Status::Submitted {
        Some(feedback_payload(ctx, &meta))
    } else {
        None
    };
    Ok(json!({
        "session_id": meta.id,
        "status": meta.status.as_str(),
        "title": meta.title,
        "review_url": review_url(ctx, &meta.id),
        "dir": ctx.store.dir(&meta.id).map(|p| p.display().to_string()).unwrap_or_default(),
        "feedback": feedback,
    }))
}

async fn capture_review(ctx: &McpContext<'_>, args: &Value) -> Result<Value, String> {
    let url = string_arg(args, "url").ok_or("url is required")?;
    let title = string_arg(args, "title").unwrap_or_else(|| url.clone());
    let (width, height) = match string_arg(args, "viewport") {
        Some(viewport) => crate::capture::parse_viewport(&viewport)?,
        None => (1440, 900),
    };

    let request = crate::capture::CaptureRequest {
        url: url.clone(),
        width,
        height,
    };
    let png = tokio::task::spawn_blocking(move || crate::capture::capture(&request))
        .await
        .map_err(|err| format!("capture task failed: {err}"))??;

    let meta = ctx
        .store
        .create_from_bytes(
            &png,
            "screenshot.png",
            &title,
            &url,
            args.get("note").and_then(Value::as_str).unwrap_or_default(),
            None,
            Some("mcp"),
            args.get("project").and_then(Value::as_str),
        )
        .map_err(|err| err.to_string())?;
    let meta = if let Some(branch) = string_arg(args, "branch") {
        ctx.store
            .set_branch(&meta.id, Some(&branch))
            .map_err(|err| err.to_string())?
    } else {
        meta
    };

    let wait = args
        .get("wait_seconds")
        .and_then(Value::as_f64)
        .unwrap_or(0.0) as u64;
    let meta = if wait > 0 {
        wait_for_status(ctx.store, &meta.id, wait.min(3600))
            .await
            .unwrap_or(meta)
    } else {
        meta
    };

    Ok(json!({
        "session_id": meta.id,
        "status": meta.status.as_str(),
        "title": meta.title,
        "captured": url,
        "viewport": format!("{width}x{height}"),
        "review_url": review_url(ctx, &meta.id),
        "feedback": (meta.status == Status::Submitted).then(|| feedback_payload(ctx, &meta)),
    }))
}

fn review_url(ctx: &McpContext<'_>, id: &str) -> String {
    format!(
        "{}/s/{}?t={}",
        ctx.base_url.trim_end_matches('/'),
        id,
        ctx.token
    )
}

fn get_feedback(ctx: &McpContext<'_>, args: &Value) -> Result<Value, String> {
    let id = string_arg(args, "session_id").ok_or("session_id is required")?;
    let meta = ctx.store.require(&id).map_err(|err| err.to_string())?;
    let meta = mark_seen(ctx.store, meta);
    Ok(feedback_payload(ctx, &meta))
}

/// Reading the feedback is the moment the human learns the agent picked it up. Never downgrade a
/// session the agent is already working on.
fn mark_seen(store: &Store, meta: Meta) -> Meta {
    if meta.status == Status::Submitted && meta.agent_state.as_deref() != Some("working") {
        return store
            .set_agent_state(&meta.id, "received", true)
            .unwrap_or(meta);
    }
    meta
}

async fn wait_for_feedback(ctx: &McpContext<'_>, args: &Value) -> Result<Value, String> {
    let id = string_arg(args, "session_id").ok_or("session_id is required")?;
    let timeout = args
        .get("timeout_seconds")
        .and_then(Value::as_u64)
        .unwrap_or(600)
        .min(3600);
    let meta = ctx.store.require(&id).map_err(|err| err.to_string())?;
    let meta = wait_for_status(ctx.store, &meta.id, timeout).await?;
    if meta.status == Status::Open {
        return Ok(json!({
            "session_id": meta.id,
            "status": "open",
            "timed_out": true,
            "message": "no submission yet; call wait_for_feedback again",
        }));
    }
    let meta = mark_seen(ctx.store, meta);
    Ok(feedback_payload(ctx, &meta))
}

/// Poll the store until the session leaves `open`. Cheap: three small file reads per second.
async fn wait_for_status(store: &Store, id: &str, timeout_seconds: u64) -> Result<Meta, String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_seconds);
    loop {
        let meta = store.require(id).map_err(|err| err.to_string())?;
        if meta.status != Status::Open || tokio::time::Instant::now() >= deadline {
            return Ok(meta);
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

fn list_reviews(ctx: &McpContext<'_>, args: &Value) -> Result<Value, String> {
    let wanted = string_arg(args, "status");
    let wanted_project = string_arg(args, "project").map(|value| crate::store::slugify(&value));
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(50)
        .min(500) as usize;
    let sessions = ctx.store.list().map_err(|err| err.to_string())?;
    let items: Vec<Value> = sessions
        .into_iter()
        .filter(|meta| match &wanted {
            Some(status) => meta.status.as_str() == status.as_str(),
            None => true,
        })
        .filter(|meta| match &wanted_project {
            Some(project) => meta.project.as_deref() == Some(project.as_str()),
            None => true,
        })
        .take(limit)
        .map(|meta| {
            json!({
                "session_id": meta.id,
                "title": meta.title,
                "status": meta.status.as_str(),
                "feature_state": meta.feature_state(),
                "feature_confirmed_at": meta.feature_confirmed_at,
                "feature_cancelled_at": meta.feature_cancelled_at,
                "ship": meta.ship,
                "created_at": meta.created_at,
                "submitted_at": meta.submitted_at,
                "project": meta.project,
            })
        })
        .collect();
    Ok(json!({ "sessions": items }))
}

fn list_projects(ctx: &McpContext<'_>) -> Result<Value, String> {
    let projects = ctx.store.projects().map_err(|err| err.to_string())?;
    let sessions = ctx.store.list().map_err(|err| err.to_string())?;
    let items: Vec<Value> = projects
        .into_iter()
        .map(|project| {
            let mine: Vec<&Meta> = sessions
                .iter()
                .filter(|meta| meta.project.as_deref() == Some(project.slug.as_str()))
                .collect();
            json!({
                "slug": project.slug,
                "name": project.name,
                "agent": project.agent,
                "repo": project.repo,
                "sessions": mine.len(),
                "waiting": mine.iter().filter(|meta| meta.status == Status::Submitted).count(),
                "done": mine.iter().filter(|meta| meta.status == Status::Resolved).count(),
            })
        })
        .collect();
    Ok(json!({ "projects": items }))
}

fn post_round(ctx: &McpContext<'_>, args: &Value) -> Result<Value, String> {
    use base64::Engine as _;
    let id = string_arg(args, "session_id").ok_or("session_id is required")?;
    let note = string_arg(args, "note").unwrap_or_default();
    let video_base64 = string_arg(args, "video_base64");
    let video_path = string_arg(args, "video_path");
    if video_base64.is_some() || video_path.is_some() {
        let bytes = match (video_base64, video_path) {
            (Some(encoded), _) => {
                let raw = encoded
                    .split_once(',')
                    .map(|(_, data)| data)
                    .unwrap_or(&encoded);
                base64::engine::general_purpose::STANDARD
                    .decode(raw.trim())
                    .map_err(|err| format!("video_base64 is not valid base64: {err}"))?
            }
            (None, Some(path)) => {
                let path = std::path::PathBuf::from(&path);
                let size = std::fs::metadata(&path)
                    .map_err(|err| format!("cannot read video_path {path:?}: {err}"))?
                    .len();
                if size as usize > crate::MAX_VIDEO_BYTES {
                    return Err(format!(
                        "video is larger than {} MB",
                        crate::MAX_VIDEO_BYTES / (1024 * 1024)
                    ));
                }
                std::fs::read(&path)
                    .map_err(|err| format!("cannot read video_path {path:?}: {err}"))?
            }
            (None, None) => unreachable!(),
        };
        let poster = match string_arg(args, "poster_base64") {
            Some(encoded) => {
                let raw = encoded
                    .split_once(',')
                    .map(|(_, data)| data)
                    .unwrap_or(&encoded);
                Some(
                    base64::engine::general_purpose::STANDARD
                        .decode(raw.trim())
                        .map_err(|err| format!("poster_base64 is not valid base64: {err}"))?,
                )
            }
            None => None,
        };
        let meta = ctx
            .store
            .new_video_round(
                &id,
                &bytes,
                poster.as_deref(),
                args.get("duration_ms").and_then(Value::as_u64),
                args.get("width").and_then(Value::as_u64).map(|v| v as u32),
                args.get("height").and_then(Value::as_u64).map(|v| v as u32),
                args.get("fps").and_then(Value::as_f64),
            )
            .map_err(|err| err.to_string())?;
        return Ok(json!({
            "session_id": id,
            "posted": true,
            "status": meta.status.as_str(),
            "media": meta.media,
            "video_url": format!(
                "{}/sessions/{}/media?t={}",
                ctx.base_url.trim_end_matches('/'),
                meta.id,
                ctx.token
            ),
        }));
    }

    let (bytes, name) = match (
        string_arg(args, "image_base64"),
        string_arg(args, "image_path"),
    ) {
        (Some(encoded), _) => {
            let raw = encoded
                .split_once(',')
                .map(|(_, data)| data)
                .unwrap_or(&encoded);
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(raw.trim())
                .map_err(|err| format!("image_base64 is not valid base64: {err}"))?;
            (Some(bytes), "screenshot.png".to_string())
        }
        (None, Some(path)) => {
            let path = std::path::PathBuf::from(&path);
            let bytes = std::fs::read(&path)
                .map_err(|err| format!("cannot read image_path {path:?}: {err}"))?;
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("screenshot.png")
                .to_string();
            (Some(bytes), name)
        }
        (None, None) => (None, "screenshot.png".to_string()),
    };
    if bytes.is_none() && note.is_empty() {
        return Err("post_round needs a note, an image, or both".to_string());
    }
    let meta = ctx
        .store
        .post_round(&id, bytes.as_deref(), &name, &note, None, None)
        .map_err(|err| err.to_string())?;
    Ok(
        json!({ "session_id": id, "posted": true, "status": meta.status.as_str(), "image": meta.image }),
    )
}

async fn capture_round(ctx: &McpContext<'_>, args: &Value) -> Result<Value, String> {
    let id = string_arg(args, "session_id").ok_or("session_id is required")?;
    let url = string_arg(args, "url").ok_or("url is required")?;
    let (width, height) = match string_arg(args, "viewport") {
        Some(viewport) => crate::capture::parse_viewport(&viewport)?,
        None => (1440, 900),
    };
    let request = crate::capture::CaptureRequest { url, width, height };
    let png = tokio::task::spawn_blocking(move || crate::capture::capture(&request))
        .await
        .map_err(|err| format!("capture task failed: {err}"))??;
    let note = string_arg(args, "note").unwrap_or_default();
    let meta = ctx
        .store
        .post_round(
            &id,
            Some(&png),
            "screenshot.png",
            &note,
            Some(width),
            Some(height),
        )
        .map_err(|err| err.to_string())?;
    Ok(
        json!({ "session_id": id, "posted": true, "status": meta.status.as_str(), "image": meta.image }),
    )
}

fn progress(ctx: &McpContext<'_>, args: &Value) -> Result<Value, String> {
    let id = string_arg(args, "session_id").ok_or("session_id is required")?;
    let message = string_arg(args, "message").ok_or("message is required")?;
    let state = string_arg(args, "state").unwrap_or_else(|| "step".to_string());
    let tool = string_arg(args, "tool");
    let meta = ctx.store.require(&id).map_err(|err| err.to_string())?;
    let mut event = json!({
        "at": crate::store::now(),
        "state": state,
        "message": message,
    });
    if let Some(tool) = tool {
        event["tool"] = Value::String(tool);
    }
    ctx.store
        .append_agent_event(&id, &event)
        .map_err(|err| err.to_string())?;
    // The narration is always kept, but it may not re-arm the "working" indicator on a round that
    // is over: `post_round` hands the review back (open) and `resolve` closes it. `reply` is *not*
    // a finish signal — the agent often answers early and keeps working for minutes — so a later
    // `progress` is evidence that the run resumed and must show as working again while the round
    // is still submitted (2026-10-30).
    let finished = meta.status != Status::Submitted;
    if !finished {
        ctx.store
            .set_agent_state(&id, "working", true)
            .map_err(|err| err.to_string())?;
    }
    Ok(json!({ "session_id": id, "recorded": true }))
}

fn reply(ctx: &McpContext<'_>, args: &Value) -> Result<Value, String> {
    let id = string_arg(args, "session_id").ok_or("session_id is required")?;
    let message = string_arg(args, "message").ok_or("message is required")?;
    ctx.store.require(&id).map_err(|err| err.to_string())?;
    // Agent messages live beside the human's data but never inside it: annotations.json stays
    // the human's, agent-thread.jsonl is ours.
    let path = ctx
        .store
        .dir(&id)
        .map_err(|err| err.to_string())?
        .join("agent-thread.jsonl");
    let line = json!({ "at": crate::store::now(), "message": message });
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|err| format!("cannot write {}: {err}", path.display()))?;
    writeln!(file, "{line}").map_err(|err| err.to_string())?;
    ctx.store
        .set_agent_state(&id, "replied", true)
        .map_err(|err| err.to_string())?;
    Ok(json!({ "session_id": id, "appended": true }))
}

fn resolve(ctx: &McpContext<'_>, args: &Value) -> Result<Value, String> {
    let id = string_arg(args, "session_id").ok_or("session_id is required")?;
    let meta = ctx.store.resolve(&id).map_err(|err| err.to_string())?;
    Ok(
        json!({ "session_id": meta.id, "status": meta.status.as_str(), "resolved_at": meta.resolved_at }),
    )
}

/// The ship run's only write into RedLine: a PR URL plus a terminal state. The agent owns git;
/// the server only records what happened so the rail can say "PR #46" or "merged".
fn ship_status(ctx: &McpContext<'_>, args: &Value) -> Result<Value, String> {
    let id = string_arg(args, "session_id").ok_or("session_id is required")?;
    let state = string_arg(args, "state").ok_or("state is required")?;
    let status = match state.as_str() {
        "opened" => ShipStatus::Opened,
        "merged" => ShipStatus::Merged,
        "failed" => ShipStatus::Failed,
        "closed" => ShipStatus::Closed,
        other => {
            return Err(format!(
                "state must be opened, merged, failed or closed, not {other:?}"
            ))
        }
    };
    let pr_url = string_arg(args, "pr_url");
    if status == ShipStatus::Opened && pr_url.is_none() {
        return Err("pr_url is required when state is opened".to_string());
    }
    if status == ShipStatus::Merged && pr_url.is_none() {
        // The opened report normally carried the URL; a merged follow-up may omit it.
        let known = ctx
            .store
            .require(&id)
            .ok()
            .and_then(|meta| meta.ship)
            .and_then(|ship| ship.pr_url)
            .is_some();
        if !known {
            return Err("pr_url is required when state is merged".to_string());
        }
    }
    if let Some(url) = pr_url.as_deref() {
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err("pr_url must be an http(s) URL".to_string());
        }
    }
    let pr_number = args.get("pr_number").and_then(Value::as_u64);
    let merge_commit = string_arg(args, "merge_commit");
    let message = string_arg(args, "message");
    let meta = ctx
        .store
        .report_ship(
            &id,
            ShipReport {
                status,
                pr_url,
                pr_number,
                merge_commit,
                error: message,
            },
        )
        .map_err(|err| err.to_string())?;
    Ok(json!({
        "session_id": meta.id,
        "feature_state": meta.feature_state(),
        "ship": meta.ship,
    }))
}

fn feedback_payload(ctx: &McpContext<'_>, meta: &Meta) -> Value {
    let set: AnnotationSet = ctx.store.annotations(&meta.id).unwrap_or(AnnotationSet {
        session: meta.id.clone(),
        title: None,
        url: None,
        image: None,
        media: None,
        frames: Vec::new(),
        annotations: Vec::new(),
        submitted_at: None,
    });
    let dir = ctx.store.dir(&meta.id).ok();
    let markdown = dir
        .as_ref()
        .and_then(|dir| std::fs::read_to_string(dir.join("review.md")).ok());
    let has_png = dir
        .as_ref()
        .is_some_and(|dir| dir.join("annotated.png").is_file());

    json!({
        "session_id": meta.id,
        "status": meta.status.as_str(),
        "feature_state": meta.feature_state(),
        "feature_confirmed_at": meta.feature_confirmed_at,
        "feature_cancelled_at": meta.feature_cancelled_at,
        "ship": meta.ship,
        "title": meta.title,
        "url": meta.url,
        "note": meta.note,
        "prompt": meta.prompt,
        "agent_state": meta.agent_state,
        "submitted_at": meta.submitted_at,
        "media": meta.media,
        "video_url": meta.media.as_ref()
            .filter(|media| media.kind == crate::model::MediaKind::Video)
            .map(|_| format!(
                "{}/sessions/{}/media?t={}",
                ctx.base_url.trim_end_matches('/'),
                meta.id,
                ctx.token
            )),
        "frames": set.frames.iter().map(|frame| {
            json!({
                "at": frame.at,
                "mime": frame.mime,
                "url": format!(
                    "{}/sessions/{}/frames/{}?t={}",
                    ctx.base_url.trim_end_matches('/'),
                    meta.id,
                    frame.file.trim_start_matches("frames/"),
                    ctx.token
                ),
            })
        }).collect::<Vec<_>>(),
        "annotations": set.annotations,
        "markdown": markdown,
        "annotated_png": has_png.then(|| format!("redline://session/{}/annotated.png", meta.id)),
        "review_url": review_url(ctx, &meta.id),
    })
}

fn resources_list(ctx: &McpContext<'_>) -> Result<Value, Value> {
    let mut resources = Vec::new();
    for meta in ctx
        .store
        .list()
        .map_err(|err| rpc_error(-32603, err.to_string()))?
    {
        if meta.status == Status::Open {
            continue;
        }
        let base = format!("redline://session/{}", meta.id);
        resources.push(json!({
            "uri": format!("{base}/annotated.png"),
            "name": format!("{} — annotated screenshot", meta.title),
            "mimeType": "image/png",
        }));
        resources.push(json!({
            "uri": format!("{base}/review.md"),
            "name": format!("{} — review summary", meta.title),
            "mimeType": "text/markdown",
        }));
    }
    Ok(json!({ "resources": resources }))
}

fn resources_read(ctx: &McpContext<'_>, params: &Value) -> Result<Value, Value> {
    let uri = params
        .get("uri")
        .and_then(Value::as_str)
        .ok_or_else(|| rpc_error(-32602, "uri is required"))?;
    let rest = uri
        .strip_prefix("redline://session/")
        .ok_or_else(|| rpc_error(-32602, format!("unsupported uri: {uri}")))?;
    let (id, file) = rest
        .split_once('/')
        .ok_or_else(|| rpc_error(-32602, format!("unsupported uri: {uri}")))?;
    let path = ctx
        .store
        .file_path(id, file)
        .map_err(|err| rpc_error(-32602, err.to_string()))?;
    let bytes = std::fs::read(&path)
        .map_err(|err| rpc_error(-32603, format!("cannot read {uri}: {err}")))?;

    let content = if file.ends_with(".png") {
        use base64::Engine as _;
        json!({
            "uri": uri,
            "mimeType": "image/png",
            "blob": base64::engine::general_purpose::STANDARD.encode(bytes),
        })
    } else {
        json!({
            "uri": uri,
            "mimeType": if file.ends_with(".json") { "application/json" } else { "text/markdown" },
            "text": String::from_utf8_lossy(&bytes),
        })
    };
    Ok(json!({ "contents": [content] }))
}
