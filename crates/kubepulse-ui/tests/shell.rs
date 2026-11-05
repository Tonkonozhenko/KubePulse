mod support;

use std::{fs, path::PathBuf, sync::Arc, time::Duration};

use gpui_kit::{
    AppContext as _, Entity, TestAppContext, VisualTestContext,
    component::dock::{DockAreaState, DockPlacement, PanelInfo, PanelState},
    test::TestWindowExt as _,
};
use kubepulse_cluster::{ClusterEntry, ConnectionState, SessionOptions};
use kubepulse_ui::{
    cluster_model::{ClusterModel, SharedClusterModel},
    cluster_tab::ClusterTab,
    identity::{ClusterKey, IdentityStore},
    persistence::{ConfigPaths, write_atomic},
    runtime::{ClusterCommand, ClusterRuntime, ClusterState, ClusterStatus},
    services::SharedServices,
    workspace::Workspace,
};
use support::FakeServices;
use tokio::sync::{mpsc, watch};

struct Harness {
    state: watch::Sender<Arc<ClusterState>>,
    commands: mpsc::UnboundedReceiver<ClusterCommand>,
    model: Entity<ClusterModel>,
}

fn status(context: &str, state: ConnectionState, work: &[&str]) -> ClusterStatus {
    ClusterStatus {
        entry: ClusterEntry {
            context: context.to_owned(),
            cluster: Some(context.to_owned()),
            server: Some(format!("https://{context}.example")),
            user: None,
            namespace: None,
        },
        state,
        active_work: work.iter().map(|w| w.to_string()).collect(),
        resources: Arc::default(),
    }
}

fn key(context: &str) -> ClusterKey {
    ClusterKey::new(context, Some(&format!("https://{context}.example")))
}

fn clusters(statuses: Vec<ClusterStatus>) -> Arc<ClusterState> {
    Arc::new(ClusterState {
        version: 1,
        clusters: statuses,
        errors: vec![],
    })
}

fn harness(cx: &mut TestAppContext, statuses: Vec<ClusterStatus>) -> Harness {
    let (state, receiver) = watch::channel(clusters(statuses));
    let (sender, commands) = mpsc::unbounded_channel();
    let model = cx.update(|cx| {
        gpui_kit::init(cx);
        ClusterTab::register(cx);
        cx.set_global(SharedServices(Arc::new(FakeServices::idle())));
        let model = cx.new(|cx| ClusterModel::new(receiver, sender, IdentityStore::default(), cx));
        cx.set_global(SharedClusterModel(model.clone()));
        model
    });
    Harness {
        state,
        commands,
        model,
    }
}

fn window<'a>(
    cx: &'a mut TestAppContext,
    model: &Entity<ClusterModel>,
    config: Option<ConfigPaths>,
) -> (Entity<Workspace>, &'a mut VisualTestContext) {
    let model = model.clone();
    let (workspace, cx) =
        cx.add_window_view(|window, cx| Workspace::new(model, config, window, cx));
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1400.), gpui_kit::px(900.)));
    cx.run_until_parked();
    (workspace, cx)
}

fn open(workspace: &Entity<Workspace>, context: &str, cx: &mut VisualTestContext) {
    let key = key(context);
    cx.update(|window, cx| workspace.update(cx, |w, cx| w.open_cluster(key, window, cx)));
    cx.run_until_parked();
}

fn no_commands(harness: &mut Harness) {
    assert_eq!(
        harness.commands.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    );
}

fn tabs_in(state: &PanelState, found: &mut Vec<String>) {
    if let PanelInfo::Panel(value) = &state.info
        && let Some(context) = value["key"]["context"].as_str()
    {
        found.push(context.to_owned());
    }
    state
        .children
        .iter()
        .for_each(|child| tabs_in(child, found));
}

