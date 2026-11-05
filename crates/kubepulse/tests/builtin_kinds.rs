//! Every built-in kind, through the real host, navigation, and store.

use std::sync::Arc;

use kubepulse_cluster::{RawObject, ResourceKind, WatchEvent};
use kubepulse_extension_api::{ColumnSource, Manifest};
use kubepulse_extension_host::{BuiltinLoader, ExtensionHost, HostOptions};
use kubepulse_store::{Cell, ClusterStore, StoreOptions, ViewQuery, WatchSource};
use kubepulse_ui::{
    columns::{columns_for, navigation},
    runtime::{ClusterBackend, ConnectedClusters, StoreConfig},
};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

fn manifests() -> Vec<Manifest> {
    kubepulse_builtin::extensions()
        .iter()
        .map(|extension| extension.manifest())
        .collect()
}

fn host() -> ExtensionHost {
    let backend = Arc::new(ClusterBackend(Arc::default()));
    let host = ExtensionHost::new(backend, HostOptions::default());
    host.load(&BuiltinLoader::new(kubepulse_builtin::extensions()));
    host
}

/// Every kind a built-in view declares, as a cluster would serve it.
fn served_kinds() -> Vec<ResourceKind> {
    manifests()
        .iter()
        .flat_map(|manifest| &manifest.contributions.resource_views)
        .map(|view| ResourceKind {
            group: view.kind.group.clone(),
            version: "v1".into(),
            kind: view.kind.kind.clone(),
            plural: view.title.to_lowercase(),
            namespaced: true,
            verbs: vec!["get".into(), "list".into(), "watch".into()],
            printer_columns: vec![],
        })
        .collect()
}

/// Sets `value` at a path such as `.status.items[0].name`.
fn fill(object: &mut Value, path: &str, value: &str) {
    let mut current = object;
    for segment in path.trim_start_matches('.').split('.') {
        let (name, indexed) = match segment.split_once('[') {
            Some((name, _)) => (name, true),
            None => (segment, false),
        };
        current = &mut current[name];
        if indexed {
            if !current.is_array() {
                *current = json!([{}]);
            }
            current = &mut current[0];
        } else if current.is_null() {
            *current = json!({});
        }
    }
    *current = json!(value);
}

/// Lists one object carrying a value at each of the kind's column paths.
struct OneObject(Value);

impl WatchSource for OneObject {
    fn watch(&self, _: &ResourceKind, events: mpsc::Sender<WatchEvent>, _: CancellationToken) {
        let data = self.0.clone();
        tokio::spawn(async move {
            let object = RawObject {
                key: "uid".into(),
                name: "one".into(),
                namespace: Some("ns".into()),
                labels: Default::default(),
                owners: vec![],
                created: Some("2026-01-01T00:00:00Z".into()),
                data,
            };
            let _ = events.send(WatchEvent::Restarted { scope: None }).await;
            let scope = None;
            let _ = events.send(WatchEvent::Listed { scope, object }).await;
            let _ = events.send(WatchEvent::Synced { scope: None }).await;
        });
    }
}

#[test]
fn the_navigation_groups_every_built_in_kind_by_category() {
    let host = host();
    let contributions = host.contributions().borrow().clone();
    let served = served_kinds();
    assert_eq!(served.len(), 21);

    let groups = navigation(&served, &contributions);

    let names: Vec<&str> = groups.iter().map(|group| group.name.as_str()).collect();
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
    let listed: usize = groups.iter().map(|group| group.kinds.len()).sum();
    assert_eq!(listed, served.len());
}

#[tokio::test(flavor = "multi_thread")]
async fn each_built_in_kind_lists_with_its_default_columns() {
    let host = host();
    let contributions = host.contributions().borrow().clone();
    let columns = StoreConfig::for_host(Arc::<ConnectedClusters>::default(), &host).columns;

    for manifest in manifests() {
        for view in &manifest.contributions.resource_views {
            let declared: Vec<_> = manifest
                .contributions
                .columns
                .iter()
                .filter(|column| column.kind == view.kind)
                .collect();
            let mut object = json!({});
            for column in &declared {
                let ColumnSource::Path { path } = &column.source else {
                    panic!("{} is not declarative", column.id);
                };
                fill(&mut object, path, "shown");
            }
            let kind = served_kinds()
                .into_iter()
                .find(|kind| kind.kind == view.kind.kind && kind.group == view.kind.group)
                .unwrap();

            let titles: Vec<String> = columns_for(&kind, &contributions)
                .declared
                .iter()
                .map(|column| column.title.to_string())
                .collect();
            let mut expected: Vec<String> =
                declared.iter().map(|column| column.title.clone()).collect();
            expected.push("Created".to_owned());
            assert_eq!(titles, expected, "{}", view.title);

            let source = Arc::new(OneObject(object));
            let store = ClusterStore::new(source, columns.clone(), StoreOptions::default());
            let table = store.view(&kind, ViewQuery::default());
            let mut snapshots = table.subscribe();
            let snapshot = snapshots
                .wait_for(|snapshot| snapshot.synced && snapshot.rows.len() == 1)
                .await
                .unwrap()
                .clone();

            let cells = &snapshot.rows[0].cells;
            assert_eq!(cells.len(), declared.len() + 1, "{}", view.title);
            for (column, cell) in declared.iter().zip(cells.iter()) {
                assert_eq!(
                    cell,
                    &Cell::Text("shown".into()),
                    "{} {}",
                    view.title,
                    column.title
                );
            }
        }
    }
}
