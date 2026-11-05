//! Which columns a kind's table shows: declared by extensions, or by a custom
//! resource definition, evaluated without calling any extension.

use std::sync::Arc;

use kubepulse_cluster::ResourceKind;
use kubepulse_extension_api::{ColumnSource, KindRef};
use kubepulse_extension_host::ContributionSet;
use kubepulse_store::ColumnSpec;

#[derive(Clone, Debug)]
pub struct ColumnDef {
    pub title: Arc<str>,
    pub spec: ColumnSpec,
}

/// A column an extension computes, requested for visible rows only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComputedColumn {
    pub extension: String,
    pub id: String,
    pub title: String,
}

#[derive(Clone, Debug, Default)]
pub struct KindColumns {
    pub declared: Vec<ColumnDef>,
    pub computed: Vec<ComputedColumn>,
}

pub fn kind_ref(kind: &ResourceKind) -> KindRef {
    KindRef::new(&kind.group, &kind.kind)
}

fn declared(id: &str, title: &str, path: &str) -> Option<ColumnDef> {
    match ColumnSpec::new(id, path) {
        Ok(spec) => Some(ColumnDef {
            title: title.into(),
            spec,
        }),
        Err(error) => {
            tracing::warn!(%error, "skipping column");
            None
        }
    }
}

pub fn columns_for(kind: &ResourceKind, contributions: &ContributionSet) -> KindColumns {
    let wanted = kind_ref(kind);
    let mut columns = KindColumns::default();
    for (extension, contributed) in &contributions.extensions {
        for column in contributed
            .columns
            .iter()
            .filter(|column| column.kind == wanted)
        {
            match &column.source {
                ColumnSource::Path { path } => {
                    columns
                        .declared
                        .extend(declared(&column.id, &column.title, path));
                }
                ColumnSource::Computed => columns.computed.push(ComputedColumn {
                    extension: extension.clone(),
                    id: column.id.clone(),
                    title: column.title.clone(),
                }),
            }
        }
    }
    for column in &kind.printer_columns {
        // The table always shows these two itself.
        if column.name != "Name" && column.name != "Age" {
            columns
                .declared
                .extend(declared(&column.name, &column.name, &column.path));
        }
    }
    columns.declared.extend(declared(
        "created",
        "Created",
        ".metadata.creationTimestamp",
    ));
    columns
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavigationGroup {
    pub name: String,
    pub kinds: Vec<(String, ResourceKind)>,
}

/// The kinds this cluster serves, by category. Anything no extension claims is
/// still listed, so every served kind can be browsed.
pub fn navigation(
    resources: &[ResourceKind],
    contributions: &ContributionSet,
) -> Vec<NavigationGroup> {
    let mut groups: Vec<NavigationGroup> = Vec::new();
    let mut claimed = Vec::new();
    for (_, contributed) in &contributions.extensions {
        for view in &contributed.resource_views {
            let Some(kind) = resources.iter().find(|kind| kind_ref(kind) == view.kind) else {
                continue;
            };
            claimed.push(kind_ref(kind));
            let entry = (view.title.clone(), kind.clone());
            match groups.iter_mut().find(|group| group.name == view.category) {
                Some(group) => group.kinds.push(entry),
                None => groups.push(NavigationGroup {
                    name: view.category.clone(),
                    kinds: vec![entry],
                }),
            }
        }
    }
    let others: Vec<(String, ResourceKind)> = resources
        .iter()
        .filter(|kind| !claimed.contains(&kind_ref(kind)))
        .filter(|kind| kind.verbs.iter().any(|verb| verb == "list"))
        .map(|kind| (kind.plural.clone(), kind.clone()))
        .collect();
    if !others.is_empty() {
        groups.push(NavigationGroup {
            name: "Other resources".to_owned(),
            kinds: others,
        });
    }
    groups
}

#[cfg(test)]
mod tests {
    use kubepulse_cluster::PrinterColumn;
    use kubepulse_extension_api::{ColumnContribution, Contributions, ResourceViewContribution};

    use super::*;

    fn kind(group: &str, name: &str, printer_columns: Vec<PrinterColumn>) -> ResourceKind {
        ResourceKind {
            group: group.to_owned(),
            version: "v1".into(),
            kind: name.to_owned(),
            plural: format!("{}s", name.to_lowercase()),
            namespaced: true,
            verbs: vec!["list".into()],
            printer_columns,
        }
    }

    fn set(contributions: Contributions) -> ContributionSet {
        ContributionSet {
            version: 1,
            extensions: vec![("e".to_owned(), contributions)],
        }
    }

    fn pods() -> Contributions {
        let pod = KindRef::new("", "Pod");
        Contributions {
            resource_views: vec![ResourceViewContribution {
                id: "pods".into(),
                title: "Pods".into(),
                kind: pod.clone(),
                category: "Workloads".into(),
            }],
            columns: vec![
                ColumnContribution {
                    id: "status".into(),
                    title: "Status".into(),
                    kind: pod.clone(),
                    source: ColumnSource::Path {
                        path: ".status.phase".into(),
                    },
                },
                ColumnContribution {
                    id: "risk".into(),
                    title: "Risk".into(),
                    kind: pod.clone(),
                    source: ColumnSource::Computed,
                },
                ColumnContribution {
                    id: "bad".into(),
                    title: "Bad".into(),
                    kind: pod,
                    source: ColumnSource::Path { path: "[".into() },
                },
            ],
            ..Default::default()
        }
    }

    fn titles(columns: &KindColumns) -> Vec<&str> {
        columns
            .declared
            .iter()
            .map(|column| &*column.title)
            .collect()
    }

    #[test]
    fn extension_columns_apply_to_their_kind_and_invalid_paths_are_skipped() {
        let columns = columns_for(&kind("", "Pod", vec![]), &set(pods()));
        assert_eq!(titles(&columns), ["Status", "Created"]);
        assert_eq!(columns.computed[0].id, "risk");

        let other = columns_for(&kind("", "Service", vec![]), &set(pods()));
        assert_eq!(titles(&other), ["Created"]);
    }

    #[test]
    fn printer_columns_appear_without_kind_specific_support() {
        let printer = vec![
            PrinterColumn {
                name: "Size".into(),
                path: ".spec.size".into(),
            },
            PrinterColumn {
                name: "Age".into(),
                path: ".metadata.creationTimestamp".into(),
            },
        ];
        let columns = columns_for(
            &kind("example.com", "Widget", printer),
            &ContributionSet::default(),
        );
        assert_eq!(titles(&columns), ["Size", "Created"]);
    }

    #[test]
    fn navigation_groups_served_kinds_and_keeps_unclaimed_ones() {
        let resources = [
            kind("", "Pod", vec![]),
            kind("example.com", "Widget", vec![]),
        ];
        let groups = navigation(&resources, &set(pods()));
        assert_eq!(groups[0].name, "Workloads");
        assert_eq!(groups[0].kinds[0].0, "Pods");
        assert_eq!(groups[1].name, "Other resources");
        assert_eq!(groups[1].kinds[0].1.kind, "Widget");
    }

    #[test]
    fn kinds_the_cluster_does_not_serve_are_left_out() {
        let groups = navigation(&[kind("", "Service", vec![])], &set(pods()));
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "Other resources");
    }

    #[test]
    fn kinds_that_cannot_be_listed_are_left_out() {
        let mut review = kind("authentication.k8s.io", "TokenReview", vec![]);
        review.verbs = vec!["create".into()];
        assert!(navigation(&[review], &ContributionSet::default()).is_empty());
    }
}
