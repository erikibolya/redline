//! Where redline keeps its data, shared by the CLI, the headless server and the desktop app.
//!
//! One implementation on purpose: if the tray app and `redline open` disagreed about the token
//! file, every agent setup would fail with a 401 that looks like a bug.

use std::io;
use std::path::{Path, PathBuf};

use crate::auth::Auth;

/// OS-appropriate data directory, overridable with `REDLINE_DATA_DIR` (portable mode).
pub fn default_data_dir() -> PathBuf {
    if let Ok(explicit) = std::env::var("REDLINE_DATA_DIR") {
        return PathBuf::from(explicit);
    }
    if cfg!(target_os = "windows") {
        if let Ok(base) = std::env::var("LOCALAPPDATA") {
            return PathBuf::from(base).join("redline");
        }
    } else if cfg!(target_os = "macos") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join("Library/Application Support/redline");
        }
    } else if let Ok(base) = std::env::var("XDG_DATA_HOME") {
        return PathBuf::from(base).join("redline");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".local/share/redline");
    }
    PathBuf::from("./redline-data")
}

pub fn sessions_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("sessions")
}

pub fn token_path(data_dir: &Path) -> PathBuf {
    data_dir.join("token")
}

/// Read the stored token, creating one on first run.
///
/// `persist` is only true for the process that owns the server: a client command that passes
/// `--token` must not rewrite the file for everyone else.
pub fn load_or_create_token(
    data_dir: &Path,
    override_token: Option<String>,
    persist: bool,
) -> io::Result<Auth> {
    if let Some(token) = override_token {
        let auth = Auth::new(token);
        if persist {
            std::fs::create_dir_all(data_dir)?;
            let path = token_path(data_dir);
            std::fs::write(&path, format!("{}\n", auth.token()))?;
            restrict_permissions(&path);
        }
        return Ok(auth);
    }
    let path = token_path(data_dir);
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let trimmed = existing.trim();
        if !trimmed.is_empty() {
            return Ok(Auth::new(trimmed));
        }
    }
    std::fs::create_dir_all(data_dir)?;
    let auth = Auth::generate();
    std::fs::write(&path, format!("{}\n", auth.token()))?;
    restrict_permissions(&path);
    Ok(auth)
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_stable_across_calls() {
        let dir = tempfile::tempdir().unwrap();
        let first = load_or_create_token(dir.path(), None, false).unwrap();
        let second = load_or_create_token(dir.path(), None, false).unwrap();
        assert_eq!(first.token(), second.token());
        assert!(token_path(dir.path()).is_file());
    }

    #[test]
    fn explicit_token_only_persists_when_asked() {
        let dir = tempfile::tempdir().unwrap();
        let auth = load_or_create_token(dir.path(), Some("given".into()), false).unwrap();
        assert_eq!(auth.token(), "given");
        assert!(
            !token_path(dir.path()).exists(),
            "a client must not rewrite the token file"
        );

        load_or_create_token(dir.path(), Some("served".into()), true).unwrap();
        let stored = std::fs::read_to_string(token_path(dir.path())).unwrap();
        assert_eq!(stored.trim(), "served");
    }

    #[test]
    fn sessions_live_under_the_data_dir() {
        assert_eq!(
            sessions_dir(Path::new("/tmp/x")),
            Path::new("/tmp/x/sessions")
        );
    }
}
