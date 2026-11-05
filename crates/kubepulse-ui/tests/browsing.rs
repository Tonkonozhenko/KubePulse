mod support;

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use gpui_kit::{
    AppContext as _, Entity, SharedString, TestAppContext, VisualTestContext, WindowOptions,
    component::WindowExt as _, test::TestWindowExt as _,
};
use kubepulse_cluster::{EventRecord, ObjectTarget, PrinterColumn, ResourceKind, WatchEvent};
use kubepulse_extension_api::{
    ColumnContribution, ColumnSource, Contributions, DetailTabContribution, ExtensionRequest,
    FormField, FormFieldKind, KindRef, UiNode,
};
use kubepulse_extension_host::ContributionSet;
use kubepulse_store::{
    SortDirection, SortKey, WatchSource,
    synthetic::{SyntheticCluster, pod, pod_kind},
};
use kubepulse_ui::{
    cluster_model::{ClusterModel, SharedClusterModel},
    cluster_tab::ClusterTab,
    identity::{ClusterKey, IdentityStore},
    resource_browser::ResourceBrowser,
    runtime::{ClusterState, ClusterStatus},
    services::SharedServices,
    shell_state::{SharedShellState, ShellState},
    workspace::Workspace,
};
use serde_json::{Value, json};
use support::*;
use tokio::sync::{mpsc, watch};

const CONTEXT: &str = "test";
const TEN_THOUSAND: usize = 10_000;

fn key() -> ClusterKey {
    ClusterKey::new(CONTEXT, Some("https://test.example"))
}

fn state(status: ClusterStatus) -> Arc<ClusterState> {
    Arc::new(ClusterState {
        version: 1,
        clusters: vec![status],
        errors: vec![],
    })
}

struct Harness<'a> {
    workspace: Entity<Workspace>,
    shell: Entity<ShellState>,
    model: Entity<ClusterModel>,
    state: watch::Sender<Arc<ClusterState>>,
    cx: &'a mut VisualTestContext,
}

fn harness(
    cx: &mut TestAppContext,
    services: Arc<FakeServices>,
    resources: Vec<ResourceKind>,
    protected: bool,
) -> Harness<'_> {
    cx.executor().allow_parking();
    let (state, receiver) = watch::channel(state(connected(CONTEXT, resources)));
    let (commands, _) = mpsc::unbounded_channel();
    let mut identities = IdentityStore::default();
    identities.update(key(), |settings| settings.protected = protected);
    let (model, shell) = cx.update(|cx| {
        gpui_kit::init(cx);
        // Dialogs close at once, with no exit animation to wait for.
        cx.set_reduce_motion(true);
        ClusterTab::register(cx);
        cx.set_global(SharedServices(services));
        let model = cx.new(|cx| ClusterModel::new(receiver, commands, identities, cx));
        cx.set_global(SharedClusterModel(model.clone()));
        let shell = cx.new(|cx| {
            let mut shell = ShellState::default();
            shell.select(key(), cx);
            shell
        });
        cx.set_global(SharedShellState(shell.clone()));
        (model, shell)
    });
    let shared = model.clone();
    // The real window root, which is what shows dialogs.
    let (window, workspace) = cx
        .update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| Workspace::new(shared, None, window, cx))
            })
        })
        .unwrap();
    let cx = VisualTestContext::from_window(window, cx).into_mut();
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1600.), gpui_kit::px(1000.)));
    cx.run_until_parked();
    let mut harness = Harness {
        workspace,
        shell,
        model,
        state,
        cx,
    };
    if harness.exists("group-Other resources".into()) {
        harness.click("group-Other resources".into());
    }
    harness
}

