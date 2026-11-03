//! One-time invite and password-reset links.
//!
//! There is no SMTP in MVP: the owner generates a link and copies it to the person. Only the
//! SHA-256 hash is stored; the raw token is shown once. An invite expires after seven days, a
//! reset link after 24 hours, and consuming either marks it used so it can never be replayed.

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::auth::random_token;
use crate::error::{Error, Result};
use crate::sessions::hash_token;
use crate::store::{now, read_json, write_secret_json};

pub const INVITES_FILE: &str = "invites.json";
pub const SCHEMA_VERSION: u32 = 2;
pub const INVITE_TTL_SECS: i64 = 7 * 24 * 60 * 60;
pub const RESET_TTL_SECS: i64 = 24 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InviteKind {
    Invite,
    Reset,
}

impl InviteKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Invite => "invite",
            Self::Reset => "reset",
        }
    }

    fn prefix(self) -> &'static str {
        match self {
            Self::Invite => "rl_inv_",
            Self::Reset => "rl_rst_",
        }
    }

    fn id_prefix(self) -> &'static str {
        match self {
            Self::Invite => "inv",
            Self::Reset => "rst",
        }
    }

    pub fn ttl_secs(self) -> i64 {
        match self {
            Self::Invite => INVITE_TTL_SECS,
            Self::Reset => RESET_TTL_SECS,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invite {
    pub id: String,
    pub kind: InviteKind,
    pub user_id: String,
    pub hash: String,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    pub expires_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used_at: Option<String>,
}

impl Invite {
    pub fn is_active(&self) -> bool {
        if self.used_at.is_some() {
            return false;
        }
        DateTime::parse_from_rfc3339(&self.expires_at)
            .map(|expires| expires.with_timezone(&Utc) > Utc::now())
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone)]
pub struct InviteGrant {
    /// Shown once in the owner's UI; only the hash is persisted.
    pub raw: String,
    pub invite: Invite,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct InvitesFile {
    #[serde(default)]
    schema: u32,
    #[serde(default)]
    invites: Vec<Invite>,
}

#[derive(Clone, Debug)]
pub struct InviteStore {
    root: PathBuf,
    lock: Arc<Mutex<()>>,
}

impl InviteStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            lock: Arc::new(Mutex::new(())),
        }
    }

    fn path(&self) -> PathBuf {
        self.root.join(INVITES_FILE)
    }

    pub fn ensure(&self) -> Result<()> {
        fs::create_dir_all(&self.root)?;
        Ok(())
    }

    pub fn all(&self) -> Result<Vec<Invite>> {
        let file: InvitesFile = read_json(&self.path())
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default();
        Ok(file.invites)
    }

    fn save(&self, invites: &[Invite]) -> Result<()> {
        self.ensure()?;
        write_secret_json(
            &self.path(),
            &serde_json::to_value(InvitesFile {
                schema: SCHEMA_VERSION,
                invites: invites.to_vec(),
            })?,
        )
    }

    /// A new invite/reset supersedes any unused link of the same kind for that user.
    pub fn create(
        &self,
        kind: InviteKind,
        user_id: &str,
        created_by: Option<&str>,
    ) -> Result<InviteGrant> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| Error::BadRequest("invite lock poisoned".to_string()))?;
        let mut invites = self.all()?;
        let stamp = now();
        for invite in &mut invites {
            if invite.user_id == user_id
                && invite.kind == kind
                && invite.used_at.is_none()
                && invite.is_active()
            {
                invite.used_at = Some(stamp.clone());
            }
        }
        // Keep the file bounded; consumed/expired rows are only useful for a short audit window
        // (the audit log owns the long-term record).
        let cutoff = Utc::now() - Duration::days(30);
        invites.retain(|invite| {
            DateTime::parse_from_rfc3339(&invite.created_at)
                .map(|at| at.with_timezone(&Utc) >= cutoff || invite.is_active())
                .unwrap_or(true)
        });
        let raw = format!("{}{}", kind.prefix(), random_token());
        let invite = Invite {
            id: format!("{}_{}", kind.id_prefix(), uuid::Uuid::new_v4().simple()),
            kind,
            user_id: user_id.to_string(),
            hash: hash_token(&raw),
            created_at: stamp.clone(),
            created_by: created_by.map(str::to_string),
            expires_at: (Utc::now() + Duration::seconds(kind.ttl_secs()))
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
            used_at: None,
        };
        invites.push(invite.clone());
        self.save(&invites)?;
        Ok(InviteGrant { raw, invite })
    }

    pub fn lookup(&self, raw: &str, kind: InviteKind) -> Result<Option<Invite>> {
        if raw.len() < 12 {
            return Ok(None);
        }
        let hash = hash_token(raw);
        Ok(self
            .all()?
            .into_iter()
            .find(|invite| invite.kind == kind && invite.hash == hash && invite.is_active()))
    }

    /// Mark the link used. `lookup` checks the state first; a concurrent accept that loses the
    /// race gets `false` from this compare-and-set.
    pub fn consume(&self, id: &str) -> Result<bool> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| Error::BadRequest("invite lock poisoned".to_string()))?;
        let mut invites = self.all()?;
        let Some(invite) = invites.iter_mut().find(|invite| invite.id == id) else {
            return Ok(false);
        };
        if invite.used_at.is_some() || !invite.is_active() {
            return Ok(false);
        }
        invite.used_at = Some(now());
        self.save(&invites)?;
        Ok(true)
    }

    pub fn revoke_user(&self, user_id: &str) -> Result<usize> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| Error::BadRequest("invite lock poisoned".to_string()))?;
        let mut invites = self.all()?;
        let stamp = now();
        let mut count = 0;
        for invite in &mut invites {
            if invite.user_id == user_id && invite.used_at.is_none() {
                invite.used_at = Some(stamp.clone());
                count += 1;
            }
        }
        if count > 0 {
            self.save(&invites)?;
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn store() -> (TempDir, InviteStore) {
        let dir = TempDir::new().unwrap();
        let store = InviteStore::new(dir.path().join("data"));
        (dir, store)
    }

    #[test]
    fn an_invite_is_hashed_looked_up_and_consumed_once() {
        let (_dir, store) = store();
        let grant = store
            .create(InviteKind::Invite, "usr_jana", Some("usr_owner"))
            .unwrap();
        assert!(grant.raw.starts_with("rl_inv_"));
        assert_ne!(grant.invite.hash, grant.raw);
        assert!(grant.invite.expires_at.len() > 10);
        let found = store
            .lookup(&grant.raw, InviteKind::Invite)
            .unwrap()
            .expect("active invite");
        assert_eq!(found.user_id, "usr_jana");
        assert!(store
            .lookup(&grant.raw, InviteKind::Reset)
            .unwrap()
            .is_none());
        assert!(store.consume(&grant.invite.id).unwrap());
        assert!(store
            .lookup(&grant.raw, InviteKind::Invite)
            .unwrap()
            .is_none());
        assert!(!store.consume(&grant.invite.id).unwrap());
    }

    #[test]
    fn a_new_link_supersedes_the_previous_one() {
        let (_dir, store) = store();
        let first = store.create(InviteKind::Reset, "usr_jana", None).unwrap();
        let second = store.create(InviteKind::Reset, "usr_jana", None).unwrap();
        assert!(store
            .lookup(&first.raw, InviteKind::Reset)
            .unwrap()
            .is_none());
        assert!(store
            .lookup(&second.raw, InviteKind::Reset)
            .unwrap()
            .is_some());
    }

    #[test]
    fn an_expired_link_is_not_valid() {
        let (_dir, store) = store();
        let grant = store.create(InviteKind::Invite, "usr_jana", None).unwrap();
        let mut invites = store.all().unwrap();
        for invite in &mut invites {
            if invite.id == grant.invite.id {
                invite.expires_at = "2020-01-01T00:00:00+00:00".to_string();
            }
        }
        store.save(&invites).unwrap();
        assert!(store
            .lookup(&grant.raw, InviteKind::Invite)
            .unwrap()
            .is_none());
    }

    #[test]
    fn revoke_user_kills_every_unused_link() {
        let (_dir, store) = store();
        let invite = store.create(InviteKind::Invite, "usr_jana", None).unwrap();
        let reset = store.create(InviteKind::Reset, "usr_jana", None).unwrap();
        assert_eq!(store.revoke_user("usr_jana").unwrap(), 2);
        assert!(store
            .lookup(&invite.raw, InviteKind::Invite)
            .unwrap()
            .is_none());
        assert!(store
            .lookup(&reset.raw, InviteKind::Reset)
            .unwrap()
            .is_none());
    }
}
