//! The session folder format (`sessions/<id>/`) and everything that reads or writes it.
//!
//! Every write goes through [`write_atomic`] (temp file + rename) so a crash mid-submit cannot
//! leave a half-written `annotations.json` that the agent then parses successfully but wrongly.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{SecondsFormat, Utc};
use serde_json::{json, Value};

use crate::error::{Error, Result};
use crate::model::{
    AgentFailure, Annotation, AnnotationSet, FrameRef, Handoff, HandoffStatus, ImageRef, MediaKind,
    MediaRef, Meta, Status, Submission, TraceHighlight, TraceRef, TraceSummary,
};
use crate::{
    MAX_FRAMES_BYTES, MAX_FRAME_COUNT, MAX_TRACE_BYTES, MAX_TRACE_EVENTS, MAX_TRACE_TEXT,
    MAX_VIDEO_BYTES,
};

/// A frame from the browser, captured at an annotation time. `at` is seconds into the video.
#[derive(Debug, Clone)]
pub struct FrameUpload {
    pub at: f64,
    pub bytes: Vec<u8>,
}

/// The raw browser trace as uploaded. Only the fields needed for validation/summary are typed;
/// page/browser/redaction metadata is preserved in the file but not interpreted by the server.
#[derive(Debug, Clone, serde::Deserialize)]
struct TraceUpload {
    version: u32,
    kind: String,
    started_at: String,
    duration_ms: u64,
    #[serde(default)]
    redaction: Option<Value>,
    #[serde(default)]
    truncated: bool,
    #[serde(default)]
    events: Vec<Value>,
}

/// Validate and summarize a browser trace before it touches the session folder. The server never
/// trusts a client-provided summary; counts and highlights are derived here. The raw JSON is
/// stored unchanged so new event fields remain readable by later clients.
pub fn validate_trace(bytes: &[u8], video_duration_ms: Option<u64>) -> Result<TraceRef> {
    if bytes.is_empty() {
        return Err(Error::BadRequest("trace is empty".to_string()));
    }
    if bytes.len() > MAX_TRACE_BYTES {
        return Err(Error::BadRequest(format!(
            "trace is larger than {} MB",
            MAX_TRACE_BYTES / (1024 * 1024)
        )));
    }
    let upload: TraceUpload = serde_json::from_slice(bytes)
        .map_err(|err| Error::BadRequest(format!("invalid trace JSON: {err}")))?;
    if upload.version != 1 {
        return Err(Error::BadRequest(format!(
            "unsupported trace version {}",
            upload.version
        )));
    }
    if upload.kind != "browser-flow" {
        return Err(Error::BadRequest(format!(
            "unsupported trace kind {}",
            upload.kind
        )));
    }
    if upload.started_at.trim().is_empty() {
        return Err(Error::BadRequest("trace.started_at is empty".to_string()));
    }
    if upload.duration_ms == 0 {
        return Err(Error::BadRequest(
            "trace.duration_ms must be positive".to_string(),
        ));
    }
    if upload.duration_ms > crate::MAX_VIDEO_DURATION_MS + 5_000 {
        return Err(Error::BadRequest(
            "trace is longer than the video limit".to_string(),
        ));
    }
    if let Some(video) = video_duration_ms {
        if upload.duration_ms > video.saturating_add(5_000) {
            return Err(Error::BadRequest(
                "trace is longer than the attached video".to_string(),
            ));
        }
    }
    if upload.events.len() > MAX_TRACE_EVENTS {
        return Err(Error::BadRequest(format!(
            "trace has more than {MAX_TRACE_EVENTS} events"
        )));
    }

    let mut summary = TraceSummary::default();
    let mut highlights = Vec::new();
    for (index, event) in upload.events.iter().enumerate() {
        let object = event
            .as_object()
            .ok_or_else(|| Error::BadRequest(format!("trace event {index} is not an object")))?;
        let t = object
            .get("t")
            .and_then(Value::as_f64)
            .filter(|t| t.is_finite() && *t >= 0.0)
            .map(|t| t.round() as u64)
            .ok_or_else(|| Error::BadRequest(format!("trace event {index} has no t")))?;
        if t > upload.duration_ms + 2_000 {
            return Err(Error::BadRequest(format!(
                "trace event {index} is after the recording"
            )));
        }
        let kind = object
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::BadRequest(format!("trace event {index} has no kind")))?;
        match kind {
            "console" => {
                let _level = trace_string(object, "level", 32, index)?;
                let text = trace_string(object, "text", MAX_TRACE_TEXT, index)?;
                summary.console += 1;
                if _level == "error" {
                    summary.console_errors += 1;
                    push_highlight(&mut highlights, t, "console", &text);
                }
            }
            "exception" => {
                let text = trace_string(object, "text", MAX_TRACE_TEXT, index)?;
                summary.console += 1;
                summary.console_errors += 1;
                push_highlight(&mut highlights, t, "exception", &text);
            }
            "network" => {
                let method = trace_string(object, "method", 16, index)?;
                let url = trace_string(object, "url", MAX_TRACE_TEXT, index)?;
                if ![
                    "http://", "https://", "ws://", "wss://", "data:", "blob:", "file:", "about:",
                ]
                .iter()
                .any(|prefix| url.starts_with(prefix))
                {
                    return Err(Error::BadRequest(format!(
                        "trace event {index} has an unsupported url"
                    )));
                }
                let status = object
                    .get("status")
                    .and_then(Value::as_u64)
                    .unwrap_or_default();
                let failed = object
                    .get("failed")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                    || status >= 400;
                summary.network += 1;
                if failed {
                    summary.network_failed += 1;
                    push_highlight(
                        &mut highlights,
                        t,
                        "network",
                        &format!(
                            "{method} {url} → {}",
                            if status == 0 {
                                "failed".to_string()
                            } else {
                                status.to_string()
                            }
                        ),
                    );
                }
            }
            "action" => {
                let _action = trace_string(object, "action", 64, index)?;
                let _selector = trace_string(object, "selector", MAX_TRACE_TEXT, index)?;
                summary.actions += 1;
            }
            "navigation" => {
                let _url = trace_string(object, "url", MAX_TRACE_TEXT, index)?;
            }
            "marker" => {
                let _label = trace_string(object, "label", MAX_TRACE_TEXT, index)?;
                summary.markers += 1;
            }
            other => {
                return Err(Error::BadRequest(format!(
                    "trace event {index} has unknown kind {other}"
                )))
            }
        }
    }

    Ok(TraceRef {
        file: "trace.json".to_string(),
        kind: upload.kind,
        events: upload.events.len(),
        started_at: upload.started_at,
        duration_ms: upload.duration_ms,
        summary,
        redacted: upload.redaction.is_some(),
        truncated: upload.truncated,
        highlights,
    })
}

fn trace_string(
    event: &serde_json::Map<String, Value>,
    key: &str,
    max: usize,
    index: usize,
) -> Result<String> {
    let value = event
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::BadRequest(format!("trace event {index} has no {key}")))?;
    if value.len() > max {
        return Err(Error::BadRequest(format!(
            "trace event {index} {key} is too long"
        )));
    }
    Ok(value.to_string())
}

fn push_highlight(highlights: &mut Vec<TraceHighlight>, t: u64, kind: &str, text: &str) {
    if highlights.len() >= 8 {
        return;
    }
    let text = if text.chars().count() > 240 {
        text.chars().take(239).collect::<String>() + "…"
    } else {
        text.to_string()
    };
    highlights.push(TraceHighlight {
        t,
        kind: kind.to_string(),
        text,
    });
}

/// Same rule as v0.1: one leading alphanumeric, then alphanumerics/`.`/`_`/`-`, max 121 chars.
/// This is what stops `../` and friends from escaping the sessions directory.
/// The virtual project that holds sessions created without one (legacy flat folders).
pub const INBOX: &str = "inbox";

/// What an integration reports about a handoff. Kept as an input shape so the store, not the
/// MCP surface, owns the persisted fields. `label`/`url`/`message` are integration vocabulary:
/// "merged" + a PR URL, "deployed" + an environment URL, or anything else.
#[derive(Debug, Clone)]
pub struct HandoffReport {
    pub status: HandoffStatus,
    pub label: Option<String>,
    pub url: Option<String>,
    pub message: Option<String>,
}

pub fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    id.len() <= 121 && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

pub fn slugify(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.trim().to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-');
    let limited: String = trimmed.chars().take(60).collect();
    if limited.is_empty() {
        "review".to_string()
    } else {
        limited.trim_end_matches('-').to_string()
    }
}

/// The feature branch a review gets when nobody named one: `feat/<title>-<tail>`.
///
/// A review is one feature, so a review without a branch must not invent `session/<id>` — that
/// name says nothing about what is being built and cannot be the branch an agent pushes and
/// merges. The four-character tail of the session id keeps two features that share a title apart;
/// the human can rename it in the New review dialog or from the review header (v0.22).
pub fn default_branch(title: &str, id: &str) -> String {
    let slug = slugify(&fold_latin(title));
    let slug: String = slug.chars().take(48).collect();
    let slug = slug.trim_end_matches('-');
    let tail: String = id
        .rsplit('-')
        .next()
        .unwrap_or(id)
        .chars()
        .take(4)
        .collect();
    if tail.is_empty() {
        format!("feat/{slug}")
    } else {
        format!("feat/{slug}-{tail}")
    }
}

/// `slugify` keeps ASCII only, so a Czech title would lose its letters (`Tlačítko` → `tla-tko`).
/// A branch name is read by humans and typed in git commands, so fold the common Latin accents
/// before slugifying it. Unknown characters are left to `slugify`.
fn fold_latin(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            'á' | 'à' | 'ä' | 'â' | 'ã' | 'å' => 'a',
            'č' | 'ç' => 'c',
            'ď' => 'd',
            'é' | 'è' | 'ë' | 'ê' | 'ě' => 'e',
            'í' | 'ì' | 'ï' | 'î' | 'ı' => 'i',
            'ň' | 'ñ' => 'n',
            'ó' | 'ò' | 'ö' | 'ô' | 'õ' => 'o',
            'ř' => 'r',
            'š' => 's',
            'ť' => 't',
            'ú' | 'ů' | 'ü' | 'ù' | 'û' => 'u',
            'ý' | 'ÿ' => 'y',
            'ž' => 'z',
            other => other,
        })
        .collect()
}

