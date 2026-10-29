//! MCP surface: JSON-RPC 2.0 over the same HTTP port (`POST /mcp`).
//!
//! Hand-rolled instead of pulling in an SDK: the protocol surface we need is four methods and a
//! dozen fields, and keeping it dependency-free means the crate still builds on any OS with no
//! GUI libraries. Transport is streamable HTTP with plain JSON responses (no SSE), which every
//! MCP client accepts.

use std::time::Duration;

use serde_json::{json, Value};

use crate::model::{AnnotationSet, Meta, Status};
use crate::store::Store;
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
            "name": "capture_review",
            "description": "Screenshot a URL with headless Chromium, then open a review session for it. Requires the :browser image or REDLINE_CHROME on the server.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "Page to capture." },
                    "title": { "type": "string", "description": "What is being reviewed." },
                    "note": { "type": "string", "description": "What the human should look at." },
                    "project": { "type": "string", "description": "Project name or slug; created on first use. Default: Inbox." },
                    "viewport": { "type": "string", "description": "e.g. 1440x900 (default)." },
                    "wait_seconds": { "type": "number", "description": "Block up to this long for a submission." }
                },
                "required": ["url"]
            }
        },
        {
            "name": "get_feedback",
            "description": "Read the human's annotations, review.md and annotated screenshot for a session. Non-blocking.",
            "inputSchema": {
                "type": "object",
                "properties": { "session_id": { "type": "string" } },
                "required": ["session_id"]
            }
        },
        {
            "name": "wait_for_feedback",
            "description": "Block until the human submits the review (or the timeout elapses), then return the feedback.",
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
            "description": "List review sessions, newest first, optionally filtered by status or project.",
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
            "description": "Mark a session resolved once its annotations have been implemented.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "summary": { "type": "string" }
                },
                "required": ["session_id"]
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
        "get_feedback" => get_feedback(ctx, &args),
        "wait_for_feedback" => wait_for_feedback(ctx, &args).await,
        "list_reviews" => list_reviews(ctx, &args),
        "list_projects" => list_projects(ctx),
        "progress" => progress(ctx, &args),
        "reply" => reply(ctx, &args),
        "resolve" => resolve(ctx, &args),
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
                "sessions": mine.len(),
                "waiting": mine.iter().filter(|meta| meta.status == Status::Submitted).count(),
                "done": mine.iter().filter(|meta| meta.status == Status::Resolved).count(),
            })
        })
        .collect();
    Ok(json!({ "projects": items }))
}

fn progress(ctx: &McpContext<'_>, args: &Value) -> Result<Value, String> {
    let id = string_arg(args, "session_id").ok_or("session_id is required")?;
    let message = string_arg(args, "message").ok_or("message is required")?;
    let state = string_arg(args, "state").unwrap_or_else(|| "step".to_string());
    let tool = string_arg(args, "tool");
    ctx.store.require(&id).map_err(|err| err.to_string())?;
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
    ctx.store
        .set_agent_state(&id, "working", true)
        .map_err(|err| err.to_string())?;
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

fn feedback_payload(ctx: &McpContext<'_>, meta: &Meta) -> Value {
    let set: AnnotationSet = ctx.store.annotations(&meta.id).unwrap_or(AnnotationSet {
        session: meta.id.clone(),
        title: None,
        url: None,
        image: None,
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
        "title": meta.title,
        "url": meta.url,
        "note": meta.note,
        "prompt": meta.prompt,
        "agent_state": meta.agent_state,
        "submitted_at": meta.submitted_at,
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
