//! Personal access tokens and agent/service tokens.
//!
//! A PAT is a human credential for the extension, CLI or desktop; it inherits the user's project
//! roles and can be narrowed to an explicit project allowlist (and marked read-only). An agent
//! token is a non-human service identity with its own project allowlist and jobs. Only hashes are
//! stored; the raw value is shown once at creation.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::auth::random_token;
use crate::error::{Error, Result};
use crate::sessions::hash_token;
use crate::store::{now, read_json, write_secret_json};

pub const TOKENS_FILE: &str = "tokens.json";
pub const SCHEMA_VERSION: u32 = 2;
const MAX_NAME: usize = 80;
const MAX_PROJECTS: usize = 100;
const MAX_EXPIRY_DAYS: i64 = 3650;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TokenKind {
    Personal,
    Agent,
}

impl TokenKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Personal => "personal",
            Self::Agent => "agent",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenRecord {
    pub id: String,
    pub kind: TokenKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    pub name: String,
    pub prefix: String,
    pub hash: String,
    /// `None` means every project the owner of the credential can already access.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projects: Option<Vec<String>>,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub jobs: Vec<String>,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PublicToken {
    pub id: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    pub name: String,
    pub prefix: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projects: Option<Vec<String>>,
    pub read_only: bool,
    pub jobs: Vec<String>,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<String>,
}

impl TokenRecord {
    pub fn public(&self) -> PublicToken {
        PublicToken {
            id: self.id.clone(),
            kind: self.kind.as_str().to_string(),
            user: self.user.clone(),
            agent: self.agent.clone(),
            name: self.name.clone(),
            prefix: self.prefix.clone(),
            projects: self.projects.clone(),
            read_only: self.read_only,
            jobs: self.jobs.clone(),
            created_at: self.created_at.clone(),
            last_used_at: self.last_used_at.clone(),
            expires_at: self.expires_at.clone(),
            revoked_at: self.revoked_at.clone(),
        }
    }

