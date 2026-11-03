//! Per-user in-app notifications.
//!
//! One JSONL file per user under `data/notifications/`. A notification is a small, already
//! redacted event (project/session ids and a sentence); it never contains annotations, tokens or
//! a task the recipient cannot see.

use std::fs;
use std::path::PathBuf;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::store::now;

pub const NOTIFICATIONS_DIR: &str = "notifications";
const MAX_KEPT: usize = 500;
const DEFAULT_RETENTION_DAYS: i64 = 90;

/// In-app notifications are history, not an archive: the decision was 90 days, and the value is
/// configurable for a deployment that wants a shorter or longer window.
fn retention_days() -> i64 {
    std::env::var("REDLINE_NOTIFICATION_RETENTION_DAYS")
        .ok()
        .and_then(|value| value.trim().parse::<i64>().ok())
        .filter(|days| *days > 0)
        .unwrap_or(DEFAULT_RETENTION_DAYS)
}

fn cutoff() -> DateTime<Utc> {
    Utc::now() - Duration::days(retention_days())
}

fn is_recent(notification: &Notification) -> bool {
    DateTime::parse_from_rfc3339(&notification.at)
        .map(|at| at.with_timezone(&Utc) >= cutoff())
        .unwrap_or(false)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Notification {
    pub id: String,
    pub at: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_name: Option<String>,
    pub text: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_at: Option<String>,
}

impl Notification {
    pub fn new(kind: &str, text: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            id: format!("ntf_{}", uuid::Uuid::new_v4().simple()),
            at: now(),
            kind: kind.to_string(),
            project: None,
            session: None,
            actor: None,
            actor_name: None,
            text: text.into(),
            url: url.into(),
            read_at: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct NotificationStore {
    root: PathBuf,
}

impl NotificationStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn dir(&self) -> PathBuf {
        self.root.join(NOTIFICATIONS_DIR)
    }

    fn path(&self, user_id: &str) -> Result<PathBuf> {
        if !valid_user_id(user_id) {
            return Err(Error::BadRequest("invalid notification user".to_string()));
        }
        Ok(self.dir().join(format!("{user_id}.jsonl")))
    }

    pub fn ensure(&self) -> Result<()> {
        fs::create_dir_all(self.dir())?;
        Ok(())
    }

    pub fn list(&self, user_id: &str) -> Result<Vec<Notification>> {
        let path = self.path(user_id)?;
        if !path.is_file() {
            return Ok(Vec::new());
        }
        let mut items = Vec::new();
        for line in fs::read_to_string(path)?.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(item) = serde_json::from_str::<Notification>(line) {
                if is_recent(&item) {
                    items.push(item);
                }
            }
        }
        Ok(items)
    }

    pub fn add(&self, user_id: &str, mut notification: Notification) -> Result<Notification> {
        if user_id.is_empty() {
            return Ok(notification);
        }
        self.ensure()?;
        let mut items = self.list(user_id)?;
        if notification.at.is_empty() {
            notification.at = now();
        }
        items.push(notification.clone());
        if items.len() > MAX_KEPT {
            let skip = items.len() - MAX_KEPT;
            items.drain(..skip);
        }
        self.save(user_id, &items)?;
        Ok(notification)
    }

    pub fn unread_count(&self, user_id: &str) -> Result<usize> {
        Ok(self
            .list(user_id)?
            .into_iter()
            .filter(|item| item.read_at.is_none())
            .count())
    }

    /// Mark the given ids, or every unread item when `ids` is `None`.
    pub fn mark_read(&self, user_id: &str, ids: Option<&[String]>) -> Result<usize> {
        let mut items = self.list(user_id)?;
        let stamp = now();
        let mut changed = 0;
        for item in &mut items {
            let selected = ids
                .map(|ids| ids.iter().any(|id| id == &item.id))
                .unwrap_or(true);
            if selected && item.read_at.is_none() {
                item.read_at = Some(stamp.clone());
                changed += 1;
            }
        }
        if changed > 0 {
            self.save(user_id, &items)?;
        }
        Ok(changed)
    }

    fn save(&self, user_id: &str, items: &[Notification]) -> Result<()> {
        self.ensure()?;
        let path = self.path(user_id)?;
        let mut text = String::new();
        for item in items {
            text.push_str(&serde_json::to_string(item)?);
            text.push('\n');
        }
        let temp = path.with_extension("tmp");
        fs::write(&temp, text.as_bytes())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&temp, fs::Permissions::from_mode(0o600))?;
        }
        fs::rename(&temp, &path).inspect_err(|_| {
            let _ = fs::remove_file(&temp);
        })?;
        Ok(())
    }
}

fn valid_user_id(user_id: &str) -> bool {
    !user_id.is_empty()
        && user_id.len() <= 64
        && user_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn store() -> (TempDir, NotificationStore) {
        let dir = TempDir::new().unwrap();
        let store = NotificationStore::new(dir.path().join("data"));
        (dir, store)
    }

    #[test]
    fn notifications_are_per_user_and_mark_read() {
        let (_dir, store) = store();
        store
            .add(
                "usr_anna",
                Notification::new("review.ready", "Anna ready", "/s/1"),
            )
            .unwrap();
        store
            .add(
                "usr_petr",
                Notification::new("comment", "Petr comment", "/s/2"),
            )
            .unwrap();
        assert_eq!(store.list("usr_anna").unwrap().len(), 1);
        assert_eq!(store.unread_count("usr_anna").unwrap(), 1);
        let id = store.list("usr_anna").unwrap()[0].id.clone();
        assert_eq!(store.mark_read("usr_anna", Some(&[id])).unwrap(), 1);
        assert_eq!(store.unread_count("usr_anna").unwrap(), 0);
        assert_eq!(store.unread_count("usr_petr").unwrap(), 1);
    }

    #[test]
    fn invalid_user_ids_are_refused() {
        let (_dir, store) = store();
        assert!(store.list("../escape").is_err());
        assert!(store.list("").is_err());
    }

    #[test]
    fn notifications_older_than_the_retention_window_are_dropped() {
        let (_dir, store) = store();
        let mut old = Notification::new("comment", "old", "/s/1");
        old.at = "2020-01-01T00:00:00+00:00".to_string();
        store.add("usr_anna", old).unwrap();
        store
            .add("usr_anna", Notification::new("comment", "new", "/s/2"))
            .unwrap();
        let items = store.list("usr_anna").unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].text, "new");
    }
}