impl Harness<'_> {
    fn show(&mut self, kind: &ResourceKind) {
        let kind = kind.clone();
        let workspace = self.workspace.clone();
        self.cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.show_kind(key(), kind, false, window, cx);
            });
        });
        self.cx.run_until_parked();
    }

    fn open_kind(&mut self, kind: &ResourceKind) {
        self.show(kind);
        self.wait_for_rows(0);
    }

    fn tab(&mut self) -> Entity<ClusterTab> {
        let shell = self.shell.clone();
        self.cx.update(|_, cx| {
            let shell = shell.read(cx);
            shell.tab(shell.active().expect("an active tab")).unwrap()
        })
    }

    fn browser(&mut self) -> Entity<ResourceBrowser> {
        let tab = self.tab();
        self.cx
            .update(|_, cx| tab.read(cx).browser().expect("a browser").clone())
    }

    /// Runs until the view has synced with exactly `rows` rows, or at least one
    /// row for `0`, and the table has them.
    fn wait_for_rows(&mut self, rows: usize) {
        let browser = self.browser();
        wait_until(self.cx, "the rows", |cx| {
            cx.update(|_, cx| {
                let browser = browser.read(cx);
                let snapshot = browser.snapshot();
                snapshot.synced
                    && if rows == 0 {
                        !snapshot.rows.is_empty()
                    } else {
                        snapshot.rows.len() == rows
                    }
            })
        });
    }

    fn update_browser(
        &mut self,
        change: impl FnOnce(&mut ResourceBrowser, &mut gpui_kit::Context<ResourceBrowser>),
    ) {
        let browser = self.browser();
        self.cx.update(|_, cx| browser.update(cx, change));
    }

    /// The first `rows` rows as the table shows them.
    fn cells(&mut self, rows: usize) -> (Vec<String>, Vec<Vec<String>>) {
        let browser = self.browser();
        self.cx
            .update(|_, cx| browser.read(cx).table().read(cx).dump_range(0..rows, cx))
    }

    fn click(&mut self, id: String) {
        self.cx
            .update(|window, cx| window.click(SharedString::from(id), cx));
        self.cx.run_until_parked();
    }

    fn exists(&mut self, id: String) -> bool {
        self.cx.update(|window, cx| {
            window.render_frame(cx);
            window.try_find(SharedString::from(id)).is_some()
        })
    }

    fn detail(&mut self) -> Option<Entity<kubepulse_ui::detail::DetailView>> {
        let tab = self.tab();
        self.cx.update(|_, cx| tab.read(cx).detail().cloned())
    }

    fn open_object(&mut self, target: ObjectTarget) {
        self.show(&target.kind);
        let tab = self.tab();
        self.cx.update(|window, cx| {
            tab.update(cx, |tab, cx| tab.open_object(target, window, cx));
        });
        self.wait_for_detail();
    }

    fn wait_for_detail(&mut self) {
        let detail = self.detail().expect("a detail view");
        wait_until(self.cx, "the object", |cx| {
            cx.update(|_, cx| detail.read(cx).object().is_some())
        });
        self.cx.update(|window, cx| window.render_frame(cx));
    }
}

fn contributed(columns: Vec<ColumnContribution>) -> ContributionSet {
    ContributionSet {
        version: 1,
        extensions: vec![(
            "e".to_owned(),
            Contributions {
                columns,
                ..Default::default()
            },
        )],
    }
}

fn path_column(id: &str, title: &str, path: &str) -> ColumnContribution {
    ColumnContribution {
        id: id.into(),
        title: title.into(),
        kind: KindRef::new("", "Pod"),
        source: ColumnSource::Path { path: path.into() },
    }
}

fn computed_column() -> ColumnContribution {
    ColumnContribution {
        id: "risk".into(),
        title: "Risk".into(),
        kind: KindRef::new("", "Pod"),
        source: ColumnSource::Computed,
    }
}

fn pod_columns() -> Vec<ColumnContribution> {
    vec![
        path_column("status", "Status", ".status.phase"),
        path_column(
            "restarts",
            "Restarts",
            ".status.containerStatuses[0].restartCount",
        ),
        path_column("node", "Node", ".spec.nodeName"),
    ]
}

fn ten_thousand_pods<'a>(
    cx: &'a mut TestAppContext,
    columns: Vec<ColumnContribution>,
) -> Harness<'a> {
    let source = Arc::new(SyntheticCluster::new(TEN_THOUSAND, None));
    let services = Arc::new(FakeServices::new(source, contributed(columns)));
    let mut harness = harness(cx, services, vec![pod_kind()], false);
    harness.open_kind(&pod_kind());
    harness.wait_for_rows(TEN_THOUSAND);
    harness
}

// The resource table

#[gpui_kit::test]
fn the_table_shows_declared_columns_at_ten_thousand_rows(cx: &mut TestAppContext) {
    let mut harness = ten_thousand_pods(cx, pod_columns());

    let (headers, rows) = harness.cells(2);

    assert_eq!(
        headers,
        ["Name", "Namespace", "Status", "Restarts", "Node", "Created"]
    );
    let expected = pod(0, 0);
    let first = rows.iter().find(|row| row[0] == expected.name);
    let row = first.cloned().unwrap_or_else(|| {
        let all = harness.cells(TEN_THOUSAND).1;
        all.into_iter()
            .find(|row| row[0] == expected.name)
            .expect("pod 0 is listed")
    });
    assert_eq!(row[1], "team-00");
    assert_eq!(row[2], "Running");
    assert_eq!(row[3], "0");
    assert!(row[4].starts_with("ip-10-0-"), "{}", row[4]);
    assert!(row[5].starts_with("2026-"), "{}", row[5]);
    assert!(harness.exists("resource-summary".into()));
}

