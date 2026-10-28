use redline_core::model::{Annotation, Status, Submission};
use redline_core::store::{valid_id, Store};
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
        .create_from_bytes(PNG, "shot.png", "Button / default", "http://localhost/x", "check hover", None, None)
        .expect("create");

    assert!(meta.id.starts_with("20"), "id starts with a timestamp: {}", meta.id);
    assert!(meta.id.contains("button-default"));
    assert_eq!(meta.status, Status::Open);

    let folder = store.dir(&meta.id).unwrap();
    assert_eq!(folder.join("screenshot.png").as_path(), store.image_path(&meta).unwrap());
    assert!(folder.join("screenshot.png").is_file());
    assert!(folder.join("meta.json").is_file());

    // v0.1 wrote exactly {"session": id, "annotations": []} before the first submission.
    let initial: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(folder.join("annotations.json")).unwrap()).unwrap();
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

    assert!(store.create_from_bytes(PNG, "a.png", "a", "", "", Some("../escape"), None).is_err());

    store.create_from_bytes(PNG, "a.png", "a", "", "", Some("fixed-id"), None).unwrap();
    let err = store.create_from_bytes(PNG, "a.png", "a", "", "", Some("fixed-id"), None).unwrap_err();
    assert!(matches!(err, redline_core::Error::Exists(_)));
}

#[test]
fn submission_writes_all_four_artifacts() {
    let (_dir, store) = store();
    let meta = store.create_from_bytes(PNG, "shot.png", "Button", "http://x/button", "hover + focus", None, None).unwrap();

    let annotations = vec![
        Annotation {
            kind: "text".into(),
            fields: serde_json::from_value(json!({"x": 5, "y": 6, "text": "Move this up", "color": "#e11d48"})).unwrap(),
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
    assert!(review.contains("rectangle (10, 20) → (100, 60)"), "{review}");

    assert_eq!(set.annotations.len(), 2);
    assert_eq!(std::fs::read(folder.join("annotated.png")).unwrap(), b"\x89PNG\r\n\x1a\n");
}

#[test]
fn list_is_newest_first_and_remove_is_permanent() {
    let (_dir, store) = store();
    let a = store.create_from_bytes(PNG, "a.png", "first", "", "", Some("aaa"), None).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100)); // created_at has second precision
    let b = store.create_from_bytes(PNG, "b.png", "second", "", "", Some("bbb"), None).unwrap();

    let ids: Vec<String> = store.list().unwrap().into_iter().map(|m| m.id).collect();
    assert_eq!(ids, vec![b.id.clone(), a.id.clone()]);

    store.remove(&a.id).unwrap();
    assert!(store.get(&a.id).unwrap().is_none());
    assert!(matches!(store.remove(&a.id), Err(redline_core::Error::NotFound(_))));
}

#[test]
fn writes_leave_no_temp_files_behind() {
    let (_dir, store) = store();
    let meta = store.create_from_bytes(PNG, "a.png", "t", "", "", None, None).unwrap();
    store
        .submit(
            &meta.id,
            &Submission { annotations: vec![], png: None, width: None, height: None },
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
    let meta = store.create_from_bytes(PNG, "a.png", "t", "", "", None, None).unwrap();
    let err = store
        .submit(
            &meta.id,
            &Submission {
                annotations: vec![],
                png: Some("data:image/png;base64,!!!not-base64!!!".to_string()),
                width: None,
                height: None,
            },
        )
        .unwrap_err();
    assert!(matches!(err, redline_core::Error::BadRequest(_)));
}