/// Read v0.21's `session/<id>` fallback as the feature branch the review would get today.
///
/// Claiming an unnamed review used to persist `session/<id>` into `meta.branch`; that name is not
/// a branch anybody chose, it is the absence of one. Normalise on read (like flat sessions
/// becoming the Inbox) so the rail and the agent prompt show the feature, and let the next claim
/// or rename write the real name (v0.22).
fn normalize_branch(meta: &mut Meta) {
    let legacy = meta
        .branch
        .as_deref()
        .map(|branch| meta.is_legacy_session_branch(branch))
        .unwrap_or(true);
    if legacy {
        meta.branch = Some(default_branch(&meta.title, &meta.id));
    }
}

fn effective_branch(mut meta: Meta) -> Meta {
    normalize_branch(&mut meta);
    meta
}

/// RFC 3339 with second precision and a `+00:00` offset — byte-compatible with v0.1's
/// `datetime.now(timezone.utc).isoformat(timespec="seconds")`.
pub fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, false)
}

/// A fresh requested handoff, carrying the label/URL/message from a previous failed attempt. A
/// retry must not make the rail — or the integration — forget which PR/deployment it was.
fn requested_handoff(meta: &Meta, at: String) -> Handoff {
    let mut handoff = Handoff::requested(at);
    if let Some(previous) = &meta.handoff {
        handoff.label = previous.label.clone();
        handoff.url = previous.url.clone();
        handoff.message = previous.message.clone();
    }
    handoff
}

/// Image extensions we are willing to write into a session folder. Anything else is stored as
/// `.png` rather than trusted: the value ends up in a filename.
fn image_extension(name: &str) -> &'static str {
    match Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => ".png",
        "jpg" | "jpeg" => ".jpg",
        "webp" => ".webp",
        "gif" => ".gif",
        "svg" => ".svg",
        _ => ".png",
    }
}

/// Frame format by magic bytes; the browser sends PNG unless the 60 MB budget forced JPEG.
fn frame_extension(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some(("png", "image/png"))
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(("jpg", "image/jpeg"))
    } else {
        None
    }
}

/// Validate a video/poster/duration before any folder is created. Returns the container
/// extension so callers write the same file name the media reference describes.
fn validate_video(
    video: &[u8],
    poster: Option<&[u8]>,
    duration_ms: Option<u64>,
) -> Result<&'static str> {
    if video.is_empty() {
        return Err(Error::BadRequest("empty video".to_string()));
    }
    if video.len() > MAX_VIDEO_BYTES {
        return Err(Error::BadRequest(format!(
            "video is larger than {} MB",
            MAX_VIDEO_BYTES / (1024 * 1024)
        )));
    }
    if poster.is_some_and(|bytes| bytes.len() > crate::MAX_POSTER_BYTES) {
        return Err(Error::BadRequest(format!(
            "poster is larger than {} MB",
            crate::MAX_POSTER_BYTES / (1024 * 1024)
        )));
    }
    if duration_ms.is_some_and(|duration| duration > crate::MAX_VIDEO_DURATION_MS) {
        return Err(Error::BadRequest(
            "video is longer than the 5 minute limit".to_string(),
        ));
    }
    video_extension(video)
        .ok_or_else(|| Error::BadRequest("unsupported video format (MP4 or WebM)".to_string()))
}

/// Video format by magic bytes, not by the uploaded name (which never reaches the filename).
fn video_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        // HEIC/AVIF share the ISO-BMFF box layout but are still images; a poster mislabelled as
        // a video would create a session that cannot play.
        match &bytes[8..12] {
            b"heic" | b"heix" | b"heim" | b"heis" | b"mif1" | b"msf1" | b"avif" | b"avis" => None,
            _ => Some("mp4"),
        }
    } else if bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        Some("webm")
    } else {
        None
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| Error::BadRequest(format!("invalid path: {}", path.display())))?;
    let temp = path.with_file_name(format!(".{file_name}.tmp"));
    fs::write(&temp, bytes)?;
    fs::rename(&temp, path).inspect_err(|_| {
        let _ = fs::remove_file(&temp);
    })?;
    Ok(())
}

fn write_json(path: &Path, value: &Value) -> Result<()> {
    let mut text = serde_json::to_string_pretty(value)?;
    text.push('\n');
    write_atomic(path, text.as_bytes())
}

fn read_json(path: &Path) -> Option<Value> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Numbers render the way Python's `f"{x}"` did: `12` not `12.0`, `12.5` kept.
fn num(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

/// `mm:ss.mmm` (or `hh:mm:ss.mmm` for long clips) for video timecodes.
fn timecode(seconds: f64) -> String {
    let total = (seconds.max(0.0) * 1000.0).round() as u64;
    let ms = total % 1000;
    let total_seconds = total / 1000;
    let s = total_seconds % 60;
    let m = (total_seconds / 60) % 60;
    let h = total_seconds / 3600;
    if h > 0 {
        format!("{h:02}:{m:02}:{s:02}.{ms:03}")
    } else {
        format!("{m:02}:{s:02}.{ms:03}")
    }
}

/// The `@ mm:ss.mmm` (or `@ start–end`) prefix for a video annotation.
fn annotation_time(annotation: &Annotation) -> String {
    match (annotation.at(), annotation.until()) {
        (Some(at), Some(until)) => {
            format!(" @ {}–{}", timecode(at), timecode(until.max(at)))
        }
        (Some(at), None) => format!(" @ {}", timecode(at)),
        _ => String::new(),
    }
}

pub fn review_markdown(meta: &Meta, annotations: &[Annotation], frames: &[FrameRef]) -> String {
    let mut lines = vec![format!("# Visual review: {}", meta.title), String::new()];
    if !meta.url.is_empty() {
        lines.push(format!("- Page: {}", meta.url));
    }
    if !meta.note.is_empty() {
        lines.push(format!("- Request: {}", meta.note));
    }
    if let Some(prompt) = meta.prompt.as_deref().filter(|p| !p.trim().is_empty()) {
        lines.push(format!("- Prompt: {}", prompt));
    }
    if let Some(media) = &meta.media {
        lines.push(format!(
            "- Media: {:?}{}{}",
            media.kind,
            media
                .duration_ms
                .map(|duration| format!(", duration {}", timecode(duration as f64 / 1000.0)))
                .unwrap_or_default(),
            media
                .width
                .zip(media.height)
                .map(|(w, h)| format!(", {w}×{h}"))
                .unwrap_or_default(),
        ));
    }
    lines.push(format!("- Status: {}", meta.status.as_str()));
    lines.push(format!(
        "- Submitted: {}",
        meta.submitted_at.as_deref().unwrap_or("-")
    ));
    lines.push(String::new());
    if let Some(trace) = &meta.trace {
        lines.push("## Browser trace".to_string());
        lines.push(String::new());
        lines.push(format!(
            "- {} · {} actions · {} requests ({} failed) · {} console messages ({} errors) · {} markers{}",
            timecode(trace.duration_ms as f64 / 1000.0),
            trace.summary.actions,
            trace.summary.network,
            trace.summary.network_failed,
            trace.summary.console,
            trace.summary.console_errors,
            trace.summary.markers,
            if trace.truncated { " · truncated" } else { "" },
        ));
        if !trace.highlights.is_empty() {
            lines.push(String::new());
            lines.push("### Trace highlights".to_string());
            lines.push(String::new());
            for highlight in &trace.highlights {
                lines.push(format!(
                    "- @ {} {} — {}",
                    timecode(highlight.t as f64 / 1000.0),
                    highlight.kind,
                    highlight.text
                ));
            }
        }
        lines.push(String::new());
    }
    lines.push("## Annotations".to_string());
    lines.push(String::new());

    let text_notes: Vec<&Annotation> = annotations
        .iter()
        .filter(|a| a.is_text() && a.text_of().is_some_and(|t| !t.is_empty()))
        .collect();
    if !text_notes.is_empty() {
        lines.push("### Text notes".to_string());
        lines.push(String::new());
        for note in &text_notes {
            lines.push(format!(
                "- ({}, {}){}: {}",
                num(note.number("x").unwrap_or_default()),
                num(note.number("y").unwrap_or_default()),
                annotation_time(note),
                note.text_of().unwrap_or_default()
            ));
        }
        lines.push(String::new());
    }

    // Numbers must match the badges the human sees on the canvas and on `annotated.png`, so they
    // are the position in the full annotation list, not in the filtered shape list.
    let shapes: Vec<(usize, &Annotation)> = annotations
        .iter()
        .enumerate()
        .filter(|(_, annotation)| !annotation.is_text())
        .collect();
    if !shapes.is_empty() {
        lines.push("### Marked regions".to_string());
        lines.push(String::new());
        for (index, item) in shapes {
            let name = match item.kind.as_str() {
                "rect" => "rectangle",
                "pen" => "freehand",
                other => other,
            };
            let coords = match (item.point("start"), item.point("end")) {
                (Some((sx, sy)), Some((ex, ey))) => {
                    format!(" ({}, {}) → ({}, {})", num(sx), num(sy), num(ex), num(ey))
                }
                (Some((sx, sy)), None) => format!(" ({}, {})", num(sx), num(sy)),
                _ => String::new(),
            };
            let note = item.note_of().unwrap_or("").trim();
            let note = if note.is_empty() {
                String::new()
            } else {
                format!(" — \u{201c}{note}\u{201d}")
            };
            lines.push(format!(
                "- {}. {}{}{}{}",
                index + 1,
                name,
                coords,
                annotation_time(item),
                note
            ));
        }
        lines.push(String::new());
    }

    if !frames.is_empty() {
        lines.push("### Frames".to_string());
        lines.push(String::new());
        for frame in frames {
            lines.push(format!("- {}: {}", timecode(frame.at), frame.file));
        }
        lines.push(String::new());
    }

    lines.push("## For the agent".to_string());
    lines.push(String::new());
    if meta
        .media
        .as_ref()
        .is_some_and(|media| media.kind == MediaKind::Video)
    {
        lines.push(
            "Use `annotations.json` for coordinates, text, `at`/`until` and the `frames` list; \
             `annotated.png` is a contact sheet of the annotated moments and `frames/` holds the \
             full-resolution stills. The video itself is available at `/sessions/<id>/media`."
                .to_string(),
        );
    } else {
        lines.push(
            "Use `annotations.json` for coordinates and text; `annotated.png` is the visual reference."
                .to_string(),
        );
    }
    lines.push(String::new());
    lines.join("\n")
}

/// One archived submission: the JSON and (optionally) the PNG from an earlier round.
#[derive(Debug, Clone, serde::Serialize)]
pub struct HistoryEntry {
    pub stamp: String,
    pub annotations: usize,
    pub png: bool,
    /// The round's source screenshot (`<stamp>-source.png`) is available for the read-only viewer.
    #[serde(default)]
    pub source: bool,
    /// The round's media: a video round has its clip under `history/<stamp>/video.*`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<MediaRef>,
    /// Number of full-resolution frames captured with this round.
    #[serde(default)]
    pub frames: usize,
    /// 1-based round number, oldest first.
    #[serde(default)]
    pub round: usize,
    /// This entry is the submission the session is currently parked on.
    #[serde(default)]
    pub current: bool,
    /// When the human submitted this round. Missing on v0.1 archives.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    /// When an agent actually picked this round up: its first event after the submission. The
    /// submission stamp only says when the round entered the queue, which can be long before
    /// anyone claimed it (2026-10-31). Missing when no agent event falls inside the round.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_started_at: Option<String>,
    /// When the round left the agent's hands: its next picture, the next human submission, or
    /// `resolve`. Missing while this is still the round being worked on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    /// How long the agent *ran* this round, in seconds (2026-10-31): from `run_started_at`, not
    /// from the submission, so time spent waiting in the queue is not counted as work. Derived,
    /// never stored — the archive and `agent-events.jsonl` keep the stamps it is built from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<u64>,
}

/// The newest round's own timing, for the rail and the conversation summary.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LastRun {
    pub started_at: String,
    pub finished_at: String,
    pub duration_secs: u64,
}

