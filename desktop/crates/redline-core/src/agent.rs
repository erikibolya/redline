//! Agent concurrency control: global/per-project run limits and branch ownership leases.
//!
//! The server is the authority. An agent bridge asks for work, heartbeats its claim, and marks
//! itself `waiting` when the human has the ball. A waiting claim keeps the branch lock (nobody
//! else may start the same feature) but does **not** consume an execution slot, so a review
//! waiting for the human cannot block another project.
//!
//! RedLine never touches git. The agent declares the branch/work key; the bridge creates the
//! worktree. This module only enforces that one feature branch has one owner at a time.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const DEFAULT_MAX_PARALLEL: u32 = 2;
pub const DEFAULT_PROJECT_MAX_PARALLEL: u32 = 1;
pub const DEFAULT_LEASE_SECONDS: u64 = 120;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentSettings {
    #[serde(default = "default_max_parallel")]
    pub max_parallel: u32,
    #[serde(default = "default_project_max_parallel")]
    pub project_max_parallel: u32,
    #[serde(default = "default_lease_seconds")]
    pub lease_seconds: u64,
}

fn default_max_parallel() -> u32 {
    DEFAULT_MAX_PARALLEL
}

fn default_project_max_parallel() -> u32 {
    DEFAULT_PROJECT_MAX_PARALLEL
}