    pub fn is_active(&self) -> bool {
        if self.revoked_at.is_some() {
            return false;
        }
        match self.expires_at.as_deref() {
            Some(expires) => DateTime::parse_from_rfc3339(expires)
                .map(|expires| expires.with_timezone(&Utc) > Utc::now())
                .unwrap_or(false),
            None => true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TokenGrant {
    /// Shown once; only the hash is persisted.
    pub raw: String,
    pub token: TokenRecord,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct TokensFile {
    #[serde(default)]
    schema: u32,
    #[serde(default)]
    tokens: Vec<TokenRecord>,
}

#[derive(Clone, Debug)]
pub struct TokenStore {
    root: PathBuf,
    lock: Arc<Mutex<()>>,
}

impl TokenStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            lock: Arc::new(Mutex::new(())),
        }
    }

    fn path(&self) -> PathBuf {
        self.root.join(TOKENS_FILE)
    }

    pub fn ensure(&self) -> Result<()> {
        fs::create_dir_all(&self.root)?;
        Ok(())
    }

    pub fn all(&self) -> Result<Vec<TokenRecord>> {
        let file: TokensFile = read_json(&self.path())
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default();
        Ok(file.tokens)
    }

    fn save(&self, tokens: &[TokenRecord]) -> Result<()> {
        self.ensure()?;
        write_secret_json(
            &self.path(),
            &serde_json::to_value(TokensFile {
                schema: SCHEMA_VERSION,
                tokens: tokens.to_vec(),
            })?,
        )
    }

    pub fn for_user(&self, user_id: &str) -> Result<Vec<TokenRecord>> {
        Ok(self
            .all()?
            .into_iter()
            .filter(|token| token.user.as_deref() == Some(user_id))
            .collect())
    }

    pub fn agents(&self) -> Result<Vec<TokenRecord>> {
        Ok(self
            .all()?
            .into_iter()
            .filter(|token| token.kind == TokenKind::Agent)
            .collect())
    }

    pub fn get(&self, id: &str) -> Result<Option<TokenRecord>> {
        Ok(self.all()?.into_iter().find(|token| token.id == id))
    }

    /// Resolve a raw Bearer value. Expired/revoked tokens answer `None`; a valid token's
    /// `last_used_at` is touched at most once a minute so a poll does not rewrite the file on
    /// every request.
    pub fn lookup(&self, raw: &str) -> Result<Option<TokenRecord>> {
        if raw.len() < 12 {
            return Ok(None);
        }
        let hash = hash_token(raw);
        let _guard = self
            .lock
            .lock()
            .map_err(|_| Error::BadRequest("token lock poisoned".to_string()))?;
        let mut tokens = self.all()?;
        let Some(index) = tokens.iter().position(|token| token.hash == hash) else {
            return Ok(None);
        };
        if !tokens[index].is_active() {
            return Ok(None);
        }
        let now = Utc::now();
        let stale = tokens[index]
            .last_used_at
            .as_deref()
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .map(|last| now - last.with_timezone(&Utc) > Duration::minutes(1))
            .unwrap_or(true);
        if stale {
            tokens[index].last_used_at =
                Some(now.to_rfc3339_opts(chrono::SecondsFormat::Secs, false));
            let _ = self.save(&tokens);
        }
        Ok(Some(tokens[index].clone()))
    }

    pub fn create_personal(
        &self,
        user_id: &str,
        name: &str,
        projects: Option<Vec<String>>,
        read_only: bool,
        expires_days: Option<i64>,
    ) -> Result<TokenGrant> {
        self.create(
            TokenKind::Personal,
            Some(user_id),
            None,
            name,
            projects,
            read_only,
            Vec::new(),
            expires_days,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_agent(
        &self,
        agent: &str,
        name: &str,
        projects: Option<Vec<String>>,
        jobs: Vec<String>,
        expires_days: Option<i64>,
    ) -> Result<TokenGrant> {
        self.create(
            TokenKind::Agent,
            None,
            Some(agent),
            name,
            projects,
            false,
            jobs,
            expires_days,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn create(
        &self,
        kind: TokenKind,
        user_id: Option<&str>,
        agent: Option<&str>,
        name: &str,
        projects: Option<Vec<String>>,
        read_only: bool,
        jobs: Vec<String>,
        expires_days: Option<i64>,
    ) -> Result<TokenGrant> {
        let name = validate_name(name)?;
        let projects = projects.map(validate_projects).transpose()?;
        let jobs = validate_jobs(kind, jobs)?;
        let expires_days = validate_expiry(expires_days)?;
        let _guard = self
            .lock
            .lock()
            .map_err(|_| Error::BadRequest("token lock poisoned".to_string()))?;
        let mut tokens = self.all()?;
        let raw_prefix = match kind {
            TokenKind::Personal => "rl_pat_",
            TokenKind::Agent => "rl_agt_",
        };
        let raw = format!("{raw_prefix}{}", random_token());
        let prefix = raw.chars().take(16).collect::<String>();
        let id_prefix = match kind {
            TokenKind::Personal => "pat",
            TokenKind::Agent => "agt",
        };
        let created_at = now();
        let expires_at = expires_days.map(|days| {
            (Utc::now() + Duration::days(days)).to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
        });
        let token = TokenRecord {
            id: format!("{id_prefix}_{}", uuid::Uuid::new_v4().simple()),
            kind,
            user: user_id.map(str::to_string),
            agent: agent.map(str::to_string),
            name,
            prefix,
            hash: hash_token(&raw),
            projects,
            read_only,
            jobs,
            created_at,
            last_used_at: None,
            expires_at,
            revoked_at: None,
        };
        tokens.push(token.clone());
        self.save(&tokens)?;
        Ok(TokenGrant { raw, token })
    }

    /// Revoke only when the token belongs to `user_id`; an unknown/foreign id answers `false`.
    pub fn revoke_for_user(&self, id: &str, user_id: &str) -> Result<bool> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| Error::BadRequest("token lock poisoned".to_string()))?;
        let mut tokens = self.all()?;
        let Some(token) = tokens
            .iter_mut()
            .find(|token| token.id == id && token.user.as_deref() == Some(user_id))
        else {
            return Ok(false);
        };
        token.revoked_at = Some(now());
        self.save(&tokens)?;
        Ok(true)
    }

    pub fn revoke_agent(&self, id: &str) -> Result<bool> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| Error::BadRequest("token lock poisoned".to_string()))?;
        let mut tokens = self.all()?;
        let Some(token) = tokens
            .iter_mut()
            .find(|token| token.id == id && token.kind == TokenKind::Agent)
        else {
            return Ok(false);
        };
        token.revoked_at = Some(now());
        self.save(&tokens)?;
        Ok(true)
    }
}

fn validate_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME {
        return Err(Error::BadRequest(
            "token name must be 1–80 characters".to_string(),
        ));
    }
    if name.chars().any(char::is_control) {
        return Err(Error::BadRequest(
            "token name must not contain control characters".to_string(),
        ));
    }
    Ok(name.to_string())
}

