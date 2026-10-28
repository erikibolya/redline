//! Comparing two rounds of annotations.
//!
//! The agent (and the human before submitting) wants "what changed since last time?", not two
//! JSON files to eyeball. Annotations carry a stable `id` (assigned by the UI since v0.5); older
//! ones fall back to matching on their canonical JSON, so a re-recorded identical shape still
//! counts as unchanged.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use crate::model::Annotation;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Change {
    pub kind: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Diff {
    pub added: Vec<Change>,
    pub removed: Vec<Change>,
    pub changed: Vec<Changed>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Changed {
    pub kind: String,
    pub summary: String,
    pub before: String,
    pub after: String,
}

fn canonical(annotation: &Annotation) -> String {
    let mut value = Value::Object(annotation.fields.clone());
    if let Some(object) = value.as_object_mut() {
        object.insert("type".to_string(), Value::String(annotation.kind.clone()));
    }
    serde_json::to_string(&value).unwrap_or_default()
}

/// Identity for matching: the annotation's `id`, else its canonical JSON.
fn key(annotation: &Annotation) -> String {
    annotation
        .fields
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("json:{}", canonical(annotation)))
}

/// Short human description: text preview or type + position.
pub fn summary(annotation: &Annotation) -> String {
    if annotation.is_text() {
        let text = annotation.text_of().unwrap_or_default();
        let preview: String = text.chars().take(40).collect();
        format!("text “{preview}”")
    } else if let Some((x, y)) = annotation.point("start") {
        format!("{} at {},{}", annotation.kind, x as i64, y as i64)
    } else {
        annotation.kind.clone()
    }
}

pub fn diff(old: &[Annotation], new: &[Annotation]) -> Diff {
    let mut old_by_key: BTreeMap<String, &Annotation> = BTreeMap::new();
    for annotation in old {
        old_by_key.insert(key(annotation), annotation);
    }
    let mut new_by_key: BTreeMap<String, &Annotation> = BTreeMap::new();
    for annotation in new {
        new_by_key.insert(key(annotation), annotation);
    }

    let mut result = Diff::default();
    for (id, annotation) in &new_by_key {
        match old_by_key.get(id) {
            None => result.added.push(Change {
                kind: annotation.kind.clone(),
                summary: summary(annotation),
            }),
            Some(previous) => {
                let before = canonical(previous);
                let after = canonical(annotation);
                if before != after {
                    result.changed.push(Changed {
                        kind: annotation.kind.clone(),
                        summary: summary(annotation),
                        before,
                        after,
                    });
                }
            }
        }
    }
    for (id, annotation) in &old_by_key {
        if !new_by_key.contains_key(id) {
            result.removed.push(Change {
                kind: annotation.kind.clone(),
                summary: summary(annotation),
            });
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn annotation(kind: &str, fields: Value) -> Annotation {
        let fields = fields.as_object().cloned().unwrap_or_default();
        Annotation {
            kind: kind.to_string(),
            fields,
        }
    }

    #[test]
    fn detects_added_removed_and_changed() {
        let kept = annotation("text", json!({"id": "a", "x": 1, "y": 2, "text": "keep"}));
        let edited = annotation("text", json!({"id": "b", "x": 3, "y": 4, "text": "before"}));
        let dropped = annotation(
            "rect",
            json!({"id": "c", "start": {"x": 0, "y": 0}, "end": {"x": 5, "y": 5}}),
        );

        let new_edited = annotation("text", json!({"id": "b", "x": 3, "y": 4, "text": "after"}));
        let added = annotation(
            "arrow",
            json!({"id": "d", "start": {"x": 9, "y": 9}, "end": {"x": 10, "y": 10}}),
        );

        let result = diff(&[kept.clone(), edited, dropped], &[kept, new_edited, added]);
        assert_eq!(result.added.len(), 1);
        assert_eq!(result.added[0].summary, "arrow at 9,9");
        assert_eq!(result.removed.len(), 1);
        assert_eq!(result.removed[0].kind, "rect");
        assert_eq!(result.changed.len(), 1);
        assert!(result.changed[0].after.contains("after"));
    }

    #[test]
    fn identical_rounds_produce_an_empty_diff() {
        let a = annotation("text", json!({"id": "a", "x": 1, "y": 1, "text": "same"}));
        let result = diff(std::slice::from_ref(&a), std::slice::from_ref(&a));
        assert!(result.added.is_empty() && result.removed.is_empty() && result.changed.is_empty());
    }

    #[test]
    fn annotations_without_ids_fall_back_to_content_matching() {
        let legacy = annotation("text", json!({"x": 1, "y": 1, "text": "no id"}));
        let result = diff(std::slice::from_ref(&legacy), std::slice::from_ref(&legacy));
        assert!(result.added.is_empty() && result.removed.is_empty());
    }
}
