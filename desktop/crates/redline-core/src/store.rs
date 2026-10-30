//! The session folder format (`sessions/<id>/`) and everything that reads or writes it.
//!
//! Every write goes through [`write_atomic`] (temp file + rename) so a crash mid-submit cannot
//! leave a half-written `annotations.json` that the agent then parses successfully but wrongly.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{SecondsFormat, Utc};
use serde_json::{json, Value};

use crate::error::{Error, Result};
use crate::model::{Annotation, AnnotationSet, ImageRef, Meta, Status, Submission};

/// Same rule as v0.1: one leading alphanumeric, then alphanumerics/`.`/`_`/`-`, max 121 chars.
/// This is what stops `../` and friends from escaping the sessions directory.
/// The virtual project that holds sessions created without one (legacy flat folders).
pub const INBOX: &str = "inbox";

pub fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    id.len() <= 121 && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

pub fn slugify(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.trim().to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-');
    let limited: String = trimmed.chars().take(60).collect();
    if limited.is_empty() {
        "review".to_string()
    } else {
        limited.trim_end_matches('-').to_string()
    }
}

/// RFC 3339 with second precision and a `+00:00` offset — byte-compatible with v0.1's
/// `datetime.now(timezone.utc).isoformat(timespec="seconds")`.
pub fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, false)
}

/// Image extensions we are willing to write into a session folder. Anything else is stored as
/// `.png` rather than trusted: the value ends up in a filename.
fn image_extension(name: &str) -> &'static str {
    match Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => ".png",
        "jpg" | "jpeg" => ".jpg",
        "webp" => ".webp",
        "gif" => ".gif",
        "svg" => ".svg",
        _ => ".png",
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| Error::BadRequest(format!("invalid path: {}", path.display())))?;
    let temp = path.with_file_name(format!(".{file_name}.tmp"));
    fs::write(&temp, bytes)?;
    fs::rename(&temp, path).inspect_err(|_| {
        let _ = fs::remove_file(&temp);
    })?;
    Ok(())
}

fn write_json(path: &Path, value: &Value) -> Result<()> {
    let mut text = serde_json::to_string_pretty(value)?;
    text.push('\n');
    write_atomic(path, text.as_bytes())
}

fn read_json(path: &Path) -> Option<Value> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Numbers render the way Python's `f"{x}"` did: `12` not `12.0`, `12.5` kept.
fn num(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

pub fn review_markdown(meta: &Meta, annotations: &[Annotation]) -> String {
    let mut lines = vec![format!("# Visual review: {}", meta.title), String::new()];
    if !meta.url.is_empty() {
        lines.push(format!("- Page: {}", meta.url));
    }
    if !meta.note.is_empty() {
        lines.push(format!("- Request: {}", meta.note));
    }
    if let Some(prompt) = meta.prompt.as_deref().filter(|p| !p.trim().is_empty()) {
        lines.push(format!("- Prompt: {}", prompt));
    }
    lines.push(format!("- Status: {}", meta.status.as_str()));
    lines.push(format!(
        "- Submitted: {}",
        meta.submitted_at.as_deref().unwrap_or("-")
    ));
    lines.push(String::new());
    lines.push("## Annotations".to_string());
    lines.push(String::new());

    let text_notes: Vec<&Annotation> = annotations
        .iter()
        .filter(|a| a.is_text() && a.text_of().is_some_and(|t| !t.is_empty()))
        .collect();
    if !text_notes.is_empty() {
        lines.push("### Text notes".to_string());
        lines.push(String::new());
        for note in &text_notes {
            lines.push(format!(
                "- ({}, {}): {}",
                num(note.number("x").unwrap_or_default()),
                num(note.number("y").unwrap_or_default()),
                note.text_of().unwrap_or_default()
            ));
        }
        lines.push(String::new());
    }

    // Numbers must match the badges the human sees on the canvas and on `annotated.png`, so they
    // are the position in the full annotation list, not in the filtered shape list.
    let shapes: Vec<(usize, &Annotation)> = annotations
        .iter()
        .enumerate()
        .filter(|(_, annotation)| !annotation.is_text())
        .collect();
    if !shapes.is_empty() {
        lines.push("### Marked regions".to_string());
        lines.push(String::new());
        for (index, item) in shapes {
            let name = match item.kind.as_str() {
                "rect" => "rectangle",
                "pen" => "freehand",
                other => other,
            };
            let coords = match (item.point("start"), item.point("end")) {
                (Some((sx, sy)), Some((ex, ey))) => {
                    format!(" ({}, {}) → ({}, {})", num(sx), num(sy), num(ex), num(ey))
                }
                (Some((sx, sy)), None) => format!(" ({}, {})", num(sx), num(sy)),
                _ => String::new(),
            };
            let note = item.note_of().unwrap_or("").trim();
            let note = if note.is_empty() {
                String::new()
            } else {
                format!(" — \u{201c}{note}\u{201d}")
            };
            lines.push(format!("- {}. {}{}{}", index + 1, name, coords, note));
        }
        lines.push(String::new());
    }

    lines.push("## For the agent".to_string());
    lines.push(String::new());
    lines.push(
        "Use `annotations.json` for coordinates and text; `annotated.png` is the visual reference."
            .to_string(),
    );
    lines.push(String::new());
    lines.join("\n")
}

/// One archived submission: the JSON and (optionally) the PNG from an earlier round.
#[derive(Debug, Clone, serde::Serialize)]
pub struct HistoryEntry {
    pub stamp: String,
    pub annotations: usize,
    pub png: bool,
    /// The round's source screenshot (`<stamp>-source.png`) is available for the read-only viewer.
    #[serde(default)]
    pub source: bool,
    /// 1-based round number, oldest first.
    #[serde(default)]
    pub round: usize,
    /// This entry is the submission the session is currently parked on.
    #[serde(default)]
    pub current: bool,
}

fn safe_stamp(stamp: &str) -> String {
    stamp
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => c,
            _ => '-',
        })
        .collect()
}

