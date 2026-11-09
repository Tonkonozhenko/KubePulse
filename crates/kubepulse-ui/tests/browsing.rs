mod support;

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use gpui_kit::{
    AppContext as _, ElementId, Entity, Modifiers, SharedString, TestAppContext, VisualTestContext,
    WindowOptions, component::WindowExt as _, test::TestWindowExt as _,
};
use kubepulse_cluster::{
    ConnectionState, EventRecord, ForwardFailure, ForwardKind, ForwardRequest, ForwardState,
    ObjectError, ObjectTarget, PrinterColumn, ResourceKind, WatchEvent,
};
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
    cluster_model::{ClusterModel, ClusterModelEvent},
    cluster_tab::ClusterTab,
    forward_dialog::{ForwardDialog, ForwardField},
    forwards::SharedForwards,
    identity::{ClusterKey, IdentityStore},
    preferences::Preferences,
    resource_browser::ResourceBrowser,
    runtime::{ClusterState, ClusterStatus},
    services::SharedServices,
    settings_tab,
    shell_state::{SharedShellState, ShellState},
    status::Tone,
    workspace::Workspace,
};
use serde_json::{Value, json};
use support::*;
use tokio::sync::{mpsc, watch};

const CONTEXT: &str = "test";
const TEN_THOUSAND: usize = 10_000;
const CONTROL_COLUMNS: usize = 2;

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
        cx.set_global(SharedServices(services));
        let model = cx.new(|cx| ClusterModel::new(receiver, commands, identities, cx));
        let shell = cx.new(|cx| {
            let mut shell = ShellState::default();
            shell.select(key(), cx);
            shell
        });
        cx.set_global(SharedShellState(shell.clone()));
        settings_tab::init(Preferences::default(), None, cx);
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
        let (headers, rows) = self
            .cx
            .update(|_, cx| browser.read(cx).table().read(cx).dump_range(0..rows, cx));
        // The checkbox and action columns hold no text.
        let data = |cells: Vec<String>| cells[CONTROL_COLUMNS..].to_vec();
        (data(headers), rows.into_iter().map(data).collect())
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
        ..ContributionSet::default()
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

    harness.select_namespaces(&["team-01", "team-02"]);
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
fn a_new_tab_has_no_filter_label_selector_or_sort(cx: &mut TestAppContext) {
    let mut harness = ten_thousand_pods(cx, pod_columns());
    let first = harness.cells(1).1[0][0].clone();
    harness.update_browser(|browser, cx| {
        browser.set_filter(&pod(1234, 0).name, cx);
        browser.set_labels("tier=none", cx);
        browser.set_sort(Some((SortKey::Name, SortDirection::Descending)), cx);
    });

    let tab = harness.tab().entity_id();
    let shell = harness.shell.clone();
    harness
        .cx
        .update(|_, cx| shell.update(cx, |shell, cx| shell.request_close(tab, cx)));
    harness.cx.run_until_parked();
    harness.open_kind(&pod_kind());
    harness.wait_for_rows(TEN_THOUSAND);

    assert_eq!(harness.cells(1).1[0][0], first);
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
        window.click(("icon-sort", 2usize), cx);
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

    harness.click("table-cell-3:2".into());
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

    harness.click("table-cell-3:2".into());
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

    harness.click("table-cell-0:2".into());
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
        ..ContributionSet::default()
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
    let tab = harness.tab();
    harness.cx.update(|_, cx| {
        let dialog = tab.read(cx).delete_dialog().expect("an open dialog");
        dialog.update(cx, |dialog, cx| dialog.type_name(text, cx));
    });
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

fn node_kind() -> ResourceKind {
    kind("", "Node", "nodes", false)
}

fn namespace_kind() -> ResourceKind {
    kind("", "Namespace", "namespaces", false)
}

fn other_key() -> ClusterKey {
    ClusterKey::new("other", Some("https://other.example"))
}

/// Pods in `prod` and `dev`, and an `idle` namespace that holds none.
fn namespaced_cluster(cx: &mut TestAppContext) -> (Harness<'_>, Arc<ScriptedSource>) {
    let source = Arc::new(
        ScriptedSource::default()
            .with(
                "Pod",
                vec![
                    raw("web", Some("prod"), json!({})),
                    raw("api", Some("dev"), json!({})),
                ],
            )
            .with(
                "Deployment",
                vec![
                    raw("web-deploy", Some("prod"), json!({})),
                    raw("api-deploy", Some("dev"), json!({})),
                ],
            )
            .with("Node", vec![raw("node-1", None, json!({}))])
            .with(
                "Namespace",
                ["prod", "dev", "idle"]
                    .map(|name| raw(name, None, json!({})))
                    .to_vec(),
            ),
    );
    let services = Arc::new(FakeServices::new(
        source.clone() as Arc<dyn WatchSource>,
        ContributionSet::default(),
    ));
    let resources = vec![pod_kind(), deployment_kind(), node_kind(), namespace_kind()];
    let mut harness = harness(cx, services, resources, false);
    harness.open_kind(&pod_kind());
    harness.wait_for_rows(2);
    harness.wait_for_options(&["dev", "idle", "prod"]);
    (harness, source)
}

impl Harness<'_> {
    fn select_namespaces(&mut self, namespaces: &[&str]) {
        let selected = namespaces.iter().map(|name| (*name).to_owned()).collect();
        self.update_browser(|browser, cx| browser.set_namespaces(selected, cx));
        self.cx.run_until_parked();
    }

    fn picker_values(&mut self) -> Vec<String> {
        let browser = self.browser();
        self.cx.update(|_, cx| {
            let browser = browser.read(cx);
            let picker = browser.namespace_picker().expect("a picker");
            picker.read(cx).selected_values()
        })
    }

    fn wait_for_options(&mut self, options: &[&str]) {
        let browser = self.browser();
        wait_until(self.cx, "the namespace options", |cx| {
            cx.update(|_, cx| browser.read(cx).namespace_options().iter().eq(options))
        });
    }

    fn row_names(&mut self, browser: &Entity<ResourceBrowser>) -> Vec<String> {
        self.cx.update(|_, cx| {
            let snapshot = browser.read(cx).snapshot().clone();
            snapshot
                .rows
                .iter()
                .map(|row| row.name.to_string())
                .collect()
        })
    }

    fn wait_for_names(&mut self, browser: &Entity<ResourceBrowser>, names: &[&str]) {
        wait_until(self.cx, "the selected rows", |cx| {
            cx.update(|_, cx| {
                let snapshot = browser.read(cx).snapshot();
                snapshot.synced
                    && snapshot
                        .rows
                        .iter()
                        .map(|row| &*row.name)
                        .eq(names.iter().copied())
            })
        });
    }

    fn set_query(&mut self, text: &str) {
        let browser = self.browser();
        let picker = self.cx.update(|_, cx| {
            browser
                .read(cx)
                .namespace_picker()
                .expect("a picker")
                .clone()
        });
        self.cx.update(|window, cx| {
            picker.update(cx, |picker, cx| picker.set_query(text, window, cx))
        });
        self.cx.run_until_parked();
    }

    fn search_text(&mut self) -> String {
        let browser = self.browser();
        self.cx.update(|_, cx| {
            let browser = browser.read(cx);
            let picker = browser.namespace_picker().expect("a picker");
            picker.read(cx).query(cx).to_string()
        })
    }
}

