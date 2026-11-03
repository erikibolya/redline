//! Server-side login sessions.
//!
//! The browser gets a random `redline_session` cookie; the server stores only its SHA-256 hash
//! and a separate hashed CSRF token. A session record is the only thing that proves a cookie is
//! still valid, so logout and "revoke all" are immediate.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::auth::{random_token, SESSION_TTL_SECS};
use crate::error::Result;
use crate::store::{now, read_json, write_secret_json};

pub const SESSIONS_FILE: &str = "auth-sessions.json";
pub const SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id_hash: String,
    pub user_id: String,
    pub csrf_hash: String,
    pub created_at: String,
    pub last_seen_at: String,
    pub expires_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SessionGrant {
    /// Raw cookie value; only the hash is persisted.
    pub token: String,
    /// Raw CSRF value; only the hash is persisted.
    pub csrf: String,
    pub session: Session,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SessionsFile {
    #[serde(default)]
    schema: u32,
    #[serde(default)]
    sessions: Vec<Session>,
}

#[derive(Clone, Debug)]
pub struct SessionStore {
    root: PathBuf,
}

impl SessionStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path(&self) -> PathBuf {
        self.root.join(SESSIONS_FILE)
    }

    pub fn ensure(&self) -> Result<()> {
        fs::create_dir_all(&self.root)?;
        Ok(())
    }

    pub fn sessions(&self) -> Result<Vec<Session>> {
        let file: SessionsFile = read_json(&self.path())
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default();
        Ok(file.sessions)
    }

    fn save(&self, sessions: &[Session]) -> Result<()> {
        self.ensure()?;
        write_secret_json(
            &self.path(),
            &serde_json::to_value(SessionsFile {
                schema: SCHEMA_VERSION,
                sessions: sessions.to_vec(),
            })?,
        )
    }

    pub fn create(
        &self,
        user_id: &str,
        ip: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<SessionGrant> {
        self.create_with_ttl(user_id, ip, user_agent, SESSION_TTL_SECS as i64)
    }

    pub fn create_with_ttl(
        &self,
        user_id: &str,
        ip: Option<&str>,
        user_agent: Option<&str>,
        ttl_secs: i64,
    ) -> Result<SessionGrant> {
        let mut sessions = self.sessions()?;
        // Keep the file bounded; this is also where expired/revoked records disappear.
        sessions.retain(|session| !is_dead(session));
        let token = random_token();
        let csrf = random_token();
        let stamp = Utc::now();
        let session = Session {
            id_hash: hash_token(&token),
            user_id: user_id.to_string(),
            csrf_hash: hash_token(&csrf),
            created_at: stamp.to_rfc3339_opts(SecondsFormat::Secs, false),
            last_seen_at: stamp.to_rfc3339_opts(SecondsFormat::Secs, false),
            expires_at: (stamp + Duration::seconds(ttl_secs))
                .to_rfc3339_opts(SecondsFormat::Secs, false),
            ip: ip.map(bounded).filter(|value| !value.is_empty()),
            user_agent: user_agent.map(bounded).filter(|value| !value.is_empty()),
            revoked_at: None,
        };
        sessions.push(session.clone());
        self.save(&sessions)?;
        Ok(SessionGrant {
            token,
            csrf,
            session,
        })
    }

    pub fn lookup(&self, token: &str) -> Result<Option<Session>> {
        let hash = hash_token(token);
        Ok(self
            .sessions()?
            .into_iter()
            .find(|session| session.id_hash == hash && !is_dead(session)))
    }

    pub fn revoke(&self, token: &str) -> Result<()> {
        let hash = hash_token(token);
        let mut sessions = self.sessions()?;
        let mut changed = false;
        let stamp = now();
        for session in &mut sessions {
            if session.id_hash == hash && session.revoked_at.is_none() {
                session.revoked_at = Some(stamp.clone());
                changed = true;
            }
        }
        if changed {
            self.save(&sessions)?;
        }
        Ok(())
    }

    pub fn revoke_user(&self, user_id: &str) -> Result<usize> {
        let mut sessions = self.sessions()?;
        let stamp = now();
        let mut count = 0;
        for session in &mut sessions {
            if session.user_id == user_id && session.revoked_at.is_none() {
                session.revoked_at = Some(stamp.clone());
                count += 1;
            }
        }
        if count > 0 {
            self.save(&sessions)?;
        }
        Ok(count)
    }

    pub fn verify_csrf(session: &Session, token: &str) -> bool {
        !token.is_empty() && hash_token(token) == session.csrf_hash
    }
}

fn bounded(value: &str) -> String {
    value.chars().take(200).collect()
}

fn is_dead(session: &Session) -> bool {
    if session.revoked_at.is_some() {
        return true;
    }
    let Ok(expires) = DateTime::parse_from_rfc3339(&session.expires_at) else {
        return true;
    };
    expires.with_timezone(&Utc) <= Utc::now()
}

pub fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn store() -> (TempDir, SessionStore) {
        let dir = TempDir::new().unwrap();
        let store = SessionStore::new(dir.path().join("data"));
        (dir, store)
    }

    #[test]
    fn creates_looks_up_and_revokes_a_session() {
        let (_dir, store) = store();
        let grant = store
            .create("usr_1", Some("127.0.0.1"), Some("test"))
            .unwrap();
        let found = store.lookup(&grant.token).unwrap().unwrap();
        assert_eq!(found.user_id, "usr_1");
        assert!(SessionStore::verify_csrf(&found, &grant.csrf));
        assert!(!SessionStore::verify_csrf(&found, "wrong"));
        store.revoke(&grant.token).unwrap();
        assert!(store.lookup(&grant.token).unwrap().is_none());
    }

    #[test]
    fn an_expired_session_is_not_valid() {
        let (_dir, store) = store();
        let grant = store.create_with_ttl("usr_1", None, None, -1).unwrap();
        assert!(store.lookup(&grant.token).unwrap().is_none());
    }

    #[test]
    fn revoke_user_revokes_all_of_that_users_sessions() {
        let (_dir, store) = store();
        let first = store.create("usr_1", None, None).unwrap();
        let second = store.create("usr_1", None, None).unwrap();
        let other = store.create("usr_2", None, None).unwrap();
        assert_eq!(store.revoke_user("usr_1").unwrap(), 2);
        assert!(store.lookup(&first.token).unwrap().is_none());
        assert!(store.lookup(&second.token).unwrap().is_none());
        assert!(store.lookup(&other.token).unwrap().is_some());
    }
}
