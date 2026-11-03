//! Human accounts and the single owner of a RedLine server.
//!
//! The legacy token remains valid for agents (and as a migration owner credential), but humans
//! get named accounts with Argon2id password hashes. `server.json` is the source of truth for
//! ownership: exactly one user id, and a transfer is an explicit, audited operation.

use std::fs;
use std::path::{Path, PathBuf};

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::store::{now, read_json, write_secret_json};

pub const USERS_FILE: &str = "users.json";
pub const SERVER_FILE: &str = "server.json";
pub const SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UserRole {
    Owner,
    Member,
}

impl UserRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Member => "member",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UserStatus {
    Invited,
    Active,
    Disabled,
}

impl UserStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Invited => "invited",
            Self::Active => "active",
            Self::Disabled => "disabled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default)]
    pub password_hash: String,
    #[serde(default = "default_role")]
    pub role: UserRole,
    #[serde(default = "default_status")]
    pub status: UserStatus,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_login_at: Option<String>,
    #[serde(default)]
    pub must_change_password: bool,
}

fn default_role() -> UserRole {
    UserRole::Member
}

fn default_status() -> UserStatus {
    UserStatus::Active
}

impl User {
    /// The API shape: never includes the password hash.
    pub fn public(&self, is_owner: bool) -> PublicUser {
        PublicUser {
            id: self.id.clone(),
            username: self.username.clone(),
            display_name: self.display_name.clone(),
            email: self.email.clone(),
            role: if is_owner { "owner" } else { "member" }.to_string(),
            status: self.status.as_str().to_string(),
            created_at: self.created_at.clone(),
            last_login_at: self.last_login_at.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PublicUser {
    pub id: String,
    pub username: String,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    pub role: String,
    pub status: String,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_login_at: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct UsersFile {
    #[serde(default)]
    schema: u32,
    #[serde(default)]
    users: Vec<User>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ServerFile {
    #[serde(default)]
    schema: u32,
    #[serde(default)]
    owner_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct UserStore {
    root: PathBuf,
}

impl UserStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn users_path(&self) -> PathBuf {
        self.root.join(USERS_FILE)
    }

    fn server_path(&self) -> PathBuf {
        self.root.join(SERVER_FILE)
    }

    pub fn ensure(&self) -> Result<()> {
        fs::create_dir_all(&self.root)?;
        Ok(())
    }

    pub fn users(&self) -> Result<Vec<User>> {
        let file: UsersFile = read_json(&self.users_path())
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default();
        Ok(file.users)
    }

    fn save_users(&self, users: &[User]) -> Result<()> {
        self.ensure()?;
        write_secret_json(
            &self.users_path(),
            &serde_json::to_value(UsersFile {
                schema: SCHEMA_VERSION,
                users: users.to_vec(),
            })?,
        )
    }

    fn server(&self) -> Option<ServerFile> {
        read_json(&self.server_path()).and_then(|value| serde_json::from_value(value).ok())
    }

    /// `server.json` is authoritative. A legacy/partial file with exactly one `role: owner`
    /// user is accepted so a failed bootstrap can recover instead of locking the server.
    pub fn owner_id(&self) -> Result<Option<String>> {
        let users = self.users()?;
        if let Some(id) = self.server().and_then(|server| server.owner_id) {
            if users.iter().any(|user| user.id == id) {
                return Ok(Some(id));
            }
        }
        let owners: Vec<&User> = users
            .iter()
            .filter(|user| user.role == UserRole::Owner)
            .collect();
        match owners.len() {
            0 => Ok(None),
            1 => Ok(Some(owners[0].id.clone())),
            _ => Err(Error::Conflict(
                "multiple owners in users.json; repair server.json".to_string(),
            )),
        }
    }

    pub fn owner(&self) -> Result<Option<User>> {
        let Some(id) = self.owner_id()? else {
            return Ok(None);
        };
        Ok(self.users()?.into_iter().find(|user| user.id == id))
    }

    pub fn is_owner(&self, id: &str) -> Result<bool> {
        Ok(self.owner_id()?.as_deref() == Some(id))
    }

    pub fn has_users(&self) -> Result<bool> {
        Ok(!self.users()?.is_empty())
    }

    pub fn get(&self, id: &str) -> Result<Option<User>> {
        Ok(self.users()?.into_iter().find(|user| user.id == id))
    }

    pub fn by_username(&self, username: &str) -> Result<Option<User>> {
        let username = normalize_username(username);
        Ok(self
            .users()?
            .into_iter()
            .find(|user| user.username == username))
    }

    /// Create a regular member. The owner is created by `create_owner`.
    pub fn create_user(&self, username: &str, display_name: &str, password: &str) -> Result<User> {
        self.create_with_role(username, display_name, password, UserRole::Member)
    }

    /// A user who exists but cannot sign in yet. The one-time invite link is the only way to set
    /// the first password; `verify_login` already refuses a non-active account.
    pub fn create_invited(&self, username: &str, display_name: &str) -> Result<User> {
        let username = validate_username(username)?;
        let display_name = validate_display_name(display_name)?;
        let mut users = self.users()?;
        if users
            .iter()
            .any(|user| user.username.eq_ignore_ascii_case(&username))
        {
            return Err(Error::Exists(format!("username {username}")));
        }
        let user = User {
            id: format!("usr_{}", uuid::Uuid::new_v4().simple()),
            username,
            display_name,
            email: None,
            password_hash: String::new(),
            role: UserRole::Member,
            status: UserStatus::Invited,
            created_at: now(),
            last_login_at: None,
            must_change_password: false,
        };
        users.push(user.clone());
        self.save_users(&users)?;
        Ok(user)
    }

    /// First-run bootstrap: exactly one owner, then never again through this path.
    pub fn create_owner(&self, username: &str, display_name: &str, password: &str) -> Result<User> {
        if self.owner_id()?.is_some() {
            return Err(Error::Conflict(
                "this server already has an owner".to_string(),
            ));
        }
        let user = self.create_with_role(username, display_name, password, UserRole::Owner)?;
        self.write_owner(&user.id)?;
        Ok(user)
    }

    fn create_with_role(
        &self,
        username: &str,
        display_name: &str,
        password: &str,
        role: UserRole,
    ) -> Result<User> {
        let username = validate_username(username)?;
        let display_name = validate_display_name(display_name)?;
        validate_password(password)?;
        let mut users = self.users()?;
        if users
            .iter()
            .any(|user| user.username.eq_ignore_ascii_case(&username))
        {
            return Err(Error::Exists(format!("username {username}")));
        }
        let user = User {
            id: format!("usr_{}", uuid::Uuid::new_v4().simple()),
            username,
            display_name,
            email: None,
            password_hash: hash_password(password)?,
            role,
            status: UserStatus::Active,
            created_at: now(),
            last_login_at: None,
            must_change_password: false,
        };
        users.push(user.clone());
        self.save_users(&users)?;
        Ok(user)
    }

    fn write_owner(&self, owner_id: &str) -> Result<()> {
        self.ensure()?;
        write_secret_json(
            &self.server_path(),
            &serde_json::to_value(ServerFile {
                schema: SCHEMA_VERSION,
                owner_id: Some(owner_id.to_string()),
            })?,
        )
    }

    /// Verify a login. `None` is the same answer for an unknown user, a disabled user and a
    /// wrong password; the HTTP layer turns it into one generic 401.
    pub fn verify_login(&self, username: &str, password: &str) -> Result<Option<User>> {
        let Some(user) = self.by_username(username)? else {
            return Ok(None);
        };
        if user.status != UserStatus::Active || !verify_password(password, &user.password_hash) {
            return Ok(None);
        }
        let mut users = self.users()?;
        let stamp = now();
        if let Some(stored) = users.iter_mut().find(|stored| stored.id == user.id) {
            stored.last_login_at = Some(stamp.clone());
        }
        // A failed last-login write must not turn a valid password into a login failure.
        let _ = self.save_users(&users);
        Ok(Some(User {
            last_login_at: Some(stamp),
            ..user
        }))
    }

    pub fn set_password(&self, id: &str, password: &str) -> Result<User> {
        validate_password(password)?;
        let mut users = self.users()?;
        let user = users
            .iter_mut()
            .find(|user| user.id == id)
            .ok_or_else(|| Error::NotFound(id.to_string()))?;
        user.password_hash = hash_password(password)?;
        user.must_change_password = false;
        let updated = user.clone();
        self.save_users(&users)?;
        Ok(updated)
    }

    /// Consume an invite/reset link: set the password and make the account active in one write.
    pub fn activate_with_password(&self, id: &str, password: &str) -> Result<User> {
        validate_password(password)?;
        let mut users = self.users()?;
        let user = users
            .iter_mut()
            .find(|user| user.id == id)
            .ok_or_else(|| Error::NotFound(id.to_string()))?;
        if user.status == UserStatus::Disabled {
            return Err(Error::Forbidden(
                "this account is disabled; ask the owner to re-enable it".to_string(),
            ));
        }
        user.password_hash = hash_password(password)?;
        user.status = UserStatus::Active;
        user.must_change_password = false;
        let updated = user.clone();
        self.save_users(&users)?;
        Ok(updated)
    }

    pub fn set_status(&self, id: &str, status: UserStatus) -> Result<User> {
        if self.owner_id()?.as_deref() == Some(id) && status != UserStatus::Active {
            return Err(Error::Conflict(
                "the owner cannot be disabled; transfer ownership first".to_string(),
            ));
        }
        let mut users = self.users()?;
        let user = users
            .iter_mut()
            .find(|user| user.id == id)
            .ok_or_else(|| Error::NotFound(id.to_string()))?;
        user.status = status;
        let updated = user.clone();
        self.save_users(&users)?;
        Ok(updated)
    }

    /// Move the single owner bit to another active user. The old owner becomes a member.
    pub fn transfer_owner(&self, target_id: &str) -> Result<User> {
        let current = self
            .owner_id()?
            .ok_or_else(|| Error::Conflict("this server has no owner".to_string()))?;
        let mut users = self.users()?;
        let target = users
            .iter()
            .find(|user| user.id == target_id)
            .ok_or_else(|| Error::NotFound(target_id.to_string()))?;
        if target.status != UserStatus::Active {
            return Err(Error::BadRequest(
                "the new owner must have an active account".to_string(),
            ));
        }
        if current == target_id {
            return Ok(target.clone());
        }
        // server.json is the source of truth; write it first so a failed users.json write still
        // leaves exactly one owner (the new one).
        self.write_owner(target_id)?;
        for user in &mut users {
            user.role = if user.id == target_id {
                UserRole::Owner
            } else {
                UserRole::Member
            };
        }
        self.save_users(&users)?;
        Ok(users
            .into_iter()
            .find(|user| user.id == target_id)
            .expect("target was checked above"))
    }
}

fn normalize_username(username: &str) -> String {
    username.trim().to_ascii_lowercase()
}

fn validate_username(username: &str) -> Result<String> {
    let username = normalize_username(username);
    let len = username.chars().count();
    if !(2..=32).contains(&len) {
        return Err(Error::BadRequest(
            "username must be 2–32 characters".to_string(),
        ));
    }
    let mut chars = username.chars();
    let first = chars.next().expect("length checked");
    if !first.is_ascii_alphanumeric() {
        return Err(Error::BadRequest(
            "username must start with a letter or digit".to_string(),
        ));
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')) {
        return Err(Error::BadRequest(
            "username may contain only letters, digits, '.', '_' and '-'".to_string(),
        ));
    }
    Ok(username)
}

fn validate_display_name(display_name: &str) -> Result<String> {
    let display_name = display_name.trim();
    if display_name.is_empty() || display_name.chars().count() > 80 {
        return Err(Error::BadRequest(
            "display name must be 1–80 characters".to_string(),
        ));
    }
    if display_name.chars().any(char::is_control) {
        return Err(Error::BadRequest(
            "display name must not contain control characters".to_string(),
        ));
    }
    Ok(display_name.to_string())
}

pub(crate) fn validate_password(password: &str) -> Result<()> {
    let len = password.chars().count();
    if !(12..=200).contains(&len) {
        return Err(Error::BadRequest(
            "password must be 12–200 characters".to_string(),
        ));
    }
    if password.chars().any(char::is_control) {
        return Err(Error::BadRequest(
            "password must not contain control characters".to_string(),
        ));
    }
    Ok(())
}

pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|err| Error::BadRequest(format!("cannot hash password: {err}")))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn store() -> (TempDir, UserStore) {
        let dir = TempDir::new().unwrap();
        let store = UserStore::new(dir.path().join("data"));
        (dir, store)
    }

    #[test]
    fn hashes_and_verifies_a_password() {
        let hash = hash_password("correct horse battery staple").unwrap();
        assert!(verify_password("correct horse battery staple", &hash));
        assert!(!verify_password("wrong password", &hash));
    }

    #[test]
    fn creates_one_owner_and_transfers_it() {
        let (_dir, store) = store();
        let owner = store
            .create_owner("anna", "Anna", "correct horse battery staple")
            .unwrap();
        assert_eq!(
            store.owner_id().unwrap().as_deref(),
            Some(owner.id.as_str())
        );
        assert!(store
            .create_owner("other", "Other", "correct horse battery staple")
            .is_err());

        let member = store
            .create_user("petr", "Petr", "correct horse battery staple")
            .unwrap();
        assert!(!store.is_owner(&member.id).unwrap());
        store.transfer_owner(&member.id).unwrap();
        assert_eq!(
            store.owner_id().unwrap().as_deref(),
            Some(member.id.as_str())
        );
        assert!(!store.is_owner(&owner.id).unwrap());
        assert_eq!(
            store.get(&owner.id).unwrap().unwrap().role,
            UserRole::Member
        );
    }

    #[test]
    fn rejects_duplicate_usernames_and_weak_passwords() {
        let (_dir, store) = store();
        store
            .create_owner("anna", "Anna", "correct horse battery staple")
            .unwrap();
        assert!(store
            .create_user("ANNA", "Other", "correct horse battery staple")
            .is_err());
        assert!(store.create_user("petr", "Petr", "short").is_err());
        assert!(store
            .create_user("bad name", "Petr", "correct horse battery staple")
            .is_err());
    }

    #[test]
    fn an_invited_user_cannot_log_in_until_the_password_is_set() {
        let (_dir, store) = store();
        let user = store
            .create_invited("jana", "Jana")
            .expect("create invited");
        assert_eq!(user.status, UserStatus::Invited);
        assert!(user.password_hash.is_empty());
        assert!(store.verify_login("jana", "anything").unwrap().is_none());
        let active = store
            .activate_with_password(&user.id, "correct horse battery staple")
            .unwrap();
        assert_eq!(active.status, UserStatus::Active);
        assert!(store
            .verify_login("jana", "correct horse battery staple")
            .unwrap()
            .is_some());
    }

    #[test]
    fn public_user_does_not_leak_the_hash() {
        let (_dir, store) = store();
        let owner = store
            .create_owner("anna", "Anna", "correct horse battery staple")
            .unwrap();
        let value = serde_json::to_value(owner.public(true)).unwrap();
        assert_eq!(value["role"], "owner");
        assert!(value.get("password_hash").is_none());
        assert!(value.get("password").is_none());
    }

    #[test]
    fn login_is_generic_for_unknown_and_wrong_passwords() {
        let (_dir, store) = store();
        store
            .create_owner("anna", "Anna", "correct horse battery staple")
            .unwrap();
        assert!(store
            .verify_login("anna", "correct horse battery staple")
            .unwrap()
            .is_some());
        assert!(store.verify_login("anna", "nope").unwrap().is_none());
        assert!(store.verify_login("nobody", "nope").unwrap().is_none());
    }
}