#[gpui_kit::test]
fn a_selection_belongs_to_one_cluster_and_outlives_the_connection(cx: &mut TestAppContext) {
    let (harness, _) = namespaced_cluster(cx);
    let model = harness.model.clone();
    let selected: std::collections::BTreeSet<String> = ["prod".to_owned()].into();
    let events = harness.cx.update(|_, cx| {
        let events = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = events.clone();
        cx.subscribe(&model, move |_, _: &ClusterModelEvent, _| {
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        })
        .detach();
        model.update(cx, |model, cx| {
            model.set_namespaces(key(), selected.clone(), cx);
            model.set_namespaces(key(), selected.clone(), cx);
        });
        events
    });
    harness.cx.run_until_parked();

    assert_eq!(events.load(std::sync::atomic::Ordering::SeqCst), 1);
    let (ours, other) = harness.cx.update(|_, cx| {
        let model = model.read(cx);
        (
            model.selected_namespaces(&key()),
            model.selected_namespaces(&other_key()),
        )
    });
    assert_eq!(ours, selected);
    assert!(other.is_empty());

    let mut disconnected = connected(CONTEXT, vec![]);
    disconnected.state = ConnectionState::Disconnected;
    harness.state.send_replace(state(disconnected));
    harness.cx.run_until_parked();
    let kept = harness
        .cx
        .update(|_, cx| model.read(cx).selected_namespaces(&key()));
    assert_eq!(kept, selected);
}

#[gpui_kit::test]
fn another_kind_opens_with_the_cluster_selection(cx: &mut TestAppContext) {
    let (mut harness, _) = namespaced_cluster(cx);
    harness.select_namespaces(&["prod"]);

    harness.show(&deployment_kind());
    let browser = harness.browser();
    wait_until(harness.cx, "the deployments", |cx| {
        cx.update(|_, cx| {
            let snapshot = browser.read(cx).snapshot();
            assert!(snapshot.rows.len() <= 1, "shown before it was filtered");
            snapshot.synced
        })
    });

    assert_eq!(harness.row_names(&browser), ["web-deploy"]);
    assert_eq!(harness.picker_values(), ["prod"]);
}

#[gpui_kit::test]
fn tabs_on_a_cluster_follow_one_selection(cx: &mut TestAppContext) {
    let (mut harness, _) = namespaced_cluster(cx);
    let (workspace, pods) = (harness.workspace.clone(), harness.browser());
    harness.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.show_kind(key(), pod_kind(), true, window, cx);
            workspace.show_kind(key(), deployment_kind(), true, window, cx);
        });
    });
    harness.cx.run_until_parked();
    let deployments = harness.browser();
    assert_ne!(pods, deployments);
    harness.wait_for_names(&deployments, &["api-deploy", "web-deploy"]);

    harness.select_namespaces(&["dev"]);

    harness.wait_for_names(&deployments, &["api-deploy"]);
    harness.wait_for_names(&pods, &["api"]);
    let values = harness.cx.update(|_, cx| {
        let browser = pods.read(cx);
        let picker = browser.namespace_picker().expect("a picker");
        picker.read(cx).selected_values()
    });
    assert_eq!(values, ["dev"]);
}

#[gpui_kit::test]
fn another_clusters_selection_changes_nothing_here(cx: &mut TestAppContext) {
    let (mut harness, _) = namespaced_cluster(cx);
    let model = harness.model.clone();
    harness.cx.update(|_, cx| {
        model.update(cx, |model, cx| {
            model.set_namespaces(other_key(), ["prod".to_owned()].into(), cx);
        });
    });
    harness.cx.run_until_parked();

    assert_eq!(harness.browser_names(), ["api", "web"]);
    assert!(harness.picker_values().is_empty());
}

impl Harness<'_> {
    fn browser_names(&mut self) -> Vec<String> {
        let browser = self.browser();
        self.row_names(&browser)
    }
}

#[gpui_kit::test]
fn the_picker_selects_searches_and_clears(cx: &mut TestAppContext) {
    let (mut harness, _) = namespaced_cluster(cx);
    let browser = harness.browser();
    assert!(harness.exists("namespace-picker".into()));

    harness.click("namespace-picker".into());
    harness.set_query("pro");
    harness.cx.simulate_keystrokes("enter");
    harness.cx.run_until_parked();

    harness.wait_for_names(&browser, &["web"]);
    assert_eq!(harness.picker_values(), ["prod"]);
    assert_eq!(harness.search_text(), "pro", "a toggle keeps the search");

    let picker = harness.cx.update(|_, cx| {
        browser
            .read(cx)
            .namespace_picker()
            .expect("a picker")
            .clone()
    });
    harness
        .cx
        .update(|_, cx| picker.update(cx, |picker, cx| picker.clear_selection(cx)));
    harness.cx.run_until_parked();

    harness.wait_for_names(&browser, &["api", "web"]);
    assert!(harness.picker_values().is_empty());
}

#[gpui_kit::test]
fn an_empty_selection_shows_every_namespace(cx: &mut TestAppContext) {
    let (mut harness, _) = namespaced_cluster(cx);
    assert!(harness.picker_values().is_empty());
    let browser = harness.browser();
    harness.wait_for_names(&browser, &["api", "web"]);
}

#[gpui_kit::test]
fn a_namespaced_custom_kind_has_the_picker(cx: &mut TestAppContext) {
    let widget = raw("big", Some("ns"), json!({"spec": {"size": 7}}));
    let source = Arc::new(ScriptedSource::default().with("Widget", vec![widget]));
    let services = Arc::new(FakeServices::new(source, ContributionSet::default()));
    let mut harness = harness(cx, services, vec![widget_kind()], false);
    harness.open_kind(&widget_kind());
    assert!(harness.exists("namespace-picker".into()));
    harness.wait_for_options(&["ns"]);
}

#[gpui_kit::test]
fn cluster_scoped_kinds_ignore_the_selection(cx: &mut TestAppContext) {
    let (mut harness, _) = namespaced_cluster(cx);
    harness.select_namespaces(&["prod"]);

    harness.show(&node_kind());
    let nodes = harness.browser();
    harness.wait_for_names(&nodes, &["node-1"]);
    assert!(!harness.exists("namespace-picker".into()));

    harness.show(&pod_kind());
    let pods = harness.browser();
    harness.wait_for_names(&pods, &["web"]);
    assert_eq!(harness.picker_values(), ["prod"]);
}

#[gpui_kit::test]
fn namespaces_with_no_objects_of_the_kind_are_offered(cx: &mut TestAppContext) {
    let (mut harness, _) = namespaced_cluster(cx);
    harness.select_namespaces(&["idle"]);
    harness.wait_for_options(&["dev", "idle", "prod"]);
    let browser = harness.browser();
    harness.wait_for_names(&browser, &[]);
}

