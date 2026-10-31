use redline_core::model::{Annotation, MediaKind, Status, Submission};
use redline_core::store::{default_branch, valid_id, Store};
use serde_json::json;
use tempfile::TempDir;

fn store() -> (TempDir, Store) {
    let dir = TempDir::new().expect("tempdir");
    let store = Store::new(dir.path().join("sessions"));
    (dir, store)
}

const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0];

#[test]
fn creates_the_v01_folder_layout() {
    let (_dir, store) = store();
    let meta = store
        .create_from_bytes(
            PNG,
            "shot.png",
            "Button / default",
            "http://localhost/x",
            "check hover",
            None,
            None,
            None,
        )
        .expect("create");

    assert!(
        meta.id.starts_with("20"),
        "id starts with a timestamp: {}",
        meta.id
    );
    assert!(meta.id.contains("button-default"));
    assert_eq!(meta.status, Status::Open);

    let folder = store.dir(&meta.id).unwrap();
    assert_eq!(
        folder.join("screenshot.png").as_path(),
        store.image_path(&meta).unwrap()
    );
    assert!(folder.join("screenshot.png").is_file());
    assert!(folder.join("meta.json").is_file());

    // v0.1 wrote exactly {"session": id, "annotations": []} before the first submission.
    let initial: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(folder.join("annotations.json")).unwrap())
            .unwrap();
    assert_eq!(initial, json!({ "session": meta.id, "annotations": [] }));

    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(folder.join("meta.json")).unwrap()).unwrap();
    assert_eq!(saved["title"], "Button / default");
    assert_eq!(saved["status"], "open");
    assert_eq!(saved["submitted_at"], serde_json::Value::Null);
    assert_eq!(saved["image"], "screenshot.png");
}

#[test]
fn rejects_path_traversal_and_duplicate_ids() {
    let (_dir, store) = store();
    assert!(!valid_id("../escape"));
    assert!(!valid_id(""));
    assert!(!valid_id("-leading-dash"));
    assert!(valid_id("a"));
    assert!(valid_id("20260101-120000-button-1a2b"));

    assert!(store
        .create_from_bytes(PNG, "a.png", "a", "", "", Some("../escape"), None, None)
        .is_err());

    store
        .create_from_bytes(PNG, "a.png", "a", "", "", Some("fixed-id"), None, None)
        .unwrap();
    let err = store
        .create_from_bytes(PNG, "a.png", "a", "", "", Some("fixed-id"), None, None)
        .unwrap_err();
    assert!(matches!(err, redline_core::Error::Exists(_)));
}

#[test]
fn submission_writes_all_four_artifacts() {
    let (_dir, store) = store();
    let meta = store
        .create_from_bytes(
            PNG,
            "shot.png",
            "Button",
            "http://x/button",
            "hover + focus",
            None,
            None,
            None,
        )
        .unwrap();

    let annotations = vec![
        Annotation {
            kind: "text".into(),
            fields: serde_json::from_value(
                json!({"x": 5, "y": 6, "text": "Move this up", "color": "#e11d48"}),
            )
            .unwrap(),
        },
        Annotation {
            kind: "rect".into(),
            fields: serde_json::from_value(json!({
                "start": {"x": 10, "y": 20}, "end": {"x": 100, "y": 60},
                "color": "#e11d48", "width": 3
            }))
            .unwrap(),
        },
    ];
    let submission = Submission {
        note: None,
        annotations: annotations.clone(),
        png: Some("data:image/png;base64,iVBORw0KGgo=".to_string()),
        width: Some(1440),
        height: Some(900),
    };

    let (meta, set) = store.submit(&meta.id, &submission).expect("submit");
    assert_eq!(meta.status, Status::Submitted);
    assert!(meta.submitted_at.is_some());
    assert_eq!(meta.image_width, Some(1440));

    let folder = store.dir(&meta.id).unwrap();
    assert!(folder.join("annotations.json").is_file());
    assert!(folder.join("annotated.png").is_file());
    assert!(folder.join("review.md").is_file());

    let review = std::fs::read_to_string(folder.join("review.md")).unwrap();
    assert!(review.contains("# Visual review: Button"));
    assert!(review.contains("- Page: http://x/button"));
    assert!(review.contains("- Request: hover + focus"));
    // v0.1 printed integer coordinates without a decimal point.
    assert!(review.contains("- (5, 6): Move this up"), "{review}");
    assert!(
        review.contains("rectangle (10, 20) → (100, 60)"),
        "{review}"
    );

    assert_eq!(set.annotations.len(), 2);
    assert_eq!(
        std::fs::read(folder.join("annotated.png")).unwrap(),
        b"\x89PNG\r\n\x1a\n"
    );
}