fn validate_projects(projects: Vec<String>) -> Result<Vec<String>> {
    if projects.len() > MAX_PROJECTS {
        return Err(Error::BadRequest(format!(
            "a token can name at most {MAX_PROJECTS} projects"
        )));
    }
    let mut seen = HashSet::new();
    let mut clean = Vec::new();
    for project in projects {
        let project = project.trim().to_string();
        if project.is_empty()
            || project.len() > 100
            || project.starts_with('/')
            || project.contains('\\')
            || project.contains('/')
            || project.chars().any(char::is_control)
            || project == "."
            || project == ".."
        {
            return Err(Error::BadRequest(format!(
                "invalid project slug {project:?}"
            )));
        }
        if seen.insert(project.clone()) {
            clean.push(project);
        }
    }
    Ok(clean)
}

fn validate_jobs(kind: TokenKind, jobs: Vec<String>) -> Result<Vec<String>> {
    if kind == TokenKind::Personal {
        return Ok(Vec::new());
    }
    let mut seen = HashSet::new();
    let mut clean = Vec::new();
    for job in jobs {
        let job = job.trim().to_ascii_lowercase();
        if !matches!(job.as_str(), "review" | "handoff") {
            return Err(Error::BadRequest(format!(
                "unknown agent job {job:?}; use review or handoff"
            )));
        }
        if seen.insert(job.clone()) {
            clean.push(job);
        }
    }
    if clean.is_empty() {
        clean.push("review".to_string());
    }
    Ok(clean)
}

fn validate_expiry(days: Option<i64>) -> Result<Option<i64>> {
    match days {
        None => Ok(None),
        Some(days) if (1..=MAX_EXPIRY_DAYS).contains(&days) => Ok(Some(days)),
        Some(_) => Err(Error::BadRequest(format!(
            "expires_days must be between 1 and {MAX_EXPIRY_DAYS}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn store() -> (TempDir, TokenStore) {
        let dir = TempDir::new().unwrap();
        let store = TokenStore::new(dir.path().join("data"));
        (dir, store)
    }

    #[test]
    fn a_personal_token_hashes_the_raw_value_and_is_scoped() {
        let (_dir, store) = store();
        let grant = store
            .create_personal(
                "usr_anna",
                "Extension",
                Some(vec!["redline".to_string()]),
                true,
                Some(30),
            )
            .unwrap();
        assert!(grant.raw.starts_with("rl_pat_"));
        assert!(grant.token.hash != grant.raw);
        assert_eq!(
            grant.token.projects.as_deref(),
            Some(&["redline".to_string()][..])
        );
        assert!(grant.token.read_only);
        assert!(grant.token.expires_at.is_some());
        let found = store.lookup(&grant.raw).unwrap().unwrap();
        assert_eq!(found.id, grant.token.id);
        assert!(store.lookup("rl_pat_wrong").unwrap().is_none());
        assert_eq!(store.for_user("usr_anna").unwrap().len(), 1);
    }

    #[test]
    fn an_agent_token_has_jobs_and_an_allowlist() {
        let (_dir, store) = store();
        let grant = store
            .create_agent(
                "holly",
                "Bridge",
                Some(vec!["redline".to_string()]),
                vec!["review".to_string(), "handoff".to_string()],
                None,
            )
            .unwrap();
        assert!(grant.raw.starts_with("rl_agt_"));
        assert_eq!(grant.token.agent.as_deref(), Some("holly"));
        assert_eq!(grant.token.jobs, vec!["review", "handoff"]);
        assert!(store
            .agents()
            .unwrap()
            .iter()
            .any(|token| token.id == grant.token.id));
    }

    #[test]
    fn revoked_and_expired_tokens_stop_working() {
        let (_dir, store) = store();
        let grant = store
            .create_personal("usr_anna", "CLI", None, false, None)
            .unwrap();
        assert!(store
            .revoke_for_user(&grant.token.id, "usr_petr")
            .unwrap()
            .eq(&false));
        assert!(store.lookup(&grant.raw).unwrap().is_some());
        assert!(store.revoke_for_user(&grant.token.id, "usr_anna").unwrap());
        assert!(store.lookup(&grant.raw).unwrap().is_none());

        let short = store
            .create_personal("usr_anna", "Expired", None, false, Some(1))
            .unwrap();
        let mut tokens = store.all().unwrap();
        for token in &mut tokens {
            if token.id == short.token.id {
                token.expires_at = Some("2020-01-01T00:00:00+00:00".to_string());
            }
        }
        store.save(&tokens).unwrap();
        assert!(store.lookup(&short.raw).unwrap().is_none());
    }

    #[test]
    fn validation_rejects_bad_names_scopes_and_jobs() {
        let (_dir, store) = store();
        assert!(store
            .create_personal("usr_anna", "", None, false, None)
            .is_err());
        assert!(store
            .create_personal(
                "usr_anna",
                "x",
                Some(vec!["../escape".to_string()]),
                false,
                None,
            )
            .is_err());
        assert!(store
            .create_agent("holly", "Bridge", None, vec!["deploy".to_string()], None)
            .is_err());
    }
}
