//! Manifests as the user edits them: YAML without the fields the server owns.

use serde_json::{Map, Value};
use similar::TextDiff;

const SERVER_METADATA: [&str; 8] = [
    "creationTimestamp",
    "deletionGracePeriodSeconds",
    "deletionTimestamp",
    "generation",
    "managedFields",
    "resourceVersion",
    "selfLink",
    "uid",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManifestError {
    /// Lines and columns count from 1.
    Syntax {
        line: usize,
        column: usize,
        message: String,
    },
    NotAMapping,
}

fn sort_keys(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            fields.sort_keys();
            fields.values_mut().for_each(sort_keys);
        }
        Value::Array(items) => items.iter_mut().for_each(sort_keys),
        _ => {}
    }
}

/// The object as the user sees it, with keys in a stable order so that two
/// versions compare field by field.
pub fn strip(object: &Value) -> Value {
    let mut stripped = object.clone();
    if let Some(fields) = stripped.as_object_mut() {
        fields.remove("status");
    }
    if let Some(metadata) = stripped["metadata"].as_object_mut() {
        for field in SERVER_METADATA {
            metadata.remove(field);
        }
    }
    sort_keys(&mut stripped);
    stripped
}

fn copy_field(from: &Map<String, Value>, to: &mut Map<String, Value>, field: &str) {
    match from.get(field) {
        Some(value) => to.insert(field.to_owned(), value.clone()),
        None => to.remove(field),
    };
}

/// Puts back what [`strip`] removed, from the object the user opened. Its
/// `resourceVersion` makes the server refuse the write if the object has moved
/// on. Text the user typed for these fields does not count.
pub fn restore(edited: Value, opened: &Value) -> Value {
    let mut restored = edited;
    if let (Some(to), Some(from)) = (restored.as_object_mut(), opened.as_object()) {
        copy_field(from, to, "status");
    }
    if let (Some(to), Some(from)) = (
        restored.get_mut("metadata").and_then(Value::as_object_mut),
        opened.get("metadata").and_then(Value::as_object),
    ) {
        for field in SERVER_METADATA {
            copy_field(from, to, field);
        }
    }
    restored
}

pub fn to_yaml(value: &Value) -> String {
    yaml_serde::to_string(value).expect("a JSON value serializes as YAML")
}

pub fn parse(text: &str) -> Result<Value, ManifestError> {
    let value: Value = yaml_serde::from_str(text).map_err(|error| {
        let location = error.location();
        ManifestError::Syntax {
            line: location.as_ref().map_or(1, |at| at.line()),
            column: location.as_ref().map_or(1, |at| at.column()),
            message: error.to_string(),
        }
    })?;
    if value.is_object() {
        Ok(value)
    } else {
        Err(ManifestError::NotAMapping)
    }
}

/// Carries the user's edits from `base` over to `theirs`. Lists merge as one
/// value. Where both sides changed a value the user's wins, which the diff
/// review then shows against the newer version.
pub fn rebase(base: &Value, ours: &Value, theirs: &Value) -> Value {
    if ours == base {
        return theirs.clone();
    }
    let (Value::Object(base), Value::Object(ours), Value::Object(theirs)) = (base, ours, theirs)
    else {
        return ours.clone();
    };
    let mut merged = theirs.clone();
    for (key, ours) in ours {
        let value = match (base.get(key), theirs.get(key)) {
            (Some(base), Some(theirs)) => rebase(base, ours, theirs),
            _ => ours.clone(),
        };
        merged.insert(key.clone(), value);
    }
    for key in base.keys().filter(|key| !ours.contains_key(*key)) {
        if theirs.get(key) == base.get(key) {
            merged.remove(key);
        }
    }
    merged.sort_keys();
    Value::Object(merged)
}

