//! Which columns a kind's table shows: declared by extensions, or by a custom
//! resource definition, evaluated without calling any extension.

use std::{collections::BTreeMap, sync::Arc};

use kubepulse_cluster::ResourceKind;
use kubepulse_extension_api::{ColumnSource, KindRef, ViewIcon};
use kubepulse_extension_host::ContributionSet;
use kubepulse_store::ColumnSpec;
use rust_i18n::t;

use crate::object_dialog::Operation;

#[derive(Clone, Debug)]
pub struct ColumnDef {
    pub title: Arc<str>,
    pub spec: ColumnSpec,
    /// Shown as a tag colored by health.
    pub status: bool,
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

pub fn is_service(kind: &ResourceKind) -> bool {
    kind.group.is_empty() && kind.kind == "Service"
}

pub fn is_pod(kind: &ResourceKind) -> bool {
    kind.group.is_empty() && kind.kind == "Pod"
}

/// A workload whose pods restart through its own rollout.
pub fn is_restartable(kind: &ResourceKind) -> bool {
    kind.group == "apps"
        && matches!(
            kind.kind.as_str(),
            "Deployment" | "StatefulSet" | "DaemonSet"
        )
}

pub fn is_cronjob(kind: &ResourceKind) -> bool {
    kind.group == "batch" && kind.kind == "CronJob"
}

/// The operation, besides deleting, that this kind offers on its objects.
pub fn kind_operation(kind: &ResourceKind) -> Option<Operation> {
    if is_restartable(kind) {
        Some(Operation::Restart)
    } else if is_cronjob(kind) {
        Some(Operation::Run)
    } else {
        None
    }
}

fn declared(id: &str, title: &str, paths: &[impl AsRef<str>], status: bool) -> Option<ColumnDef> {
    match ColumnSpec::first_of(id, paths) {
        Ok(spec) => Some(ColumnDef {
            title: title.into(),
            spec,
            status,
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
                        .extend(declared(&column.id, &column.title, &[path], false));
                }
                ColumnSource::Status { paths } => {
                    columns
                        .declared
                        .extend(declared(&column.id, &column.title, paths, true));
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
            columns.declared.extend(declared(
                &column.name,
                &column.name,
                &[&column.path],
                is_status_name(&column.name),
            ));
        }
    }
    columns.declared.extend(declared(
        "created",
        &t!("column.created"),
        &[".metadata.creationTimestamp"],
        false,
    ));
    columns
}

fn is_status_name(name: &str) -> bool {
    ["status", "phase", "state"]
        .iter()
        .any(|status| name.eq_ignore_ascii_case(status))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavigationEntry {
    pub title: String,
    pub icon: ViewIcon,
    pub kind: ResourceKind,
}

/// The kinds of one API group, inside the group of undeclared kinds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavigationSection {
    pub name: String,
    pub title: String,
    pub kinds: Vec<NavigationEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavigationGroup {
    /// As declared, so it stays the same in every language.
    pub name: String,
    pub title: String,
    pub kinds: Vec<NavigationEntry>,
    pub sections: Vec<NavigationSection>,
}

pub fn kind_title(kind: &KindRef, contributions: &ContributionSet) -> String {
    contributions
        .extensions
        .iter()
        .flat_map(|(_, contributed)| &contributed.resource_views)
        .find(|view| view.kind == *kind)
        .map_or_else(|| kind.kind.clone(), |view| view.title.clone())
}

pub fn kind_icon(kind: &KindRef, contributions: &ContributionSet) -> ViewIcon {
    contributions
        .extensions
        .iter()
        .flat_map(|(_, contributed)| &contributed.resource_views)
        .find(|view| view.kind == *kind)
        .map_or(ViewIcon::Generic, |view| view.icon)
}

const OTHER_RESOURCES: &str = "Other resources";
const CORE_GROUP: &str = "core";

/// The kinds this cluster serves, by category. Anything no extension claims is
/// still listed, by API group, so every served kind can be browsed.
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
            let entry = NavigationEntry {
                title: view.title.clone(),
                icon: view.icon,
                kind: kind.clone(),
            };
            match groups.iter_mut().find(|group| group.name == view.category) {
                Some(group) => group.kinds.push(entry),
                None => groups.push(NavigationGroup {
                    name: view.category.clone(),
                    title: contributions
                        .category_titles
                        .get(&view.category)
                        .unwrap_or(&view.category)
                        .clone(),
                    kinds: vec![entry],
                    sections: Vec::new(),
                }),
            }
        }
    }
    let mut sections: BTreeMap<String, Vec<NavigationEntry>> = BTreeMap::new();
    for kind in resources
        .iter()
        .filter(|kind| !claimed.contains(&kind_ref(kind)))
        .filter(|kind| kind.verbs.iter().any(|verb| verb == "list"))
    {
        let name = match kind.group.as_str() {
            "" => CORE_GROUP.to_owned(),
            group => group.to_owned(),
        };
        sections.entry(name).or_default().push(NavigationEntry {
            title: kind.plural.clone(),
            icon: ViewIcon::Generic,
            kind: kind.clone(),
        });
    }
    if !sections.is_empty() {
        groups.push(NavigationGroup {
            name: OTHER_RESOURCES.to_owned(),
            title: t!("sidebar.other_resources").to_string(),
            kinds: Vec::new(),
            sections: sections
                .into_iter()
                .map(|(name, mut kinds)| {
                    kinds.sort_by(|a, b| a.title.cmp(&b.title));
                    let title = if name == CORE_GROUP {
                        t!("sidebar.core_group").to_string()
                    } else {
                        name.clone()
                    };
                    NavigationSection { name, title, kinds }
                })
                .collect(),
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
            ..ContributionSet::default()
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
                icon: ViewIcon::default(),
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

    fn status_flags(columns: &KindColumns) -> Vec<(&str, bool)> {
        columns
            .declared
            .iter()
            .map(|column| (&*column.title, column.status))
            .collect()
    }

    #[test]
    fn a_status_column_is_marked_and_an_invalid_path_skips_it() {
        let mut contributed = pods();
        contributed.columns.push(ColumnContribution {
            id: "health".into(),
            title: "Health".into(),
            kind: KindRef::new("", "Pod"),
            source: ColumnSource::Status {
                paths: vec![".status.reason".into(), ".status.phase".into()],
            },
        });
        contributed.columns.push(ColumnContribution {
            id: "broken".into(),
            title: "Broken".into(),
            kind: KindRef::new("", "Pod"),
            source: ColumnSource::Status {
                paths: vec![".status.phase".into(), "[".into()],
            },
        });
        let columns = columns_for(&kind("", "Pod", vec![]), &set(contributed));
        assert_eq!(
            status_flags(&columns),
            [("Status", false), ("Health", true), ("Created", false)]
        );
    }

    #[test]
    fn printer_columns_named_like_a_status_are_statuses_in_any_case() {
        let printer = ["Status", "PHASE", "state", "Size"]
            .map(|name| PrinterColumn {
                name: name.into(),
                path: ".spec.size".into(),
            })
            .to_vec();
        let columns = columns_for(
            &kind("example.com", "Widget", printer),
            &ContributionSet::default(),
        );
        assert_eq!(
            status_flags(&columns),
            [
                ("Status", true),
                ("PHASE", true),
                ("state", true),
                ("Size", false),
                ("Created", false),
            ]
        );
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
    fn a_kind_uses_its_views_icon_and_an_undeclared_kind_is_generic() {
        let mut declared = pods();
        declared.resource_views[0].icon = ViewIcon::Box;
        let set = set(declared);
        assert_eq!(kind_icon(&KindRef::new("", "Pod"), &set), ViewIcon::Box);
        assert_eq!(
            kind_icon(&KindRef::new("example.com", "Widget"), &set),
            ViewIcon::Generic
        );
    }

    #[test]
    fn navigation_groups_served_kinds_and_keeps_unclaimed_ones() {
        let resources = [
            kind("", "Pod", vec![]),
            kind("example.com", "Widget", vec![]),
        ];
        let groups = navigation(&resources, &set(pods()));
        assert_eq!(groups[0].name, "Workloads");
        assert_eq!(groups[0].kinds[0].title, "Pods");
        assert_eq!(groups[1].name, "Other resources");
        assert_eq!(groups[1].sections[0].kinds[0].kind.kind, "Widget");
    }

    #[test]
    fn undeclared_kinds_are_nested_by_api_group_and_sorted() {
        let resources = [
            kind("example.com", "Widget", vec![]),
            kind("cert-manager.io", "Issuer", vec![]),
            kind("example.com", "Gadget", vec![]),
            kind("", "PodTemplate", vec![]),
        ];
        let groups = navigation(&resources, &ContributionSet::default());
        let sections: Vec<(&str, Vec<&str>)> = groups[0]
            .sections
            .iter()
            .map(|section| {
                let titles = section.kinds.iter().map(|e| e.title.as_str()).collect();
                (section.name.as_str(), titles)
            })
            .collect();
        assert_eq!(
            sections,
            [
                ("cert-manager.io", vec!["issuers"]),
                ("core", vec!["podtemplates"]),
                ("example.com", vec!["gadgets", "widgets"]),
            ]
        );
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

    #[test]
    fn only_apps_workloads_restart_and_only_batch_cronjobs_run() {
        for name in ["Deployment", "StatefulSet", "DaemonSet"] {
            assert_eq!(
                kind_operation(&kind("apps", name, vec![])),
                Some(Operation::Restart),
                "{name}"
            );
        }
        assert_eq!(
            kind_operation(&kind("batch", "CronJob", vec![])),
            Some(Operation::Run)
        );
        for (group, name) in [
            ("", "Pod"),
            ("apps", "ReplicaSet"),
            ("batch", "Job"),
            ("example.com", "Deployment"),
            ("example.com", "CronJob"),
        ] {
            assert_eq!(
                kind_operation(&kind(group, name, vec![])),
                None,
                "{group}/{name}"
            );
        }
    }
}
