//! Built-in views, tabs, and actions, written against the extension contract.
//! Each one is declarative, so tables stay fast and no code runs per row.

use std::sync::Arc;

use async_trait::async_trait;
use kubepulse_extension_api::{
    CONTRACT_VERSION, ColumnContribution, ColumnSource, Contributions, Extension, ExtensionError,
    ExtensionRequest, ExtensionResponse, Host, KindRef, Manifest, Permission,
    ResourceViewContribution, Verb,
};

struct KindView {
    group: &'static str,
    kind: &'static str,
    title: &'static str,
    columns: &'static [(&'static str, &'static str)],
}

const fn view(
    group: &'static str,
    kind: &'static str,
    title: &'static str,
    columns: &'static [(&'static str, &'static str)],
) -> KindView {
    KindView {
        group,
        kind,
        title,
        columns,
    }
}

const WORKLOADS: &[KindView] = &[
    view(
        "",
        "Pod",
        "Pods",
        &[
            ("Status", ".status.phase"),
            ("Restarts", ".status.containerStatuses[0].restartCount"),
            ("Node", ".spec.nodeName"),
            ("IP", ".status.podIP"),
        ],
    ),
    view(
        "apps",
        "Deployment",
        "Deployments",
        &[
            ("Desired", ".spec.replicas"),
            ("Ready", ".status.readyReplicas"),
            ("Updated", ".status.updatedReplicas"),
        ],
    ),
    view(
        "apps",
        "StatefulSet",
        "StatefulSets",
        &[
            ("Desired", ".spec.replicas"),
            ("Ready", ".status.readyReplicas"),
        ],
    ),
    view(
        "apps",
        "DaemonSet",
        "DaemonSets",
        &[
            ("Desired", ".status.desiredNumberScheduled"),
            ("Ready", ".status.numberReady"),
        ],
    ),
    view(
        "apps",
        "ReplicaSet",
        "ReplicaSets",
        &[
            ("Desired", ".spec.replicas"),
            ("Ready", ".status.readyReplicas"),
        ],
    ),
    view(
        "batch",
        "Job",
        "Jobs",
        &[
            ("Completions", ".spec.completions"),
            ("Succeeded", ".status.succeeded"),
        ],
    ),
    view(
        "batch",
        "CronJob",
        "CronJobs",
        &[
            ("Schedule", ".spec.schedule"),
            ("Suspended", ".spec.suspend"),
            ("Last run", ".status.lastScheduleTime"),
        ],
    ),
];

const NETWORK: &[KindView] = &[
    view(
        "",
        "Service",
        "Services",
        &[("Type", ".spec.type"), ("Cluster IP", ".spec.clusterIP")],
    ),
    view(
        "networking.k8s.io",
        "Ingress",
        "Ingresses",
        &[
            ("Class", ".spec.ingressClassName"),
            ("Host", ".spec.rules[0].host"),
        ],
    ),
    view("networking.k8s.io", "NetworkPolicy", "NetworkPolicies", &[]),
    view(
        "discovery.k8s.io",
        "EndpointSlice",
        "EndpointSlices",
        &[("Address type", ".addressType")],
    ),
];

const CONFIG: &[KindView] = &[
    view("", "ConfigMap", "ConfigMaps", &[]),
    // Secret columns must never read `.data`, so values stay out of the cache.
    view("", "Secret", "Secrets", &[("Type", ".type")]),
];

const STORAGE: &[KindView] = &[
    view(
        "",
        "PersistentVolumeClaim",
        "PersistentVolumeClaims",
        &[
            ("Status", ".status.phase"),
            ("Volume", ".spec.volumeName"),
            ("Storage class", ".spec.storageClassName"),
        ],
    ),
    view(
        "",
        "PersistentVolume",
        "PersistentVolumes",
        &[
            ("Capacity", ".spec.capacity.storage"),
            ("Status", ".status.phase"),
            ("Storage class", ".spec.storageClassName"),
        ],
    ),
    view(
        "storage.k8s.io",
        "StorageClass",
        "StorageClasses",
        &[("Provisioner", ".provisioner")],
    ),
];