/// Seconds between two stamps from this server's own clock. A stamp it cannot parse (a
/// hand-edited round, an older client) yields `None` rather than a wrong duration.
fn seconds_between(start: &str, end: &str) -> Option<u64> {
    let start = chrono::DateTime::parse_from_rfc3339(start).ok()?;
    let end = chrono::DateTime::parse_from_rfc3339(end).ok()?;
    (end - start).num_seconds().try_into().ok()
}

/// When a round left the agent's hands: its next picture (`post_round`), the human's next
/// submission, or `resolve`. Only the live round can have been resolved, so callers pass
/// `resolved_at` for the newest one. Shared by [`Store::history`] and [`Store::last_run`] so the
/// two can never disagree about the end of a run.
fn round_finished(
    started: &str,
    next_submit: Option<&str>,
    handed_back: &[String],
    resolved_at: Option<&str>,
) -> Option<String> {
    let mut ends: Vec<String> = Vec::new();
    // `>=`: a submit and a hand-back in the same second is a real (zero-second) round.
    if let Some(at) = handed_back
        .iter()
        .find(|at| at.as_str() >= started && next_submit.map_or(true, |next| at.as_str() <= next))
    {
        ends.push(at.clone());
    }
    if let Some(next) = next_submit {
        ends.push(next.to_string());
    }
    if let Some(resolved) = resolved_at.filter(|at| *at >= started) {
        ends.push(resolved.to_string());
    }
    ends.into_iter().min()
}

/// The first thing an agent said inside a round: the run's honest start. `None` when nobody
/// demonstrably ran it.
fn run_start(run_times: &[String], started: &str, finished: &str) -> Option<String> {
    run_times
        .iter()
        .find(|at| at.as_str() >= started && at.as_str() <= finished)
        .cloned()
}

fn safe_stamp(stamp: &str) -> String {
    stamp
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => c,
            _ => '-',
        })
        .collect()
}

/// Keep the submission we are about to overwrite. Without this, "Submit again" destroys the
/// previous round and a re-review has nothing to diff against.
/// Two submissions can land in the same second; the archive name must still be unique.
fn unique_round_stamp(dir: &std::path::Path, at: &str) -> String {
    let base = safe_stamp(at);
    let history = dir.join("history");
    let mut candidate = base.clone();
    let mut counter = 1;
    while history.join(format!("{candidate}.json")).exists() {
        counter += 1;
        candidate = format!("{base}-{counter}");
    }
    candidate
}

/// Snapshot a submitted round into `history/` **at submit time**, so History fills up immediately
/// and a round can be revisited read-only. Empty rounds are kept: a prompt without annotations is
/// still a round.
fn archive_round(
    dir: &std::path::Path,
    stamp: &str,
    set: &AnnotationSet,
    source: &str,
) -> Result<()> {
    let history = dir.join("history");
    fs::create_dir_all(&history)?;
    let safe = safe_stamp(stamp);
    write_json(
        &history.join(format!("{safe}.json")),
        &serde_json::to_value(set)?,
    )?;
    let png = dir.join("annotated.png");
    if png.is_file() {
        let bytes = fs::read(&png)?;
        write_atomic(&history.join(format!("{safe}.png")), &bytes)?;
    }
    // The source screenshot lets the viewer re-render the round crisply and read-only. Videos
    // need a different source path/history contract (phase 3); never write video bytes into a
    // `.png`, which would produce a broken viewer image.
    let is_video = set.media.as_ref().map(|media| media.kind) == Some(MediaKind::Video);
    let source_path = dir.join(source);
    if !is_video && source_path.is_file() {
        let bytes = fs::read(&source_path)?;
        write_atomic(&history.join(format!("{safe}-source.png")), &bytes)?;
    }
    // The live `frames/` directory is replaced by the next submission, so the full-resolution
    // stills of this round must be copied beside it or they are lost for the agent.
    if !set.frames.is_empty() {
        let frames = history.join(format!("{safe}-frames"));
        if frames.exists() {
            fs::remove_dir_all(&frames)?;
        }
        fs::create_dir_all(&frames)?;
        for frame in &set.frames {
            let name = Path::new(&frame.file)
                .file_name()
                .map(|name| name.to_owned())
                .unwrap_or_default();
            let bytes = fs::read(dir.join(&frame.file))?;
            write_atomic(&frames.join(name), &bytes)?;
        }
    }
    prune_history(&history, 20)?;
    Ok(())
}

/// Store the frames of a submission and return their references. An identical millisecond is
/// captured once even if two annotations share it. Replacing the directory is safe because the
/// previous round was archived on its submit.
fn write_frames(dir: &Path, frames: &[FrameUpload]) -> Result<Vec<FrameRef>> {
    let folder = dir.join("frames");
    if folder.exists() {
        fs::remove_dir_all(&folder)?;
    }
    if frames.is_empty() {
        return Ok(Vec::new());
    }
    fs::create_dir_all(&folder)?;
    let mut refs = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for frame in frames {
        let key = (frame.at * 1000.0).round() as i64;
        if !seen.insert(key) {
            continue;
        }
        let (extension, mime) = frame_extension(&frame.bytes)
            .ok_or_else(|| Error::BadRequest("frames must be PNG or JPEG images".to_string()))?;
        let name = format!("{:.3}.{extension}", key as f64 / 1000.0);
        write_atomic(&folder.join(&name), &frame.bytes)?;
        refs.push(FrameRef {
            at: key as f64 / 1000.0,
            file: format!("frames/{name}"),
            mime: Some(mime.to_string()),
        });
    }
    refs.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
    Ok(refs)
}

/// Whether two archived rounds carry the same picture. Two rounds without a picture are the same
/// picture (an annotations-only round).
fn same_picture(history: &Path, a: &str, b: &str) -> bool {
    let file = |stamp: &str| history.join(format!("{stamp}.png"));
    match (fs::read(file(a)), fs::read(file(b))) {
        (Ok(a), Ok(b)) => a == b,
        (Err(_), Err(_)) => true,
        _ => false,
    }
}

fn prune_history(history: &std::path::Path, keep: usize) -> Result<()> {
    let mut stamps: Vec<String> = fs::read_dir(history)?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension().and_then(|e| e.to_str()) == Some("json"))
                .then(|| {
                    path.file_stem()
                        .and_then(|s| s.to_str())
                        .map(str::to_string)
                })
                .flatten()
        })
        .collect();
    stamps.sort();
    while stamps.len() > keep {
        let oldest = stamps.remove(0);
        let _ = fs::remove_file(history.join(format!("{oldest}.json")));
        let _ = fs::remove_file(history.join(format!("{oldest}.png")));
        let _ = fs::remove_file(history.join(format!("{oldest}-source.png")));
        let _ = fs::remove_dir_all(history.join(format!("{oldest}-frames")));
        let _ = fs::remove_dir_all(history.join(format!("{oldest}")));
    }
    Ok(())
}