fn layout(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> DockAreaState {
    cx.update(|_, cx| workspace.read(cx).dock().read(cx).dump(cx))
}

fn open_tabs(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Vec<String> {
    let mut found = Vec::new();
    tabs_in(&layout(workspace, cx).center, &mut found);
    found
}

fn click(id: &'static str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.click(id, cx));
    cx.run_until_parked();
}

fn click_named(id: String, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.click(gpui_kit::SharedString::from(id), cx));
    cx.run_until_parked();
}

fn exists(id: &'static str, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| {
        window.render_frame(cx);
        window.try_find(id).is_some()
    })
}

fn publish(harness: &Harness, statuses: Vec<ClusterStatus>, cx: &mut VisualTestContext) {
    harness.state.send_replace(clusters(statuses));
    cx.run_until_parked();
}

// Tabs and panes

#[gpui_kit::test]
fn tabs_open_on_several_clusters_and_stay_bound(cx: &mut TestAppContext) {
    let harness = harness(
        cx,
        vec![
            status("a", ConnectionState::Disconnected, &[]),
            status("b", ConnectionState::Disconnected, &[]),
        ],
    );
    let (workspace, cx) = window(cx, &harness.model, None);
    open(&workspace, "a", cx);
    open(&workspace, "a", cx);
    open(&workspace, "b", cx);
    assert_eq!(open_tabs(&workspace, cx), ["a", "a", "b"]);
}

fn panel_ids(
    workspace: &Entity<Workspace>,
    cx: &mut VisualTestContext,
) -> Vec<gpui_kit::component::dock::PanelId> {
    cx.update(|_, cx| {
        let dock = workspace.read(cx).dock().read(cx);
        let tree = dock.layout(DockPlacement::Center).expect("a center layout");
        tree.panels().collect()
    })
}