const ACCESS: &[KindView] = &[
    view("", "ServiceAccount", "ServiceAccounts", &[]),
    view("rbac.authorization.k8s.io", "Role", "Roles", &[]),
    view(
        "rbac.authorization.k8s.io",
        "RoleBinding",
        "RoleBindings",
        &[("Role", ".roleRef.name")],
    ),
    view(
        "rbac.authorization.k8s.io",
        "ClusterRole",
        "ClusterRoles",
        &[],
    ),
    view(
        "rbac.authorization.k8s.io",
        "ClusterRoleBinding",
        "ClusterRoleBindings",
        &[("Role", ".roleRef.name")],
    ),
];

struct KindGroup {
    id: &'static str,
    name: &'static str,
    views: &'static [KindView],
}

const GROUPS: [KindGroup; 5] = [
    KindGroup {
        id: "builtin.workloads",
        name: "Workloads",
        views: WORKLOADS,
    },
    KindGroup {
        id: "builtin.network",
        name: "Network",
        views: NETWORK,
    },
    KindGroup {
        id: "builtin.config",
        name: "Config",
        views: CONFIG,
    },
    KindGroup {
        id: "builtin.storage",
        name: "Storage",
        views: STORAGE,
    },
    KindGroup {
        id: "builtin.access",
        name: "Access control",
        views: ACCESS,
    },
];

#[async_trait]
impl Extension for KindGroup {
    fn manifest(&self) -> Manifest {
        let mut contributions = Contributions::default();
        let mut permissions = Vec::new();
        for view in self.views {
            let kind = KindRef::new(view.group, view.kind);
            contributions.resource_views.push(ResourceViewContribution {
                id: view.kind.to_lowercase(),
                title: view.title.to_owned(),
                kind: kind.clone(),
                category: self.name.to_owned(),
            });
            for (title, path) in view.columns {
                contributions.columns.push(ColumnContribution {
                    id: format!("{}.{}", view.kind.to_lowercase(), title.to_lowercase()),
                    title: (*title).to_owned(),
                    kind: kind.clone(),
                    source: ColumnSource::Path {
                        path: (*path).to_owned(),
                    },
                });
            }
            permissions.push(Permission {
                kind,
                verbs: vec![Verb::Get, Verb::List],
            });
        }
        Manifest {
            id: self.id.to_owned(),
            name: self.name.to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            contract: CONTRACT_VERSION,
            activation: Vec::new(),
            contributions,
            permissions,
        }
    }

    async fn handle(
        &self,
        _: ExtensionRequest,
        _: Arc<dyn Host>,
    ) -> Result<ExtensionResponse, ExtensionError> {
        Err(ExtensionError::new("this extension only declares views"))
    }
}

pub fn extensions() -> Vec<Arc<dyn Extension>> {
    GROUPS
        .into_iter()
        .map(|group| Arc::new(group) as Arc<dyn Extension>)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifests() -> Vec<Manifest> {
        extensions()
            .iter()
            .map(|extension| extension.manifest())
            .collect()
    }

    #[test]
    fn covers_every_category_from_the_spec() {
        let names: Vec<String> = manifests().into_iter().map(|m| m.name).collect();
        assert_eq!(
            names,
            [
                "Workloads",
                "Network",
                "Config",
                "Storage",
                "Access control"
            ]
        );
    }

    #[test]
    fn each_view_is_declarative_and_permitted() {
        for manifest in manifests() {
            let contributions = &manifest.contributions;
            assert!(!contributions.resource_views.is_empty(), "{}", manifest.id);
            assert!(
                contributions
                    .columns
                    .iter()
                    .all(|column| matches!(column.source, ColumnSource::Path { .. }))
            );
            for view in &contributions.resource_views {
                assert!(
                    manifest.permissions.iter().any(|p| p.kind == view.kind),
                    "{} needs a permission",
                    view.kind.kind
                );
            }
        }
    }

    #[test]
    fn secret_columns_never_read_values() {
        let config = &manifests()[2];
        let secret = KindRef::new("", "Secret");
        for column in config
            .contributions
            .columns
            .iter()
            .filter(|c| c.kind == secret)
        {
            let ColumnSource::Path { path } = &column.source else {
                unreachable!()
            };
            assert!(
                !path.starts_with(".data") && !path.starts_with(".stringData"),
                "{path}"
            );
        }
    }
}