#[gpui_kit::test]
fn selecting_namespaces_shows_only_those_namespaces(cx: &mut TestAppContext) {
    let mut harness = ten_thousand_pods(cx, pod_columns());

    harness.update_browser(|browser, cx| browser.set_namespaces("team-01, team-02", cx));
    harness.wait_for_rows(500);

    let (_, rows) = harness.cells(500);
    assert!(
        rows.iter()
            .all(|row| row[1] == "team-01" || row[1] == "team-02")
    );
    assert_eq!(rows.iter().filter(|row| row[1] == "team-01").count(), 250);
}

#[gpui_kit::test]
fn filtering_by_text_keeps_matches_and_updates_the_count(cx: &mut TestAppContext) {
    let mut harness = ten_thousand_pods(cx, pod_columns());

    let name = pod(1234, 0).name;
    harness.update_browser(|browser, cx| browser.set_filter(&name, cx));
    harness.wait_for_rows(1);

    assert_eq!(harness.cells(1).1[0][0], name);
    let browser = harness.browser();
    let (shown, total) = harness.cx.update(|_, cx| {
        let snapshot = browser.read(cx).snapshot().clone();
        (snapshot.rows.len(), snapshot.total)
    });
    assert_eq!((shown, total), (1, TEN_THOUSAND));
}

#[gpui_kit::test]
fn filtering_by_label_keeps_matches_and_an_unfinished_selector_is_reported(
    cx: &mut TestAppContext,
) {
    let mut harness = ten_thousand_pods(cx, pod_columns());

    harness
        .update_browser(|browser, cx| browser.set_labels("app.kubernetes.io/part-of=team-03", cx));
    harness.wait_for_rows(250);

    harness.update_browser(|browser, cx| browser.set_labels("tier in (", cx));
    assert!(harness.exists("label-error".into()));
    // The last valid selector stays until the new one parses.
    harness.wait_for_rows(250);

    harness.update_browser(|browser, cx| browser.set_labels("", cx));
    harness.wait_for_rows(TEN_THOUSAND);
    assert!(!harness.exists("label-error".into()));
}

#[gpui_kit::test]
fn sorting_orders_ten_thousand_rows_and_can_be_cleared(cx: &mut TestAppContext) {
    let mut harness = ten_thousand_pods(cx, pod_columns());

    harness.update_browser(|browser, cx| {
        browser.set_sort(Some((SortKey::Name, SortDirection::Descending)), cx);
    });
    let browser = harness.browser();
    wait_until(harness.cx, "the sorted rows", |cx| {
        cx.update(|_, cx| {
            let rows = &browser.read(cx).snapshot().rows;
            rows.windows(2).all(|pair| pair[0].name >= pair[1].name)
        })
    });

    harness.update_browser(|browser, cx| browser.set_sort(None, cx));
    wait_until(harness.cx, "the original order", |cx| {
        cx.update(|_, cx| {
            let rows = &browser.read(cx).snapshot().rows;
            !rows.windows(2).all(|pair| pair[0].name >= pair[1].name)
        })
    });
}

#[gpui_kit::test]
fn a_header_click_reaches_the_view(cx: &mut TestAppContext) {
    let mut harness = ten_thousand_pods(cx, pod_columns());

    harness.cx.update(|window, cx| {
        window.render_frame(cx);
        window.click(("icon-sort", 0usize), cx);
    });
    let browser = harness.browser();
    wait_until(harness.cx, "the sorted rows", |cx| {
        cx.update(|_, cx| {
            let rows = &browser.read(cx).snapshot().rows;
            rows.windows(2).all(|pair| pair[0].name >= pair[1].name)
                || rows.windows(2).all(|pair| pair[0].name <= pair[1].name)
        })
    });
}

// Stable selection

/// Ten thousand pods sorted by name, with a source for live changes.
fn selectable(cx: &mut TestAppContext) -> (Harness<'_>, Arc<ScriptedSource>) {
    let objects = (0..TEN_THOUSAND).map(|index| pod(index, 0)).collect();
    let source = Arc::new(ScriptedSource::default().with("Pod", objects));
    let services = Arc::new(FakeServices::new(
        source.clone() as Arc<dyn WatchSource>,
        contributed(pod_columns()),
    ));
    add_openable_pods(&services);
    let mut harness = harness(cx, services, vec![pod_kind()], false);
    harness.open_kind(&pod_kind());
    harness.wait_for_rows(TEN_THOUSAND);
    harness.update_browser(|browser, cx| {
        browser.set_sort(Some((SortKey::Name, SortDirection::Ascending)), cx);
    });
    let browser = harness.browser();
    wait_until(harness.cx, "the sorted rows", |cx| {
        cx.update(|_, cx| {
            let rows = &browser.read(cx).snapshot().rows;
            rows.windows(2).all(|pair| pair[0].name <= pair[1].name)
        })
    });
    (harness, source)
}