#[gpui_kit::test]
fn a_new_namespace_appears_and_the_selection_survives_it(cx: &mut TestAppContext) {
    let (mut harness, source) = namespaced_cluster(cx);
    let browser = harness.browser();
    harness.select_namespaces(&["dev", "prod"]);

    source.send(WatchEvent::Applied(raw("aaa", None, json!({}))));
    harness.wait_for_options(&["aaa", "dev", "idle", "prod"]);
    assert_eq!(harness.picker_values(), ["dev", "prod"]);

    let picker = harness.cx.update(|_, cx| {
        browser
            .read(cx)
            .namespace_picker()
            .expect("a picker")
            .clone()
    });
    harness.click("namespace-picker".into());
    harness.set_query("dev");
    harness.cx.simulate_keystrokes("enter");
    harness.cx.run_until_parked();

    harness.wait_for_names(&browser, &["web"]);
    let values = harness.cx.update(|_, cx| picker.read(cx).selected_values());
    assert_eq!(values, ["prod"]);
}

#[gpui_kit::test]
fn a_deleted_namespace_stays_selected_until_removed(cx: &mut TestAppContext) {
    let (mut harness, source) = namespaced_cluster(cx);
    let browser = harness.browser();
    harness.select_namespaces(&["idle"]);

    source.send(WatchEvent::Deleted {
        key: "uid-idle".to_owned(),
    });
    wait_until(harness.cx, "the namespace to go", |cx| {
        cx.update(|_, cx| browser.read(cx).cluster_namespaces().len() == 2)
    });
    harness.wait_for_options(&["dev", "idle", "prod"]);
    assert_eq!(harness.picker_values(), ["idle"]);
    harness.wait_for_names(&browser, &[]);

    harness.select_namespaces(&[]);
    harness.wait_for_options(&["dev", "prod"]);
    harness.wait_for_names(&browser, &["api", "web"]);
}

#[gpui_kit::test]
fn a_denied_namespace_list_offers_the_tables_namespaces(cx: &mut TestAppContext) {
    let pods = vec![
        raw("web", Some("prod"), json!({})),
        raw("api", Some("dev"), json!({})),
    ];
    let source = Arc::new(ScriptedSource::default().with("Pod", pods));
    let services = Arc::new(FakeServices::new(
        source.clone() as Arc<dyn WatchSource>,
        ContributionSet::default(),
    ));
    let mut harness = harness(cx, services, vec![pod_kind(), namespace_kind()], false);
    harness.open_kind(&pod_kind());
    harness.wait_for_options(&["dev", "prod"]);
    let browser = harness.browser();
    assert!(
        !harness
            .cx
            .update(|_, cx| browser.read(cx).namespaces_incomplete())
    );

    source.send(WatchEvent::Denied {
        reason: "namespaces are forbidden".to_owned(),
    });
    wait_until(harness.cx, "the denial", |cx| {
        cx.update(|_, cx| browser.read(cx).namespaces_incomplete())
    });
    harness.wait_for_options(&["dev", "prod"]);

    harness.click("namespace-picker".into());
    assert!(harness.exists("namespaces-incomplete".into()));
}

#[gpui_kit::test]
fn a_cluster_without_namespaces_says_the_list_is_incomplete(cx: &mut TestAppContext) {
    let pods = vec![raw("web", Some("prod"), json!({}))];
    let source = Arc::new(ScriptedSource::default().with("Pod", pods));
    let services = Arc::new(FakeServices::new(source, ContributionSet::default()));
    let mut harness = harness(cx, services, vec![pod_kind()], false);
    harness.open_kind(&pod_kind());
    harness.wait_for_options(&["prod"]);

    harness.click("namespace-picker".into());
    assert!(harness.exists("namespaces-incomplete".into()));
}

// Status colors

fn status_column(paths: &[&str]) -> ColumnContribution {
    ColumnContribution {
        id: "status".into(),
        title: "Status".into(),
        kind: KindRef::new("", "Pod"),
        source: ColumnSource::Status {
            paths: paths.iter().map(|path| (*path).to_owned()).collect(),
        },
    }
}

const WAITING: &str = ".status.containerStatuses[?(@.state.waiting)].state.waiting.reason";

fn crashing_pod() -> Value {
    json!({
        "metadata": {"name": "crashing", "uid": "uid-crashing"},
        "status": {"phase": "Running", "containerStatuses": [
            {"state": {"waiting": {"reason": "CrashLoopBackOff"}}},
        ]},
    })
}

#[gpui_kit::test]
fn a_crash_looping_pod_reads_the_same_in_the_table_and_the_panel(cx: &mut TestAppContext) {
    let objects = vec![
        raw("crashing", Some("shop"), crashing_pod()),
        raw(
            "healthy",
            Some("shop"),
            json!({"status": {"phase": "Running"}}),
        ),
        raw(
            "strange",
            Some("shop"),
            json!({"status": {"phase": "Reticulating"}}),
        ),
    ];
    let services = Arc::new(FakeServices::new(
        Arc::new(ScriptedSource::default().with("Pod", objects)),
        contributed(vec![status_column(&[WAITING, ".status.phase"])]),
    ));
    services.add_object(&pod_kind(), Some("shop"), crashing_pod());
    let mut harness = harness(cx, services, vec![pod_kind()], false);
    harness.open_kind(&pod_kind());
    harness.wait_for_rows(3);
    harness.update_browser(|browser, cx| {
        browser.set_sort(Some((SortKey::Name, SortDirection::Ascending)), cx);
    });
    harness.settle();

    let browser = harness.browser();
    let tone = |row: usize, column: usize, harness: &mut Harness<'_>| {
        harness.cx.update(|_, cx| {
            let table = browser.read(cx).table().read(cx);
            table.delegate().cell_tone(row, column, cx)
        })
    };
    // After the checkbox, actions, name, and namespace columns.
    assert_eq!(harness.cells(3).1[0][2], "CrashLoopBackOff");
    assert_eq!(tone(0, 4, &mut harness), Some(Tone::Danger));
    assert_eq!(tone(1, 4, &mut harness), Some(Tone::Success));
    assert_eq!(tone(2, 4, &mut harness), Some(Tone::Neutral));
    assert_eq!(tone(0, 2, &mut harness), None, "the name is not a status");

    harness.open_object(target(pod_kind(), Some("shop"), "crashing"));
    let detail = harness.detail().unwrap();
    let status = harness
        .cx
        .update(|_, cx| detail.read(cx).status().map(str::to_owned));
    assert_eq!(status.as_deref(), Some("CrashLoopBackOff"));
}

// Row actions and selection

/// Pods `pod-00` onward in `shop`, sorted by name, all of which can be opened.
fn few_pods<'a>(
    cx: &'a mut TestAppContext,
    count: usize,
    protected: bool,
) -> (Harness<'a>, Arc<FakeServices>, Arc<ScriptedSource>) {
    let names = (0..count).map(|index| format!("pod-{index:02}"));
    let objects = names
        .clone()
        .map(|name| raw(&name, Some("shop"), json!({})))
        .collect();
    let source = Arc::new(ScriptedSource::default().with("Pod", objects));
    let services = Arc::new(FakeServices::new(
        source.clone() as Arc<dyn WatchSource>,
        ContributionSet::default(),
    ));
    for name in names {
        services.add_object(&pod_kind(), Some("shop"), owned(&name, None));
    }
    let mut harness = harness(cx, services.clone(), vec![pod_kind()], protected);
    harness.open_kind(&pod_kind());
    harness.wait_for_rows(count);
    harness.update_browser(|browser, cx| {
        browser.set_sort(Some((SortKey::Name, SortDirection::Ascending)), cx);
    });
    harness.settle();
    (harness, services, source)
}