#[test]
fn a_new_round_clears_the_feature_decision() {
    let (_dir, store) = store();
    let meta = store
        .create_from_bytes(PNG, "shot.png", "Feature", "", "", None, None, None)
        .unwrap();
    store.confirm_feature(&meta.id).unwrap();

    let submission = Submission {
        note: None,
        annotations: Vec::new(),
        png: Some("data:image/png;base64,iVBORw0KGgo=".to_string()),
        width: None,
        height: None,
    };
    let (meta, _) = store.submit(&meta.id, &submission).unwrap();
    assert_eq!(meta.status, Status::Submitted);
    assert_eq!(meta.feature_state(), "pending");

    store.cancel_feature(&meta.id).unwrap();
    let meta = store
        .post_round(&meta.id, None, "", "new round", None, None)
        .unwrap();
    assert_eq!(meta.status, Status::Open);
    assert_eq!(meta.feature_state(), "pending");
}

#[test]
fn list_is_newest_first_and_remove_is_permanent() {
    let (_dir, store) = store();
    let a = store
        .create_from_bytes(PNG, "a.png", "first", "", "", Some("aaa"), None, None)
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100)); // created_at has second precision
    let b = store
        .create_from_bytes(PNG, "b.png", "second", "", "", Some("bbb"), None, None)
        .unwrap();

    let ids: Vec<String> = store.list().unwrap().into_iter().map(|m| m.id).collect();
    assert_eq!(ids, vec![b.id.clone(), a.id.clone()]);

    store.remove(&a.id).unwrap();
    assert!(store.get(&a.id).unwrap().is_none());
    assert!(matches!(
        store.remove(&a.id),
        Err(redline_core::Error::NotFound(_))
    ));
}

#[test]
fn writes_leave_no_temp_files_behind() {
    let (_dir, store) = store();
    let meta = store
        .create_from_bytes(PNG, "a.png", "t", "", "", None, None, None)
        .unwrap();
    store
        .submit(
            &meta.id,
            &Submission {
                note: None,
                annotations: vec![],
                png: None,
                width: None,
                height: None,
            },
        )
        .unwrap();

    let folder = store.dir(&meta.id).unwrap();
    let leftovers: Vec<String> = std::fs::read_dir(&folder)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| name.ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "temp files left: {leftovers:?}");
}

#[test]
fn malformed_png_payload_is_a_bad_request_not_a_crash() {
    let (_dir, store) = store();
    let meta = store
        .create_from_bytes(PNG, "a.png", "t", "", "", None, None, None)
        .unwrap();
    let err = store
        .submit(
            &meta.id,
            &Submission {
                note: None,
                annotations: vec![],
                png: Some("data:image/png;base64,!!!not-base64!!!".to_string()),
                width: None,
                height: None,
            },
        )
        .unwrap_err();
    assert!(matches!(err, redline_core::Error::BadRequest(_)));
}

#[test]
fn repo_projects_are_idempotent_and_survive_agent_resync() {
    let (_dir, store) = store();
    let first = store
        .sync_repo_project("holly", "ActiUnit/Au.Net", "Au.Net")
        .unwrap();
    assert_eq!(first.slug, "actiunit-au-net");
    assert_eq!(first.repo.as_deref(), Some("ActiUnit/Au.Net"));
    assert_eq!(first.agent.as_deref(), Some("holly"));

    let renamed = store
        .sync_repo_project("holly", "ActiUnit/Au.Net", "Au.Net UI")
        .unwrap();
    assert_eq!(renamed.slug, first.slug);
    assert_eq!(renamed.name, "Au.Net UI");

    // A second agent reporting the same key updates the record instead of duplicating it.
    let other = store
        .sync_repo_project("local", "ActiUnit/Au.Net", "Au.Net")
        .unwrap();
    assert_eq!(other.slug, first.slug);
    assert_eq!(other.agent.as_deref(), Some("local"));

    // A legacy label with the same name must not be silently turned into a repo project.
    let legacy = store.ensure_project("redline").unwrap();
    assert_eq!(legacy.slug, "redline");
    let repo = store
        .sync_repo_project("holly", "redline", "redline")
        .unwrap();
    assert_eq!(repo.slug, "redline-2");
    assert_eq!(repo.repo.as_deref(), Some("redline"));

    let projects = store.projects().unwrap();
    let found = projects
        .iter()
        .find(|project| project.slug == repo.slug)
        .unwrap();
    assert_eq!(found.repo.as_deref(), Some("redline"));
}

