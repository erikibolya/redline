//! Project membership: who may read, create and submit in which project.
//!
//! The owner is global and is not stored here. A user can be a `developer`, `tester` or
//! `viewer` in each project; the pair `(project, user)` is unique.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::store::{read_json, write_json};

pub const MEMBERSHIPS_FILE: &str = "memberships.json";
pub const SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectRole {
    Developer,
    Tester,
    Viewer,
}

impl ProjectRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Developer => "developer",
            Self::Tester => "tester",
            Self::Viewer => "viewer",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "developer" => Some(Self::Developer),
            "tester" => Some(Self::Tester),
            "viewer" => Some(Self::Viewer),
            _ => None,
        }
    }

    pub fn can_create(self) -> bool {
        matches!(self, Self::Developer | Self::Tester)
    }

    pub fn can_submit(self) -> bool {
        self == Self::Developer
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Membership {
    pub project: String,
    pub user: String,
    pub role: ProjectRole,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct MembershipsFile {
    #[serde(default)]
    schema: u32,
    #[serde(default)]
    memberships: Vec<Membership>,
}

#[derive(Clone, Debug)]
pub struct MembershipStore {
    root: PathBuf,
}

impl MembershipStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path(&self) -> PathBuf {
        self.root.join(MEMBERSHIPS_FILE)
    }

    pub fn ensure(&self) -> Result<()> {
        fs::create_dir_all(&self.root)?;
        Ok(())
    }

    pub fn all(&self) -> Result<Vec<Membership>> {
        let file: MembershipsFile = read_json(&self.path())
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default();
        Ok(file.memberships)
    }

    fn save(&self, memberships: &[Membership]) -> Result<()> {
        self.ensure()?;
        write_json(
            &self.path(),
            &serde_json::to_value(MembershipsFile {
                schema: SCHEMA_VERSION,
                memberships: memberships.to_vec(),
            })?,
        )
    }

    pub fn for_user(&self, user_id: &str) -> Result<Vec<Membership>> {
        Ok(self
            .all()?
            .into_iter()
            .filter(|membership| membership.user == user_id)
            .collect())
    }

    pub fn role_for(&self, user_id: &str, project: &str) -> Result<Option<ProjectRole>> {
        Ok(self
            .all()?
            .into_iter()
            .find(|membership| membership.user == user_id && membership.project == project)
            .map(|membership| membership.role))
    }

    pub fn members(&self, project: &str) -> Result<Vec<Membership>> {
        Ok(self
            .all()?
            .into_iter()
            .filter(|membership| membership.project == project)
            .collect())
    }

    pub fn set_role(&self, project: &str, user_id: &str, role: Option<ProjectRole>) -> Result<()> {
        let project = project.trim();
        let user_id = user_id.trim();
        if project.is_empty() || user_id.is_empty() {
            return Err(Error::BadRequest(
                "project and user are required".to_string(),
            ));
        }
        let mut memberships = self.all()?;
        memberships
            .retain(|membership| !(membership.project == project && membership.user == user_id));
        if let Some(role) = role {
            memberships.push(Membership {
                project: project.to_string(),
                user: user_id.to_string(),
                role,
            });
        }
        self.save(&memberships)
    }

    /// Replace the members of one project. Used by the People & access matrix.
    pub fn replace_project(&self, project: &str, members: &[(String, ProjectRole)]) -> Result<()> {
        let project = project.trim();
        if project.is_empty() {
            return Err(Error::BadRequest("project is required".to_string()));
        }
        let mut memberships = self.all()?;
        memberships.retain(|membership| membership.project != project);
        for (user_id, role) in members {
            if user_id.trim().is_empty() {
                return Err(Error::BadRequest("user is required".to_string()));
            }
            memberships.push(Membership {
                project: project.to_string(),
                user: user_id.trim().to_string(),
                role: *role,
            });
        }
        self.save(&memberships)
    }

    /// Replace one user's project roles without touching any other member. The People page edits
    /// a person, not a project, so a save must not rewrite every other user in those projects.
    pub fn replace_user_projects(
        &self,
        user_id: &str,
        memberships: &[(String, ProjectRole)],
    ) -> Result<()> {
        let user_id = user_id.trim();
        if user_id.is_empty() {
            return Err(Error::BadRequest("user is required".to_string()));
        }
        let mut all = self.all()?;
        all.retain(|membership| membership.user != user_id);
        for (project, role) in memberships {
            let project = project.trim();
            if project.is_empty() {
                return Err(Error::BadRequest("project is required".to_string()));
            }
            all.push(Membership {
                project: project.to_string(),
                user: user_id.to_string(),
                role: *role,
            });
        }
        self.save(&all)
    }

    pub fn remove_user(&self, user_id: &str) -> Result<usize> {
        let mut memberships = self.all()?;
        let before = memberships.len();
        memberships.retain(|membership| membership.user != user_id);
        let removed = before - memberships.len();
        if removed > 0 {
            self.save(&memberships)?;
        }
        Ok(removed)
    }

    pub fn remove_project(&self, project: &str) -> Result<usize> {
        let mut memberships = self.all()?;
        let before = memberships.len();
        memberships.retain(|membership| membership.project != project);
        let removed = before - memberships.len();
        if removed > 0 {
            self.save(&memberships)?;
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn store() -> (TempDir, MembershipStore) {
        let dir = TempDir::new().unwrap();
        let store = MembershipStore::new(dir.path().join("data"));
        (dir, store)
    }

    #[test]
    fn roles_are_unique_per_project_and_user() {
        let (_dir, store) = store();
        store
            .set_role("actiunit-au-net", "usr_anna", Some(ProjectRole::Developer))
            .unwrap();
        store
            .set_role("actiunit-au-net", "usr_anna", Some(ProjectRole::Tester))
            .unwrap();
        assert_eq!(
            store.role_for("usr_anna", "actiunit-au-net").unwrap(),
            Some(ProjectRole::Tester)
        );
        assert_eq!(store.members("actiunit-au-net").unwrap().len(), 1);
        store.set_role("actiunit-au-net", "usr_anna", None).unwrap();
        assert_eq!(store.role_for("usr_anna", "actiunit-au-net").unwrap(), None);
    }

    #[test]
    fn a_user_can_have_different_roles_in_different_projects() {
        let (_dir, store) = store();
        store
            .set_role("actiunit-au-net", "usr_anna", Some(ProjectRole::Developer))
            .unwrap();
        store
            .set_role("redline", "usr_anna", Some(ProjectRole::Tester))
            .unwrap();
        assert_eq!(store.for_user("usr_anna").unwrap().len(), 2);
        assert_eq!(
            store.role_for("usr_anna", "actiunit-au-net").unwrap(),
            Some(ProjectRole::Developer)
        );
        assert_eq!(
            store.role_for("usr_anna", "redline").unwrap(),
            Some(ProjectRole::Tester)
        );
        assert_eq!(store.role_for("usr_anna", "other").unwrap(), None);
    }

    #[test]
    fn replacing_a_users_projects_keeps_other_members() {
        let (_dir, store) = store();
        store
            .set_role("a", "usr_anna", Some(ProjectRole::Developer))
            .unwrap();
        store
            .set_role("b", "usr_anna", Some(ProjectRole::Tester))
            .unwrap();
        store
            .set_role("a", "usr_petr", Some(ProjectRole::Viewer))
            .unwrap();
        store
            .replace_user_projects("usr_anna", &[("b".to_string(), ProjectRole::Developer)])
            .unwrap();
        assert_eq!(store.role_for("usr_anna", "a").unwrap(), None);
        assert_eq!(
            store.role_for("usr_anna", "b").unwrap(),
            Some(ProjectRole::Developer)
        );
        assert_eq!(
            store.role_for("usr_petr", "a").unwrap(),
            Some(ProjectRole::Viewer)
        );
    }

    #[test]
    fn replacing_a_project_keeps_other_projects() {
        let (_dir, store) = store();
        store
            .set_role("a", "usr_anna", Some(ProjectRole::Developer))
            .unwrap();
        store
            .set_role("b", "usr_petr", Some(ProjectRole::Tester))
            .unwrap();
        store
            .replace_project("a", &[("usr_jana".to_string(), ProjectRole::Tester)])
            .unwrap();
        assert_eq!(store.members("a").unwrap().len(), 1);
        assert_eq!(store.members("a").unwrap()[0].user, "usr_jana");
        assert_eq!(store.members("b").unwrap().len(), 1);
    }
}