impl Harness<'_> {
    /// Lets snapshots from a query or a watch reach the table.
    fn settle(&mut self) {
        self.cx.executor().advance_clock(Duration::from_millis(50));
        self.cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(30));
        self.cx.run_until_parked();
    }

    fn click_at(&mut self, id: impl Into<ElementId>) {
        let id = id.into();
        self.cx.update(|window, cx| {
            window.render_frame(cx);
            window.click(id, cx);
        });
        self.cx.run_until_parked();
    }

    fn exists_at(&mut self, id: impl Into<ElementId>) -> bool {
        let id = id.into();
        self.cx.update(|window, cx| {
            window.render_frame(cx);
            window.try_find(id).is_some()
        })
    }

    fn check(&mut self, row: usize) {
        self.click_at(("row-check", row));
    }

    fn shift_check(&mut self, row: usize) {
        self.cx.update(|window, cx| {
            window.render_frame(cx);
            let shift = Modifiers {
                shift: true,
                ..Modifiers::default()
            };
            window.click_with_modifiers(("row-check", row), shift, cx);
        });
        self.cx.run_until_parked();
    }

    /// The names of the checked rows, in table order.
    fn checked(&mut self) -> Vec<String> {
        let browser = self.browser();
        self.cx.update(|_, cx| {
            let browser = browser.read(cx);
            let delegate = browser.table().read(cx).delegate();
            browser
                .snapshot()
                .rows
                .iter()
                .filter(|row| delegate.checked().contains(&row.key))
                .map(|row| row.name.to_string())
                .collect()
        })
    }

    fn deleted_names(&self, services: &FakeServices) -> Vec<String> {
        let mut names: Vec<_> = deletions(services)
            .into_iter()
            .map(|target| target.name)
            .collect();
        names.sort();
        names
    }
}

fn names(indices: impl IntoIterator<Item = usize>) -> Vec<String> {
    indices
        .into_iter()
        .map(|index| format!("pod-{index:02}"))
        .collect()
}

#[gpui_kit::test]
fn pods_offer_logs_and_other_kinds_do_not(cx: &mut TestAppContext) {
    let source = ScriptedSource::default()
        .with("Pod", vec![raw("web", Some("shop"), json!({}))])
        .with("Secret", vec![raw("key", Some("shop"), json!({}))]);
    let services = Arc::new(FakeServices::new(
        Arc::new(source),
        ContributionSet::default(),
    ));
    let mut harness = harness(cx, services, vec![pod_kind(), secret_kind()], false);

    harness.open_kind(&pod_kind());
    assert!(harness.exists_at(("row-logs", 0usize)));
    assert!(harness.exists_at(("row-shell", 0usize)));
    assert!(harness.exists_at(("row-edit", 0usize)));
    assert!(harness.exists_at(("row-delete", 0usize)));

    harness.open_kind(&secret_kind());
    assert!(!harness.exists_at(("row-logs", 0usize)));
    assert!(!harness.exists_at(("row-shell", 0usize)));
    assert!(harness.exists_at(("row-edit", 0usize)));
    assert!(harness.exists_at(("row-delete", 0usize)));
}

#[gpui_kit::test]
fn row_buttons_act_without_opening_the_panel_or_changing_checks(cx: &mut TestAppContext) {
    let (mut harness, services, _) = few_pods(cx, 3, false);
    harness.check(0);

    harness.click_at(("row-delete", 1usize));
    assert!(dialog_open(&mut harness));
    harness.click("delete-confirm".into());
    assert_eq!(harness.deleted_names(&services), names([1]));

    assert!(harness.detail().is_none());
    assert_eq!(harness.checked(), names([0]));
}

fn open_panels(harness: &mut Harness<'_>) -> usize {
    let workspace = harness.workspace.clone();
    harness.cx.update(|_, cx| {
        let dock = workspace.read(cx).dock().read(cx);
        let tree = dock.layout(gpui_kit::component::dock::DockPlacement::Center);
        tree.map_or(0, |tree| tree.panels().count())
    })
}

#[gpui_kit::test]
fn row_buttons_open_their_tabs_and_not_the_panel(cx: &mut TestAppContext) {
    let (mut harness, _, _) = few_pods(cx, 2, false);
    let tab = harness.tab();
    let before = open_panels(&mut harness);

    let panel = gpui_kit::component::dock::PanelId::from(tab.entity_id());
    let workspace = harness.workspace.clone();
    for (opened, button) in ["row-logs", "row-shell", "row-edit"]
        .into_iter()
        .enumerate()
    {
        harness.cx.update(|window, cx| {
            let dock = workspace.read(cx).dock().clone();
            dock.update(cx, |dock, cx| dock.select_panel(panel, window, cx));
        });
        harness.cx.run_until_parked();
        harness.click_at((button, 1usize));
        assert_eq!(open_panels(&mut harness), before + opened + 1, "{button}");
    }

    let detail = harness.cx.update(|_, cx| tab.read(cx).detail().cloned());
    assert!(detail.is_none());
}

#[gpui_kit::test]
fn the_right_click_menu_keeps_its_object_when_rows_move(cx: &mut TestAppContext) {
    let (mut harness, services, source) = few_pods(cx, 3, false);
    harness.cx.update(|window, cx| {
        window.render_frame(cx);
        window.right_click(SharedString::from("table-cell-1:2"), cx);
    });
    harness.cx.run_until_parked();

    let mut first = raw("aaa-first", Some("shop"), json!({}));
    first.key = "uid-first".into();
    source.send(WatchEvent::Applied(first));
    harness.wait_for_rows(4);

    harness.cx.update(|window, cx| {
        window.render_frame(cx);
        window.within("popup-menu").click(ElementId::Integer(4), cx);
    });
    harness.cx.run_until_parked();
    assert!(dialog_open(&mut harness));
    harness.click("delete-confirm".into());

    assert_eq!(harness.deleted_names(&services), names([1]));
}

#[gpui_kit::test]
fn a_checkbox_toggles_its_row_and_opens_nothing(cx: &mut TestAppContext) {
    let (mut harness, _, _) = few_pods(cx, 5, false);

    harness.check(2);
    assert_eq!(harness.checked(), names([2]));
    assert!(harness.detail().is_none());

    harness.check(2);
    assert!(harness.checked().is_empty());

    harness.check(1);
    harness.click("table-cell-3:2".into());
    assert!(harness.detail().is_some(), "a row click still opens it");
    assert_eq!(harness.checked(), names([1]));
}

#[gpui_kit::test]
fn shift_selects_a_range_in_either_direction(cx: &mut TestAppContext) {
    let (mut harness, _, _) = few_pods(cx, 9, false);

    harness.check(3);
    harness.shift_check(7);
    assert_eq!(harness.checked(), names(3..=7));

    harness.update_browser(|browser, cx| browser.clear_checked(cx));
    harness.check(6);
    harness.shift_check(2);
    assert_eq!(harness.checked(), names(2..=6));
    assert!(harness.detail().is_none());
}

