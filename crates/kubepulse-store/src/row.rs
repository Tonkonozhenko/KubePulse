//! Compact rows. Everything an object does not need for its columns is dropped,
//! and repeated strings are shared. The interner frees a string when its last
//! row goes, so pods coming and going do not grow memory.

use std::sync::Arc;

use internment::ArcIntern;
use kubepulse_cluster::RawObject;
use serde_json::Value;
use serde_json_path::JsonPath;

/// A shared string.
pub type Symbol = ArcIntern<str>;

/// A column read from a path into the object, such as `.status.phase`.
#[derive(Clone, Debug)]
pub struct ColumnSpec {
    pub id: Arc<str>,
    path: JsonPath,
}

#[derive(Debug)]
pub struct ColumnError(pub String);

impl std::fmt::Display for ColumnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid column path {:?}", self.0)
    }
}

impl std::error::Error for ColumnError {}

impl ColumnSpec {
    pub fn new(id: &str, path: &str) -> Result<Self, ColumnError> {
        let path =
            JsonPath::parse(&format!("${path}")).map_err(|_| ColumnError(path.to_owned()))?;
        Ok(Self {
            id: id.into(),
            path,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Cell {
    Empty,
    Number(i64),
    Text(Symbol),
}

impl Cell {
    fn from_value(value: Option<&Value>) -> Self {
        match value {
            None | Some(Value::Null) => Self::Empty,
            Some(Value::String(text)) => Self::Text(text.as_str().into()),
            Some(Value::Number(number)) if number.is_i64() => {
                number.as_i64().map_or(Self::Empty, Self::Number)
            }
            Some(other) => Self::Text(other.to_string().into()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Owner {
    pub kind: Symbol,
    pub name: Symbol,
    pub uid: Symbol,
}

/// One object as a table sees it. Rows are immutable, and a change replaces the
/// whole row, so snapshots can share them.
#[derive(Debug, PartialEq, Eq)]
pub struct Row {
    /// Stable across changes, so selection can follow an object.
    pub key: Arc<str>,
    pub name: Box<str>,
    pub namespace: Option<Symbol>,
    pub labels: Box<[(Symbol, Symbol)]>,
    pub owners: Box<[Owner]>,
    pub created: Option<Symbol>,
    pub cells: Box<[Cell]>,
}

impl Row {
    pub fn project(object: &RawObject, columns: &[ColumnSpec]) -> Self {
        Self {
            key: object.key.as_str().into(),
            name: object.name.as_str().into(),
            namespace: object.namespace.as_deref().map(Symbol::from),
            labels: object
                .labels
                .iter()
                .map(|(key, value)| (key.as_str().into(), value.as_str().into()))
                .collect(),
            owners: object
                .owners
                .iter()
                .map(|owner| Owner {
                    kind: owner.kind.as_str().into(),
                    name: owner.name.as_str().into(),
                    uid: owner.uid.as_str().into(),
                })
                .collect(),
            created: object.created.as_deref().map(Symbol::from),
            cells: columns
                .iter()
                .map(|column| Cell::from_value(column.path.query(&object.data).first()))
                .collect(),
        }
    }

    pub fn label(&self, key: &str) -> Option<&str> {
        self.labels
            .iter()
            .find(|(k, _)| &**k == key)
            .map(|(_, value)| &**value)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use kubepulse_cluster::OwnerReference;
    use serde_json::json;

    use super::*;

    fn object() -> RawObject {
        RawObject {
            key: "uid-1".into(),
            name: "web-1".into(),
            namespace: Some("prod".into()),
            labels: BTreeMap::from([("app".to_owned(), "web".to_owned())]),
            owners: vec![OwnerReference {
                kind: "ReplicaSet".into(),
                name: "web".into(),
                uid: "uid-0".into(),
            }],
            created: Some("2026-01-01T00:00:00Z".into()),
            data: json!({
                "metadata": {"labels": {"app.kubernetes.io/name": "web"}},
                "spec": {"containers": [{"image": "nginx:1"}, {"image": "envoy:2"}]},
                "status": {"phase": "Running", "restarts": 3, "ready": true, "conditions": null},
            }),
        }
    }

    fn cell(path: &str) -> Cell {
        let column = ColumnSpec::new("c", path).unwrap();
        Row::project(&object(), &[column]).cells[0].clone()
    }

    #[test]
    fn evaluates_declarative_paths() {
        assert_eq!(cell(".status.phase"), Cell::Text("Running".into()));
        assert_eq!(
            cell(".spec.containers[1].image"),
            Cell::Text("envoy:2".into())
        );
        assert_eq!(
            cell(".metadata.labels['app.kubernetes.io/name']"),
            Cell::Text("web".into())
        );
        assert_eq!(cell(".status.restarts"), Cell::Number(3));
        assert_eq!(cell(".status.ready"), Cell::Text("true".into()));
    }

    #[test]
    fn missing_and_null_fields_are_empty() {
        assert_eq!(cell(".status.nope"), Cell::Empty);
        assert_eq!(cell(".status.conditions"), Cell::Empty);
    }

    #[test]
    fn invalid_path_is_refused() {
        assert!(ColumnSpec::new("c", ".status[").is_err());
    }

    #[test]
    fn keeps_identity_fields() {
        let row = Row::project(&object(), &[]);
        assert_eq!(&*row.key, "uid-1");
        assert_eq!(&*row.name, "web-1");
        assert_eq!(row.namespace.as_deref(), Some("prod"));
        assert_eq!(row.label("app"), Some("web"));
        assert_eq!(&*row.owners[0].kind, "ReplicaSet");
    }

    #[test]
    fn repeated_strings_share_one_allocation() {
        let (one, two) = (Row::project(&object(), &[]), Row::project(&object(), &[]));
        let (one, two) = (one.namespace.unwrap(), two.namespace.unwrap());
        assert!(std::ptr::eq(&*one, &*two));
    }
}