fn add_openable_pods(services: &FakeServices) {
    for index in 0..TEN_THOUSAND {
        let object = pod(index, 0);
        services.add_object(
            &pod_kind(),
            object.namespace.as_deref(),
            json!({"metadata": {"name": object.name, "uid": object.key}}),
        );
    }
}

#[gpui_kit::test]
fn the_selection_follows_its_object_as_rows_arrive_above_it(cx: &mut TestAppContext) {
    let (mut harness, source) = selectable(cx);
    harness.cx.update(|window, cx| window.render_frame(cx));

    harness.click("table-cell-3:0".into());
    let browser = harness.browser();
    let selected = harness
        .cx
        .update(|_, cx| browser.read(cx).selected().cloned())
        .expect("the click selects a row");
    let opened = harness.detail().expect("selecting opens the object");

    let mut first = pod(0, 0);
    first.key = "uid-new".into();
    first.name = "aaa-first".into();
    source.send(WatchEvent::Applied(first));
    harness.wait_for_rows(TEN_THOUSAND + 1);

    let table_row = harness.cx.update(|_, cx| {
        let browser = browser.read(cx);
        let rows = &browser.snapshot().rows;
        let expected = rows.iter().position(|row| row.key == selected);
        (browser.table().read(cx).selected_row(), expected)
    });
    assert_eq!(table_row.0, table_row.1);
    assert_eq!(table_row.1, Some(4));
    assert_eq!(
        harness
            .cx
            .update(|_, cx| browser.read(cx).selected().cloned()),
        Some(selected),
    );
    // A row moving is not the user opening it again.
    assert_eq!(harness.detail(), Some(opened));
}

#[gpui_kit::test]
fn the_selection_survives_scrolling_away_while_rows_change(cx: &mut TestAppContext) {
    let (mut harness, source) = selectable(cx);
    harness.cx.update(|window, cx| window.render_frame(cx));

    harness.click("table-cell-3:0".into());
    let browser = harness.browser();
    let selected = harness
        .cx
        .update(|_, cx| browser.read(cx).selected().cloned())
        .expect("the click selects a row");

    harness.cx.update(|window, cx| {
        browser.update(cx, |browser, cx| {
            browser
                .table()
                .update(cx, |table, cx| table.scroll_to_row(5_000, cx));
        });
        window.render_frame(cx);
    });
    harness.cx.update(|window, cx| window.render_frame(cx));
    harness.cx.run_until_parked();

    let mut first = pod(0, 0);
    first.key = "uid-new".into();
    first.name = "aaa-first".into();
    source.send(WatchEvent::Applied(first));
    harness.wait_for_rows(TEN_THOUSAND + 1);

    assert_eq!(
        harness
            .cx
            .update(|_, cx| browser.read(cx).selected().cloned()),
        Some(selected.clone()),
    );
    assert_eq!(selected_row(&browser, &mut harness), None);

    // Coming back shows it again, with no new change from the cluster.
    harness.cx.update(|window, cx| {
        browser.update(cx, |browser, cx| {
            browser
                .table()
                .update(cx, |table, cx| table.scroll_to_row(4, cx));
        });
        window.render_frame(cx);
    });
    harness.cx.run_until_parked();
    harness.cx.update(|window, cx| window.render_frame(cx));
    harness.cx.run_until_parked();

    assert_eq!(selected_row(&browser, &mut harness), Some(4));
}

fn selected_row(browser: &Entity<ResourceBrowser>, harness: &mut Harness<'_>) -> Option<usize> {
    harness
        .cx
        .update(|_, cx| browser.read(cx).table().read(cx).selected_row())
}

// Custom resources

fn widget_kind() -> ResourceKind {
    let mut kind = kind("example.com", "Widget", "widgets", true);
    kind.printer_columns = vec![
        PrinterColumn {
            name: "Size".into(),
            path: ".spec.size".into(),
        },
        PrinterColumn {
            name: "Age".into(),
            path: ".metadata.creationTimestamp".into(),
        },
    ];
    kind
}