#[gpui_kit::test]
fn shift_adds_to_the_selection_and_never_unchecks(cx: &mut TestAppContext) {
    let (mut harness, _, _) = few_pods(cx, 9, false);

    harness.check(0);
    harness.check(4);
    harness.shift_check(6);
    assert_eq!(harness.checked(), names([0, 4, 5, 6]));

    harness.shift_check(5);
    assert_eq!(harness.checked(), names([0, 4, 5, 6]));
}

#[gpui_kit::test]
fn shift_follows_the_order_shown_after_sorting(cx: &mut TestAppContext) {
    let (mut harness, _, _) = few_pods(cx, 6, false);

    harness.check(1);
    harness.update_browser(|browser, cx| {
        browser.set_sort(Some((SortKey::Name, SortDirection::Descending)), cx);
    });
    harness.settle();
    // Descending, so pod-01 is at row 4 and row 2 is pod-03.
    harness.shift_check(2);

    assert_eq!(harness.checked(), ["pod-03", "pod-02", "pod-01"]);
}

#[gpui_kit::test]
fn shift_with_no_earlier_click_checks_only_that_row(cx: &mut TestAppContext) {
    let (mut harness, _, _) = few_pods(cx, 6, false);

    harness.shift_check(3);
    assert_eq!(harness.checked(), names([3]));
}

#[gpui_kit::test]
fn shift_after_the_earlier_row_left_checks_only_that_row(cx: &mut TestAppContext) {
    let (mut harness, _, _) = few_pods(cx, 6, false);

    harness.check(1);
    harness.update_browser(|browser, cx| browser.set_filter("pod-0", cx));
    harness.settle();
    harness.update_browser(|browser, cx| browser.set_filter("pod-05", cx));
    harness.settle();
    harness.update_browser(|browser, cx| browser.set_filter("", cx));
    harness.settle();

    harness.shift_check(4);
    assert_eq!(harness.checked(), names([4]));
}

#[gpui_kit::test]
fn the_header_checkbox_selects_every_row_shown_and_clears(cx: &mut TestAppContext) {
    let (mut harness, _, _) = few_pods(cx, 5, false);
    harness.update_browser(|browser, cx| browser.set_filter("pod-0", cx));
    harness.settle();

    harness.check(1);
    harness.click("select-all".into());
    assert_eq!(harness.checked(), names(0..5));

    harness.click("select-all".into());
    assert!(harness.checked().is_empty());
}

#[gpui_kit::test]
fn checks_stay_on_their_objects_as_rows_arrive(cx: &mut TestAppContext) {
    let (mut harness, _, source) = few_pods(cx, 4, false);
    harness.check(2);

    let mut first = raw("aaa-first", Some("shop"), json!({}));
    first.key = "uid-first".into();
    source.send(WatchEvent::Applied(first));
    harness.wait_for_rows(5);

    assert_eq!(harness.checked(), names([2]));
}

#[gpui_kit::test]
fn a_row_hidden_by_a_filter_stops_being_checked(cx: &mut TestAppContext) {
    let (mut harness, _, _) = few_pods(cx, 4, false);
    harness.check(0);
    harness.check(1);

    harness.update_browser(|browser, cx| browser.set_filter("pod-01", cx));
    harness.settle();
    assert_eq!(harness.checked(), names([1]));

    harness.update_browser(|browser, cx| browser.set_filter("", cx));
    harness.settle();
    assert_eq!(harness.checked(), names([1]));
}

#[gpui_kit::test]
fn a_checked_object_deleted_elsewhere_leaves_the_count(cx: &mut TestAppContext) {
    let (mut harness, _, source) = few_pods(cx, 4, false);
    harness.check(0);
    harness.check(1);
    assert!(harness.exists("checked-count".into()));

    source.send(WatchEvent::Deleted {
        key: "uid-pod-01".into(),
    });
    harness.wait_for_rows(3);

    assert_eq!(harness.checked(), names([0]));
}

#[gpui_kit::test]
fn the_selection_bar_appears_with_checks_and_clears_them(cx: &mut TestAppContext) {
    let (mut harness, _, _) = few_pods(cx, 3, false);
    assert!(!harness.exists("checked-count".into()));

    harness.check(0);
    assert!(harness.exists("checked-count".into()));

    harness.click("clear-checked".into());
    assert!(harness.checked().is_empty());
    assert!(!harness.exists("checked-count".into()));
}

#[gpui_kit::test]
fn selecting_every_row_of_ten_thousand_is_quick(cx: &mut TestAppContext) {
    let (mut harness, _) = selectable(cx);
    let browser = harness.browser();
    harness.cx.update(|window, cx| window.render_frame(cx));

    // Timed without the simulated mouse, which is most of a click in a test.
    let table = harness.cx.update(|_, cx| browser.read(cx).table().clone());
    let started = std::time::Instant::now();
    harness.cx.update(|window, cx| {
        table.update(cx, |table, cx| {
            table.delegate_mut().click_header_checkbox();
            cx.notify();
        });
        window.render_frame(cx);
    });
    let elapsed = started.elapsed();

    let checked = harness.cx.update(|_, cx| {
        let table = browser.read(cx).table().read(cx);
        table.delegate().checked().len()
    });
    assert_eq!(checked, TEN_THOUSAND);
    assert!(elapsed < Duration::from_millis(100), "{elapsed:?}");
}

// Deleting several objects

#[gpui_kit::test]
fn several_objects_are_listed_and_nothing_goes_until_confirmed(cx: &mut TestAppContext) {
    let (mut harness, services, _) = few_pods(cx, 4, false);
    harness.check(0);
    harness.shift_check(2);

    harness.click("delete-checked".into());

    assert!(harness.exists("delete-confirmation".into()));
    assert!(harness.exists("delete-list".into()));
    for row in 0..3usize {
        assert!(harness.exists_at(("delete-item", row)));
    }
    assert!(!harness.exists_at(("delete-item", 3usize)));

    harness.cx.update(|window, cx| window.press("enter", cx));
    harness.cx.run_until_parked();
    assert!(deletions(&services).is_empty());

    harness.click("delete-confirm".into());
    assert_eq!(harness.deleted_names(&services), names(0..3));
    assert!(!dialog_open(&mut harness));
    assert!(harness.checked().is_empty());
}

#[gpui_kit::test]
fn several_objects_on_a_protected_cluster_need_the_cluster_name(cx: &mut TestAppContext) {
    let (mut harness, services, _) = few_pods(cx, 3, true);
    harness.check(0);
    harness.check(1);
    harness.click("delete-checked".into());

    assert!(!harness.exists("delete-confirm".into()));
    type_name(&mut harness, "pod-00");
    assert!(!harness.exists("delete-confirm".into()));

    type_name(&mut harness, CONTEXT);
    assert!(harness.exists("delete-confirm".into()));
    assert!(deletions(&services).is_empty());
}

#[gpui_kit::test]
fn a_row_delete_on_a_protected_cluster_needs_the_object_name(cx: &mut TestAppContext) {
    let (mut harness, _, _) = few_pods(cx, 3, true);
    harness.click_at(("row-delete", 1usize));

    type_name(&mut harness, CONTEXT);
    assert!(!harness.exists("delete-confirm".into()));

    type_name(&mut harness, "pod-01");
    assert!(harness.exists("delete-confirm".into()));
}

