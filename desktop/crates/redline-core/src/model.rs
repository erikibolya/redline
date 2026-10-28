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
    /// Who asked for the review (agent name, "cli", "mcp"). Additive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<String>,
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
        Self { kind: kind.to_string(), fields: Map::new() }
    }

    pub fn text_of(&self) -> Option<&str> {
        self.fields.get("text").and_then(Value::as_str)
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
}