/// Keep the submission we are about to overwrite. Without this, "Submit again" destroys the
/// previous round and a re-review has nothing to diff against.
/// Two submissions can land in the same second; the archive name must still be unique.
fn unique_round_stamp(dir: &std::path::Path, at: &str) -> String {
    let base = safe_stamp(at);
    let history = dir.join("history");
    let mut candidate = base.clone();
    let mut counter = 1;
    while history.join(format!("{candidate}.json")).exists() {
        counter += 1;
        candidate = format!("{base}-{counter}");
    }
    candidate
}

/// Snapshot a submitted round into `history/` **at submit time**, so History fills up immediately
/// and a round can be revisited read-only. Empty rounds are kept: a prompt without annotations is
/// still a round.
fn archive_round(
    dir: &std::path::Path,
    stamp: &str,
    set: &AnnotationSet,
    source: &str,
) -> Result<()> {
    let history = dir.join("history");
    fs::create_dir_all(&history)?;
    let safe = safe_stamp(stamp);
    write_json(
        &history.join(format!("{safe}.json")),
        &serde_json::to_value(set)?,
    )?;
    let png = dir.join("annotated.png");
    if png.is_file() {
        let bytes = fs::read(&png)?;
        write_atomic(&history.join(format!("{safe}.png")), &bytes)?;
    }
    // The source screenshot lets the viewer re-render the round crisply and read-only.
    let source_path = dir.join(source);
    if source_path.is_file() {
        let bytes = fs::read(&source_path)?;
        write_atomic(&history.join(format!("{safe}-source.png")), &bytes)?;
    }
    prune_history(&history, 20)?;
    Ok(())
}

/// Whether two archived rounds carry the same picture. Two rounds without a picture are the same
/// picture (an annotations-only round).
fn same_picture(history: &Path, a: &str, b: &str) -> bool {
    let file = |stamp: &str| history.join(format!("{stamp}.png"));
    match (fs::read(file(a)), fs::read(file(b))) {
        (Ok(a), Ok(b)) => a == b,
        (Err(_), Err(_)) => true,
        _ => false,
    }
}