#[gpui_kit::test]
fn a_custom_resource_shows_its_declared_columns(cx: &mut TestAppContext) {
    let widget = raw("big", Some("ns"), json!({"spec": {"size": 7}}));
    let source = Arc::new(ScriptedSource::default().with("Widget", vec![widget]));
    let services = Arc::new(FakeServices::new(source, ContributionSet::default()));
    let mut harness = harness(cx, services, vec![widget_kind()], false);

    assert!(harness.exists("kind-widgets.example.com".into()));
    harness.click("kind-widgets.example.com".into());
    harness.wait_for_rows(1);

    let (headers, rows) = harness.cells(1);
    assert_eq!(headers, ["Name", "Namespace", "Size", "Created"]);
    assert_eq!(rows[0][..3], ["big", "ns", "7"]);
}

#[gpui_kit::test]
fn a_definition_installed_while_connected_joins_the_navigation(cx: &mut TestAppContext) {
    let services = Arc::new(FakeServices::idle());
    let mut harness = harness(cx, services, vec![pod_kind()], false);
    assert!(harness.exists("kind-pods".into()));
    assert!(!harness.exists("kind-widgets.example.com".into()));

    let status = connected(CONTEXT, vec![pod_kind(), widget_kind()]);
    harness.state.send_replace(state(status));
    let model = harness.model.clone();
    wait_until(harness.cx, "the new kind", |cx| {
        cx.update(|_, cx| model.read(cx).status(CONTEXT).unwrap().resources.len() == 2)
    });

    assert!(harness.exists("kind-widgets.example.com".into()));
}

#[gpui_kit::test]
fn the_same_plural_in_two_groups_gets_two_entries(cx: &mut TestAppContext) {
    let services = Arc::new(FakeServices::idle());
    let core = kind("", "Event", "events", true);
    let other = kind("events.k8s.io", "Event", "events", true);
    let mut harness = harness(cx, services, vec![core, other], false);

    assert!(harness.exists("kind-events".into()));
    assert!(harness.exists("kind-events.events.k8s.io".into()));
}

// Computed columns

fn visible_targets(services: &FakeServices) -> Vec<usize> {
    services
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter_map(|(_, request)| match request {
            ExtensionRequest::ComputeColumn { targets, .. } => Some(targets.len()),
            _ => None,
        })
        .collect()
}

#[gpui_kit::test]
fn a_slow_extension_never_delays_frames_or_scrolling(cx: &mut TestAppContext) {
    let source = Arc::new(SyntheticCluster::new(TEN_THOUSAND, None));
    let services = Arc::new(FakeServices::new(
        source,
        contributed(vec![computed_column()]),
    ));
    services.stall_extensions();
    let mut harness = harness(cx, services.clone(), vec![pod_kind()], false);
    harness.open_kind(&pod_kind());
    harness.wait_for_rows(TEN_THOUSAND);
    harness.cx.update(|window, cx| window.render_frame(cx));
    harness.cx.run_until_parked();

    let requested = visible_targets(&services);
    assert!(!requested.is_empty(), "visible rows are requested");
    let total: usize = requested.iter().sum();
    assert!(total < 100, "only visible rows are requested: {total}");
    let (_, rows) = harness.cells(1);
    assert_eq!(rows[0].last().unwrap(), "…");

    // The extension has not answered, and every frame still finishes.
    let browser = harness.browser();
    for step in 1..=50 {
        let frame = std::time::Instant::now();
        harness.cx.update(|window, cx| {
            browser.update(cx, |browser, cx| {
                browser
                    .table()
                    .update(cx, |table, cx| table.scroll_to_row(step * 150, cx));
            });
            window.render_frame(cx);
        });
        harness.cx.run_until_parked();
        assert!(
            frame.elapsed() < Duration::from_millis(500),
            "a frame took {:?}",
            frame.elapsed()
        );
    }
    let during = visible_targets(&services).iter().sum::<usize>();
    assert!(
        during < 3_000,
        "scrolling asks for far less than the table: {during}"
    );
}

#[gpui_kit::test]
fn computed_cells_fill_in_when_the_extension_answers(cx: &mut TestAppContext) {
    let source = Arc::new(SyntheticCluster::new(100, None));
    let services = Arc::new(FakeServices::new(
        source,
        contributed(vec![computed_column()]),
    ));
    services.stall_extensions();
    let mut harness = harness(cx, services.clone(), vec![pod_kind()], false);
    harness.open_kind(&pod_kind());
    harness.wait_for_rows(100);
    harness.cx.update(|window, cx| window.render_frame(cx));
    harness.cx.run_until_parked();
    assert_eq!(harness.cells(1).1[0].last().unwrap(), "…");

    services.release_extensions();
    let browser = harness.browser();
    wait_until(harness.cx, "the computed cell", |cx| {
        cx.update(|_, cx| {
            let table = browser.read(cx).table().read(cx);
            table.dump_range(0..1, cx).1[0]
                .last()
                .unwrap()
                .starts_with("computed ")
        })
    });
}