#[gpui_kit::test]
fn a_failure_leaves_the_rest_deleted_and_only_it_is_retried(cx: &mut TestAppContext) {
    let (mut harness, services, _) = few_pods(cx, 3, false);
    services
        .delete_errors
        .lock()
        .unwrap()
        .insert("pod-01".into(), ObjectError::Denied("forbidden".into()));
    harness.click("select-all".into());
    harness.click("delete-checked".into());

    harness.click("delete-confirm".into());

    assert!(dialog_open(&mut harness), "the failure keeps it open");
    assert_eq!(harness.deleted_names(&services), names(0..3));
    assert!(harness.exists_at(("delete-item", 0usize)));
    assert!(!harness.exists_at(("delete-item", 1usize)));
    assert_eq!(harness.checked(), names([1]));

    services.deleted.lock().unwrap().clear();
    harness.click("delete-confirm".into());

    assert_eq!(harness.deleted_names(&services), names([1]));
    assert!(!dialog_open(&mut harness));
    assert!(harness.checked().is_empty());
}

#[gpui_kit::test]
fn an_object_already_gone_counts_as_deleted(cx: &mut TestAppContext) {
    let (mut harness, services, _) = few_pods(cx, 2, false);
    services
        .delete_errors
        .lock()
        .unwrap()
        .insert("pod-00".into(), ObjectError::NotFound("gone".into()));
    harness.click("select-all".into());
    harness.click("delete-checked".into());

    harness.click("delete-confirm".into());

    assert!(!dialog_open(&mut harness));
    assert!(harness.checked().is_empty());
}

#[gpui_kit::test]
fn cancelling_starts_no_more_deletions(cx: &mut TestAppContext) {
    let (mut harness, services, _) = few_pods(cx, 12, false);
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    *services.delete_gate.lock().unwrap() = Some(gate.clone());
    harness.click("select-all".into());
    harness.click("delete-checked".into());

    harness.click("delete-confirm".into());
    assert_eq!(deletions(&services).len(), 8, "at most eight at once");
    assert!(harness.exists("delete-progress".into()));

    harness.click("delete-cancel".into());
    gate.add_permits(12);
    harness.settle();

    assert_eq!(deletions(&services).len(), 8);
    assert!(!dialog_open(&mut harness));
}

#[gpui_kit::test]
fn escape_during_deletion_starts_no_more_of_them(cx: &mut TestAppContext) {
    let (mut harness, services, _) = few_pods(cx, 12, false);
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    *services.delete_gate.lock().unwrap() = Some(gate.clone());
    harness.click("select-all".into());
    harness.click("delete-checked".into());
    harness.click("delete-confirm".into());

    harness.cx.update(|window, cx| window.press("escape", cx));
    harness.cx.run_until_parked();
    gate.add_permits(12);
    harness.settle();

    assert_eq!(deletions(&services).len(), 8);
    assert!(!dialog_open(&mut harness));
}

#[gpui_kit::test]
fn deleting_the_open_object_with_others_closes_the_panel(cx: &mut TestAppContext) {
    let (mut harness, _, _) = few_pods(cx, 3, false);
    harness.click("table-cell-1:2".into());
    harness.wait_for_detail();
    harness.click("select-all".into());
    harness.click("delete-checked".into());

    harness.click("delete-confirm".into());

    assert!(harness.detail().is_none());
}

// Detail panel

#[gpui_kit::test]
fn the_panel_closes_and_the_same_row_opens_again(cx: &mut TestAppContext) {
    let (mut harness, _, _) = few_pods(cx, 3, false);
    harness.check(0);
    harness.click("table-cell-1:2".into());
    harness.wait_for_detail();

    harness.click("close-detail".into());

    assert!(harness.detail().is_none());
    let browser = harness.browser();
    let open = harness
        .cx
        .update(|_, cx| browser.read(cx).selected().cloned());
    assert!(open.is_none());
    assert_eq!(selected_row(&browser, &mut harness), None);
    assert_eq!(harness.checked(), names([0]), "checks survive closing");

    harness.click("table-cell-1:2".into());
    harness.wait_for_detail();
    assert!(harness.detail().is_some());
}

#[gpui_kit::test]
fn escape_in_the_table_closes_the_panel(cx: &mut TestAppContext) {
    let (mut harness, _, _) = few_pods(cx, 3, false);
    harness.click("table-cell-1:2".into());
    harness.wait_for_detail();

    harness.cx.update(|window, cx| window.press("escape", cx));
    harness.cx.run_until_parked();

    assert!(harness.detail().is_none());
}

#[gpui_kit::test]
fn escape_closes_the_panel_while_its_row_is_scrolled_away(cx: &mut TestAppContext) {
    let (mut harness, _) = selectable(cx);
    harness.cx.update(|window, cx| window.render_frame(cx));
    harness.click("table-cell-3:2".into());
    harness.wait_for_detail();
    let browser = harness.browser();

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
    assert_eq!(selected_row(&browser, &mut harness), None);

    harness.cx.update(|window, cx| window.press("escape", cx));
    harness.cx.run_until_parked();

    assert!(harness.detail().is_none());
}

fn detailed_pod() -> Value {
    json!({
        "metadata": {
            "name": "web",
            "uid": "uid-web",
            "labels": {"app": "web", "tier": "front"},
            "annotations": {"note": "kept in full"},
        },
        "status": {"conditions": [
            {"type": "Ready", "status": "False", "reason": "Crash", "message": "back-off"},
            {"type": "DiskPressure", "status": "True"},
        ]},
    })
}

fn detailed(cx: &mut TestAppContext, object: Value, kind: ResourceKind) -> Harness<'_> {
    let services = Arc::new(FakeServices::idle());
    services.add_object(&kind, Some("shop"), object);
    services.events.lock().unwrap().push(EventRecord {
        kind: "Warning".into(),
        reason: "BackOff".into(),
        message: "restarting failed container".into(),
        count: 3,
        last_seen: None,
    });
    let mut harness = harness(cx, services, vec![pod_kind(), secret_kind()], false);
    harness.open_object(target(kind, Some("shop"), "web"));
    harness
}

#[gpui_kit::test]
fn labels_conditions_and_events_have_their_sections(cx: &mut TestAppContext) {
    let mut harness = detailed(cx, detailed_pod(), pod_kind());
    let detail = harness.detail().unwrap();
    wait_until(harness.cx, "the events", |cx| {
        cx.update(|_, cx| !detail.read(cx).events().is_empty())
    });

    assert!(harness.exists("detail-labels".into()));
    assert!(harness.exists("detail-conditions".into()));
    assert!(harness.exists_at(("detail-event", 0usize)));
    assert!(harness.exists("detail-header".into()));
    let tones = harness.cx.update(|_, cx| {
        let conditions = detail.read(cx).conditions();
        conditions.iter().map(|c| c.tone).collect::<Vec<_>>()
    });
    assert_eq!(tones, [Tone::Warning, Tone::Danger]);
}