fn prune_history(history: &std::path::Path, keep: usize) -> Result<()> {
    let mut stamps: Vec<String> = fs::read_dir(history)?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension().and_then(|e| e.to_str()) == Some("json"))
                .then(|| {
                    path.file_stem()
                        .and_then(|s| s.to_str())
                        .map(str::to_string)
                })
                .flatten()
        })
        .collect();
    stamps.sort();
    while stamps.len() > keep {
        let oldest = stamps.remove(0);
        let _ = fs::remove_file(history.join(format!("{oldest}.json")));
        let _ = fs::remove_file(history.join(format!("{oldest}.png")));
        let _ = fs::remove_file(history.join(format!("{oldest}-source.png")));
    }
    Ok(())
}

/// A session folder root. Cheap to clone; holds no open handles.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Parent of `sessions/`: server-level state such as agent settings and claims lives here.
    pub fn data_dir(&self) -> PathBuf {
        self.root
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.root.clone())
    }

    pub fn ensure(&self) -> Result<()> {
        fs::create_dir_all(&self.root)?;
        Ok(())
    }

    pub fn dir(&self, id: &str) -> Result<PathBuf> {
        if !valid_id(id) {
            return Err(Error::InvalidId);
        }
        let flat = self.root.join(id);
        if flat.is_dir() {
            return Ok(flat);
        }
        // Project sessions live one level deeper: projects/<slug>/<id>.
        if let Ok(entries) = fs::read_dir(self.projects_root()) {
            for entry in entries.flatten() {
                let candidate = entry.path().join(id);
                if candidate.is_dir() {
                    return Ok(candidate);
                }
            }
        }
        Ok(flat)
    }

    pub fn projects_root(&self) -> PathBuf {
        self.root.join("projects")
    }

    /// Read a project by slug (name or slug accepted for the caller's convenience).
    pub fn project(&self, name: &str) -> Result<Option<crate::model::Project>> {
        let slug = slugify(name);
        let path = self.projects_root().join(&slug).join("project.json");
        Ok(read_json(&path).and_then(|value| serde_json::from_value(value).ok()))
    }

    /// Create the project folder if it does not exist yet. Idempotent: agents pass a name.
    pub fn ensure_project(&self, name: &str) -> Result<crate::model::Project> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::BadRequest("project name is required".to_string()));
        }
        if let Some(existing) = self.project(name)? {
            return Ok(existing);
        }
        let slug = slugify(name);
        let dir = self.projects_root().join(&slug);
        fs::create_dir_all(&dir)?;
        let project = crate::model::Project {
            slug: slug.clone(),
            name: name.to_string(),
            agent: None,
            repo: None,
            max_parallel: None,
            created_at: now(),
        };
        write_json(&dir.join("project.json"), &serde_json::to_value(&project)?)?;
        Ok(project)
    }

    /// Set or clear a project's concurrent-run override (v0.22).
    pub fn set_project_max_parallel(
        &self,
        slug: &str,
        max_parallel: Option<u32>,
    ) -> Result<crate::model::Project> {
        let slug = slugify(slug);
        let mut project = self
            .project(&slug)?
            .ok_or_else(|| Error::NotFound(slug.clone()))?;
        project.max_parallel = max_parallel;
        write_json(
            &self
                .projects_root()
                .join(&project.slug)
                .join("project.json"),
            &serde_json::to_value(&project)?,
        )?;
        Ok(project)
    }

    /// Bind a repository discovered by an agent to a project. Repo keys are global, so a second
    /// agent reporting the same key updates the record instead of creating a duplicate project.
    /// The slug is derived once from the key and never changes.
    pub fn sync_repo_project(
        &self,
        agent: &str,
        repo: &str,
        name: &str,
    ) -> Result<crate::model::Project> {
        let name = name.trim();
        let name = if name.is_empty() {
            repo.rsplit('/').next().unwrap_or(repo)
        } else {
            name
        };
        if let Some(mut project) = self
            .projects()?
            .into_iter()
            .find(|project| project.repo.as_deref() == Some(repo))
        {
            let mut changed = false;
            if project.name != name {
                project.name = name.to_string();
                changed = true;
            }
            if project.agent.as_deref() != Some(agent) {
                project.agent = Some(agent.to_string());
                changed = true;
            }
            if changed {
                let path = self
                    .projects_root()
                    .join(&project.slug)
                    .join("project.json");
                write_json(&path, &serde_json::to_value(&project)?)?;
            }
            return Ok(project);
        }

        let base = slugify(repo);
        let mut slug = base.clone();
        let mut suffix = 2;
        while self.project(&slug)?.is_some() {
            slug = format!("{base}-{suffix}");
            suffix += 1;
        }
        let dir = self.projects_root().join(&slug);
        fs::create_dir_all(&dir)?;
        let project = crate::model::Project {
            slug: slug.clone(),
            name: name.to_string(),
            agent: Some(agent.to_string()),
            repo: Some(repo.to_string()),
            max_parallel: None,
            created_at: now(),
        };
        write_json(&dir.join("project.json"), &serde_json::to_value(&project)?)?;
        Ok(project)
    }

    /// Every project, plus the virtual Inbox that holds legacy flat sessions.
    pub fn projects(&self) -> Result<Vec<crate::model::Project>> {
        let mut out: Vec<crate::model::Project> = Vec::new();
        if let Ok(entries) = fs::read_dir(self.projects_root()) {
            for entry in entries.flatten() {
                if !entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                    continue;
                }
                if let Some(project) = read_json(&entry.path().join("project.json"))
                    .and_then(|value| serde_json::from_value(value).ok())
                {
                    out.push(project);
                }
            }
        }
        if !out.iter().any(|project| project.slug == INBOX) {
            out.push(crate::model::Project {
                slug: INBOX.to_string(),
                name: "Inbox".to_string(),
                agent: None,
                repo: None,
                max_parallel: None,
                created_at: String::new(),
            });
        }
        out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(out)
    }

    pub fn get(&self, id: &str) -> Result<Option<Meta>> {
        let dir = self.dir(id)?;
        Ok(read_json(&dir.join("meta.json")).and_then(|value| serde_json::from_value(value).ok()))
    }

    pub fn require(&self, id: &str) -> Result<Meta> {
        self.get(id)?.ok_or_else(|| Error::NotFound(id.to_string()))
    }

    pub fn list(&self) -> Result<Vec<Meta>> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }
        let mut metas = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() || entry.file_name() == "projects" {
                continue;
            }
            if let Some(meta) = read_json(&entry.path().join("meta.json"))
                .and_then(|value| serde_json::from_value::<Meta>(value).ok())
            {
                metas.push(meta);
            }
        }
        if let Ok(projects) = fs::read_dir(self.projects_root()) {
            for project in projects.flatten() {
                if !project
                    .file_type()
                    .map(|kind| kind.is_dir())
                    .unwrap_or(false)
                {
                    continue;
                }
                let Ok(sessions) = fs::read_dir(project.path()) else {
                    continue;
                };
                for entry in sessions.flatten() {
                    if !entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                        continue;
                    }
                    if let Some(meta) = read_json(&entry.path().join("meta.json"))
                        .and_then(|value| serde_json::from_value::<Meta>(value).ok())
                    {
                        metas.push(meta);
                    }
                }
            }
        }
        // Flat sessions are the Inbox: no folder, no migration, just a label on read.
        for meta in &mut metas {
            if meta.project.is_none() {
                meta.project = Some(INBOX.to_string());
            }
        }
        metas.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(metas)
    }

    pub fn annotations(&self, id: &str) -> Result<AnnotationSet> {
        let dir = self.dir(id)?;
        Ok(read_json(&dir.join("annotations.json"))
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_else(|| AnnotationSet {
                session: id.to_string(),
                title: None,
                url: None,
                image: None,
                annotations: Vec::new(),
                submitted_at: None,
            }))
    }

    /// Record what the agent is doing. `state` is `received`, `working` or `replied`; `seen`
    /// also stamps the first time the agent read this submission.
    pub fn set_agent_state(&self, id: &str, state: &str, seen: bool) -> Result<Meta> {
        let dir = self.dir(id)?;
        let mut meta = self.require(id)?;
        meta.agent_state = Some(state.to_string());
        if seen && meta.seen_by_agent_at.is_none() {
            meta.seen_by_agent_at = Some(now());
        }
        write_json(&dir.join("meta.json"), &serde_json::to_value(&meta)?)?;
        Ok(meta)
    }

    /// Set or clear the feature branch shown in the rail and used as the branch-lock key.
    pub fn set_branch(&self, id: &str, branch: Option<&str>) -> Result<Meta> {
        let dir = self.dir(id)?;
        let mut meta = self.require(id)?;
        let branch = branch
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        if let Some(value) = branch.as_deref() {
            if value.len() > 200 {
                return Err(Error::BadRequest("branch is too long".to_string()));
            }
            if value.chars().any(char::is_control) {
                return Err(Error::BadRequest(
                    "branch must not contain control characters".to_string(),
                ));
            }
        }
        meta.branch = branch;
        write_json(&dir.join("meta.json"), &serde_json::to_value(&meta)?)?;
        Ok(meta)
    }

    /// Append one line to `agent-events.jsonl` (the activity feed the human watches).
    pub fn append_agent_event(&self, id: &str, event: &serde_json::Value) -> Result<()> {
        use std::io::Write as _;
        let dir = self.dir(id)?;
        let path = dir.join("agent-events.jsonl");
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        writeln!(file, "{event}")?;
        Ok(())
    }

    pub fn agent_events(&self, id: &str) -> Result<Vec<serde_json::Value>> {
        let path = self.dir(id)?.join("agent-events.jsonl");
        let mut events = Vec::new();
        if path.is_file() {
            for line in fs::read_to_string(&path)?.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                    events.push(value);
                }
            }
        }
        Ok(events)
    }

    /// Create a session from bytes already in memory (MCP uploads, HTTP `image_base64`).
    #[allow(clippy::too_many_arguments)]
    pub fn create_from_bytes(
        &self,
        image: &[u8],
        image_name: &str,
        title: &str,
        url: &str,
        note: &str,
        id: Option<&str>,
        agent: Option<&str>,
        project: Option<&str>,
    ) -> Result<Meta> {
        if image.is_empty() {
            return Err(Error::BadRequest("empty image".to_string()));
        }
        self.ensure()?;
        let project = match project.map(str::trim).filter(|name| !name.is_empty()) {
            Some(name) => Some(self.ensure_project(name)?),
            None => None,
        };
        let parent = match &project {
            Some(project) => self.projects_root().join(&project.slug),
            None => self.root.clone(),
        };
        let sid = self.new_id(title, id)?;
        let dir = parent.join(&sid);
        fs::create_dir(&dir).map_err(|err| {
            if err.kind() == std::io::ErrorKind::AlreadyExists {
                Error::Exists(sid.clone())
            } else {
                Error::Io(err)
            }
        })?;

        let extension = image_extension(image_name);
        write_atomic(&dir.join(format!("screenshot{extension}")), image)?;

        let meta = Meta {
            id: sid.clone(),
            title: if title.is_empty() {
                sid.clone()
            } else {
                title.to_string()
            },
            url: url.to_string(),
            note: note.to_string(),
            status: Status::Open,
            created_at: now(),
            submitted_at: None,
            image: format!("screenshot{extension}"),
            image_width: None,
            image_height: None,
            agent: agent.map(str::to_string),
            resolved_at: None,
            feature_confirmed_at: None,
            feature_cancelled_at: None,
            prompt: None,
            seen_by_agent_at: None,
            agent_state: None,
            cancelled_at: None,
            round_stamp: None,
            project: project.as_ref().map(|project| project.slug.clone()),
            branch: None,
        };
        write_json(&dir.join("meta.json"), &serde_json::to_value(&meta)?)?;
        write_json(
            &dir.join("annotations.json"),
            &json!({ "session": sid, "annotations": [] }),
        )?;
        Ok(meta)
    }

    /// Create a session from a file on this machine (the CLI path).
    pub fn create_from_path(
        &self,
        image: &Path,
        title: &str,
        url: &str,
        note: &str,
        id: Option<&str>,
        project: Option<&str>,
    ) -> Result<Meta> {
        let bytes = fs::read(image)?;
        let name = image
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("screenshot.png");
        let title = if title.is_empty() {
            image.file_stem().and_then(|s| s.to_str()).unwrap_or("")
        } else {
            title
        };
        self.create_from_bytes(&bytes, name, title, url, note, id, None, project)
    }

    fn new_id(&self, title: &str, id: Option<&str>) -> Result<String> {
        if let Some(id) = id {
            if !valid_id(id) {
                return Err(Error::InvalidId);
            }
            if self.dir(id)?.is_dir() {
                return Err(Error::Exists(id.to_string()));
            }
            return Ok(id.to_string());
        }
        let stamp = Utc::now().format("%Y%m%d-%H%M%S").to_string();
        let slug = slugify(title);
        for _ in 0..8 {
            let suffix = uuid::Uuid::new_v4().simple().to_string();
            let candidate = format!("{stamp}-{slug}-{}", &suffix[..4]);
            if !self.dir(&candidate)?.is_dir() {
                return Ok(candidate);
            }
        }
        Err(Error::BadRequest(
            "could not allocate a session id".to_string(),
        ))
    }

    /// Stop a submission. The canvas stays as it is; the agent is supposed to stop.
    ///
    /// If no agent has read the submission yet the round is withdrawn from history (the human
    /// stopped it by mistake). Once an agent has seen it the round stays: it may have acted on it.
    pub fn cancel(&self, id: &str) -> Result<Meta> {
        let dir = self.dir(id)?;
        let mut meta = self.require(id)?;
        if meta.status != Status::Submitted {
            return Err(Error::BadRequest("nothing to cancel".to_string()));
        }
        if meta.seen_by_agent_at.is_none() {
            if let Some(stamp) = meta.round_stamp.take() {
                let history = dir.join("history");
                let _ = fs::remove_file(history.join(format!("{stamp}.json")));
                let _ = fs::remove_file(history.join(format!("{stamp}.png")));
                let _ = fs::remove_file(history.join(format!("{stamp}-source.png")));
            }
        }
        meta.status = Status::Open;
        meta.agent_state = None;
        meta.cancelled_at = Some(now());
        write_json(&dir.join("meta.json"), &serde_json::to_value(&meta)?)?;
        Ok(meta)
    }

    /// Post a new round into an existing session: the agent's answer, optionally with a fresh
    /// screenshot. This is what keeps one task as one rail item instead of a new review per turn.
    ///
    /// The previous round is archived, the canvas image is replaced (old files stay for history),
    /// the annotations are cleared and the session goes back to the human (`open`).
    pub fn post_round(
        &self,
        id: &str,
        image: Option<&[u8]>,
        image_name: &str,
        note: &str,
        width: Option<u32>,
        height: Option<u32>,
    ) -> Result<Meta> {
        let dir = self.dir(id)?;
        let mut meta = self.require(id)?;
        let stamp = now();

        let mut image_file = meta.image.clone();
        if let Some(bytes) = image {
            let extension = image_extension(image_name);
            image_file = format!("screenshot-{}{extension}", safe_stamp(&stamp));
            write_atomic(&dir.join(&image_file), bytes)?;
            meta.image = image_file.clone();
            meta.image_width = width;
            meta.image_height = height;
        }

        let set = AnnotationSet {
            session: id.to_string(),
            title: Some(meta.title.clone()),
            url: Some(meta.url.clone()),
            image: Some(ImageRef {
                file: Some(image_file.clone()),
                width,
                height,
            }),
            annotations: Vec::new(),
            submitted_at: None,
        };
        write_json(&dir.join("annotations.json"), &serde_json::to_value(&set)?)?;

        meta.status = Status::Open;
        meta.submitted_at = None;
        meta.resolved_at = None;
        // New work after a decision makes the feature pending again.
        meta.feature_confirmed_at = None;
        meta.feature_cancelled_at = None;
        meta.agent_state = Some("replied".to_string());
        write_json(&dir.join("meta.json"), &serde_json::to_value(&meta)?)?;

        // The conversation keeps the round (and its picture) beside the human's annotations.
        use std::io::Write as _;
        let line = json!({ "at": stamp, "message": note, "image": image_file });
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("agent-thread.jsonl"))?;
        writeln!(file, "{line}")?;
        Ok(meta)
    }

    /// Write a submission: `annotations.json`, `meta.json`, `review.md`, and `annotated.png`.
    pub fn submit(&self, id: &str, submission: &Submission) -> Result<(Meta, AnnotationSet)> {
        let dir = self.dir(id)?;
        let mut meta = self.require(id)?;
        let submitted_at = now();
        let round_stamp = unique_round_stamp(&dir, &submitted_at);
        meta.round_stamp = Some(round_stamp.clone());
        meta.status = Status::Submitted;
        meta.submitted_at = Some(submitted_at.clone());
        // A fresh round has not been seen by the agent yet, and it reopens a decided feature.
        meta.seen_by_agent_at = None;
        meta.agent_state = None;
        meta.feature_confirmed_at = None;
        meta.feature_cancelled_at = None;
        if let Some(prompt) = submission.note.as_deref() {
            let prompt = prompt.trim();
            if !prompt.is_empty() {
                meta.prompt = Some(prompt.to_string());
            }
        }
        if let Some(width) = submission.width {
            meta.image_width = Some(width);
        }
        if let Some(height) = submission.height {
            meta.image_height = Some(height);
        }

        let set = AnnotationSet {
            session: id.to_string(),
            title: Some(meta.title.clone()),
            url: Some(meta.url.clone()),
            image: Some(ImageRef {
                file: Some(meta.image.clone()),
                width: submission.width,
                height: submission.height,
            }),
            annotations: submission.annotations.clone(),
            submitted_at: Some(submitted_at.clone()),
        };

        write_json(&dir.join("annotations.json"), &serde_json::to_value(&set)?)?;
        write_json(&dir.join("meta.json"), &serde_json::to_value(&meta)?)?;
        write_atomic(
            &dir.join("review.md"),
            review_markdown(&meta, &set.annotations).as_bytes(),
        )?;

        if let Some(png) = &submission.png {
            let raw = png.split_once(',').map(|(_, data)| data).unwrap_or(png);
            use base64::Engine as _;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(raw.trim())
                .map_err(|err| Error::BadRequest(format!("invalid png payload: {err}")))?;
            write_atomic(&dir.join("annotated.png"), &bytes)?;
        }

        // The round exists as soon as it is submitted, not when the next one replaces it.
        archive_round(&dir, &round_stamp, &set, &meta.image)?;
        Ok((meta, set))
    }

    /// Mark a session resolved (the agent is done with it). Additive status, not destructive.
    pub fn resolve(&self, id: &str) -> Result<Meta> {
        let dir = self.dir(id)?;
        let mut meta = self.require(id)?;
        meta.status = Status::Resolved;
        meta.resolved_at = Some(now());
        write_json(&dir.join("meta.json"), &serde_json::to_value(&meta)?)?;
        Ok(meta)
    }

    /// Undo `resolve` and any feature decision: back to the open queue.
    pub fn reopen(&self, id: &str) -> Result<Meta> {
        let dir = self.dir(id)?;
        let mut meta = self.require(id)?;
        meta.status = Status::Open;
        meta.resolved_at = None;
        meta.feature_confirmed_at = None;
        meta.feature_cancelled_at = None;
        write_json(&dir.join("meta.json"), &serde_json::to_value(&meta)?)?;
        Ok(meta)
    }

    /// The human says the feature is complete. RedLine only records the decision and closes the
    /// review; the agent maps `feature_state == "confirmed"` to its own workflow (open a PR,
    /// merge it, deploy — whatever the project uses).
    pub fn confirm_feature(&self, id: &str) -> Result<Meta> {
        let dir = self.dir(id)?;
        let mut meta = self.require(id)?;
        let stamp = now();
        meta.status = Status::Resolved;
        meta.resolved_at.get_or_insert_with(|| stamp.clone());
        meta.feature_confirmed_at = Some(stamp);
        meta.feature_cancelled_at = None;
        write_json(&dir.join("meta.json"), &serde_json::to_value(&meta)?)?;
        Ok(meta)
    }

    /// The human abandons the feature. Nothing is merged and the review files are kept; the
    /// agent can see `feature_state == "cancelled"`. `reopen` undoes it.
    pub fn cancel_feature(&self, id: &str) -> Result<Meta> {
        let dir = self.dir(id)?;
        let mut meta = self.require(id)?;
        let stamp = now();
        meta.status = Status::Resolved;
        meta.resolved_at.get_or_insert_with(|| stamp.clone());
        meta.feature_cancelled_at = Some(stamp);
        meta.feature_confirmed_at = None;
        write_json(&dir.join("meta.json"), &serde_json::to_value(&meta)?)?;
        Ok(meta)
    }

    pub fn remove(&self, id: &str) -> Result<PathBuf> {
        let dir = self.dir(id)?;
        if !dir.exists() {
            return Err(Error::NotFound(id.to_string()));
        }
        fs::remove_dir_all(&dir)?;
        Ok(dir)
    }

    pub fn history(&self, id: &str) -> Result<Vec<HistoryEntry>> {
        let history = self.dir(id)?.join("history");
        if !history.exists() {
            return Ok(Vec::new());
        }
        // Each row carries the round's marks and picture, so a run of copies can be collapsed
        // below and the round still on the canvas can be recognised.
        let mut entries: Vec<(HistoryEntry, Value, Option<String>)> = Vec::new();
        for entry in fs::read_dir(&history)? {
            let path = entry?.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let Some(stamp) = path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_string)
            else {
                continue;
            };
            let set = fs::read_to_string(&path)
                .ok()
                .and_then(|text| serde_json::from_str::<AnnotationSet>(&text).ok());
            let marks = set
                .as_ref()
                .map(|set| json!(&set.annotations))
                .unwrap_or(Value::Null);
            let picture = set
                .as_ref()
                .and_then(|set| set.image.as_ref())
                .and_then(|image| image.file.clone());
            entries.push((
                HistoryEntry {
                    png: history.join(format!("{stamp}.png")).is_file(),
                    source: history.join(format!("{stamp}-source.png")).is_file(),
                    stamp,
                    annotations: set.map(|set| set.annotations.len()).unwrap_or(0),
                    round: 0,
                    current: false,
                },
                marks,
                picture,
            ));
        }
        entries.sort_by(|a, b| b.0.stamp.cmp(&a.0.stamp));
        // One round is one row. STOP then *Submit again* without touching the canvas archived the
        // same annotations and the same picture a second time, so History read as "this one task
        // ran twice" (2026-10-30). Copies are indistinguishable by definition, so keep the newest
        // of a run; `dedup_by` drops the older one.
        entries.dedup_by(|older, newer| {
            older.1 == newer.1 && same_picture(&history, &older.0.stamp, &newer.0.stamp)
        });
        let total = entries.len();
        let meta = self.require(id)?;
        // The round the canvas still shows must appear once. A submission is archived at submit
        // time and the canvas keeps it after resolve or reopen, so History listed the same round
        // as "Round N" *and* "Current" (2026-10-30). Status alone cannot tell — an agent round
        // replaces the picture while the session is open — so compare the saved marks and picture
        // with the newest row.
        let live = self.annotations(id)?;
        let live_marks = json!(&live.annotations);
        let current = entries.first().and_then(|(entry, marks, picture)| {
            (*marks == live_marks && *picture == Some(meta.image.clone()))
                .then(|| entry.stamp.clone())
        });
        for (index, (entry, _, _)) in entries.iter_mut().enumerate() {
            // Newest first, but numbered oldest = 1.
            entry.round = total - index;
            entry.current = current.as_deref() == Some(entry.stamp.as_str());
        }
        Ok(entries.into_iter().map(|(entry, _, _)| entry).collect())
    }

    pub fn history_file(&self, id: &str, name: &str) -> Result<PathBuf> {
        if name.contains('/') || name.contains('\\') || name.contains("..") {
            return Err(Error::BadRequest("invalid history file".to_string()));
        }
        if !name.ends_with(".png") && !name.ends_with(".json") {
            return Err(Error::BadRequest("invalid history file".to_string()));
        }
        Ok(self.dir(id)?.join("history").join(name))
    }

    pub fn image_path(&self, meta: &Meta) -> Result<PathBuf> {
        Ok(self.dir(&meta.id)?.join(&meta.image))
    }

    pub fn file_path(&self, id: &str, name: &str) -> Result<PathBuf> {
        if name.contains('/') || name.contains('\\') || name.contains("..") {
            return Err(Error::BadRequest("invalid file name".to_string()));
        }
        Ok(self.dir(id)?.join(name))
    }
}
