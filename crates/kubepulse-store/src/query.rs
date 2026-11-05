//! Filtering and sorting for one view.

use std::{cmp::Ordering, collections::BTreeSet, fmt, sync::Arc};

use crate::row::{Cell, Row};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Namespace,
    Column(usize),
}

#[derive(Clone, Debug, Default)]
pub struct ViewQuery {
    /// Empty means every namespace.
    pub namespaces: BTreeSet<String>,
    /// Case-insensitive text matched against names and cells.
    pub text: String,
    pub labels: Option<LabelSelector>,
    pub sort: Option<(SortKey, SortDirection)>,
}

impl ViewQuery {
    pub fn matches(&self, row: &Row) -> bool {
        let in_namespace = self.namespaces.is_empty()
            || row
                .namespace
                .as_ref()
                .is_some_and(|namespace| self.namespaces.contains(&**namespace));
        in_namespace
            && self
                .labels
                .as_ref()
                .is_none_or(|labels| labels.matches(row))
            && self.matches_text(row)
    }

    fn matches_text(&self, row: &Row) -> bool {
        if self.text.is_empty() {
            return true;
        }
        let needle = self.text.to_lowercase();
        let contains = |text: &str| text.to_lowercase().contains(&needle);
        contains(&row.name)
            || row
                .namespace
                .as_ref()
                .is_some_and(|namespace| contains(namespace))
            || row.cells.iter().any(|cell| match cell {
                Cell::Text(text) => contains(text),
                Cell::Number(number) => contains(&number.to_string()),
                Cell::Empty => false,
            })
    }

    /// The rows this view shows, in order. Ties fall back to the row key, so the
    /// order does not shift between snapshots.
    pub fn apply(&self, rows: &[Arc<Row>]) -> Vec<Arc<Row>> {
        let mut selected: Vec<Arc<Row>> = rows
            .iter()
            .filter(|row| self.matches(row))
            .cloned()
            .collect();
        let (key, direction) = self
            .sort
            .unwrap_or((SortKey::Name, SortDirection::Ascending));
        selected.sort_unstable_by(|a, b| {
            let order = compare(a, b, key);
            let order = if direction == SortDirection::Descending {
                order.reverse()
            } else {
                order
            };
            order.then_with(|| a.key.cmp(&b.key))
        });
        selected
    }
}

fn compare(a: &Row, b: &Row, key: SortKey) -> Ordering {
    match key {
        SortKey::Name => a.name.cmp(&b.name),
        SortKey::Namespace => a.namespace.as_deref().cmp(&b.namespace.as_deref()),
        SortKey::Column(index) => match (a.cells.get(index), b.cells.get(index)) {
            (Some(Cell::Text(a)), Some(Cell::Text(b))) => (**a).cmp(&**b),
            (a, b) => a.cmp(&b),
        },
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Requirement {
    Equals(String, String),
    NotEquals(String, String),
    In(String, Vec<String>),
    NotIn(String, Vec<String>),
    Exists(String),
    Missing(String),
}

/// A Kubernetes label selector, such as `app=web,tier in (a,b),!legacy`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LabelSelector(Vec<Requirement>);

#[derive(Debug, PartialEq, Eq)]
pub struct SelectorError(pub String);

impl fmt::Display for SelectorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid label selector term {:?}", self.0)
    }
}

impl std::error::Error for SelectorError {}

impl LabelSelector {
    pub fn parse(text: &str) -> Result<Self, SelectorError> {
        split_terms(text)
            .into_iter()
            .map(parse_term)
            .collect::<Result<_, _>>()
            .map(Self)
    }

    fn matches(&self, row: &Row) -> bool {
        self.0.iter().all(|requirement| match requirement {
            Requirement::Equals(key, value) => row.label(key) == Some(value),
            Requirement::NotEquals(key, value) => row.label(key) != Some(value),
            Requirement::In(key, values) => row
                .label(key)
                .is_some_and(|label| values.iter().any(|value| value == label)),
            Requirement::NotIn(key, values) => row
                .label(key)
                .is_none_or(|label| !values.iter().any(|value| value == label)),
            Requirement::Exists(key) => row.label(key).is_some(),
            Requirement::Missing(key) => row.label(key).is_none(),
        })
    }
}