#[gpui_kit::test]
fn events_can_be_scrolled_into_view_below_a_tall_panel(cx: &mut TestAppContext) {
    let conditions: Vec<Value> = (0..40)
        .map(|index| {
            json!({"type": format!("Check{index}"), "status": "True", "reason": "Fine",
                "message": "a message long enough to need its own line in the panel"})
        })
        .collect();
    let object = json!({
        "metadata": {"name": "web", "uid": "uid-web"},
        "status": {"conditions": conditions},
    });
    let mut harness = detailed(cx, object, pod_kind());
    let detail = harness.detail().unwrap();
    wait_until(harness.cx, "the events", |cx| {
        cx.update(|_, cx| !detail.read(cx).events().is_empty())
    });

    let bounds = |id: ElementId, harness: &mut Harness<'_>| {
        harness.cx.update(|window, cx| {
            window.render_frame(cx);
            window.try_find(id).expect("the element").bounds()
        })
    };
    let body = bounds("detail-body".into(), &mut harness);
    let event = ElementId::from(("detail-event", 0usize));
    assert!(body.size.height < gpui_kit::px(1000.), "{body:?}");
    assert!(bounds(event.clone(), &mut harness).top() > body.bottom());

    harness.cx.update(|window, cx| {
        window.render_frame(cx);
        window.scroll(
            "detail-body",
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(gpui_kit::px(0.), gpui_kit::px(-5000.))),
            cx,
        );
    });
    harness.cx.run_until_parked();

    let shown = bounds(event, &mut harness);
    assert!(shown.bottom() <= body.bottom(), "{shown:?} in {body:?}");
    assert!(shown.top() >= body.top(), "{shown:?} in {body:?}");
}

#[gpui_kit::test]
fn a_short_window_scrolls_the_panel_rather_than_squashing_it(cx: &mut TestAppContext) {
    let mut harness = detailed(cx, detailed_pod(), pod_kind());
    let detail = harness.detail().unwrap();
    wait_until(harness.cx, "the events", |cx| {
        cx.update(|_, cx| !detail.read(cx).events().is_empty())
    });

    let height = gpui_kit::px(400.);
    harness
        .cx
        .simulate_resize(gpui_kit::size(gpui_kit::px(1280.), height));
    harness.cx.run_until_parked();

    for id in [
        "detail-summary",
        "detail-labels",
        "detail-conditions",
        "detail-events",
    ] {
        let bounds = harness.cx.update(|window, cx| {
            window.render_frame(cx);
            let found = window.try_find(SharedString::from(id));
            found.expect("the section").bounds()
        });
        assert!(bounds.size.height > gpui_kit::px(20.), "{id}: {bounds:?}");
    }
}

#[gpui_kit::test]
fn a_pod_with_no_events_says_so(cx: &mut TestAppContext) {
    let (mut harness, _, _) = few_pods(cx, 1, false);
    assert!(!harness.exists("no-events".into()));

    harness.click("table-cell-0:2".into());
    harness.wait_for_detail();

    assert!(harness.exists("no-events".into()));
}

#[gpui_kit::test]
fn annotations_start_collapsed_and_show_values(cx: &mut TestAppContext) {
    let mut harness = detailed(cx, detailed_pod(), pod_kind());
    let detail = harness.detail().unwrap();

    harness.click("toggle-annotations".into());

    assert!(harness.exists("detail-annotations".into()));
    let shown = harness.cx.update(|_, cx| {
        let detail = detail.read(cx);
        detail.annotations(detail.object().unwrap())
    });
    assert_eq!(
        shown,
        [("note".to_owned(), Some("kept in full".to_owned()))]
    );
}

#[gpui_kit::test]
fn a_secret_shows_annotation_keys_but_never_their_values(cx: &mut TestAppContext) {
    let secret = json!({
        "metadata": {
            "name": "web",
            "uid": "uid-web",
            "annotations": {"last-applied": "{\"data\":{\"password\":\"c2VjcmV0\"}}"},
        },
        "data": {"password": "c2VjcmV0"},
    });
    let mut harness = detailed(cx, secret, secret_kind());
    let detail = harness.detail().unwrap();

    harness.click("toggle-annotations".into());

    let shown = harness.cx.update(|_, cx| {
        let detail = detail.read(cx);
        detail.annotations(detail.object().unwrap())
    });
    assert_eq!(shown, [("last-applied".to_owned(), None)]);
}

fn service_kind() -> ResourceKind {
    kind("", "Service", "services", true)
}

/// A pod and a service that declare ports, and a secret that cannot be forwarded to.
fn forwardable(cx: &mut TestAppContext) -> (Harness<'_>, Arc<FakeServices>) {
    let pod = json!({
        "metadata": {"name": "web", "namespace": "shop", "uid": "uid-web"},
        "spec": {"containers": [{"name": "app", "ports": [{"containerPort": 8080}]}]},
    });
    let service = json!({
        "metadata": {"name": "front", "namespace": "shop", "uid": "uid-front"},
        "spec": {"ports": [{"port": 80, "targetPort": 8080}]},
    });
    let source = ScriptedSource::default()
        .with("Pod", vec![raw("web", Some("shop"), pod.clone())])
        .with("Service", vec![raw("front", Some("shop"), service.clone())])
        .with("Secret", vec![raw("key", Some("shop"), json!({}))]);
    let services = Arc::new(FakeServices::new(
        Arc::new(source),
        ContributionSet::default(),
    ));
    services.add_object(&pod_kind(), Some("shop"), pod);
    services.add_object(&service_kind(), Some("shop"), service);
    let resources = vec![pod_kind(), service_kind(), secret_kind()];
    (harness(cx, services.clone(), resources, false), services)
}