// Object details

fn deployment_kind() -> ResourceKind {
    kind("apps", "Deployment", "deployments", true)
}

fn replica_set_kind() -> ResourceKind {
    kind("apps", "ReplicaSet", "replicasets", true)
}

fn owned(name: &str, owner: Option<(&str, &str, &str)>) -> Value {
    let owners: Vec<Value> = owner
        .map(|(api, kind, name)| {
            json!({"apiVersion": api, "kind": kind, "name": name, "uid": format!("uid-{name}")})
        })
        .into_iter()
        .collect();
    json!({
        "metadata": {
            "name": name,
            "namespace": "shop",
            "uid": format!("uid-{name}"),
            "creationTimestamp": "2026-01-01T00:00:00Z",
            "labels": {"app": "web"},
            "ownerReferences": owners,
        },
        "status": {"phase": "Running", "conditions": [{"type": "Ready", "status": "True"}]},
    })
}

fn target(kind: ResourceKind, namespace: Option<&str>, name: &str) -> ObjectTarget {
    ObjectTarget {
        kind,
        namespace: namespace.map(str::to_owned),
        name: name.to_owned(),
    }
}

fn deployment_pod(cx: &mut TestAppContext) -> Harness<'_> {
    let row = raw("web-abc-1", Some("shop"), json!({}));
    let source = Arc::new(ScriptedSource::default().with("Pod", vec![row]));
    let services = Arc::new(FakeServices::new(source, ContributionSet::default()));
    services.add_object(
        &pod_kind(),
        Some("shop"),
        owned("web-abc-1", Some(("apps/v1", "ReplicaSet", "web-abc"))),
    );
    services.add_object(
        &replica_set_kind(),
        Some("shop"),
        owned("web-abc", Some(("apps/v1", "Deployment", "web"))),
    );
    services.add_object(&deployment_kind(), Some("shop"), owned("web", None));
    *services.events.lock().unwrap() = vec![EventRecord {
        kind: "Warning".into(),
        reason: "BackOff".into(),
        message: "restarting".into(),
        count: 3,
        last_seen: None,
    }];
    harness(
        cx,
        services,
        vec![pod_kind(), replica_set_kind(), deployment_kind()],
        false,
    )
}

#[gpui_kit::test]
fn opening_a_deployment_owned_pod_shows_its_details_and_events(cx: &mut TestAppContext) {
    let mut harness = deployment_pod(cx);
    harness.click("kind-pods".into());
    harness.wait_for_rows(1);
    harness.cx.update(|window, cx| window.render_frame(cx));

    harness.click("table-cell-0:0".into());
    harness.wait_for_detail();

    let detail = harness.detail().unwrap();
    wait_until(harness.cx, "the events", |cx| {
        cx.update(|_, cx| !detail.read(cx).events().is_empty())
    });
    let (name, reason) = harness.cx.update(|_, cx| {
        let detail = detail.read(cx);
        (
            detail.target().name.clone(),
            detail.events()[0].reason.clone(),
        )
    });
    assert_eq!((name.as_str(), reason.as_str()), ("web-abc-1", "BackOff"));
    assert!(harness.exists("detail-events".into()));
}

#[gpui_kit::test]
fn owner_references_lead_from_a_pod_to_its_deployment(cx: &mut TestAppContext) {
    let mut harness = deployment_pod(cx);
    harness.open_object(target(pod_kind(), Some("shop"), "web-abc-1"));

    harness.click("owner-web-abc".into());
    harness.wait_for_detail();
    let detail = harness.detail().unwrap();
    let opened = harness.cx.update(|_, cx| detail.read(cx).target().clone());
    assert_eq!(opened, target(replica_set_kind(), Some("shop"), "web-abc"));

    harness.click("owner-web".into());
    harness.wait_for_detail();
    let detail = harness.detail().unwrap();
    let opened = harness.cx.update(|_, cx| detail.read(cx).target().clone());
    assert_eq!(opened, target(deployment_kind(), Some("shop"), "web"));
}

#[gpui_kit::test]
fn an_owner_of_a_kind_the_cluster_does_not_serve_is_named_but_not_a_link(cx: &mut TestAppContext) {
    let services = Arc::new(FakeServices::idle());
    services.add_object(
        &pod_kind(),
        Some("shop"),
        owned("p", Some(("example.com/v1", "Widget", "w"))),
    );
    let mut harness = harness(cx, services, vec![pod_kind()], false);

    harness.open_object(target(pod_kind(), Some("shop"), "p"));

    assert!(!harness.exists("owner-w".into()));
}