#[test]
fn a_default_branch_names_the_feature_not_the_session() {
    // The title is what the human typed; the tail keeps two features of the same name apart.
    assert_eq!(
        default_branch("Wrong branches", "20261030-161714-wrong-branches-52e0"),
        "feat/wrong-branches-52e0"
    );
    // Diacritics and punctuation cannot reach git, so they become dashes.
    assert_eq!(
        default_branch(
            "Tlačítko: hover / disabled!",
            "20261030-120000-tlacitko-9f1c"
        ),
        "feat/tlacitko-hover-disabled-9f1c"
    );
    // An untitled review still gets a usable branch.
    assert_eq!(
        default_branch("", "20261030-120000-x-abcd"),
        "feat/review-abcd"
    );
    // Long titles are cut before they hit git's own length limits.
    let long = default_branch(&"a".repeat(120), "20261030-120000-x-abcd");
    assert!(long.len() <= 60, "{long}");
}

#[test]
fn a_review_created_without_a_branch_already_has_its_feature_branch() {
    let (_dir, store) = store();
    let meta = store
        .create_from_bytes(PNG, "shot.png", "Checkbox group", "", "", None, None, None)
        .unwrap();
    let branch = meta.branch.as_deref().unwrap_or_default();
    assert!(branch.starts_with("feat/checkbox-group-"), "{branch}");
    assert!(!branch.starts_with("session/"), "{branch}");
}

/// Minimal MP4 magic bytes; the store validates the container, not the codec payload.
const MP4: &[u8] = b"\x00\x00\x00\x18ftypmp42\x00\x00\x00\x00mp42isom";

#[test]
fn video_sessions_keep_media_metadata_and_timecoded_annotations() {
    let (_dir, store) = store();
    let meta = store
        .create_video_from_bytes(
            MP4,
            "Checkout flow",
            "http://localhost/checkout",
            "watch the transition",
            None,
            None,
            None,
            Some(PNG),
            Some(5_000),
            Some(1280),
            Some(720),
            Some(30.0),
        )
        .expect("create video");
    let media = meta.media.as_ref().expect("media metadata");
    assert_eq!(media.kind, MediaKind::Video);
    assert_eq!(media.file, "video.mp4");
    assert_eq!(media.duration_ms, Some(5_000));
    assert_eq!(meta.image, "video.mp4");
    assert!(store.poster_path(&meta).unwrap().is_file());

    let mut rect = Annotation::new("rect");
    rect.fields
        .insert("start".to_string(), json!({"x": 1, "y": 2}));
    rect.fields
        .insert("end".to_string(), json!({"x": 3, "y": 4}));
    rect.fields.insert("at".to_string(), json!(1.25));
    rect.fields.insert("until".to_string(), json!(2.5));
    let submission = Submission {
        note: None,
        annotations: vec![rect],
        png: None,
        width: Some(1280),
        height: Some(720),
    };
    let (_meta, set) = store.submit(&meta.id, &submission).expect("submit");
    assert_eq!(set.media.as_ref().map(|m| m.kind), Some(MediaKind::Video));

    let review = std::fs::read_to_string(store.dir(&meta.id).unwrap().join("review.md")).unwrap();
    assert!(review.contains("Media: Video"), "{review}");
    assert!(review.contains("@ 00:01.250–00:02.500"), "{review}");

    let saved: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(store.dir(&meta.id).unwrap().join("meta.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(saved["media"]["kind"], "video");
    assert_eq!(saved["media"]["duration_ms"], 5000);
}