/// Splits on commas outside parentheses.
fn split_terms(text: &str) -> Vec<&str> {
    let mut terms = Vec::new();
    let (mut depth, mut start) = (0, 0);
    for (index, character) in text.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                terms.push(&text[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    terms.push(&text[start..]);
    terms
        .into_iter()
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .collect()
}

fn parse_term(term: &str) -> Result<Requirement, SelectorError> {
    let invalid = || SelectorError(term.to_owned());
    let name = |text: &str| {
        let text = text.trim();
        let valid = !text.is_empty()
            && text
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_./".contains(c));
        valid.then(|| text.to_owned()).ok_or_else(invalid)
    };
    let value = |text: &str| {
        let text = text.trim();
        text.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
            .then(|| text.to_owned())
            .ok_or_else(invalid)
    };

    if let Some(key) = term.strip_prefix('!') {
        return Ok(Requirement::Missing(name(key)?));
    }
    if let Some((key, expected)) = term.split_once("!=") {
        return Ok(Requirement::NotEquals(name(key)?, value(expected)?));
    }
    if let Some((key, expected)) = term.split_once("==").or_else(|| term.split_once('=')) {
        return Ok(Requirement::Equals(name(key)?, value(expected)?));
    }
    if let Some(open) = term.find('(') {
        let list = term[open + 1..].strip_suffix(')').ok_or_else(invalid)?;
        let values = list.split(',').map(value).collect::<Result<Vec<_>, _>>()?;
        let head: Vec<&str> = term[..open].split_whitespace().collect();
        return match head[..] {
            [key, "in"] => Ok(Requirement::In(name(key)?, values)),
            [key, "notin"] => Ok(Requirement::NotIn(name(key)?, values)),
            _ => Err(invalid()),
        };
    }
    Ok(Requirement::Exists(name(term)?))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use kubepulse_cluster::RawObject;
    use serde_json::json;

    use super::*;
    use crate::row::ColumnSpec;

    fn row(
        name: &str,
        namespace: &str,
        labels: &[(&str, &str)],
        phase: &str,
        restarts: i64,
    ) -> Arc<Row> {
        let object = RawObject {
            key: name.into(),
            name: name.into(),
            namespace: Some(namespace.into()),
            labels: labels
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<BTreeMap<_, _>>(),
            owners: vec![],
            created: None,
            data: json!({"status": {"phase": phase, "restarts": restarts}}),
        };
        let columns = [
            ColumnSpec::new("phase", ".status.phase").unwrap(),
            ColumnSpec::new("restarts", ".status.restarts").unwrap(),
        ];
        Arc::new(Row::project(&object, &columns))
    }

    fn rows() -> Vec<Arc<Row>> {
        vec![
            row(
                "web-1",
                "prod",
                &[("app", "web"), ("tier", "front")],
                "Running",
                2,
            ),
            row("web-2", "dev", &[("app", "web")], "Pending", 10),
            row(
                "db-1",
                "prod",
                &[("app", "db"), ("tier", "back")],
                "Failed",
                0,
            ),
        ]
    }

    fn names(query: &ViewQuery) -> Vec<String> {
        query
            .apply(&rows())
            .iter()
            .map(|row| row.name.to_string())
            .collect()
    }

    fn selected(selector: &str) -> Vec<String> {
        names(&ViewQuery {
            labels: Some(LabelSelector::parse(selector).unwrap()),
            ..Default::default()
        })
    }

    #[test]
    fn empty_query_shows_everything_sorted_by_name() {
        assert_eq!(names(&ViewQuery::default()), ["db-1", "web-1", "web-2"]);
    }

    #[test]
    fn filters_by_namespace() {
        let two = ViewQuery {
            namespaces: ["prod".to_owned()].into(),
            ..Default::default()
        };
        assert_eq!(names(&two), ["db-1", "web-1"]);
    }

    #[test]
    fn filters_by_text_in_any_column_ignoring_case() {
        let text = |text: &str| {
            names(&ViewQuery {
                text: text.into(),
                ..Default::default()
            })
        };
        assert_eq!(text("WEB"), ["web-1", "web-2"]);
        assert_eq!(text("pending"), ["web-2"]);
        assert_eq!(text("10"), ["web-2"]);
        assert_eq!(text("nothing"), Vec::<String>::new());
    }

    #[test]
    fn label_selector_forms() {
        assert_eq!(selected("app=web"), ["web-1", "web-2"]);
        assert_eq!(selected("app==db"), ["db-1"]);
        assert_eq!(selected("app!=web"), ["db-1"]);
        assert_eq!(selected("tier in (front, back)"), ["db-1", "web-1"]);
        assert_eq!(selected("tier notin (front)"), ["db-1", "web-2"]);
        assert_eq!(selected("tier"), ["db-1", "web-1"]);
        assert_eq!(selected("!tier"), ["web-2"]);
        assert_eq!(selected("app=web,tier"), ["web-1"]);
        assert_eq!(selected("tier in (front,back),app=db"), ["db-1"]);
    }

    #[test]
    fn invalid_selectors_are_refused() {
        for selector in ["a=b=c", "tier in (a", "tier maybe (a)", "=x", "!"] {
            assert!(LabelSelector::parse(selector).is_err(), "{selector}");
        }
    }

    #[test]
    fn sorts_numbers_numerically_in_both_directions() {
        let sort = |direction| {
            names(&ViewQuery {
                sort: Some((SortKey::Column(1), direction)),
                ..Default::default()
            })
        };
        assert_eq!(sort(SortDirection::Ascending), ["db-1", "web-1", "web-2"]);
        assert_eq!(sort(SortDirection::Descending), ["web-2", "web-1", "db-1"]);
    }

    #[test]
    fn ties_keep_a_stable_order() {
        let sort = ViewQuery {
            sort: Some((SortKey::Namespace, SortDirection::Ascending)),
            ..Default::default()
        };
        assert_eq!(names(&sort), ["web-2", "db-1", "web-1"]);
    }
}
