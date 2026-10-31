//! On-disk shapes. Field names and ordering follow v0.1 (`redline.py`) so a `sessions/` folder
//! written by either implementation opens in the other.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// `meta.json`. Additive fields (`image_width`, `image_height`, later `agent`/`reviewer`) are
/// omitted when absent to keep v0.1 output byte-identical.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meta {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub note: String,
    pub status: Status,
    pub created_at: String,
    #[serde(default)]
    pub submitted_at: Option<String>,
    pub image: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_height: Option<u32>,
    /// Image or video metadata (v0.23). Missing means a legacy image session; `image` still names
    /// the media file for older clients.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<MediaRef>,
    /// First-frame thumbnail for a video session (v0.23); served by `/sessions/<id>/image`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poster: Option<String>,
    /// Who asked for the review (agent name, "cli", "mcp"). Additive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<String>,
    /// When the human confirmed the feature is complete (v0.21). This is the signal agents map
    /// to their own workflow — RedLine itself never opens a PR or merges anything.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_confirmed_at: Option<String>,
    /// When the human cancelled the feature without shipping it (v0.21).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_cancelled_at: Option<String>,
    /// The prompt the human wrote when submitting a round (v0.9).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// First time an agent read the current submission (v0.9).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seen_by_agent_at: Option<String>,
    /// What the agent is doing: `received`, `working`, `replied` (v0.9).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_state: Option<String>,
    /// When the human stopped a submission before the agent finished (v0.17).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancelled_at: Option<String>,
    /// Stamp of the round this submission was archived under (v0.16). Unique even when two
    /// submissions land in the same second.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round_stamp: Option<String>,
    /// Project slug this session belongs to (v0.10). Missing on disk means the Inbox.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// Feature branch the agent is working on (v0.22). Set at session creation or on the first
    /// claim; the fallback is `session/<id>`. RedLine uses it as the branch-lock key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    Image,
    Video,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaRef {
    pub kind: MediaKind,
    /// File name inside the session folder (`screenshot.png`, `video.mp4`).
    pub file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Nominal frames per second, when the producer knows it. Variable-frame-rate videos may
    /// omit it; the UI steps by a nominal frame then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fps: Option<f64>,
}

/// A conversation area: one agent + one piece of work, holding its sessions and history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub slug: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// Repository key relative to the agent's repo root, e.g. `redline` or `ActiUnit/Au.Net`.
    /// `None` for legacy label-only projects.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    /// Per-project concurrent-run override (v0.22). `None` inherits the server default (1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_parallel: Option<u32>,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Open,
    Submitted,
    Resolved,
}

impl Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Open => "open",
            Status::Submitted => "submitted",
            Status::Resolved => "resolved",
        }
    }
}

impl Meta {
    /// The human's feature decision: `confirmed`, `cancelled`, or `pending` when they have not
    /// decided yet. A `resolved` status alone (the agent finished a round) is not a decision.
    pub fn feature_state(&self) -> &'static str {
        if self.feature_confirmed_at.is_some() {
            "confirmed"
        } else if self.feature_cancelled_at.is_some() {
            "cancelled"
        } else {
            "pending"
        }
    }

    /// Key used to serialize work on one feature branch. A review the human created without
    /// naming a branch still runs on a feature branch: `session/<id>` was v0.21's fallback and
    /// says nothing about the feature, so it is not a work key any more (v0.22).
    pub fn work_key(&self) -> String {
        self.branch
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .filter(|value| !self.is_legacy_session_branch(value))
            .map(str::to_string)
            .unwrap_or_else(|| crate::store::default_branch(&self.title, &self.id))
    }

    /// True when `branch` is v0.21's `session/<own id>` fallback rather than a branch a human or
    /// agent chose. Those reviews get the derived `feat/...` name instead.
    pub fn is_legacy_session_branch(&self, branch: &str) -> bool {
        branch.trim() == format!("session/{}", self.id)
    }
}

/// A single annotation, kept schema-tolerant on purpose: the browser sends `rect`/`arrow`/`pen`/
/// `text`, but an agent or a future tool may send shapes this crate has never heard of. Unknown
/// fields survive a round-trip instead of failing the request (v0.1 did no validation either).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Annotation {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(flatten)]
    pub fields: Map<String, Value>,
}

impl Annotation {
    pub fn new(kind: &str) -> Self {
        Self {
            kind: kind.to_string(),
            fields: Map::new(),
        }
    }

    pub fn text_of(&self) -> Option<&str> {
        self.fields.get("text").and_then(Value::as_str)
    }

    /// The remark the human attached to a shape (v0.8: text lives beside the mark, not on it).
    pub fn note_of(&self) -> Option<&str> {
        self.fields.get("note").and_then(Value::as_str)
    }

    pub fn number(&self, key: &str) -> Option<f64> {
        self.fields.get(key).and_then(Value::as_f64)
    }

    pub fn point(&self, key: &str) -> Option<(f64, f64)> {
        let object = self.fields.get(key)?.as_object()?;
        Some((
            object.get("x").and_then(Value::as_f64)?,
            object.get("y").and_then(Value::as_f64)?,
        ))
    }

    pub fn points(&self) -> Vec<(f64, f64)> {
        self.fields
            .get("points")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        Some((
                            item.get("x").and_then(Value::as_f64)?,
                            item.get("y").and_then(Value::as_f64)?,
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn color(&self) -> Option<&str> {
        self.fields.get("color").and_then(Value::as_str)
    }

    pub fn is_text(&self) -> bool {
        self.kind == "text"
    }

    /// Video timestamp in seconds, when this annotation belongs to a moment (v0.23).
    pub fn at(&self) -> Option<f64> {
        self.number("at")
    }

    /// End of a time-range annotation in seconds (v0.23). Absent means a point annotation.
    pub fn until(&self) -> Option<f64> {
        self.number("until")
    }
}

/// `annotations.json` after a submission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotationSet {
    pub session: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<ImageRef>,
    /// Image or video metadata for the round (v0.23).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<MediaRef>,
    #[serde(default)]
    pub annotations: Vec<Annotation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submitted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
}

/// The payload the browser POSTs to `/api/sessions/{id}/annotations`.
#[derive(Debug, Clone, Deserialize)]
pub struct Submission {
    /// The prompt the human wrote with this round. Additive (v0.9).
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub annotations: Vec<Annotation>,
    /// `data:image/png;base64,…` or bare base64. Optional (v0.1 behaviour).
    #[serde(default)]
    pub png: Option<String>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

/// The payload used to create a session over HTTP/MCP instead of the CLI.
#[derive(Debug, Clone, Deserialize)]
pub struct NewSession {
    pub title: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    /// Base64 (optionally a `data:` URL) — the usual path for an agent uploading a screenshot.
    #[serde(default)]
    pub image_base64: Option<String>,
    /// Absolute path on the machine running redline. Convenience for local agents.
    #[serde(default)]
    pub image_path: Option<String>,
    /// Video upload (v0.23): base64 or an absolute path on the server; `media` becomes video.
    #[serde(default)]
    pub video_base64: Option<String>,
    #[serde(default)]
    pub video_path: Option<String>,
    /// Optional first-frame thumbnail for a video (base64 PNG).
    #[serde(default)]
    pub poster_base64: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub fps: Option<f64>,
    /// Project name or slug (v0.10). Created on first use; omitted means the Inbox.
    #[serde(default)]
    pub project: Option<String>,
    /// Feature branch the agent is working on (v0.22). Optional; the bridge falls back to
    /// `session/<id>` when it claims the work.
    #[serde(default)]
    pub branch: Option<String>,
}