fn default_lease_seconds() -> u64 {
    DEFAULT_LEASE_SECONDS
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            max_parallel: DEFAULT_MAX_PARALLEL,
            project_max_parallel: DEFAULT_PROJECT_MAX_PARALLEL,
            lease_seconds: DEFAULT_LEASE_SECONDS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClaimState {
    Running,
    Waiting,
}

impl ClaimState {
    pub fn as_str(self) -> &'static str {
        match self {
            ClaimState::Running => "running",
            ClaimState::Waiting => "waiting",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Claim {
    pub session_id: String,
    pub agent_id: String,
    pub project: String,
    /// Branch or `session/<id>`; the branch-lock key.
    pub work_key: String,
    pub state: ClaimState,
    pub claimed_at: u64,
    pub heartbeat_at: u64,
    pub lease_until: u64,
}

#[derive(Debug, Clone)]
pub struct ClaimRequest {
    pub session_id: String,
    pub agent_id: String,
    pub project: String,
    pub work_key: String,
    pub state: ClaimState,
    pub global_max: u32,
    pub project_max: u32,
    pub lease_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimError {
    SessionClaimed { agent: String },
    BranchLocked { agent: String, session: String },
    AtCapacity { scope: String },
    NotClaimed,
    NotOwner { agent: String },
    Invalid(String),
}

impl fmt::Display for ClaimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClaimError::SessionClaimed { agent } => {
                write!(f, "this review is already claimed by {agent}")
            }
            ClaimError::BranchLocked { agent, session } => write!(
                f,
                "branch is already owned by {agent} for session {session}"
            ),
            ClaimError::AtCapacity { scope } => {
                write!(f, "no free execution slot ({scope})")
            }
            ClaimError::NotClaimed => write!(f, "the session is not claimed"),
            ClaimError::NotOwner { agent } => {
                write!(f, "the claim belongs to {agent}, not this agent")
            }
            ClaimError::Invalid(message) => write!(f, "{message}"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentStatus {
    pub settings: AgentSettings,
    pub running: usize,
    pub waiting: usize,
    pub claims: Vec<Claim>,
}

/// In-memory authority for settings and claims, persisted beside the sessions folder.
#[derive(Debug)]
pub struct AgentControl {
    data_dir: PathBuf,
    settings: AgentSettings,
    claims: BTreeMap<String, Claim>,
}

impl AgentControl {
    pub fn load(data_dir: impl Into<PathBuf>) -> Self {
        let data_dir = data_dir.into();
        let _ = fs::create_dir_all(&data_dir);
        let settings = read_json(&settings_path(&data_dir)).unwrap_or_default();
        let claims = read_json(&claims_path(&data_dir)).unwrap_or_default();
        let mut control = Self {
            data_dir,
            settings,
            claims,
        };
        control.cleanup();
        control
    }

    pub fn settings(&self) -> &AgentSettings {
        &self.settings
    }

    pub fn set_settings(&mut self, settings: AgentSettings) -> Result<AgentSettings, String> {
        if !(1..=32).contains(&settings.max_parallel) {
            return Err("max_parallel must be between 1 and 32".to_string());
        }
        if !(1..=32).contains(&settings.project_max_parallel) {
            return Err("project_max_parallel must be between 1 and 32".to_string());
        }
        if !(30..=3600).contains(&settings.lease_seconds) {
            return Err("lease_seconds must be between 30 and 3600".to_string());
        }
        self.settings = settings;
        self.save_settings()?;
        Ok(self.settings.clone())
    }

    pub fn claims(&mut self) -> Vec<Claim> {
        self.cleanup();
        self.claims.values().cloned().collect()
    }

    pub fn status(&mut self) -> AgentStatus {
        self.cleanup();
        let running = self
            .claims
            .values()
            .filter(|claim| claim.state == ClaimState::Running)
            .count();
        let waiting = self
            .claims
            .values()
            .filter(|claim| claim.state == ClaimState::Waiting)
            .count();
        AgentStatus {
            settings: self.settings.clone(),
            running,
            waiting,
            claims: self.claims.values().cloned().collect(),
        }
    }

    /// Claim a session, or update an existing claim owned by the same agent.
    pub fn try_claim(&mut self, request: ClaimRequest) -> Result<Claim, ClaimError> {
        let now = now();
        self.cleanup_at(now);

        let work_key = request.work_key.trim().to_string();
        if work_key.is_empty() {
            return Err(ClaimError::Invalid("work_key is required".to_string()));
        }
        if request.agent_id.trim().is_empty() {
            return Err(ClaimError::Invalid("agent_id is required".to_string()));
        }
        let lease_seconds = request.lease_seconds.max(1);

        if let Some(existing) = self.claims.get(&request.session_id) {
            if existing.agent_id != request.agent_id {
                return Err(ClaimError::SessionClaimed {
                    agent: existing.agent_id.clone(),
                });
            }
            self.ensure_branch_free(&request.session_id, &work_key)?;
            if request.state == ClaimState::Running && existing.state == ClaimState::Waiting {
                self.ensure_capacity(
                    &request.project,
                    request.global_max,
                    request.project_max,
                    Some(&request.session_id),
                )?;
            }
            let claim = Claim {
                session_id: request.session_id.clone(),
                agent_id: request.agent_id.clone(),
                project: request.project.clone(),
                work_key,
                state: request.state,
                claimed_at: existing.claimed_at,
                heartbeat_at: now,
                lease_until: now + lease_seconds,
            };
            self.claims.insert(request.session_id, claim.clone());
            self.persist_claims()?;
            return Ok(claim);
        }

        self.ensure_branch_free(&request.session_id, &work_key)?;
        if request.state == ClaimState::Running {
            self.ensure_capacity(
                &request.project,
                request.global_max,
                request.project_max,
                Some(&request.session_id),
            )?;
        }

        let claim = Claim {
            session_id: request.session_id.clone(),
            agent_id: request.agent_id.clone(),
            project: request.project.clone(),
            work_key,
            state: request.state,
            claimed_at: now,
            heartbeat_at: now,
            lease_until: now + lease_seconds,
        };
        self.claims.insert(request.session_id, claim.clone());
        self.persist_claims()?;
        Ok(claim)
    }

    pub fn heartbeat(
        &mut self,
        session_id: &str,
        agent_id: &str,
        lease_seconds: u64,
    ) -> Result<Claim, ClaimError> {
        let now = now();
        self.cleanup_at(now);
        let claim = self
            .claims
            .get_mut(session_id)
            .ok_or(ClaimError::NotClaimed)?;
        if claim.agent_id != agent_id {
            return Err(ClaimError::NotOwner {
                agent: claim.agent_id.clone(),
            });
        }
        claim.heartbeat_at = now;
        claim.lease_until = now + lease_seconds.max(1);
        let updated = claim.clone();
        self.persist_claims()?;
        Ok(updated)
    }

    pub fn set_state(
        &mut self,
        session_id: &str,
        agent_id: &str,
        state: ClaimState,
        global_max: u32,
        project_max: u32,
        lease_seconds: u64,
    ) -> Result<Claim, ClaimError> {
        let now = now();
        self.cleanup_at(now);
        let existing = self
            .claims
            .get(session_id)
            .cloned()
            .ok_or(ClaimError::NotClaimed)?;
        if existing.agent_id != agent_id {
            return Err(ClaimError::NotOwner {
                agent: existing.agent_id,
            });
        }
        if state == ClaimState::Running && existing.state == ClaimState::Waiting {
            self.ensure_capacity(&existing.project, global_max, project_max, Some(session_id))?;
        }
        let claim = Claim {
            state,
            heartbeat_at: now,
            lease_until: now + lease_seconds.max(1),
            ..existing
        };
        self.claims.insert(session_id.to_string(), claim.clone());
        self.persist_claims()?;
        Ok(claim)
    }

    pub fn release(&mut self, session_id: &str, agent_id: &str) -> Result<Claim, ClaimError> {
        self.cleanup();
        let claim = self
            .claims
            .get(session_id)
            .cloned()
            .ok_or(ClaimError::NotClaimed)?;
        if claim.agent_id != agent_id {
            return Err(ClaimError::NotOwner {
                agent: claim.agent_id.clone(),
            });
        }
        self.claims.remove(session_id);
        self.persist_claims()?;
        Ok(claim)
    }

    fn ensure_branch_free(&self, session_id: &str, work_key: &str) -> Result<(), ClaimError> {
        for (other_id, other) in &self.claims {
            if other_id == session_id {
                continue;
            }
            if other.work_key == work_key {
                return Err(ClaimError::BranchLocked {
                    agent: other.agent_id.clone(),
                    session: other_id.clone(),
                });
            }
        }
        Ok(())
    }

    fn ensure_capacity(
        &self,
        project: &str,
        global_max: u32,
        project_max: u32,
        exclude_session: Option<&str>,
    ) -> Result<(), ClaimError> {
        let running: Vec<&Claim> = self
            .claims
            .values()
            .filter(|claim| claim.state == ClaimState::Running)
            .filter(|claim| exclude_session != Some(claim.session_id.as_str()))
            .collect();
        if running.len() as u32 >= global_max {
            return Err(ClaimError::AtCapacity {
                scope: "global".to_string(),
            });
        }
        if running
            .iter()
            .filter(|claim| claim.project == project)
            .count() as u32
            >= project_max
        {
            return Err(ClaimError::AtCapacity {
                scope: format!("project {project}"),
            });
        }
        Ok(())
    }

    fn cleanup(&mut self) {
        self.cleanup_at(now());
    }

    fn cleanup_at(&mut self, now: u64) {
        let before = self.claims.len();
        self.claims.retain(|_, claim| claim.lease_until > now);
        if self.claims.len() != before {
            let _ = self.save_claims();
        }
    }

    fn save_settings(&self) -> Result<(), String> {
        write_json(&settings_path(&self.data_dir), &self.settings)
    }

    fn save_claims(&self) -> Result<(), String> {
        write_json(&claims_path(&self.data_dir), &self.claims)
    }

    fn persist_claims(&self) -> Result<(), ClaimError> {
        self.save_claims()
            .map_err(|err| ClaimError::Invalid(format!("cannot persist claims: {err}")))
    }
}

fn settings_path(data_dir: &Path) -> PathBuf {
    data_dir.join("agent-settings.json")
}

fn claims_path(data_dir: &Path) -> PathBuf {
    data_dir.join("agent-claims.json")
}

fn now() -> u64 {
    chrono::Utc::now().timestamp().max(0) as u64
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let data = serde_json::to_vec_pretty(value).map_err(|err| err.to_string())?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, data).map_err(|err| err.to_string())?;
    fs::rename(&tmp, path).map_err(|err| err.to_string())
}