/// A session folder root. Cheap to clone; holds no open handles.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Parent of `sessions/`: server-level state such as agent settings and claims lives here.
    pub fn data_dir(&self) -> PathBuf {
        self.root
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.root.clone())
    }

    pub fn ensure(&self) -> Result<()> {
        fs::create_dir_all(&self.root)?;
        Ok(())
    }

    pub fn dir(&self, id: &str) -> Result<PathBuf> {
        if !valid_id(id) {
            return Err(Error::InvalidId);
        }
        let flat = self.root.join(id);
        if flat.is_dir() {
            return Ok(flat);
        }
        // Project sessions live one level deeper: projects/<slug>/<id>.
        if let Ok(entries) = fs::read_dir(self.projects_root()) {
            for entry in entries.flatten() {
                let candidate = entry.path().join(id);
                if candidate.is_dir() {
                    return Ok(candidate);
                }
            }
        }
        Ok(flat)
    }

    pub fn projects_root(&self) -> PathBuf {
        self.root.join("projects")
    }

    /// Read a project by slug (name or slug accepted for the caller's convenience).
    pub fn project(&self, name: &str) -> Result<Option<crate::model::Project>> {
        let slug = slugify(name);
        let path = self.projects_root().join(&slug).join("project.json");
        Ok(read_json(&path).and_then(|value| serde_json::from_value(value).ok()))
    }

    /// Create the project folder if it does not exist yet. Idempotent: agents pass a name.
    pub fn ensure_project(&self, name: &str) -> Result<crate::model::Project> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::BadRequest("project name is required".to_string()));
        }
        if let Some(existing) = self.project(name)? {
            return Ok(existing);
        }
        let slug = slugify(name);
        let dir = self.projects_root().join(&slug);
        fs::create_dir_all(&dir)?;
        let project = crate::model::Project {
            slug: slug.clone(),
            name: name.to_string(),
            agent: None,
            repo: None,
            max_parallel: None,
            created_at: now(),
        };
        write_json(&dir.join("project.json"), &serde_json::to_value(&project)?)?;
        Ok(project)
    }

    /// Set or clear a project's concurrent-run override (v0.22).
    pub fn set_project_max_parallel(
        &self,
        slug: &str,
        max_parallel: Option<u32>,
    ) -> Result<crate::model::Project> {
        let slug = slugify(slug);
        let mut project = self
            .project(&slug)?
            .ok_or_else(|| Error::NotFound(slug.clone()))?;
        project.max_parallel = max_parallel;
        write_json(
            &self
                .projects_root()
                .join(&project.slug)
                .join("project.json"),
            &serde_json::to_value(&project)?,
        )?;
        Ok(project)
    }

    /// Bind a repository discovered by an agent to a project. Repo keys are global, so a second
    /// agent reporting the same key updates the record instead of creating a duplicate project.
    /// The slug is derived once from the key and never changes.
    pub fn sync_repo_project(
        &self,
        agent: &str,
        repo: &str,
        name: &str,
    ) -> Result<crate::model::Project> {
        let name = name.trim();
        let name = if name.is_empty() {
            repo.rsplit('/').next().unwrap_or(repo)
        } else {
            name
        };
        if let Some(mut project) = self
            .projects()?
            .into_iter()
            .find(|project| project.repo.as_deref() == Some(repo))
        {
            let mut changed = false;
            if project.name != name {
                project.name = name.to_string();
                changed = true;
            }
            if project.agent.as_deref() != Some(agent) {
                project.agent = Some(agent.to_string());
                changed = true;
            }
            if changed {
                let path = self
                    .projects_root()
                    .join(&project.slug)
                    .join("project.json");
                write_json(&path, &serde_json::to_value(&project)?)?;
            }
            return Ok(project);
        }

        let base = slugify(repo);
        let mut slug = base.clone();
        let mut suffix = 2;
        while self.project(&slug)?.is_some() {
            slug = format!("{base}-{suffix}");
            suffix += 1;
        }
        let dir = self.projects_root().join(&slug);
        fs::create_dir_all(&dir)?;
        let project = crate::model::Project {
            slug: slug.clone(),
            name: name.to_string(),
            agent: Some(agent.to_string()),
            repo: Some(repo.to_string()),
            max_parallel: None,
            created_at: now(),
        };
        write_json(&dir.join("project.json"), &serde_json::to_value(&project)?)?;
        Ok(project)
    }

    /// Every project, plus the virtual Inbox that holds legacy flat sessions.
    pub fn projects(&self) -> Result<Vec<crate::model::Project>> {
        let mut out: Vec<crate::model::Project> = Vec::new();
        if let Ok(entries) = fs::read_dir(self.projects_root()) {
            for entry in entries.flatten() {
                if !entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                    continue;
                }
                if let Some(project) = read_json(&entry.path().join("project.json"))
                    .and_then(|value| serde_json::from_value(value).ok())
                {
                    out.push(project);
                }
            }
        }
        if !out.iter().any(|project| project.slug == INBOX) {
            out.push(crate::model::Project {
                slug: INBOX.to_string(),
                name: "Inbox".to_string(),
                agent: None,
                repo: None,
                max_parallel: None,
                created_at: String::new(),
            });
        }
        out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(out)
    }

    pub fn get(&self, id: &str) -> Result<Option<Meta>> {
        let dir = self.dir(id)?;
        Ok(read_json(&dir.join("meta.json"))
            .and_then(|value| serde_json::from_value(value).ok())
            .map(effective_branch))
    }

    pub fn require(&self, id: &str) -> Result<Meta> {
        self.get(id)?.ok_or_else(|| Error::NotFound(id.to_string()))
    }

    pub fn list(&self) -> Result<Vec<Meta>> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }
        let mut metas = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() || entry.file_name() == "projects" {
                continue;
            }
            if let Some(meta) = read_json(&entry.path().join("meta.json"))
                .and_then(|value| serde_json::from_value::<Meta>(value).ok())
            {
                metas.push(meta);
            }
        }
        if let Ok(projects) = fs::read_dir(self.projects_root()) {
            for project in projects.flatten() {
                if !project
                    .file_type()
                    .map(|kind| kind.is_dir())
                    .unwrap_or(false)
                {
                    continue;
                }
                let Ok(sessions) = fs::read_dir(project.path()) else {
                    continue;
                };
                for entry in sessions.flatten() {
                    if !entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                        continue;
                    }
                    if let Some(meta) = read_json(&entry.path().join("meta.json"))
                        .and_then(|value| serde_json::from_value::<Meta>(value).ok())
                    {
                        metas.push(meta);
                    }
                }
            }
        }
        // Flat sessions are the Inbox: no folder, no migration, just a label on read.
        for meta in &mut metas {
            if meta.project.is_none() {
                meta.project = Some(INBOX.to_string());
            }
            normalize_branch(meta);
        }
        metas.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(metas)
    }

    pub fn annotations(&self, id: &str) -> Result<AnnotationSet> {
        let dir = self.dir(id)?;
        Ok(read_json(&dir.join("annotations.json"))
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_else(|| AnnotationSet {
                session: id.to_string(),
                title: None,
                url: None,
                image: None,
                media: None,
                trace: None,
                frames: Vec::new(),
                annotations: Vec::new(),
                submitted_at: None,
            }))
    }

    /// Record what the agent is doing. `state` is `received`, `working` or `replied`; `seen`
    /// also stamps the first time the agent read this submission.
    pub fn set_agent_state(&self, id: &str, state: &str, seen: bool) -> Result<Meta> {
        let dir = self.dir(id)?;
        let mut meta = self.require(id)?;
        meta.agent_state = Some(state.to_string());
        // A new run (or a reply) clears the previous failure; only `fail_round` keeps one.
        if state != "failed" {
            meta.agent_failure = None;
        }
        if seen && meta.seen_by_agent_at.is_none() {
            meta.seen_by_agent_at = Some(now());
        }
        write_json(&dir.join("meta.json"), &serde_json::to_value(&meta)?)?;
        Ok(meta)
    }

    /// The agent could not run this round. That is neither "working" (nothing is running) nor
    /// "replied" (the agent has no answer to give), and it must not leave the review locked:
    /// a human who cannot edit a round nobody is working on is stuck (2026-10-31). The round goes
    /// back to the human, the annotations stay, and `agent_state=failed` keeps the reason visible.
    pub fn fail_round(&self, id: &str, code: &str, message: &str) -> Result<Meta> {
        let dir = self.dir(id)?;
        let mut meta = self.require(id)?;
        if meta.status == Status::Submitted {
            meta.status = Status::Open;
        }
        meta.agent_state = Some("failed".to_string());
        meta.agent_failure = Some(AgentFailure {
            code: code.to_string(),
            message: message.to_string(),
            at: now(),
        });
        if meta.seen_by_agent_at.is_none() {
            meta.seen_by_agent_at = Some(now());
        }
        write_json(&dir.join("meta.json"), &serde_json::to_value(&meta)?)?;
        Ok(meta)
    }

    /// Set or clear the feature branch shown in the rail and used as the branch-lock key.
    pub fn set_branch(&self, id: &str, branch: Option<&str>) -> Result<Meta> {
        let dir = self.dir(id)?;
        let mut meta = self.require(id)?;
        let branch = branch
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        if let Some(value) = branch.as_deref() {
            if value.len() > 200 {
                return Err(Error::BadRequest("branch is too long".to_string()));
            }
            if value.chars().any(char::is_control) {
                return Err(Error::BadRequest(
                    "branch must not contain control characters".to_string(),
                ));
            }
        }
        meta.branch = branch;
        write_json(&dir.join("meta.json"), &serde_json::to_value(&meta)?)?;
        Ok(meta)
    }

    /// Append one line to `agent-events.jsonl` (the activity feed the human watches).
    pub fn append_agent_event(&self, id: &str, event: &serde_json::Value) -> Result<()> {
        use std::io::Write as _;
        let dir = self.dir(id)?;
        let path = dir.join("agent-events.jsonl");
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        writeln!(file, "{event}")?;
        Ok(())
    }

    pub fn agent_events(&self, id: &str) -> Result<Vec<serde_json::Value>> {
        let path = self.dir(id)?.join("agent-events.jsonl");
        let mut events = Vec::new();
        if path.is_file() {
            for line in fs::read_to_string(&path)?.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                    events.push(value);
                }
            }
        }
        Ok(events)
    }

    /// Create a session from bytes already in memory (MCP uploads, HTTP `image_base64`).
    #[allow(clippy::too_many_arguments)]
    pub fn create_from_bytes(
        &self,
        image: &[u8],
        image_name: &str,
        title: &str,
        url: &str,
        note: &str,
        id: Option<&str>,
        agent: Option<&str>,
        project: Option<&str>,
    ) -> Result<Meta> {
        if image.is_empty() {
            return Err(Error::BadRequest("empty image".to_string()));
        }
        self.ensure()?;
        let project = match project.map(str::trim).filter(|name| !name.is_empty()) {
            Some(name) => Some(self.ensure_project(name)?),
            None => None,
        };
        let parent = match &project {
            Some(project) => self.projects_root().join(&project.slug),
            None => self.root.clone(),
        };
        let sid = self.new_id(title, id)?;
        let dir = parent.join(&sid);
        fs::create_dir(&dir).map_err(|err| {
            if err.kind() == std::io::ErrorKind::AlreadyExists {
                Error::Exists(sid.clone())
            } else {
                Error::Io(err)
            }
        })?;

        let extension = image_extension(image_name);
        write_atomic(&dir.join(format!("screenshot{extension}")), image)?;

        let title = if title.is_empty() {
            sid.clone()
        } else {
            title.to_string()
        };
        let branch = default_branch(&title, &sid);
        let meta = Meta {
            id: sid.clone(),
            title,
            url: url.to_string(),
            note: note.to_string(),
            status: Status::Open,
            created_at: now(),
            submitted_at: None,
            image: format!("screenshot{extension}"),
            image_width: None,
            image_height: None,
            media: None,
            poster: None,
            trace: None,
            agent: agent.map(str::to_string),
            resolved_at: None,
            feature_confirmed_at: None,
            feature_cancelled_at: None,
            handoff: None,
            prompt: None,
            seen_by_agent_at: None,
            agent_state: None,
            agent_failure: None,
            cancelled_at: None,
            round_stamp: None,
            project: project.as_ref().map(|project| project.slug.clone()),
            branch: Some(branch),
        };
        write_json(&dir.join("meta.json"), &serde_json::to_value(&meta)?)?;
        write_json(
            &dir.join("annotations.json"),
            &json!({ "session": sid, "annotations": [] }),
        )?;
        Ok(meta)
    }

    /// Create a video session from bytes already in memory (MCP uploads, HTTP `video_base64`).
    /// The video is validated by magic bytes; the optional poster is served by the image route
    /// so the existing gallery/rail cards keep working.
    #[allow(clippy::too_many_arguments)]
    pub fn create_video_from_bytes(
        &self,
        video: &[u8],
        title: &str,
        url: &str,
        note: &str,
        id: Option<&str>,
        agent: Option<&str>,
        project: Option<&str>,
        poster: Option<&[u8]>,
        duration_ms: Option<u64>,
        width: Option<u32>,
        height: Option<u32>,
        fps: Option<f64>,
    ) -> Result<Meta> {
        let extension = validate_video(video, poster, duration_ms)?;
        self.ensure()?;
        let project = match project.map(str::trim).filter(|name| !name.is_empty()) {
            Some(name) => Some(self.ensure_project(name)?),
            None => None,
        };
        let parent = match &project {
            Some(project) => self.projects_root().join(&project.slug),
            None => self.root.clone(),
        };
        let sid = self.new_id(title, id)?;
        let dir = parent.join(&sid);
        fs::create_dir(&dir).map_err(|err| {
            if err.kind() == std::io::ErrorKind::AlreadyExists {
                Error::Exists(sid.clone())
            } else {
                Error::Io(err)
            }
        })?;

        let file = format!("video.{extension}");
        write_atomic(&dir.join(&file), video)?;
        let has_poster = poster.is_some_and(|bytes| !bytes.is_empty());
        if let Some(poster) = poster.filter(|bytes| !bytes.is_empty()) {
            write_atomic(&dir.join("poster.png"), poster)?;
        }

        let title = if title.is_empty() {
            sid.clone()
        } else {
            title.to_string()
        };
        let branch = default_branch(&title, &sid);
        let media = MediaRef {
            kind: MediaKind::Video,
            file: file.clone(),
            mime: Some(format!("video/{extension}")),
            width,
            height,
            duration_ms,
            fps,
        };
        let meta = Meta {
            id: sid.clone(),
            title,
            url: url.to_string(),
            note: note.to_string(),
            status: Status::Open,
            created_at: now(),
            submitted_at: None,
            image: file,
            image_width: width,
            image_height: height,
            media: Some(media),
            poster: has_poster.then(|| "poster.png".to_string()),
            trace: None,
            agent: agent.map(str::to_string),
            resolved_at: None,
            feature_confirmed_at: None,
            feature_cancelled_at: None,
            handoff: None,
            prompt: None,
            seen_by_agent_at: None,
            agent_state: None,
            agent_failure: None,
            cancelled_at: None,
            round_stamp: None,
            project: project.as_ref().map(|project| project.slug.clone()),
            branch: Some(branch),
        };
        write_json(&dir.join("meta.json"), &serde_json::to_value(&meta)?)?;
        write_json(
            &dir.join("annotations.json"),
            &json!({ "session": sid, "annotations": [] }),
        )?;
        Ok(meta)
    }

    /// Create a video session from a file on this machine (the CLI/MCP path).
    #[allow(clippy::too_many_arguments)]
    pub fn create_video_from_path(
        &self,
        video: &Path,
        title: &str,
        url: &str,
        note: &str,
        id: Option<&str>,
        project: Option<&str>,
        poster: Option<&Path>,
        duration_ms: Option<u64>,
        width: Option<u32>,
        height: Option<u32>,
        fps: Option<f64>,
    ) -> Result<Meta> {
        let size = fs::metadata(video)?.len();
        if size as usize > MAX_VIDEO_BYTES {
            return Err(Error::BadRequest(format!(
                "video is larger than {} MB",
                MAX_VIDEO_BYTES / (1024 * 1024)
            )));
        }
        let bytes = fs::read(video)?;
        let poster_bytes = match poster {
            Some(path) => {
                let size = fs::metadata(path)?.len();
                if size as usize > crate::MAX_POSTER_BYTES {
                    return Err(Error::BadRequest(format!(
                        "poster is larger than {} MB",
                        crate::MAX_POSTER_BYTES / (1024 * 1024)
                    )));
                }
                Some(fs::read(path)?)
            }
            None => None,
        };
        let title = if title.is_empty() {
            video.file_stem().and_then(|s| s.to_str()).unwrap_or("")
        } else {
            title
        };
        self.create_video_from_bytes(
            &bytes,
            title,
            url,
            note,
            id,
            None,
            project,
            poster_bytes.as_deref(),
            duration_ms,
            width,
            height,
            fps,
        )
    }

    /// Post a new video into an existing review. The current round is archived (annotations,
    /// contact sheet, frames and the clip itself); the session returns to `open` with an empty
    /// canvas so the human can annotate the new clip and submit it as the next round.
    pub fn new_video_round(
        &self,
        id: &str,
        video: &[u8],
        poster: Option<&[u8]>,
        duration_ms: Option<u64>,
        width: Option<u32>,
        height: Option<u32>,
        fps: Option<f64>,
    ) -> Result<Meta> {
        let extension = validate_video(video, poster, duration_ms)?;
        let dir = self.dir(id)?;
        let mut meta = self.require(id)?;
        let set = self.annotations(id)?;
        let has_round = meta.status == Status::Submitted
            || !set.annotations.is_empty()
            || dir.join("annotated.png").is_file()
            || !set.frames.is_empty()
            || meta.trace.is_some();
        let stamp = if has_round {
            let stamp = meta
                .round_stamp
                .clone()
                .unwrap_or_else(|| unique_round_stamp(&dir, &now()));
            archive_round(&dir, &stamp, &set, &meta.image)?;
            Some(stamp)
        } else {
            None
        };

        // Write the new clip under a temporary name before touching the old one: a failed write
        // must leave the current round playable.
        let temp = format!("video.new.{extension}");
        write_atomic(&dir.join(&temp), video)?;

        if let Some(stamp) = &stamp {
            let media_dir = dir.join("history").join(safe_stamp(stamp));
            if meta.trace.is_some() {
                fs::create_dir_all(&media_dir)?;
                let trace = dir.join("trace.json");
                if trace.is_file() {
                    fs::rename(&trace, media_dir.join("trace.json"))?;
                }
            }
            if meta.media.as_ref().map(|media| media.kind) == Some(MediaKind::Video) {
                fs::create_dir_all(&media_dir)?;
                let source = dir.join(&meta.image);
                if source.is_file() {
                    let old_extension = Path::new(&meta.image)
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .unwrap_or("webm");
                    fs::rename(&source, media_dir.join(format!("video.{old_extension}")))?;
                }
                if let Some(poster) = meta.poster.as_deref() {
                    let old_poster = dir.join(poster);
                    if old_poster.is_file() {
                        fs::rename(&old_poster, media_dir.join("poster.png"))?;
                    }
                }
            }
        }

        let file = format!("video.{extension}");
        fs::rename(dir.join(&temp), dir.join(&file))?;
        if meta.image != file && stamp.is_none() {
            let _ = fs::remove_file(dir.join(&meta.image));
        }
        let has_poster = poster.is_some_and(|bytes| !bytes.is_empty());
        if let Some(poster) = poster.filter(|bytes| !bytes.is_empty()) {
            write_atomic(&dir.join("poster.png"), poster)?;
        } else {
            let _ = fs::remove_file(dir.join("poster.png"));
        }
        let _ = fs::remove_file(dir.join("annotated.png"));
        let _ = fs::remove_file(dir.join("review.md"));
        let _ = fs::remove_dir_all(dir.join("frames"));
        write_json(
            &dir.join("annotations.json"),
            &json!({ "session": id, "annotations": [] }),
        )?;

        meta.image = file.clone();
        meta.image_width = width;
        meta.image_height = height;
        meta.media = Some(MediaRef {
            kind: MediaKind::Video,
            file,
            mime: Some(format!("video/{extension}")),
            width,
            height,
            duration_ms,
            fps,
        });
        meta.poster = has_poster.then(|| "poster.png".to_string());
        meta.trace = None;
        meta.status = Status::Open;
        meta.submitted_at = None;
        meta.round_stamp = None;
        meta.seen_by_agent_at = None;
        meta.agent_state = None;
        meta.agent_failure = None;
        // A round that lands while a handoff is queued or running is not new human work: the
        // human already decided, and dropping the confirmation here orphaned the handoff so its
        // result could never be reported (2026-10-31).
        if meta.handoff.is_none() {
            meta.feature_confirmed_at = None;
            meta.feature_cancelled_at = None;
        }
        meta.prompt = None;
        write_json(&dir.join("meta.json"), &serde_json::to_value(&meta)?)?;
        Ok(meta)
    }

    /// Create a session from a file on this machine (the CLI path).
    pub fn create_from_path(
        &self,
        image: &Path,
        title: &str,
        url: &str,
        note: &str,
        id: Option<&str>,
        project: Option<&str>,
    ) -> Result<Meta> {
        let bytes = fs::read(image)?;
        let name = image
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("screenshot.png");
        let title = if title.is_empty() {
            image.file_stem().and_then(|s| s.to_str()).unwrap_or("")
        } else {
            title
        };
        self.create_from_bytes(&bytes, name, title, url, note, id, None, project)
    }

    fn new_id(&self, title: &str, id: Option<&str>) -> Result<String> {
        if let Some(id) = id {
            if !valid_id(id) {
                return Err(Error::InvalidId);
            }
            if self.dir(id)?.is_dir() {
                return Err(Error::Exists(id.to_string()));
            }
            return Ok(id.to_string());
        }
        let stamp = Utc::now().format("%Y%m%d-%H%M%S").to_string();
        let slug = slugify(title);
        for _ in 0..8 {
            let suffix = uuid::Uuid::new_v4().simple().to_string();
            let candidate = format!("{stamp}-{slug}-{}", &suffix[..4]);
            if !self.dir(&candidate)?.is_dir() {
                return Ok(candidate);
            }
        }
        Err(Error::BadRequest(
            "could not allocate a session id".to_string(),
        ))
    }

    /// Stop a submission. The canvas stays as it is; the agent is supposed to stop.
    ///
    /// If no agent has read the submission yet the round is withdrawn from history (the human
    /// stopped it by mistake). Once an agent has seen it the round stays: it may have acted on it.
    pub fn cancel(&self, id: &str) -> Result<Meta> {
        let dir = self.dir(id)?;
        let mut meta = self.require(id)?;
        if meta.status != Status::Submitted {
            return Err(Error::BadRequest("nothing to cancel".to_string()));
        }
        if meta.seen_by_agent_at.is_none() {
            if let Some(stamp) = meta.round_stamp.take() {
                let history = dir.join("history");
                let _ = fs::remove_file(history.join(format!("{stamp}.json")));
                let _ = fs::remove_file(history.join(format!("{stamp}.png")));
                let _ = fs::remove_file(history.join(format!("{stamp}-source.png")));
                let _ = fs::remove_dir_all(history.join(format!("{stamp}-frames")));
                let _ = fs::remove_dir_all(history.join(&stamp));
            }
        }
        meta.status = Status::Open;
        meta.agent_state = None;
        meta.agent_failure = None;
        meta.cancelled_at = Some(now());
        write_json(&dir.join("meta.json"), &serde_json::to_value(&meta)?)?;
        Ok(meta)
    }

    /// Post a new round into an existing session: the agent's answer, optionally with a fresh
    /// screenshot. This is what keeps one task as one rail item instead of a new review per turn.
    ///
    /// The previous round is archived, the canvas image is replaced (old files stay for history),
    /// the annotations are cleared and the session goes back to the human (`open`).
    pub fn post_round(
        &self,
        id: &str,
        image: Option<&[u8]>,
        image_name: &str,
        note: &str,
        width: Option<u32>,
        height: Option<u32>,
    ) -> Result<Meta> {
        let dir = self.dir(id)?;
        let mut meta = self.require(id)?;
        let stamp = now();
        // A new agent round is new evidence; a trace from the previous round no longer matches.
        if meta.trace.take().is_some() {
            let _ = fs::remove_file(dir.join("trace.json"));
        }

        if image.is_some() && meta.media.as_ref().map(|media| media.kind) == Some(MediaKind::Video)
        {
            return Err(Error::BadRequest(
                "a video review cannot take a screenshot round; post the next video".to_string(),
            ));
        }

        let mut image_file = meta.image.clone();
        if let Some(bytes) = image {
            let extension = image_extension(image_name);
            image_file = format!("screenshot-{}{extension}", safe_stamp(&stamp));
            write_atomic(&dir.join(&image_file), bytes)?;
            meta.image = image_file.clone();
            meta.image_width = width;
            meta.image_height = height;
        }

        let set = AnnotationSet {
            session: id.to_string(),
            title: Some(meta.title.clone()),
            url: Some(meta.url.clone()),
            image: Some(ImageRef {
                file: Some(image_file.clone()),
                width,
                height,
            }),
            media: meta.media.clone(),
            trace: None,
            frames: Vec::new(),
            annotations: Vec::new(),
            submitted_at: None,
        };
        write_json(&dir.join("annotations.json"), &serde_json::to_value(&set)?)?;

        meta.status = Status::Open;
        meta.submitted_at = None;
        meta.resolved_at = None;
        // New work after a decision makes the feature pending again — unless the integration is
        // holding a handoff for it. That round is the agent's own result, not the human coming
        // back, and dropping the decision stranded the claim with no way to report it.
        if meta.handoff.is_none() {
            meta.feature_confirmed_at = None;
            meta.feature_cancelled_at = None;
        }
        meta.agent_state = Some("replied".to_string());
        meta.agent_failure = None;
        write_json(&dir.join("meta.json"), &serde_json::to_value(&meta)?)?;

        // The conversation keeps the round (and its picture) beside the human's annotations.
        use std::io::Write as _;
        let line = json!({ "at": stamp, "message": note, "image": image_file });
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("agent-thread.jsonl"))?;
        writeln!(file, "{line}")?;
        Ok(meta)
    }

    /// Write a submission: `annotations.json`, `meta.json`, `review.md`, `annotated.png` and the
    /// captured frames. The JSON path (images and older clients) has no frames; a video review
    /// uploads them as multipart and calls [`Store::submit_round`] with the extracted bytes.
    pub fn submit(&self, id: &str, submission: &Submission) -> Result<(Meta, AnnotationSet)> {
        let png = match &submission.png {
            Some(png) => {
                let raw = png.split_once(',').map(|(_, data)| data).unwrap_or(png);
                use base64::Engine as _;
                Some(
                    base64::engine::general_purpose::STANDARD
                        .decode(raw.trim())
                        .map_err(|err| Error::BadRequest(format!("invalid png payload: {err}")))?,
                )
            }
            None => None,
        };
        self.submit_round(id, submission, png.as_deref(), &[])
    }

    /// The single submit choke point: validates everything first, then writes the frames, the
    /// contact sheet, the annotations, the meta and the archive.
    pub fn submit_round(
        &self,
        id: &str,
        submission: &Submission,
        contact_sheet: Option<&[u8]>,
        frames: &[FrameUpload],
    ) -> Result<(Meta, AnnotationSet)> {
        if frames.len() > MAX_FRAME_COUNT {
            return Err(Error::BadRequest(format!(
                "at most {MAX_FRAME_COUNT} annotated timestamps per submission"
            )));
        }
        let total: usize = frames.iter().map(|frame| frame.bytes.len()).sum();
        if total > MAX_FRAMES_BYTES {
            return Err(Error::BadRequest(format!(
                "frames are larger than {} MB",
                MAX_FRAMES_BYTES / (1024 * 1024)
            )));
        }
        if frames
            .iter()
            .any(|frame| !frame.at.is_finite() || frame.at < 0.0)
        {
            return Err(Error::BadRequest(
                "frame timecodes must be finite and non-negative".to_string(),
            ));
        }

        let dir = self.dir(id)?;
        let mut meta = self.require(id)?;
        let submitted_at = now();
        let round_stamp = unique_round_stamp(&dir, &submitted_at);
        meta.round_stamp = Some(round_stamp.clone());
        meta.status = Status::Submitted;
        meta.submitted_at = Some(submitted_at.clone());
        // A fresh round has not been seen by the agent yet, and it reopens a decided feature.
        meta.seen_by_agent_at = None;
        meta.agent_state = None;
        meta.agent_failure = None;
        meta.feature_confirmed_at = None;
        meta.feature_cancelled_at = None;
        meta.handoff = None;
        if let Some(prompt) = submission.note.as_deref() {
            let prompt = prompt.trim();
            if !prompt.is_empty() {
                meta.prompt = Some(prompt.to_string());
            }
        }
        if let Some(width) = submission.width {
            meta.image_width = Some(width);
        }
        if let Some(height) = submission.height {
            meta.image_height = Some(height);
        }

        let frame_refs = write_frames(&dir, frames)?;
        let set = AnnotationSet {
            session: id.to_string(),
            title: Some(meta.title.clone()),
            url: Some(meta.url.clone()),
            image: Some(ImageRef {
                file: Some(meta.image.clone()),
                width: submission.width,
                height: submission.height,
            }),
            media: meta.media.clone(),
            trace: meta.trace.clone(),
            frames: frame_refs,
            annotations: submission.annotations.clone(),
            submitted_at: Some(submitted_at.clone()),
        };

        write_json(&dir.join("annotations.json"), &serde_json::to_value(&set)?)?;
        write_json(&dir.join("meta.json"), &serde_json::to_value(&meta)?)?;
        write_atomic(
            &dir.join("review.md"),
            review_markdown(&meta, &set.annotations, &set.frames).as_bytes(),
        )?;

        if let Some(bytes) = contact_sheet {
            if !bytes.is_empty() {
                write_atomic(&dir.join("annotated.png"), bytes)?;
            }
        }

        // The round exists as soon as it is submitted, not when the next one replaces it.
        archive_round(&dir, &round_stamp, &set, &meta.image)?;
        Ok((meta, set))
    }

    /// Mark a session resolved (the agent is done with it). Additive status, not destructive.
    pub fn resolve(&self, id: &str) -> Result<Meta> {
        let dir = self.dir(id)?;
        let mut meta = self.require(id)?;
        meta.status = Status::Resolved;
        meta.resolved_at = Some(now());
        write_json(&dir.join("meta.json"), &serde_json::to_value(&meta)?)?;
        Ok(meta)
    }

    /// Undo `resolve` and any feature decision: back to the open queue.
    pub fn reopen(&self, id: &str) -> Result<Meta> {
        let dir = self.dir(id)?;
        let mut meta = self.require(id)?;
        meta.status = Status::Open;
        meta.resolved_at = None;
        meta.feature_confirmed_at = None;
        meta.feature_cancelled_at = None;
        meta.handoff = None;
        write_json(&dir.join("meta.json"), &serde_json::to_value(&meta)?)?;
        Ok(meta)
    }

    /// The human says the feature is complete. RedLine records the decision and, when the human
    /// asked for one, queues a generic **handoff**. An integration claims it and does whatever
    /// its workflow means (PR + merge, deploy, publish, …); RedLine never runs git itself.
    /// Repeating the confirmation retries a failed handoff; a done handoff is left alone.
    pub fn confirm_feature(&self, id: &str, handoff: bool) -> Result<Meta> {
        let dir = self.dir(id)?;
        let mut meta = self.require(id)?;
        let stamp = now();
        meta.status = Status::Resolved;
        meta.resolved_at.get_or_insert_with(|| stamp.clone());
        meta.feature_confirmed_at = Some(stamp.clone());
        meta.feature_cancelled_at = None;
        if handoff {
            if !meta.handoff.as_ref().is_some_and(Handoff::is_done) {
                let next = requested_handoff(&meta, stamp);
                meta.handoff = Some(next);
            }
        } else if !meta.handoff.as_ref().is_some_and(Handoff::is_done) {
            // Confirming without a handoff clears a stale failed/queued one.
            meta.handoff = None;
        }
        write_json(&dir.join("meta.json"), &serde_json::to_value(&meta)?)?;
        Ok(meta)
    }

    /// Queue (or retry) a handoff for a confirmed feature. Used by the rail's retry action and
    /// by callers that need to hand off a feature confirmed before v0.23. A retry keeps the
    /// label/URL/message from the failed attempt so the integration can reuse them.
    pub fn request_handoff(&self, id: &str) -> Result<Meta> {
        let dir = self.dir(id)?;
        let mut meta = self.require(id)?;
        if meta.feature_confirmed_at.is_none() {
            return Err(Error::BadRequest(
                "confirm the feature before handing it off".to_string(),
            ));
        }
        if meta.handoff.as_ref().is_some_and(Handoff::is_done) {
            return Ok(meta);
        }
        let next = requested_handoff(&meta, now());
        meta.handoff = Some(next);
        write_json(&dir.join("meta.json"), &serde_json::to_value(&meta)?)?;
        Ok(meta)
    }

    /// The integration claimed the handoff. Idempotent: an already done/failed handoff is left
    /// as reported, so a duplicate claim cannot rewind the rail.
    pub fn mark_handoff_running(&self, id: &str) -> Result<Meta> {
        let dir = self.dir(id)?;
        let mut meta = self.require(id)?;
        let Some(mut handoff) = meta.handoff.take() else {
            return Err(Error::BadRequest("no handoff is queued".to_string()));
        };
        if !handoff.status.claimable() {
            meta.handoff = Some(handoff);
            return Ok(meta);
        }
        handoff.status = HandoffStatus::Running;
        handoff.started_at.get_or_insert_with(now);
        meta.handoff = Some(handoff);
        write_json(&dir.join("meta.json"), &serde_json::to_value(&meta)?)?;
        Ok(meta)
    }

    /// Record what the integration's handoff did. `done` and `failed` are terminal; the MCP
    /// surface validates that. A report for a feature the human reopened or cancelled in the
    /// meantime is refused rather than resurrecting a dead decision.
    pub fn report_handoff(&self, id: &str, report: HandoffReport) -> Result<Meta> {
        let dir = self.dir(id)?;
        let mut meta = self.require(id)?;
        // Refuse only a dead decision: the human cancelled the feature, or reopened it and its
        // handoff went with the round. A missing confirmation while a handoff is still queued or
        // running is a round that landed mid-handoff (0.25.0 cleared the stamp and orphaned the
        // claim), and refusing that stranded the integration's result forever (2026-10-31).
        if meta.feature_cancelled_at.is_some() {
            return Err(Error::BadRequest(
                "the feature was cancelled; ignoring the handoff result".to_string(),
            ));
        }
        if meta.feature_confirmed_at.is_none() && meta.handoff.is_none() {
            return Err(Error::BadRequest(
                "the feature is not confirmed; ignoring the handoff result".to_string(),
            ));
        }
        let stamp = now();
        let mut handoff = meta
            .handoff
            .take()
            .unwrap_or_else(|| Handoff::requested(stamp.clone()));
        handoff.status = report.status;
        let label = report.label.filter(|label| !label.trim().is_empty());
        let message = report.message.filter(|message| !message.trim().is_empty());
        if report.status == HandoffStatus::Done {
            // "done" is the integration's new outcome; do not keep a previous "PR open" label
            // or a failure message.
            handoff.label = label.map(|label| label.trim().to_string());
            handoff.message = message.map(|message| message.trim().to_string());
        } else {
            if let Some(label) = label {
                handoff.label = Some(label.trim().to_string());
            }
            handoff.message = Some(
                message
                    .map(|message| message.trim().to_string())
                    .unwrap_or_else(|| "the integration reported a failed handoff".to_string()),
            );
        }
        if let Some(url) = report.url.filter(|url| !url.trim().is_empty()) {
            handoff.url = Some(url.trim().to_string());
        }
        if report.status.terminal() {
            handoff.finished_at = Some(stamp);
        }
        meta.handoff = Some(handoff);
        write_json(&dir.join("meta.json"), &serde_json::to_value(&meta)?)?;
        Ok(meta)
    }

    /// The human abandons the feature. Nothing is merged and the review files are kept; the
    /// agent can see `feature_state == "cancelled"`. `reopen` undoes it.
    pub fn cancel_feature(&self, id: &str) -> Result<Meta> {
        let dir = self.dir(id)?;
        let mut meta = self.require(id)?;
        let stamp = now();
        meta.status = Status::Resolved;
        meta.resolved_at.get_or_insert_with(|| stamp.clone());
        meta.feature_cancelled_at = Some(stamp);
        meta.feature_confirmed_at = None;
        meta.handoff = None;
        write_json(&dir.join("meta.json"), &serde_json::to_value(&meta)?)?;
        Ok(meta)
    }

    pub fn remove(&self, id: &str) -> Result<PathBuf> {
        let dir = self.dir(id)?;
        if !dir.exists() {
            return Err(Error::NotFound(id.to_string()));
        }
        fs::remove_dir_all(&dir)?;
        Ok(dir)
    }

    pub fn history(&self, id: &str) -> Result<Vec<HistoryEntry>> {
        let history = self.dir(id)?.join("history");
        if !history.exists() {
            return Ok(Vec::new());
        }
        // Each row carries the round's marks and picture, so a run of copies can be collapsed
        // below and the round still on the canvas can be recognised.
        let mut entries: Vec<(HistoryEntry, Value, Option<String>)> = Vec::new();
        for entry in fs::read_dir(&history)? {
            let path = entry?.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let Some(stamp) = path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_string)
            else {
                continue;
            };
            let set = fs::read_to_string(&path)
                .ok()
                .and_then(|text| serde_json::from_str::<AnnotationSet>(&text).ok());
            let marks = set
                .as_ref()
                .map(|set| json!(&set.annotations))
                .unwrap_or(Value::Null);
            let picture = set
                .as_ref()
                .and_then(|set| set.image.as_ref())
                .and_then(|image| image.file.clone());
            let media = set.as_ref().and_then(|set| set.media.clone());
            let frames = set.as_ref().map(|set| set.frames.len()).unwrap_or(0);
            let started_at = set.as_ref().and_then(|set| set.submitted_at.clone());
            entries.push((
                HistoryEntry {
                    png: history.join(format!("{stamp}.png")).is_file(),
                    source: history.join(format!("{stamp}-source.png")).is_file(),
                    stamp,
                    annotations: set.map(|set| set.annotations.len()).unwrap_or(0),
                    media,
                    frames,
                    round: 0,
                    current: false,
                    started_at,
                    run_started_at: None,
                    finished_at: None,
                    duration_secs: None,
                },
                marks,
                picture,
            ));
        }
        entries.sort_by(|a, b| b.0.stamp.cmp(&a.0.stamp));
        // One round is one row. STOP then *Submit again* without touching the canvas archived the
        // same annotations and the same picture a second time, so History read as "this one task
        // ran twice" (2026-10-30). Copies are indistinguishable by definition, so keep the newest
        // of a run; `dedup_by` drops the older one.
        entries.dedup_by(|older, newer| {
            older.1 == newer.1 && same_picture(&history, &older.0.stamp, &newer.0.stamp)
        });
        let total = entries.len();
        let meta = self.require(id)?;
        // The round the canvas still shows must appear once. A submission is archived at submit
        // time and the canvas keeps it after resolve or reopen, so History listed the same round
        // as "Round N" *and* "Current" (2026-10-30). Status alone cannot tell — an agent round
        // replaces the picture while the session is open — so compare the saved marks and picture
        // with the newest row.
        let live = self.annotations(id)?;
        let live_marks = json!(&live.annotations);
        let current = entries.first().and_then(|(entry, marks, picture)| {
            (*marks == live_marks && *picture == Some(meta.image.clone()))
                .then(|| entry.stamp.clone())
        });
        // How long each round ran. A run starts when an agent *picked the round up*, not when the
        // human submitted it: a review can sit in the queue for an hour before anyone claims it,
        // and counting that wait made an 8-minute run read as 42 minutes (2026-10-31). The honest
        // start is the agent's own first event; the end is when it handed the round back — its
        // next picture (`post_round`), the human's next submission, or `resolve`. Older rounds
        // have no `resolved_at` of their own, so the next submission is their only end.
        let starts: Vec<Option<String>> = entries
            .iter()
            .map(|(entry, _, _)| entry.started_at.clone())
            .collect();
        let handed_back = self.agent_picture_times(id)?;
        let run_times = self.agent_run_times(id)?;
        let resolved_at = meta.resolved_at.clone();
        for (index, (entry, _, _)) in entries.iter_mut().enumerate() {
            // Newest first, but numbered oldest = 1.
            entry.round = total - index;
            entry.current = current.as_deref() == Some(entry.stamp.as_str());
            let Some(started) = starts[index].clone() else {
                continue;
            };
            let next_submit = index.checked_sub(1).and_then(|newer| starts[newer].clone());
            // Only the newest round can have been resolved: `resolve` acts on the live round.
            let resolved = (index == 0).then_some(resolved_at.as_deref()).flatten();
            if let Some(finished) =
                round_finished(&started, next_submit.as_deref(), &handed_back, resolved)
            {
                // No agent event inside the round means nobody demonstrably ran it (a round
                // resolved by hand, a v0.1 archive): say nothing rather than print the queue time.
                entry.run_started_at = run_start(&run_times, &started, &finished);
                if let Some(start) = entry.run_started_at.clone() {
                    entry.duration_secs = seconds_between(&start, &finished);
                }
                entry.finished_at = Some(finished);
            }
        }
        Ok(entries.into_iter().map(|(entry, _, _)| entry).collect())
    }

    /// Timestamps of the pictures the agent posted back with `post_round` (newest last). They are
    /// the honest end of a run: `reply` is a message, not the finish line.
    fn agent_picture_times(&self, id: &str) -> Result<Vec<String>> {
        let path = self.dir(id)?.join("agent-thread.jsonl");
        let mut times = Vec::new();
        if path.is_file() {
            for line in fs::read_to_string(&path)?.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Ok(value) = serde_json::from_str::<Value>(line) {
                    if value.get("image").is_some_and(|image| !image.is_null()) {
                        if let Some(at) = value.get("at").and_then(|at| at.as_str()) {
                            times.push(at.to_string());
                        }
                    }
                }
            }
        }
        Ok(times)
    }

    /// Timestamps of the agent's own activity, oldest first. This is where a run really starts:
    /// the submission only says when the round entered the queue (2026-10-31). Raw output
    /// (`state: "log"`) is not evidence — a trailing build line can land after the agent already
    /// handed the round back — so only its reported steps count.
    fn agent_run_times(&self, id: &str) -> Result<Vec<String>> {
        Ok(self
            .agent_events(id)?
            .into_iter()
            .filter(|event| {
                event
                    .get("state")
                    .and_then(|state| state.as_str())
                    .map_or(true, |state| state != "log")
            })
            .filter_map(|event| {
                event
                    .get("at")
                    .and_then(|at| at.as_str())
                    .map(str::to_string)
            })
            .collect())
    }

    /// The newest round's own timing, for the rail and the conversation summary. `None` while
    /// that round is still in the agent's hands — a run in flight has no cost, and the previous
    /// round's cost is not what the human is looking at. It is the same measure History prints
    /// (from the agent's pickup, not the submission — 2026-10-31), but read without `history`'s
    /// picture comparisons: the rail asks for it on every poll, and re-reading every archived PNG
    /// per session would turn an 8 s refresh into a stall.
    pub fn last_run(&self, id: &str) -> Result<Option<LastRun>> {
        let history = self.dir(id)?.join("history");
        if !history.is_dir() {
            return Ok(None);
        }
        // Only the newest submission matters: it is the round the human is looking at, and a
        // round still in the agent's hands has no cost yet (the UI hides it while submitted).
        let mut stamps: Vec<String> = fs::read_dir(&history)?
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                let path = entry.path();
                (path.extension().and_then(|ext| ext.to_str()) == Some("json"))
                    .then(|| {
                        path.file_stem()
                            .and_then(|stem| stem.to_str())
                            .map(str::to_string)
                    })
                    .flatten()
            })
            .collect();
        stamps.sort_by(|a, b| b.cmp(a));
        let meta = self.require(id)?;
        let Some(stamp) = stamps.first() else {
            return Ok(None);
        };
        let Some(started) = fs::read_to_string(history.join(format!("{stamp}.json")))
            .ok()
            .and_then(|text| serde_json::from_str::<AnnotationSet>(&text).ok())
            .and_then(|set| set.submitted_at)
        else {
            return Ok(None);
        };
        let handed_back = self.agent_picture_times(id)?;
        let run_times = self.agent_run_times(id)?;
        let Some(finished) =
            round_finished(&started, None, &handed_back, meta.resolved_at.as_deref())
        else {
            return Ok(None);
        };
        Ok(
            run_start(&run_times, &started, &finished).and_then(|start| {
                let duration_secs = seconds_between(&start, &finished)?;
                Some(LastRun {
                    started_at: start,
                    finished_at: finished,
                    duration_secs,
                })
            }),
        )
    }

    /// Total bytes of a session folder (screenshots, videos, history). Used to warn before the
    /// archive grows into tens of gigabytes; RedLine never deletes rounds silently.
    pub fn session_size(&self, id: &str) -> Result<u64> {
        fn walk(path: &Path) -> std::io::Result<u64> {
            let mut total = 0;
            for entry in fs::read_dir(path)? {
                let entry = entry?;
                let meta = entry.metadata()?;
                total += if meta.is_dir() {
                    walk(&entry.path())?
                } else {
                    meta.len()
                };
            }
            Ok(total)
        }
        Ok(walk(&self.dir(id)?)?)
    }

    pub fn history_file(&self, id: &str, name: &str) -> Result<PathBuf> {
        if name.contains('/') || name.contains('\\') || name.contains("..") {
            return Err(Error::BadRequest("invalid history file".to_string()));
        }
        if !name.ends_with(".png") && !name.ends_with(".json") {
            return Err(Error::BadRequest("invalid history file".to_string()));
        }
        Ok(self.dir(id)?.join("history").join(name))
    }

    /// The archived clip of a submitted video round (`history/<stamp>/video.*`).
    pub fn history_video_path(&self, id: &str, stamp: &str) -> Result<PathBuf> {
        let dir = self.dir(id)?.join("history").join(safe_stamp(stamp));
        for extension in ["mp4", "webm"] {
            let path = dir.join(format!("video.{extension}"));
            if path.is_file() {
                return Ok(path);
            }
        }
        Err(Error::NotFound(format!("{id}/history/{stamp}/video")))
    }

    /// A full-resolution frame archived with a video round.
    pub fn history_frame_path(&self, id: &str, stamp: &str, file: &str) -> Result<PathBuf> {
        if file.contains('/')
            || file.contains('\\')
            || file.contains("..")
            || !(file.ends_with(".png") || file.ends_with(".jpg") || file.ends_with(".jpeg"))
        {
            return Err(Error::BadRequest("invalid frame name".to_string()));
        }
        Ok(self
            .dir(id)?
            .join("history")
            .join(format!("{}-frames", safe_stamp(stamp)))
            .join(file))
    }

    pub fn image_path(&self, meta: &Meta) -> Result<PathBuf> {
        Ok(self.dir(&meta.id)?.join(&meta.image))
    }

    /// The poster/thumbnail served by `/sessions/<id>/image`; falls back to the media file for
    /// legacy image sessions (which have no separate poster).
    pub fn poster_path(&self, meta: &Meta) -> Result<PathBuf> {
        if meta.media.as_ref().map(|media| media.kind) == Some(MediaKind::Video)
            && meta.poster.is_none()
        {
            return Err(Error::NotFound(format!("{}/poster.png", meta.id)));
        }
        let name = meta.poster.as_deref().unwrap_or(&meta.image);
        Ok(self.dir(&meta.id)?.join(name))
    }

    pub fn file_path(&self, id: &str, name: &str) -> Result<PathBuf> {
        if name.contains('/') || name.contains('\\') || name.contains("..") {
            return Err(Error::BadRequest("invalid file name".to_string()));
        }
        Ok(self.dir(id)?.join(name))
    }

    /// Validate and store `trace.json` for an existing session, then point `meta.trace` at it.
    /// The caller usually validates first (to fail before creating a session); this repeats the
    /// validation so a direct store call cannot write an unchecked trace.
    pub fn attach_trace(&self, id: &str, bytes: &[u8]) -> Result<Meta> {
        let dir = self.dir(id)?;
        let mut meta = self.require(id)?;
        let trace = validate_trace(
            bytes,
            meta.media.as_ref().and_then(|media| media.duration_ms),
        )?;
        write_atomic(&dir.join("trace.json"), bytes)?;
        meta.trace = Some(trace);
        write_json(&dir.join("meta.json"), &serde_json::to_value(&meta)?)?;
        Ok(meta)
    }

    /// The current `trace.json`, if this session has one.
    pub fn trace_path(&self, id: &str) -> Result<PathBuf> {
        let path = self.dir(id)?.join("trace.json");
        if path.is_file() {
            Ok(path)
        } else {
            Err(Error::NotFound(format!("{id}/trace")))
        }
    }

    /// The trace archived with a video round.
    pub fn history_trace_path(&self, id: &str, stamp: &str) -> Result<PathBuf> {
        let path = self
            .dir(id)?
            .join("history")
            .join(safe_stamp(stamp))
            .join("trace.json");
        if path.is_file() {
            Ok(path)
        } else {
            Err(Error::NotFound(format!("{id}/history/{stamp}/trace")))
        }
    }

    /// A full-resolution still captured at an annotation time (`frames/<at>.<png|jpg>`).
    pub fn frame_path(&self, id: &str, name: &str) -> Result<PathBuf> {
        if name.contains('/')
            || name.contains('\\')
            || name.contains("..")
            || !(name.ends_with(".png") || name.ends_with(".jpg") || name.ends_with(".jpeg"))
        {
            return Err(Error::BadRequest("invalid frame name".to_string()));
        }
        Ok(self.dir(id)?.join("frames").join(name))
    }
}
