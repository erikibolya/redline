//! Human discussion on a task. Stored as `comments.jsonl` inside the session folder, separate
//! from the agent's `agent-thread.jsonl`.

use serde::{Deserialize, Serialize};

use crate::store::now;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Comment {
    pub id: String,
    pub author: String,
    pub author_name: String,
    pub at: String,
    pub body: String,
    #[serde(default)]
    pub mentions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_by: Option<String>,
}

impl Comment {
    pub fn new(author: &str, author_name: &str, body: &str, mentions: Vec<String>) -> Self {
        Self {
            id: format!("cmt_{}", uuid::Uuid::new_v4().simple()),
            author: author.to_string(),
            author_name: author_name.to_string(),
            at: now(),
            body: body.to_string(),
            mentions,
            resolved_at: None,
            resolved_by: None,
        }
    }
}