/// A unified patch between two manifests, named for syntax highlighting.
pub fn patch(old: &str, new: &str) -> String {
    TextDiff::from_lines(old, new)
        .unified_diff()
        .header("a/manifest.yaml", "b/manifest.yaml")
        .to_string()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn live() -> Value {
        json!({
            "apiVersion": "v1",
            "kind": "ConfigMap",
            "metadata": {
                "name": "app",
                "namespace": "default",
                "labels": {"tier": "web"},
                "uid": "1234",
                "resourceVersion": "42",
                "generation": 3,
                "creationTimestamp": "2026-01-01T00:00:00Z",
                "managedFields": [{"manager": "kubectl"}],
            },
            "data": {"key": "value"},
            "status": {"phase": "Ready"},
        })
    }

    #[test]
    fn stripping_leaves_only_what_the_user_owns() {
        assert_eq!(
            strip(&live()),
            json!({
                "apiVersion": "v1",
                "kind": "ConfigMap",
                "metadata": {
                    "name": "app",
                    "namespace": "default",
                    "labels": {"tier": "web"},
                },
                "data": {"key": "value"},
            })
        );
    }

    #[test]
    fn stripping_then_restoring_gives_back_the_object() {
        assert_eq!(restore(strip(&live()), &live()), live());
    }

    #[test]
    fn a_yaml_round_trip_changes_nothing() {
        let stripped = strip(&live());
        assert_eq!(parse(&to_yaml(&stripped)), Ok(stripped));
    }

    #[test]
    fn restoring_keeps_edits_and_ignores_typed_server_fields() {
        let mut edited = strip(&live());
        edited["data"]["key"] = json!("changed");
        edited["metadata"]["resourceVersion"] = json!("1");
        edited["metadata"]["deletionTimestamp"] = json!("2026-01-01T00:00:00Z");
        edited["status"] = json!({"phase": "Typed"});

        let restored = restore(edited, &live());

        let mut expected = live();
        expected["data"]["key"] = json!("changed");
        assert_eq!(restored, expected);
    }

    #[test]
    fn a_syntax_error_has_a_location() {
        let Err(ManifestError::Syntax { line, column, .. }) = parse("a: 1\nb: [\n") else {
            panic!("the text is not valid YAML");
        };
        assert!(line >= 2, "line {line}");
        assert!(column >= 1, "column {column}");
    }

    #[test]
    fn a_manifest_must_be_a_mapping() {
        assert_eq!(parse("- a\n- b\n"), Err(ManifestError::NotAMapping));
        assert_eq!(parse(""), Err(ManifestError::NotAMapping));
    }

    #[test]
    fn rebasing_keeps_both_sides_changes() {
        let base = json!({"data": {"a": "1", "b": "2", "c": "3"}, "list": [1]});
        let ours = json!({"data": {"a": "ours", "b": "2"}, "list": [1]});
        let theirs = json!({"data": {"a": "1", "b": "theirs", "c": "3"}, "list": [1, 2]});
        assert_eq!(
            rebase(&base, &ours, &theirs),
            json!({"data": {"a": "ours", "b": "theirs"}, "list": [1, 2]})
        );
    }

    #[test]
    fn when_both_sides_change_a_value_the_users_wins() {
        let base = json!({"a": "1"});
        assert_eq!(
            rebase(&base, &json!({"a": "ours"}), &json!({"a": "theirs"})),
            json!({"a": "ours"})
        );
    }

    #[test]
    fn a_deleted_field_that_they_changed_comes_back() {
        let base = json!({"a": "1", "b": "2"});
        assert_eq!(
            rebase(&base, &json!({"a": "1"}), &json!({"a": "1", "b": "theirs"})),
            json!({"a": "1", "b": "theirs"})
        );
    }

    #[test]
    fn a_patch_shows_the_changed_line() {
        let patch = patch("a: 1\nb: 2\n", "a: 1\nb: 3\n");
        assert!(patch.contains("-b: 2\n"), "{patch}");
        assert!(patch.contains("+b: 3\n"), "{patch}");
    }
}
