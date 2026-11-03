//! Append-only audit trail.
//!
//! `audit.jsonl` is the record of who did what: authentication, user/membership changes,
//! project and review actions, comments and notifications. It is deliberately a plain JSONL
//! file (inspectable with `tail`/`jq`, no database) and is retained for one year by default.

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{Error, Result};
use crate::model::Meta;
use crate::store::now;

pub const AUDIT_FILE: &str = "audit.jsonl";
const DEFAULT_RETENTION_DAYS: i64 = 365;
const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 1000;
/// Compact only when the file is large enough that rewriting it is cheaper than keeping dead
/// lines forever. `list` always hides expired events, so retention is correct without a rewrite.
const COMPACT_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub at: String,
    #[serde(default = "default_actor_type")]
    pub actor_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    #[serde(default)]
    pub actor_name: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub details: Value,
}

fn default_actor_type() -> String {
    "user".to_string()
}

impl AuditEvent {
    pub fn new(actor_type: &str, actor_id: Option<&str>, actor_name: &str, action: &str) -> Self {
        Self {
            at: now(),
            actor_type: actor_type.to_string(),
            actor_id: actor_id.map(str::to_string),
            actor_name: actor_name.to_string(),
            action: action.to_string(),
            project: None,
            session: None,
            target: None,
            ip: None,
            user_agent: None,
            details: json!({}),
        }
    }

    pub fn task(mut self, meta: &Meta) -> Self {
        self.project = meta.project.clone();
        self.session = Some(meta.id.clone());
        self
    }

    pub fn target(mut self, target: impl Into<String>) -> Self {
        self.target = Some(target.into());
        self
    }

    pub fn client(mut self, ip: Option<&str>, user_agent: Option<&str>) -> Self {
        self.ip = ip.map(str::to_string);
        self.user_agent = user_agent.map(str::to_string);
        self
    }

    pub fn details(mut self, details: Value) -> Self {
        self.details = details;
        self
    }
}

#[derive(Debug, Default, Clone)]
pub struct AuditFilter {
    pub limit: usize,
    pub project: Option<String>,
    pub session: Option<String>,
    pub actor: Option<String>,
    pub action: Option<String>,
    pub since: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug)]
pub struct AuditStore {
    root: PathBuf,
    lock: Arc<Mutex<()>>,
}

impl AuditStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path(&self) -> PathBuf {
        self.root.join(AUDIT_FILE)
    }

    pub fn ensure(&self) -> Result<()> {
        fs::create_dir_all(&self.root)?;
        Ok(())
    }

    /// Append one event. A failing audit write must be visible to the caller; the API turns it
    /// into a warning so a full disk does not silently erase the record (and does not block the
    /// human action either).
    pub fn record(&self, event: AuditEvent) -> Result<AuditEvent> {
        validate_action(&event.action)?;
        self.ensure()?;
        let _guard = self
            .lock
            .lock()
            .map_err(|_| Error::BadRequest("audit lock poisoned".to_string()))?;
        let path = self.path();
        let large = fs::metadata(&path)
            .map(|metadata| metadata.len() > COMPACT_BYTES)
            .unwrap_or(false);
        if large {
            let recent = read_all_recent(&path)?;
            write_events(&path, &recent)?;
        }
        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        }
        let mut line = serde_json::to_string(&event)?;
        line.push('\n');
        file.write_all(line.as_bytes())?;
        file.flush()?;
        Ok(event)
    }

    /// Newest first. Expired events are hidden even if the file has not been compacted yet.
    pub fn list(&self, filter: &AuditFilter) -> Result<Vec<AuditEvent>> {
        let path = self.path();
        if !path.is_file() {
            return Ok(Vec::new());
        }
        let _guard = self
            .lock
            .lock()
            .map_err(|_| Error::BadRequest("audit lock poisoned".to_string()))?;
        read_events(&path, filter)
    }
}

impl AuditFilter {
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit.clamp(1, MAX_LIMIT);
        self
    }

    fn matches(&self, event: &AuditEvent, cutoff: DateTime<Utc>) -> bool {
        if !is_recent(&event.at, cutoff) {
            return false;
        }
        if let Some(project) = &self.project {
            if event.project.as_deref() != Some(project.as_str()) {
                return false;
            }
        }
        if let Some(session) = &self.session {
            if event.session.as_deref() != Some(session.as_str()) {
                return false;
            }
        }
        if let Some(actor) = &self.actor {
            if event.actor_id.as_deref() != Some(actor.as_str()) {
                return false;
            }
        }
        if let Some(action) = &self.action {
            if !event.action.starts_with(action) {
                return false;
            }
        }
        if let Some(since) = self.since {
            let Ok(at) = DateTime::parse_from_rfc3339(&event.at) else {
                return false;
            };
            if at.with_timezone(&Utc) < since {
                return false;
            }
        }
        true
    }
}

fn read_events(path: &Path, filter: &AuditFilter) -> Result<Vec<AuditEvent>> {
    let cutoff = cutoff();
    let mut events = Vec::new();
    for line in fs::read_to_string(path)?.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(event) = serde_json::from_str::<AuditEvent>(line) {
            if filter.matches(&event, cutoff) {
                events.push(event);
            }
        }
    }
    events.reverse();
    let limit = if filter.limit == 0 {
        DEFAULT_LIMIT
    } else {
        filter.limit.min(MAX_LIMIT)
    };
    events.truncate(limit);
    Ok(events)
}