#[gpui_kit::test]
fn a_detail_tab_form_reaches_its_extension(cx: &mut TestAppContext) {
    let contributions = ContributionSet {
        version: 1,
        extensions: vec![(
            "e".to_owned(),
            Contributions {
                detail_tabs: vec![DetailTabContribution {
                    id: "scale".into(),
                    title: "Scale".into(),
                    kind: KindRef::new("", "Pod"),
                }],
                ..Default::default()
            },
        )],
    };
    let source = Arc::new(ScriptedSource::default());
    let services = Arc::new(FakeServices::new(source, contributions));
    services.add_object(&pod_kind(), Some("shop"), owned("p", None));
    *services.tab_ui.lock().unwrap() = Some(UiNode::Form {
        id: "f".into(),
        submit_label: "Go".into(),
        fields: vec![FormField {
            id: "replicas".into(),
            label: "Replicas".into(),
            kind: FormFieldKind::Text { value: "3".into() },
        }],
    });
    let mut harness = harness(cx, services.clone(), vec![pod_kind()], false);
    harness.open_object(target(pod_kind(), Some("shop"), "p"));

    harness.click("detail-tab-scale".into());
    let detail = harness.detail().unwrap();
    wait_until(harness.cx, "the tab", |cx| {
        cx.update(|_, cx| detail.read(cx).extension_tab().is_some())
    });
    harness.click("submit-f".into());

    let requests = services.requests.lock().unwrap();
    let values = BTreeMap::from([("replicas".to_owned(), "3".to_owned())]);
    assert_eq!(
        requests[1],
        (
            "e".to_owned(),
            ExtensionRequest::SubmitForm {
                form: "f".into(),
                values
            }
        )
    );
}

// Secrets

fn secret_kind() -> ResourceKind {
    kind("", "Secret", "secrets", true)
}

fn secret_harness(cx: &mut TestAppContext) -> Harness<'_> {
    let services = Arc::new(FakeServices::idle());
    let object = json!({
        "metadata": {"name": "db", "namespace": "shop", "uid": "uid-db"},
        // "hunter2" and bytes that are not text.
        "data": {"password": "aHVudGVyMg==", "blob": "/w=="},
    });
    services.add_object(&secret_kind(), Some("shop"), object);
    let mut harness = harness(cx, services, vec![secret_kind()], false);
    harness.open_object(target(secret_kind(), Some("shop"), "db"));
    harness
}

fn secret_text(harness: &mut Harness<'_>, key: &str) -> String {
    let detail = harness.detail().unwrap();
    harness
        .cx
        .update(|_, cx| detail.read(cx).secret_text(key))
        .expect("the key is listed")
}

#[gpui_kit::test]
fn secret_keys_are_listed_and_values_stay_hidden(cx: &mut TestAppContext) {
    let mut harness = secret_harness(cx);

    assert!(harness.exists("secret-value-password".into()));
    assert!(harness.exists("secret-value-blob".into()));
    assert_eq!(secret_text(&mut harness, "password"), "••••••");
    assert_eq!(secret_text(&mut harness, "blob"), "••••••");
}

#[gpui_kit::test]
fn revealing_shows_one_value_and_hiding_covers_it_again(cx: &mut TestAppContext) {
    let mut harness = secret_harness(cx);

    harness.click("reveal-password".into());
    assert_eq!(secret_text(&mut harness, "password"), "hunter2");
    assert_eq!(secret_text(&mut harness, "blob"), "••••••");

    harness.click("reveal-blob".into());
    assert_eq!(secret_text(&mut harness, "blob"), "(binary value)");

    harness.click("reveal-password".into());
    assert_eq!(secret_text(&mut harness, "password"), "••••••");
}

// Deletion

fn deletable(cx: &mut TestAppContext, protected: bool) -> (Harness<'_>, Arc<FakeServices>) {
    let services = Arc::new(FakeServices::idle());
    services.add_object(&pod_kind(), Some("shop"), owned("web", None));
    let resources = vec![pod_kind(), secret_kind()];
    let mut harness = harness(cx, services.clone(), resources, protected);
    harness.open_object(target(pod_kind(), Some("shop"), "web"));
    harness.click("delete-object".into());
    (harness, services)
}

fn deletions(services: &FakeServices) -> Vec<ObjectTarget> {
    services.deleted.lock().unwrap().clone()
}

fn dialog_open(harness: &mut Harness<'_>) -> bool {
    harness.cx.update(|window, cx| {
        window.render_frame(cx);
        window.has_active_dialog(cx)
    })
}

fn type_name(harness: &mut Harness<'_>, text: &str) {
    let detail = harness.detail().unwrap();
    harness
        .cx
        .update(|_, cx| detail.update(cx, |detail, cx| detail.type_name(text, cx)));
    harness.cx.run_until_parked();
}