impl Harness<'_> {
    fn forward_dialog(&mut self) -> Entity<ForwardDialog> {
        let tab = self.tab();
        self.cx
            .update(|_, cx| tab.read(cx).forward_dialog().expect("an open dialog"))
    }

    fn restart_dialog(&mut self) -> Entity<ForwardDialog> {
        let shell = self.shell.clone();
        self.cx.update(|_, cx| {
            let tab = shell.read(cx).forwards_tab().expect("the forwards tab");
            tab.read(cx).dialog().expect("an open dialog")
        })
    }

    fn set_field(&mut self, dialog: &Entity<ForwardDialog>, field: ForwardField, text: &str) {
        self.cx.update(|window, cx| {
            dialog.update(cx, |dialog, cx| dialog.set_field(field, text, window, cx));
        });
        self.cx.run_until_parked();
    }

    fn requested(&mut self, dialog: &Entity<ForwardDialog>) -> Option<ForwardRequest> {
        self.cx.update(|_, cx| dialog.read(cx).request(cx))
    }

    /// Each listed forward's request and state.
    fn forwards(&mut self) -> Vec<(ForwardRequest, ForwardState)> {
        self.cx.update(|_, cx| {
            let registry = cx.global::<SharedForwards>().0.read(cx);
            registry
                .entries()
                .iter()
                .map(|entry| (entry.request.clone(), entry.state.clone()))
                .collect()
        })
    }

    fn wait_for_forwards(
        &mut self,
        what: &str,
        mut done: impl FnMut(&[(ForwardRequest, ForwardState)]) -> bool,
    ) {
        let started = std::time::Instant::now();
        loop {
            self.cx.run_until_parked();
            if done(&self.forwards()) {
                return;
            }
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "timed out waiting for {what}"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Opens the dialog from the first row of `kind` and waits for the port the
    /// object declares.
    fn open_forward(&mut self, kind: &ResourceKind, port: u16) -> Entity<ForwardDialog> {
        self.open_kind(kind);
        self.click_at(("row-forward", 0usize));
        assert!(dialog_open(self));
        let dialog = self.forward_dialog();
        wait_until(self.cx, "the declared port", |cx| {
            cx.update(|_, cx| dialog.read(cx).request(cx).map(|request| request.port)) == Some(port)
        });
        dialog
    }

    fn start_pod_forward(&mut self) {
        self.open_forward(&pod_kind(), 8080);
        self.click("forward-start".into());
        self.wait_for_forwards("the forward", |forwards| forwards.len() == 1);
    }
}

fn pod_request(name: &str, local_port: Option<u16>) -> ForwardRequest {
    ForwardRequest {
        kind: ForwardKind::Pod,
        namespace: "shop".into(),
        name: name.into(),
        port: 8080,
        local_port,
    }
}

#[gpui_kit::test]
fn pods_and_services_offer_forwarding_and_other_kinds_do_not(cx: &mut TestAppContext) {
    let (mut harness, _) = forwardable(cx);
    harness.open_kind(&pod_kind());
    assert!(harness.exists_at(("row-forward", 0usize)));
    harness.open_kind(&service_kind());
    assert!(harness.exists_at(("row-forward", 0usize)));
    harness.open_kind(&secret_kind());
    assert!(!harness.exists_at(("row-forward", 0usize)));

    harness.open_object(ObjectTarget {
        kind: service_kind(),
        namespace: Some("shop".into()),
        name: "front".into(),
    });
    assert!(harness.exists("open-forward".into()));
}

#[gpui_kit::test]
fn a_pod_forward_uses_the_declared_port_and_shows_the_picked_local_port(cx: &mut TestAppContext) {
    let (mut harness, services) = forwardable(cx);
    harness.start_pod_forward();

    assert!(!dialog_open(&mut harness));
    let started: Vec<_> = services
        .forwards
        .lock()
        .unwrap()
        .iter()
        .map(|forward| forward.request.clone())
        .collect();
    assert_eq!(started, [pod_request("web", None)]);
    let (request, state) = harness.forwards().remove(0);
    assert_ne!(
        request.local_port.unwrap_or_default(),
        0,
        "the picked port is shown"
    );
    assert_eq!(state, ForwardState::Starting);
    assert!(
        harness.exists_at(("forward-ports", 0usize)),
        "the list opened"
    );

    let active = ForwardState::Active {
        pod: "web".into(),
        port: 8080,
    };
    services.forwards.lock().unwrap()[0].set_state(active.clone());
    harness.wait_for_forwards("the active state", |forwards| forwards[0].1 == active);
}

#[gpui_kit::test]
fn a_service_forward_asks_for_a_service_port(cx: &mut TestAppContext) {
    let (mut harness, services) = forwardable(cx);
    harness.open_forward(&service_kind(), 80);
    harness.click("forward-start".into());
    harness.wait_for_forwards("the forward", |forwards| forwards.len() == 1);
    let request = services.forwards.lock().unwrap()[0].request.clone();
    assert_eq!(request.kind, ForwardKind::Service);
    assert_eq!((request.name.as_str(), request.port), ("front", 80));
}

#[gpui_kit::test]
fn a_local_port_in_use_is_reported_and_starts_nothing(cx: &mut TestAppContext) {
    let (mut harness, services) = forwardable(cx);
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port();
    let dialog = harness.open_forward(&pod_kind(), 8080);
    harness.set_field(&dialog, ForwardField::LocalPort, &port.to_string());

    harness.click("forward-start".into());

    assert!(dialog_open(&mut harness));
    let error = harness
        .cx
        .update(|_, cx| dialog.read(cx).error().map(str::to_owned));
    assert!(error.unwrap().contains(&port.to_string()));
    assert!(harness.exists("forward-error".into()));
    assert!(services.forwards.lock().unwrap().is_empty());
    assert!(harness.forwards().is_empty());
}

#[gpui_kit::test]
fn an_invalid_port_cannot_be_started(cx: &mut TestAppContext) {
    let (mut harness, _) = forwardable(cx);
    let dialog = harness.open_forward(&pod_kind(), 8080);
    harness.set_field(&dialog, ForwardField::Port, "70000");
    assert_eq!(harness.requested(&dialog), None);
    harness.set_field(&dialog, ForwardField::Port, "8080");
    harness.set_field(&dialog, ForwardField::LocalPort, "local");
    assert_eq!(harness.requested(&dialog), None);
}

#[gpui_kit::test]
fn a_forward_outlives_its_tab_and_stopping_it_releases_it(cx: &mut TestAppContext) {
    let (mut harness, services) = forwardable(cx);
    harness.start_pod_forward();
    let tab = harness.tab().entity_id();
    let shell = harness.shell.clone();
    harness
        .cx
        .update(|_, cx| shell.update(cx, |shell, cx| shell.request_close(tab, cx)));
    harness.cx.run_until_parked();
    assert!(harness.cx.update(|_, cx| shell.read(cx).tab(tab).is_none()));
    assert_eq!(harness.forwards().len(), 1);

    harness.click_at(("forward-stop", 0usize));

    assert!(harness.forwards().is_empty());
    assert!(services.forwards.lock().unwrap()[0].feed.stopped());
    assert!(harness.exists("no-forwards".into()));
}

#[gpui_kit::test]
fn a_forward_leaves_the_list_when_its_session_ends(cx: &mut TestAppContext) {
    let (mut harness, services) = forwardable(cx);
    harness.start_pod_forward();
    drop(services.forwards.lock().unwrap().remove(0));
    harness.wait_for_forwards("the forward to go", <[_]>::is_empty);
}

#[gpui_kit::test]
fn a_failed_forward_can_be_restarted_against_another_pod(cx: &mut TestAppContext) {
    let (mut harness, services) = forwardable(cx);
    harness.start_pod_forward();
    let local_port = harness.forwards()[0].0.local_port;
    assert!(!harness.exists_at(("forward-restart", 0usize)));

    services.forwards.lock().unwrap()[0].fail(ForwardFailure::PodDeleted);
    let failed = ForwardState::Failed(ForwardFailure::PodDeleted);
    harness.wait_for_forwards("the failure", |forwards| forwards[0].1 == failed);
    harness.click_at(("forward-restart", 0usize));
    let dialog = harness.restart_dialog();
    assert_eq!(
        harness.requested(&dialog),
        Some(pod_request("web", local_port))
    );
    harness.set_field(&dialog, ForwardField::Name, "web-2");
    harness.click("forward-start".into());

    assert!(!dialog_open(&mut harness));
    let restarted = services.forwards.lock().unwrap()[1].request.clone();
    assert_eq!(
        restarted,
        pod_request("web-2", local_port),
        "the same port is used"
    );
    let listed = harness.forwards();
    assert_eq!(listed.len(), 1, "the failed forward was replaced");
    assert_eq!(listed[0].0.name, "web-2");
    assert_eq!(listed[0].1, ForwardState::Starting);
}

#[gpui_kit::test]
fn the_rail_opens_the_forward_list(cx: &mut TestAppContext) {
    let (mut harness, _) = forwardable(cx);
    harness.click("rail-forwards".into());
    assert!(harness.exists("no-forwards".into()));
}