fn read_all_recent(path: &Path) -> Result<Vec<AuditEvent>> {
    let cutoff = cutoff();
    let mut events = Vec::new();
    for line in fs::read_to_string(path)?.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(event) = serde_json::from_str::<AuditEvent>(line) {
            if is_recent(&event.at, cutoff) {
                events.push(event);
            }
        }
    }
    Ok(events)
}

fn write_events(path: &Path, events: &[AuditEvent]) -> Result<()> {
    let mut text = String::new();
    for event in events {
        text.push_str(&serde_json::to_string(event)?);
        text.push('\n');
    }
    let temp = path.with_extension("tmp");
    fs::write(&temp, text.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temp, fs::Permissions::from_mode(0o600))?;
    }
    fs::rename(&temp, path).inspect_err(|_| {
        let _ = fs::remove_file(&temp);
    })?;
    Ok(())
}

fn retention_days() -> i64 {
    std::env::var("REDLINE_AUDIT_RETENTION_DAYS")
        .ok()
        .and_then(|value| value.trim().parse::<i64>().ok())
        .filter(|days| *days > 0)
        .unwrap_or(DEFAULT_RETENTION_DAYS)
}

fn cutoff() -> DateTime<Utc> {
    Utc::now() - Duration::days(retention_days())
}

fn is_recent(at: &str, cutoff: DateTime<Utc>) -> bool {
    DateTime::parse_from_rfc3339(at)
        .map(|at| at.with_timezone(&Utc) >= cutoff)
        .unwrap_or(false)
}

fn validate_action(action: &str) -> Result<()> {
    if action.is_empty()
        || action.len() > 80
        || !action
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    {
        return Err(Error::BadRequest(format!(
            "invalid audit action {action:?}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn store() -> (TempDir, AuditStore) {
        let dir = TempDir::new().unwrap();
        let store = AuditStore::new(dir.path().join("data"));
        (dir, store)
    }

    #[test]
    fn records_newest_first_and_filters() {
        let (_dir, store) = store();
        store
            .record(
                AuditEvent::new("user", Some("usr_anna"), "Anna", "review.create")
                    .target("s1")
                    .details(json!({ "title": "One" })),
            )
            .unwrap();
        let mut second = AuditEvent::new("user", Some("usr_petr"), "Petr", "review.submit");
        second.project = Some("redline".to_string());
        second.session = Some("s2".to_string());
        store.record(second).unwrap();
        store
            .record(AuditEvent::new("system", None, "system", "auth.login"))
            .unwrap();

        let all = store.list(&AuditFilter::default().with_limit(10)).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].action, "auth.login");
        assert_eq!(all[2].action, "review.create");

        let project = store
            .list(&AuditFilter {
                project: Some("redline".to_string()),
                ..AuditFilter::default().with_limit(10)
            })
            .unwrap();
        assert_eq!(project.len(), 1);
        assert_eq!(project[0].actor_id.as_deref(), Some("usr_petr"));

        let by_actor = store
            .list(&AuditFilter {
                actor: Some("usr_anna".to_string()),
                ..AuditFilter::default().with_limit(10)
            })
            .unwrap();
        assert_eq!(by_actor.len(), 1);

        let by_prefix = store
            .list(&AuditFilter {
                action: Some("review.".to_string()),
                ..AuditFilter::default().with_limit(10)
            })
            .unwrap();
        assert_eq!(by_prefix.len(), 2);
    }

    #[test]
    fn expired_events_are_hidden() {
        let (_dir, store) = store();
        let mut old = AuditEvent::new("user", None, "Old", "review.create");
        old.at = "2020-01-01T00:00:00+00:00".to_string();
        store.record(old).unwrap();
        store
            .record(AuditEvent::new("user", None, "New", "review.create"))
            .unwrap();
        let events = store.list(&AuditFilter::default().with_limit(10)).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].actor_name, "New");
    }

    #[test]
    fn rejects_an_invalid_action() {
        let (_dir, store) = store();
        assert!(store
            .record(AuditEvent::new("user", None, "Anna", "bad action"))
            .is_err());
        assert!(store
            .record(AuditEvent::new("user", None, "Anna", "../escape"))
            .is_err());
    }

    #[test]
    fn concurrent_writes_do_not_interleave() {
        let (_dir, store) = store();
        let mut handles = Vec::new();
        for worker in 0..6 {
            let store = store.clone();
            handles.push(std::thread::spawn(move || {
                for index in 0..10 {
                    store
                        .record(
                            AuditEvent::new("user", Some("usr_anna"), "Anna", "review.create")
                                .details(json!({ "worker": worker, "index": index })),
                        )
                        .unwrap();
                }
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }
        let events = store
            .list(&AuditFilter::default().with_limit(MAX_LIMIT))
            .unwrap();
        assert_eq!(events.len(), 60);
        assert!(events.iter().all(|event| event.action == "review.create"));
    }
}