#[gpui_kit::test]
fn nothing_is_deleted_until_the_user_confirms(cx: &mut TestAppContext) {
    let (mut harness, services) = deletable(cx, false);

    assert!(dialog_open(&mut harness));
    assert!(harness.exists("delete-confirmation".into()));
    assert!(deletions(&services).is_empty());

    harness.click("delete-confirm".into());

    assert_eq!(
        deletions(&services),
        [target(pod_kind(), Some("shop"), "web")]
    );
    assert!(!dialog_open(&mut harness));
    assert!(harness.detail().is_none(), "the deleted object closes");
}

#[gpui_kit::test]
fn cancelling_deletes_nothing(cx: &mut TestAppContext) {
    let (mut harness, services) = deletable(cx, false);

    harness.click("delete-cancel".into());

    assert!(!dialog_open(&mut harness));
    assert!(deletions(&services).is_empty());
    assert!(harness.detail().is_some());
}

#[gpui_kit::test]
fn escape_closes_the_dialog_and_enter_never_deletes(cx: &mut TestAppContext) {
    let (mut harness, services) = deletable(cx, false);

    harness.cx.update(|window, cx| window.press("enter", cx));
    harness.cx.run_until_parked();
    assert!(dialog_open(&mut harness));
    assert!(deletions(&services).is_empty());

    harness.cx.update(|window, cx| window.press("escape", cx));
    harness.cx.run_until_parked();
    assert!(!dialog_open(&mut harness));
    assert!(deletions(&services).is_empty());
}

#[gpui_kit::test]
fn replacing_the_preview_clears_its_filter_and_detail(cx: &mut TestAppContext) {
    let services = Arc::new(FakeServices::new(
        Arc::new(
            ScriptedSource::default()
                .with("Pod", vec![raw("web", Some("shop"), json!({}))])
                .with("Widget", vec![raw("big", Some("ns"), json!({}))]),
        ),
        ContributionSet::default(),
    ));
    services.add_object(&pod_kind(), Some("shop"), owned("web", None));
    let mut harness = harness(cx, services, vec![pod_kind(), widget_kind()], false);
    harness.open_kind(&pod_kind());
    harness.update_browser(|browser, cx| browser.set_filter("web", cx));
    harness.open_object(target(pod_kind(), Some("shop"), "web"));
    assert!(harness.detail().is_some());

    // A kept filter of "web" would hide the widget, and this would time out.
    harness.open_kind(&widget_kind());

    assert!(harness.detail().is_none());
}

#[gpui_kit::test]
fn the_dialog_blocks_the_rest_of_the_window(cx: &mut TestAppContext) {
    let (mut harness, _) = deletable(cx, false);

    harness.click("kind-secrets".into());

    assert!(
        dialog_open(&mut harness),
        "clicking outside does not close it"
    );
    let shell = harness.shell.clone();
    let kind = harness.cx.update(|_, cx| {
        let tab = shell.read(cx).active_tab().cloned();
        tab.and_then(|tab| tab.kind).unwrap().kind
    });
    assert_eq!(kind, "Pod", "the click does not reach what is behind it");
}

#[gpui_kit::test]
fn a_refused_delete_stays_open_and_says_why(cx: &mut TestAppContext) {
    let (mut harness, services) = deletable(cx, false);
    *services.delete_failure.lock().unwrap() = Some("forbidden".into());

    harness.click("delete-confirm".into());

    assert!(dialog_open(&mut harness));
    assert!(harness.exists("delete-error".into()));
    assert!(harness.detail().is_some());
}

#[gpui_kit::test]
fn a_protected_cluster_requires_typing_the_name(cx: &mut TestAppContext) {
    let (mut harness, services) = deletable(cx, true);

    assert!(!harness.exists("delete-confirm".into()));

    type_name(&mut harness, "wrong");
    assert!(!harness.exists("delete-confirm".into()));

    type_name(&mut harness, "web");
    assert!(harness.exists("delete-confirm".into()));
    assert!(deletions(&services).is_empty());

    harness.click("delete-confirm".into());
    assert_eq!(deletions(&services).len(), 1);
}

#[gpui_kit::test]
fn closing_on_a_protected_cluster_forgets_the_typed_name(cx: &mut TestAppContext) {
    let (mut harness, _) = deletable(cx, true);
    type_name(&mut harness, "web");

    harness.cx.update(|window, cx| window.press("escape", cx));
    harness.cx.run_until_parked();
    harness.click("delete-object".into());

    assert!(dialog_open(&mut harness));
    assert!(!harness.exists("delete-confirm".into()));
}