fn group_count(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> usize {
    fn count(state: &PanelState) -> usize {
        let own = usize::from(matches!(state.info, PanelInfo::Tabs { .. }));
        own + state.children.iter().map(count).sum::<usize>()
    }
    count(&layout(workspace, cx).center)
}

#[gpui_kit::test]
fn splitting_and_moving_tabs_keeps_every_tab(cx: &mut TestAppContext) {
    use gpui_kit::{base::Placement, component::dock::InsertTarget};

    let harness = harness(
        cx,
        vec![
            status("a", ConnectionState::Disconnected, &[]),
            status("b", ConnectionState::Disconnected, &[]),
        ],
    );
    let (workspace, cx) = window(cx, &harness.model, None);
    open(&workspace, "a", cx);
    open(&workspace, "b", cx);
    assert_eq!(group_count(&workspace, cx), 1);

    let panels = panel_ids(&workspace, cx);
    let dock = cx.update(|_, cx| workspace.read(cx).dock().clone());
    let root = cx.update(|_, cx| {
        dock.read(cx)
            .layout(DockPlacement::Center)
            .unwrap()
            .root()
            .id()
    });
    cx.update(|window, cx| {
        dock.update(cx, |dock, cx| {
            let target = InsertTarget::Split {
                node: root,
                placement: Placement::Right,
                size: None,
            };
            dock.move_panel(panels[1], target, window, cx);
        });
    });
    cx.run_until_parked();
    assert_eq!(
        group_count(&workspace, cx),
        2,
        "the tab should be in its own pane"
    );
    let mut tabs = open_tabs(&workspace, cx);
    tabs.sort();
    assert_eq!(tabs, ["a", "b"]);

    let target = cx.update(|_, cx| {
        let tree = dock.read(cx).layout(DockPlacement::Center).unwrap();
        tree.find_panel_node(panels[0]).unwrap()
    });
    cx.update(|window, cx| {
        dock.update(cx, |dock, cx| {
            let target = InsertTarget::Tabs {
                node: target,
                ix: None,
                activate: true,
            };
            dock.move_panel(panels[1], target, window, cx);
        });
    });
    cx.run_until_parked();
    assert_eq!(
        group_count(&workspace, cx),
        1,
        "moving the tab back should close the empty pane"
    );
    assert_eq!(open_tabs(&workspace, cx), ["a", "b"]);
}

// Connecting

#[gpui_kit::test]
fn opening_a_disconnected_cluster_starts_no_connection(cx: &mut TestAppContext) {
    let mut harness = harness(cx, vec![status("a", ConnectionState::Disconnected, &[])]);
    let (workspace, cx) = window(cx, &harness.model, None);
    click_named("open-a".into(), cx);
    assert_eq!(open_tabs(&workspace, cx), ["a"]);
    no_commands(&mut harness);
    assert!(
        exists("tab-connect", cx),
        "the tab should offer a connect action"
    );
}

#[gpui_kit::test]
fn the_connect_action_connects_that_cluster_only(cx: &mut TestAppContext) {
    let mut harness = harness(
        cx,
        vec![
            status("a", ConnectionState::Disconnected, &[]),
            status("b", ConnectionState::Disconnected, &[]),
        ],
    );
    let (_workspace, cx) = window(cx, &harness.model, None);
    click_named("connect-b".into(), cx);
    assert_eq!(
        harness.commands.try_recv(),
        Ok(ClusterCommand::Connect("b".into()))
    );
    no_commands(&mut harness);
}

#[gpui_kit::test]
fn the_list_shows_each_clusters_state(cx: &mut TestAppContext) {
    let harness = harness(cx, vec![status("a", ConnectionState::Disconnected, &[])]);
    let (_workspace, cx) = window(cx, &harness.model, None);
    let labels = [
        (ConnectionState::Connecting, "disconnect-a"),
        (ConnectionState::Connected, "disconnect-a"),
        (ConnectionState::Failed { reason: "r".into() }, "connect-a"),
    ];
    for (state, action) in labels {
        publish(&harness, vec![status("a", state, &[])], cx);
        let found = cx.update(|window, cx| {
            window.render_frame(cx);
            window
                .try_find(gpui_kit::SharedString::from(action))
                .is_some()
        });
        assert!(found, "{action}");
    }
}

#[gpui_kit::test]
fn closing_the_last_tab_leaves_the_cluster_connected(cx: &mut TestAppContext) {
    let mut harness = harness(cx, vec![status("a", ConnectionState::Connected, &[])]);
    let (workspace, cx) = window(cx, &harness.model, None);
    let key = key("a");
    let tab = cx.update(|window, cx| workspace.update(cx, |w, cx| w.open_cluster(key, window, cx)));
    cx.run_until_parked();
    let dock = cx.update(|_, cx| workspace.read(cx).dock().clone());
    cx.update(|window, cx| dock.update(cx, |dock, cx| dock.remove_panel(tab, window, cx)));
    cx.run_until_parked();
    assert!(open_tabs(&workspace, cx).is_empty());
    no_commands(&mut harness);
}

#[gpui_kit::test]
fn disconnecting_keeps_tabs_open_with_a_connect_action(cx: &mut TestAppContext) {
    let mut harness = harness(cx, vec![status("a", ConnectionState::Connected, &[])]);
    let (workspace, cx) = window(cx, &harness.model, None);
    open(&workspace, "a", cx);
    assert!(!exists("tab-connect", cx));

    click_named("disconnect-a".into(), cx);
    assert_eq!(
        harness.commands.try_recv(),
        Ok(ClusterCommand::Disconnect("a".into()))
    );
    publish(
        &harness,
        vec![status("a", ConnectionState::Disconnected, &[])],
        cx,
    );

    assert_eq!(open_tabs(&workspace, cx), ["a"]);
    assert!(exists("tab-connect", cx));
}

#[gpui_kit::test]
fn disconnecting_with_active_work_warns_first(cx: &mut TestAppContext) {
    let mut harness = harness(
        cx,
        vec![status("a", ConnectionState::Connected, &["shell in web-1"])],
    );
    let (_workspace, cx) = window(cx, &harness.model, None);

    click_named("disconnect-a".into(), cx);
    no_commands(&mut harness);
    assert!(exists("disconnect-warning", cx));

    click("disconnect-cancel", cx);
    no_commands(&mut harness);
    assert!(!exists("disconnect-warning", cx));

    click_named("disconnect-a".into(), cx);
    click("disconnect-confirm", cx);
    assert_eq!(
        harness.commands.try_recv(),
        Ok(ClusterCommand::Disconnect("a".into()))
    );
    assert!(!exists("disconnect-warning", cx));
}

#[gpui_kit::test]
fn searching_five_hundred_clusters_narrows_the_list(cx: &mut TestAppContext) {
    let statuses = (0..500)
        .map(|i| status(&format!("cluster-{i}"), ConnectionState::Disconnected, &[]))
        .collect();
    let harness = harness(cx, statuses);
    let (workspace, cx) = window(cx, &harness.model, None);

    let panel = cx.update(|_, cx| workspace.read(cx).panel().clone());
    cx.update(|_, cx| panel.update(cx, |panel, cx| panel.set_query("cluster-499", cx)));
    cx.run_until_parked();
    let found = cx.update(|window, cx| {
        window.render_frame(cx);
        let wanted = window
            .try_find(gpui_kit::SharedString::from("connect-cluster-499"))
            .is_some();
        let other = window
            .try_find(gpui_kit::SharedString::from("connect-cluster-0"))
            .is_some();
        (wanted, other)
    });
    assert_eq!(found, (true, false));
}

// Restart

fn config(directory: &tempfile::TempDir) -> ConfigPaths {
    ConfigPaths::in_directory(directory.path())
}

#[gpui_kit::test]
fn a_restart_restores_the_layout_disconnected_and_connects_nothing(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let mut harness = harness(
        cx,
        vec![
            status("a", ConnectionState::Disconnected, &[]),
            status("b", ConnectionState::Disconnected, &[]),
        ],
    );

    let saved = {
        let (workspace, cx) = window(cx, &harness.model, None);
        open(&workspace, "a", cx);
        open(&workspace, "b", cx);
        layout(&workspace, cx)
    };
    write_atomic(
        &config(&directory).workspace,
        &serde_json::to_string(&saved).unwrap(),
    )
    .unwrap();

    let (restored, cx) = window(cx, &harness.model, Some(config(&directory)));
    assert_eq!(open_tabs(&restored, cx), ["a", "b"]);
    no_commands(&mut harness);

    let dock = cx.update(|_, cx| restored.read(cx).dock().clone());
    let panel = panel_ids(&restored, cx)[0];
    cx.update(|window, cx| dock.update(cx, |dock, cx| dock.select_panel(panel, window, cx)));
    cx.run_until_parked();
    no_commands(&mut harness);
    assert!(exists("tab-connect", cx));
}

#[gpui_kit::test]
fn a_damaged_layout_file_starts_empty(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    fs::write(&config(&directory).workspace, "not json").unwrap();
    let harness = harness(cx, vec![]);
    let (workspace, cx) = window(cx, &harness.model, Some(config(&directory)));
    assert!(open_tabs(&workspace, cx).is_empty());
}

#[gpui_kit::test]
fn a_restored_tab_refuses_to_connect_if_its_context_moved_servers(cx: &mut TestAppContext) {
    let mut harness = harness(cx, vec![status("a", ConnectionState::Disconnected, &[])]);
    let (workspace, cx) = window(cx, &harness.model, None);
    open(&workspace, "a", cx);
    let mut moved = status("a", ConnectionState::Disconnected, &[]);
    moved.entry.server = Some("https://elsewhere.example".into());
    publish(&harness, vec![moved], cx);

    assert!(exists("tab-server-changed", cx));
    assert!(!exists("tab-connect", cx));
    no_commands(&mut harness);
}

#[gpui_kit::test]
fn the_layout_saves_itself_after_changes(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = config(&directory).workspace;
    let harness = harness(cx, vec![status("a", ConnectionState::Disconnected, &[])]);
    let (workspace, cx) = window(cx, &harness.model, Some(config(&directory)));
    open(&workspace, "a", cx);
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    let saved: DockAreaState = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let mut tabs = Vec::new();
    tabs_in(&saved.center, &mut tabs);
    assert_eq!(tabs, ["a"]);
}

#[gpui_kit::test]
fn identity_changes_are_saved(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = config(&directory).clusters;
    let harness = harness(cx, vec![status("a", ConnectionState::Disconnected, &[])]);
    let (_workspace, cx) = window(cx, &harness.model, Some(config(&directory)));
    click_named("edit-a".into(), cx);
    click("toggle-protected", cx);
    let saved = IdentityStore::from_json(&fs::read_to_string(&path).unwrap()).unwrap();
    assert!(saved.identity(&key("a")).protected);
}

#[gpui_kit::test]
fn choosing_a_color_applies_to_the_cluster_and_is_saved(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = config(&directory).clusters;
    let harness = harness(cx, vec![status("a", ConnectionState::Disconnected, &[])]);
    let (_workspace, cx) = window(cx, &harness.model, Some(config(&directory)));
    click_named("edit-a".into(), cx);
    click_named("color-059669".into(), cx);

    let shown = cx.update(|_, cx| harness.model.read(cx).identity(&key("a")).color);
    assert_eq!(shown, 0x059669);
    let saved = IdentityStore::from_json(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(saved.identity(&key("a")).color, 0x059669);
}

// The background runtime

fn stalled_kubeconfig(directory: &std::path::Path, server: &str) -> PathBuf {
    let path = directory.join("config");
    let contents = format!(
        "apiVersion: v1\nkind: Config\nclusters:\n- name: a\n  cluster:\n    server: {server}\ncontexts:\n- name: a\n  context:\n    cluster: a\n    user: a\nusers:\n- name: a\n  user: {{}}\n"
    );
    fs::write(&path, contents).unwrap();
    path
}

/// A server that accepts connections and never answers.
fn stalled_server() -> (std::net::TcpListener, String) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    (listener, address)
}

#[gpui_kit::test]
fn the_ui_stays_responsive_while_a_cluster_stalls(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().unwrap();
    let (_listener, server) = stalled_server();
    let path = stalled_kubeconfig(directory.path(), &server);
    let runtime =
        ClusterRuntime::start(vec![path], SessionOptions::default(), Default::default()).unwrap();
    let commands = runtime.commands();

    let mut state = runtime.state();
    let wait = std::time::Instant::now();
    while state.borrow_and_update().clusters.is_empty() {
        assert!(
            wait.elapsed() < Duration::from_secs(10),
            "the list should load"
        );
        std::thread::sleep(Duration::from_millis(5));
    }

    let model = cx.update(|cx| {
        gpui_kit::init(cx);
        ClusterTab::register(cx);
        cx.set_global(SharedServices(Arc::new(FakeServices::idle())));
        let receiver = runtime.state();
        let model =
            cx.new(|cx| ClusterModel::new(receiver, commands, IdentityStore::default(), cx));
        cx.set_global(SharedClusterModel(model.clone()));
        model
    });
    let (workspace, cx) = window(cx, &model, None);

    let started = std::time::Instant::now();
    click_named("connect-a".into(), cx);
    while cx.update(|_, cx| model.read(cx).connection("a")) == ConnectionState::Disconnected {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the state should change"
        );
        std::thread::sleep(Duration::from_millis(5));
        cx.run_until_parked();
    }

    // The cluster is stalled, and frames and new tabs still finish promptly.
    for _ in 0..50 {
        let frame = std::time::Instant::now();
        cx.update(|window, cx| window.render_frame(cx));
        assert!(
            frame.elapsed() < Duration::from_millis(500),
            "a frame took {:?}",
            frame.elapsed()
        );
    }
    open(&workspace, "a", cx);
    assert_eq!(
        cx.update(|_, cx| model.read(cx).connection("a")),
        ConnectionState::Connecting
    );
    drop(runtime);
}
