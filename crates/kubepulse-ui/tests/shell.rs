mod support;

use std::{fs, path::PathBuf, sync::Arc, time::Duration};

use gpui_kit::{
    AppContext as _, Entity, EntityId, MouseButton, SharedString, TestAppContext,
    VisualTestContext, WindowOptions,
    component::dock::{DockAreaState, DockPlacement, PanelId, PanelInfo, PanelState},
    test::{ClickOptions, TestWindowExt as _},
};
use kubepulse_cluster::{ClusterEntry, ConnectionState, FileError, ResourceKind, SessionOptions};
use kubepulse_extension_api::{CommandContribution, Contributions, ExtensionRequest};
use kubepulse_extension_host::ContributionSet;
use kubepulse_store::synthetic::pod_kind;
use kubepulse_ui::{
    cluster_model::ClusterModel,
    cluster_tab::ClusterTab,
    command_palette,
    identity::{ClusterKey, IdentityStore},
    persistence::ConfigPaths,
    preferences::{Appearance, Preferences, SharedPreferences},
    runtime::{ClusterCommand, ClusterRuntime, ClusterState, ClusterStatus, StoreConfig},
    services::SharedServices,
    settings_tab,
    shell_state::{SharedShellState, ShellState},
    workspace::Workspace,
};
use support::{FakeServices, ScriptedSource, kind};
use tokio::sync::{mpsc, watch};

struct Harness {
    state: watch::Sender<Arc<ClusterState>>,
    commands: mpsc::UnboundedReceiver<ClusterCommand>,
    model: Entity<ClusterModel>,
    shell: Entity<ShellState>,
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

fn serving(context: &str, resources: Vec<ResourceKind>) -> ClusterStatus {
    let mut connected = status(context, ConnectionState::Connected, &[]);
    connected.resources = Arc::new(resources);
    connected
}

fn service_kind() -> ResourceKind {
    kind("", "Service", "services", true)
}

fn secret_kind() -> ResourceKind {
    kind("", "Secret", "secrets", true)
}

fn kinds() -> Vec<ResourceKind> {
    vec![pod_kind(), service_kind(), secret_kind()]
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
    let (model, shell) = cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_reduce_motion(true);
        command_palette::init(cx);
        cx.set_global(SharedServices(Arc::new(FakeServices::idle())));
        let model = cx.new(|cx| ClusterModel::new(receiver, sender, IdentityStore::default(), cx));
        let shell = cx.new(|_| ShellState::default());
        cx.set_global(SharedShellState(shell.clone()));
        settings_tab::init(Preferences::default(), None, cx);
        (model, shell)
    });
    Harness {
        state,
        commands,
        model,
        shell,
    }
}

fn window<'a>(
    cx: &'a mut TestAppContext,
    model: &Entity<ClusterModel>,
    config: Option<ConfigPaths>,
) -> (Entity<Workspace>, &'a mut VisualTestContext) {
    let model = model.clone();
    let (handle, workspace) = cx
        .update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| Workspace::new(model, config, window, cx))
            })
        })
        .unwrap();
    let cx = VisualTestContext::from_window(handle, cx).into_mut();
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1400.), gpui_kit::px(900.)));
    cx.run_until_parked();
    (workspace, cx)
}

/// Opens a pinned tab, which never connects.
fn open(
    workspace: &Entity<Workspace>,
    context: &str,
    cx: &mut VisualTestContext,
) -> Entity<ClusterTab> {
    open_kind(workspace, context, pod_kind(), cx)
}

fn open_kind(
    workspace: &Entity<Workspace>,
    context: &str,
    kind: ResourceKind,
    cx: &mut VisualTestContext,
) -> Entity<ClusterTab> {
    let key = key(context);
    let tab = cx
        .update(|window, cx| workspace.update(cx, |w, cx| w.open_tab(key, kind, true, window, cx)));
    cx.run_until_parked();
    tab
}

fn no_commands(harness: &mut Harness) {
    assert_eq!(
        harness.commands.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    );
}

fn layout(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> DockAreaState {
    cx.update(|_, cx| workspace.read(cx).dock().read(cx).dump(cx))
}

fn open_tabs(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Vec<String> {
    tab_states(workspace, cx)
        .into_iter()
        .map(|(context, _, _)| context)
        .collect()
}

fn click(id: &str, cx: &mut VisualTestContext) {
    let id = SharedString::from(id.to_owned());
    cx.update(|window, cx| window.click(id, cx));
    cx.run_until_parked();
}

fn double_click(id: &str, cx: &mut VisualTestContext) {
    let id = SharedString::from(id.to_owned());
    cx.update(|window, cx| window.double_click(id, cx));
    cx.run_until_parked();
}

fn right_click(id: &str, cx: &mut VisualTestContext) {
    let id = SharedString::from(id.to_owned());
    cx.update(|window, cx| window.right_click(id, cx));
    cx.run_until_parked();
}

/// The labels of the open context menu's items are positional.
fn click_menu_item(index: usize, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.render_frame(cx);
        window
            .within("popup-menu")
            .click(gpui_kit::ElementId::Integer(index as u64), cx);
    });
    cx.run_until_parked();
}

fn exists(id: &str, cx: &mut VisualTestContext) -> bool {
    let id = SharedString::from(id.to_owned());
    cx.update(|window, cx| {
        window.render_frame(cx);
        window.try_find(id).is_some()
    })
}

fn publish(harness: &Harness, statuses: Vec<ClusterStatus>, cx: &mut VisualTestContext) {
    harness.state.send_replace(clusters(statuses));
    cx.run_until_parked();
}

fn selected(harness: &Harness, cx: &mut VisualTestContext) -> Option<ClusterKey> {
    cx.update(|_, cx| harness.shell.read(cx).selected().cloned())
}

fn select(harness: &Harness, context: &str, cx: &mut VisualTestContext) {
    let key = key(context);
    cx.update(|_, cx| harness.shell.update(cx, |shell, cx| shell.select(key, cx)));
    cx.run_until_parked();
}

fn active(harness: &Harness, cx: &mut VisualTestContext) -> Option<EntityId> {
    cx.update(|_, cx| harness.shell.read(cx).active())
}

fn activate_panel(
    workspace: &Entity<Workspace>,
    tab: &Entity<ClusterTab>,
    cx: &mut VisualTestContext,
) {
    let dock = cx.update(|_, cx| workspace.read(cx).dock().clone());
    let panel = PanelId::from(tab.entity_id());
    cx.update(|window, cx| dock.update(cx, |dock, cx| dock.select_panel(panel, window, cx)));
    cx.run_until_parked();
}

/// What each tab shows, as (context, kind, pinned), in layout order.
fn tab_states(
    workspace: &Entity<Workspace>,
    cx: &mut VisualTestContext,
) -> Vec<(String, String, bool)> {
    let panels = panel_ids(workspace, cx);
    cx.update(|_, cx| {
        let tabs = cx.global::<SharedShellState>().0.read(cx).summaries();
        panels
            .iter()
            .filter_map(|panel| tabs.iter().find(|tab| PanelId::from(tab.id) == *panel))
            .map(|tab| {
                let kind = tab.kind.as_ref().map(|kind| kind.kind.clone());
                (
                    tab.key.context.clone(),
                    kind.unwrap_or_default(),
                    tab.pinned,
                )
            })
            .collect()
    })
}

fn tab_state(context: &str, kind: &str, pinned: bool) -> (String, String, bool) {
    (context.to_owned(), kind.to_owned(), pinned)
}

fn expand_other_resources(cx: &mut VisualTestContext) {
    click("group-Other resources", cx);
}

fn panel_ids(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Vec<PanelId> {
    cx.update(|_, cx| {
        let dock = workspace.read(cx).dock().read(cx);
        let tree = dock.layout(DockPlacement::Center);
        tree.map(|tree| tree.panels().collect()).unwrap_or_default()
    })
}

fn group_count(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> usize {
    fn count(state: &PanelState) -> usize {
        let own = usize::from(matches!(state.info, PanelInfo::Tabs { .. }));
        own + state.children.iter().map(count).sum::<usize>()
    }
    count(&layout(workspace, cx).center)
}

/// Moves the last tab into a pane of its own.
fn split_off_last_tab(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) {
    use gpui_kit::{base::Placement, component::dock::InsertTarget};

    let panels = panel_ids(workspace, cx);
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
            dock.move_panel(*panels.last().unwrap(), target, window, cx);
        });
    });
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

#[gpui_kit::test]
fn splitting_and_moving_tabs_keeps_every_tab(cx: &mut TestAppContext) {
    use gpui_kit::component::dock::InsertTarget;

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
    split_off_last_tab(&workspace, cx);
    assert_eq!(
        group_count(&workspace, cx),
        2,
        "the tab should be in its own pane"
    );
    let mut tabs = open_tabs(&workspace, cx);
    tabs.sort();
    assert_eq!(tabs, ["a", "b"]);

    let dock = cx.update(|_, cx| workspace.read(cx).dock().clone());
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

// Three columns

#[gpui_kit::test]
fn the_columns_stay_after_every_tab_closes(cx: &mut TestAppContext) {
    let harness = harness(cx, vec![status("a", ConnectionState::Connected, &[])]);
    let (workspace, cx) = window(cx, &harness.model, None);
    let bounds = |id: &'static str, cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            window.render_frame(cx);
            window.find(id).bounds()
        })
    };
    let rail = bounds("cluster-rail", cx);
    let sidebar = bounds("kind-sidebar", cx);
    assert!(rail.origin.x < sidebar.origin.x);
    assert!(rail.size.width < sidebar.size.width);
    assert!(exists("no-tabs-hint", cx));

    let tab = open(&workspace, "a", cx);
    assert!(!exists("no-tabs-hint", cx));
    let dock = cx.update(|_, cx| workspace.read(cx).dock().clone());
    cx.update(|window, cx| dock.update(cx, |dock, cx| dock.remove_panel(tab, window, cx)));
    cx.run_until_parked();

    assert!(exists("cluster-rail", cx));
    assert!(exists("kind-sidebar", cx));
    assert!(exists("no-tabs-hint", cx));
}

#[gpui_kit::test]
fn activating_a_tab_selects_its_cluster_but_selecting_changes_no_tab(cx: &mut TestAppContext) {
    let harness = harness(
        cx,
        vec![
            status("a", ConnectionState::Disconnected, &[]),
            status("b", ConnectionState::Disconnected, &[]),
        ],
    );
    let (workspace, cx) = window(cx, &harness.model, None);
    let first = open(&workspace, "a", cx);
    let second = open(&workspace, "b", cx);
    assert_eq!(active(&harness, cx), Some(second.entity_id()));
    assert_eq!(selected(&harness, cx), Some(key("b")));

    activate_panel(&workspace, &first, cx);
    assert_eq!(selected(&harness, cx), Some(key("a")));

    click("rail-b", cx);
    assert_eq!(selected(&harness, cx), Some(key("b")));
    assert_eq!(active(&harness, cx), Some(first.entity_id()));
    assert_eq!(open_tabs(&workspace, cx), ["a", "b"]);
}

#[gpui_kit::test]
fn a_tab_whose_context_moved_or_vanished_is_not_selected(cx: &mut TestAppContext) {
    let harness = harness(
        cx,
        vec![
            status("a", ConnectionState::Disconnected, &[]),
            status("b", ConnectionState::Disconnected, &[]),
            status("c", ConnectionState::Disconnected, &[]),
        ],
    );
    let (workspace, cx) = window(cx, &harness.model, None);
    let moved = open(&workspace, "a", cx);
    let vanished = open(&workspace, "b", cx);
    let settled = open(&workspace, "c", cx);

    let mut elsewhere = status("a", ConnectionState::Disconnected, &[]);
    elsewhere.entry.server = Some("https://elsewhere.example".into());
    publish(
        &harness,
        vec![elsewhere, status("c", ConnectionState::Disconnected, &[])],
        cx,
    );

    activate_panel(&workspace, &moved, cx);
    assert_eq!(selected(&harness, cx), Some(key("c")));
    activate_panel(&workspace, &vanished, cx);
    assert_eq!(selected(&harness, cx), Some(key("c")));
    activate_panel(&workspace, &settled, cx);
    assert_eq!(selected(&harness, cx), Some(key("c")));
}

// Preview and pinned tabs

#[gpui_kit::test]
fn single_clicks_browse_in_one_preview_tab(cx: &mut TestAppContext) {
    let harness = harness(cx, vec![serving("a", kinds())]);
    let (workspace, cx) = window(cx, &harness.model, None);
    select(&harness, "a", cx);
    expand_other_resources(cx);

    click("kind-pods", cx);
    assert_eq!(tab_states(&workspace, cx), [tab_state("a", "Pod", false)]);
    click("kind-services", cx);
    click("kind-secrets", cx);
    assert_eq!(
        tab_states(&workspace, cx),
        [tab_state("a", "Secret", false)]
    );
    assert!(exists("current-kind-secrets", cx));
    assert!(!exists("current-kind-pods", cx));
}

#[gpui_kit::test]
fn a_double_click_pins_and_the_next_click_opens_a_new_preview(cx: &mut TestAppContext) {
    let harness = harness(cx, vec![serving("a", kinds())]);
    let (workspace, cx) = window(cx, &harness.model, None);
    select(&harness, "a", cx);
    expand_other_resources(cx);

    double_click("kind-pods", cx);
    assert_eq!(tab_states(&workspace, cx), [tab_state("a", "Pod", true)]);

    click("kind-services", cx);
    assert_eq!(
        tab_states(&workspace, cx),
        [
            tab_state("a", "Pod", true),
            tab_state("a", "Service", false)
        ]
    );
    assert_eq!(
        {
            click("kind-pods", cx);
            tab_states(&workspace, cx)
        },
        [
            tab_state("a", "Pod", true),
            tab_state("a", "Service", false)
        ],
        "a kind already pinned is activated, and the preview is unchanged"
    );
}

#[gpui_kit::test]
fn double_clicking_the_kind_in_the_preview_pins_it_in_place(cx: &mut TestAppContext) {
    let harness = harness(cx, vec![serving("a", kinds())]);
    let (workspace, cx) = window(cx, &harness.model, None);
    select(&harness, "a", cx);
    expand_other_resources(cx);

    click("kind-pods", cx);
    double_click("kind-pods", cx);
    assert_eq!(tab_states(&workspace, cx), [tab_state("a", "Pod", true)]);
}

#[gpui_kit::test]
fn a_tab_title_can_still_be_dragged(cx: &mut TestAppContext) {
    let harness = harness(cx, vec![serving("a", kinds())]);
    let (workspace, cx) = window(cx, &harness.model, None);
    let first = open(&workspace, "a", cx);
    let second = open_kind(&workspace, "a", service_kind(), cx);

    let (from, to) = (
        SharedString::from(format!("tab-title-{}", second.entity_id())),
        SharedString::from(format!("tab-title-{}", first.entity_id())),
    );
    cx.update(|window, cx| {
        window.render_frame(cx);
        window.drag_to(from, to, cx);
    });
    cx.run_until_parked();

    assert_eq!(
        tab_states(&workspace, cx),
        [tab_state("a", "Service", true), tab_state("a", "Pod", true)],
        "dropping on the first tab moves the second one before it"
    );
}

#[gpui_kit::test]
fn double_clicking_the_tab_title_pins_it(cx: &mut TestAppContext) {
    let harness = harness(cx, vec![serving("a", kinds())]);
    let (workspace, cx) = window(cx, &harness.model, None);
    select(&harness, "a", cx);
    expand_other_resources(cx);
    click("kind-pods", cx);

    let id = active(&harness, cx).unwrap();
    double_click(&format!("tab-title-{id}"), cx);
    assert_eq!(tab_states(&workspace, cx), [tab_state("a", "Pod", true)]);
}

#[gpui_kit::test]
fn the_preview_stays_in_its_own_pane(cx: &mut TestAppContext) {
    let harness = harness(cx, vec![serving("a", kinds())]);
    let (workspace, cx) = window(cx, &harness.model, None);
    select(&harness, "a", cx);
    expand_other_resources(cx);
    open_kind(&workspace, "a", secret_kind(), cx);
    click("kind-pods", cx);
    split_off_last_tab(&workspace, cx);
    assert_eq!(group_count(&workspace, cx), 2);
    let preview = panel_ids(&workspace, cx)[1];

    let tabs = cx.update(|_, cx| harness.shell.read(cx).summaries());
    let pinned = tabs.iter().find(|tab| tab.pinned).unwrap().id;
    let tab = cx.update(|_, cx| harness.shell.read(cx).tab(pinned).unwrap());
    activate_panel(&workspace, &tab, cx);
    cx.update(|_, cx| tab.update(cx, |tab, cx| tab.activate(cx)));

    click("kind-services", cx);
    assert_eq!(group_count(&workspace, cx), 2);
    assert_eq!(panel_ids(&workspace, cx)[1], preview);
    assert_eq!(
        tab_states(&workspace, cx),
        [
            tab_state("a", "Secret", true),
            tab_state("a", "Service", false)
        ]
    );
}

#[gpui_kit::test]
fn a_kind_on_another_cluster_takes_the_previews_position(cx: &mut TestAppContext) {
    let harness = harness(cx, vec![serving("a", kinds()), serving("b", kinds())]);
    let (workspace, cx) = window(cx, &harness.model, None);
    select(&harness, "a", cx);
    expand_other_resources(cx);
    click("kind-pods", cx);
    open_kind(&workspace, "a", secret_kind(), cx);
    assert_eq!(
        tab_states(&workspace, cx),
        [tab_state("a", "Pod", false), tab_state("a", "Secret", true)]
    );

    select(&harness, "b", cx);
    click("kind-pods", cx);
    assert_eq!(
        tab_states(&workspace, cx),
        [tab_state("b", "Pod", false), tab_state("a", "Secret", true)]
    );
    let shown = cx.update(|_, cx| harness.shell.read(cx).active_tab().cloned());
    assert_eq!(shown.unwrap().key, key("b"));
}

#[gpui_kit::test]
fn closing_the_preview_lets_a_new_one_open(cx: &mut TestAppContext) {
    let harness = harness(cx, vec![serving("a", kinds())]);
    let (workspace, cx) = window(cx, &harness.model, None);
    select(&harness, "a", cx);
    expand_other_resources(cx);
    click("kind-pods", cx);
    let id = active(&harness, cx).unwrap();
    let tab = cx.update(|_, cx| harness.shell.read(cx).tab(id).unwrap());
    let dock = cx.update(|_, cx| workspace.read(cx).dock().clone());
    cx.update(|window, cx| dock.update(cx, |dock, cx| dock.remove_panel(tab, window, cx)));
    cx.run_until_parked();
    assert!(open_tabs(&workspace, cx).is_empty());

    click("kind-services", cx);
    assert_eq!(
        tab_states(&workspace, cx),
        [tab_state("a", "Service", false)]
    );
}

#[gpui_kit::test]
fn right_clicking_a_kind_opens_a_second_tab(cx: &mut TestAppContext) {
    let harness = harness(cx, vec![serving("a", kinds())]);
    let (workspace, cx) = window(cx, &harness.model, None);
    select(&harness, "a", cx);
    expand_other_resources(cx);
    click("kind-pods", cx);

    right_click("kind-pods", cx);
    click_menu_item(0, cx);
    assert_eq!(
        tab_states(&workspace, cx),
        [tab_state("a", "Pod", false), tab_state("a", "Pod", true)]
    );
}

// Sidebar

#[gpui_kit::test]
fn groups_collapse_and_stay_collapsed_across_clusters(cx: &mut TestAppContext) {
    let widget = kind("example.com", "Widget", "widgets", true);
    let harness = harness(
        cx,
        vec![
            serving("a", kinds()),
            serving("b", vec![widget, pod_kind()]),
        ],
    );
    let (_workspace, cx) = window(cx, &harness.model, None);
    select(&harness, "a", cx);
    assert!(!exists("kind-pods", cx), "other resources start collapsed");
    expand_other_resources(cx);
    assert!(exists("kind-pods", cx));

    click("group-Other resources/core", cx);
    assert!(!exists("kind-pods", cx));
    select(&harness, "b", cx);
    assert!(!exists("kind-pods", cx));
    assert!(exists("kind-widgets.example.com", cx));
}

#[gpui_kit::test]
fn a_new_kind_keeps_the_sidebars_collapsed_groups(cx: &mut TestAppContext) {
    let harness = harness(cx, vec![serving("a", kinds())]);
    let (_workspace, cx) = window(cx, &harness.model, None);
    select(&harness, "a", cx);
    expand_other_resources(cx);
    click("group-Other resources/core", cx);

    let mut more = kinds();
    more.push(kind("example.com", "Widget", "widgets", true));
    publish(&harness, vec![serving("a", more)], cx);

    assert!(exists("kind-widgets.example.com", cx));
    assert!(!exists("kind-pods", cx));
}

#[gpui_kit::test]
fn a_new_kind_keeps_the_sidebars_scroll_position(cx: &mut TestAppContext) {
    let many = |extra: bool| -> Vec<ResourceKind> {
        let names = (0..80).map(|i| format!("kind{i:02}"));
        let names = names.chain(extra.then(|| "zzz".to_owned()));
        names
            .map(|name| kind("example.com", &name, &name, true))
            .collect()
    };
    let harness = harness(cx, vec![serving("a", many(false))]);
    let (_workspace, cx) = window(cx, &harness.model, None);
    select(&harness, "a", cx);
    expand_other_resources(cx);
    let row = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            window.render_frame(cx);
            window
                .find(SharedString::from("kind-kind40.example.com"))
                .bounds()
                .origin
                .y
        })
    };
    let top = row(cx);
    cx.update(|window, cx| {
        let delta =
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(gpui_kit::px(0.), gpui_kit::px(-300.)));
        window.scroll("kind-sidebar", delta, cx);
    });
    cx.run_until_parked();
    let scrolled = row(cx);
    assert!(scrolled < top);

    publish(&harness, vec![serving("a", many(true))], cx);
    assert_eq!(row(cx), scrolled);
}

#[gpui_kit::test]
fn the_sidebar_explains_clusters_it_cannot_list(cx: &mut TestAppContext) {
    let mut harness = harness(cx, vec![status("a", ConnectionState::Disconnected, &[])]);
    let (_workspace, cx) = window(cx, &harness.model, None);
    assert!(exists("sidebar-no-cluster", cx));

    select(&harness, "a", cx);
    assert!(exists("sidebar-connect", cx));
    no_commands(&mut harness);

    publish(
        &harness,
        vec![status("a", ConnectionState::Connecting, &[])],
        cx,
    );
    assert!(!exists("sidebar-connect", cx));
    assert!(exists("sidebar-state", cx));

    let failed = ConnectionState::Failed { reason: "r".into() };
    publish(&harness, vec![status("a", failed, &[])], cx);
    assert!(exists("sidebar-reason", cx));
    click("sidebar-retry", cx);
    assert_eq!(
        harness.commands.try_recv(),
        Ok(ClusterCommand::Connect("a".into()))
    );

    let mut degraded = serving("a", kinds());
    degraded.state = ConnectionState::Degraded { reason: "r".into() };
    publish(&harness, vec![degraded], cx);
    assert!(exists("sidebar-stale", cx));
    assert!(exists("group-Other resources", cx));
}

// Connecting

#[gpui_kit::test]
fn a_click_selects_without_connecting(cx: &mut TestAppContext) {
    let mut harness = harness(cx, vec![status("a", ConnectionState::Disconnected, &[])]);
    let (workspace, cx) = window(cx, &harness.model, None);
    click("rail-a", cx);
    assert_eq!(selected(&harness, cx), Some(key("a")));
    assert!(exists("rail-selected-a", cx));
    assert!(open_tabs(&workspace, cx).is_empty());
    no_commands(&mut harness);
}

#[gpui_kit::test]
fn a_double_click_connects_that_cluster_only(cx: &mut TestAppContext) {
    let mut harness = harness(
        cx,
        vec![
            status("a", ConnectionState::Disconnected, &[]),
            status("b", ConnectionState::Failed { reason: "r".into() }, &[]),
        ],
    );
    let (_workspace, cx) = window(cx, &harness.model, None);
    double_click("rail-a", cx);
    assert_eq!(
        harness.commands.try_recv(),
        Ok(ClusterCommand::Connect("a".into()))
    );
    no_commands(&mut harness);

    double_click("rail-b", cx);
    assert_eq!(
        harness.commands.try_recv(),
        Ok(ClusterCommand::Connect("b".into()))
    );
    no_commands(&mut harness);
}

#[gpui_kit::test]
fn a_double_click_changes_nothing_once_an_attempt_runs_or_succeeds(cx: &mut TestAppContext) {
    let mut harness = harness(
        cx,
        vec![
            status("a", ConnectionState::Connecting, &[]),
            status("b", ConnectionState::Authenticating, &[]),
            status("c", ConnectionState::Connected, &[]),
        ],
    );
    let (_workspace, cx) = window(cx, &harness.model, None);
    for context in ["rail-a", "rail-b", "rail-c"] {
        double_click(context, cx);
    }
    no_commands(&mut harness);
    assert_eq!(selected(&harness, cx), Some(key("c")));
}

#[gpui_kit::test]
fn the_rail_shows_each_state(cx: &mut TestAppContext) {
    let harness = harness(cx, vec![status("a", ConnectionState::Disconnected, &[])]);
    let (_workspace, cx) = window(cx, &harness.model, None);
    let marks = [
        (ConnectionState::Connecting, Some("rail-attempt-a")),
        (ConnectionState::Authenticating, Some("rail-attempt-a")),
        (ConnectionState::Connected, None),
        (
            ConnectionState::Failed { reason: "r".into() },
            Some("rail-failed-a"),
        ),
        (
            ConnectionState::Degraded { reason: "r".into() },
            Some("rail-degraded-a"),
        ),
    ];
    for (state, mark) in marks {
        publish(&harness, vec![status("a", state, &[])], cx);
        assert!(exists("rail-a", cx));
        for known in ["rail-attempt-a", "rail-failed-a", "rail-degraded-a"] {
            assert_eq!(exists(known, cx), mark == Some(known), "{known}");
        }
    }
}

#[gpui_kit::test]
fn a_protected_cluster_is_marked_on_the_rail_and_in_the_sidebar(cx: &mut TestAppContext) {
    let harness = harness(cx, vec![status("a", ConnectionState::Disconnected, &[])]);
    let (_workspace, cx) = window(cx, &harness.model, None);
    select(&harness, "a", cx);
    assert!(!exists("rail-protected-a", cx));
    cx.update(|_, cx| {
        harness.model.update(cx, |model, cx| {
            model.update_identity(key("a"), |settings| settings.protected = true, cx);
        });
    });
    cx.run_until_parked();
    assert!(exists("rail-protected-a", cx));
    assert!(exists("sidebar-protected", cx));
}

#[gpui_kit::test]
fn the_rail_lists_five_hundred_clusters_with_connected_ones_on_top(cx: &mut TestAppContext) {
    let mut statuses: Vec<ClusterStatus> = (0..500)
        .map(|i| status(&format!("cluster-{i}"), ConnectionState::Disconnected, &[]))
        .collect();
    statuses[499].state = ConnectionState::Connected;
    let harness = harness(cx, statuses);
    let (_workspace, cx) = window(cx, &harness.model, None);
    assert!(
        exists("rail-cluster-499", cx),
        "connected clusters come first"
    );
    assert!(exists("rail-cluster-0", cx));
    assert!(!exists("rail-cluster-400", cx), "icons are virtualized");
}

#[gpui_kit::test]
fn the_right_click_menu_offers_the_action_for_each_state(cx: &mut TestAppContext) {
    let mut harness = harness(
        cx,
        vec![
            status("a", ConnectionState::Disconnected, &[]),
            status("b", ConnectionState::Failed { reason: "r".into() }, &[]),
            status("c", ConnectionState::Connecting, &[]),
            status("d", ConnectionState::Connected, &[]),
        ],
    );
    let (_workspace, cx) = window(cx, &harness.model, None);
    select(&harness, "a", cx);
    let actions = [
        ("rail-a", ClusterCommand::Connect("a".into())),
        ("rail-b", ClusterCommand::Connect("b".into())),
        ("rail-c", ClusterCommand::Disconnect("c".into())),
        ("rail-d", ClusterCommand::Disconnect("d".into())),
    ];
    for (icon, command) in actions {
        right_click(icon, cx);
        click_menu_item(0, cx);
        assert_eq!(harness.commands.try_recv(), Ok(command));
    }
    assert_eq!(
        selected(&harness, cx),
        Some(key("a")),
        "right-clicking does not select"
    );
}

#[gpui_kit::test]
fn closing_the_last_tab_leaves_the_cluster_connected(cx: &mut TestAppContext) {
    let mut harness = harness(cx, vec![status("a", ConnectionState::Connected, &[])]);
    let (workspace, cx) = window(cx, &harness.model, None);
    let tab = open(&workspace, "a", cx);
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

    right_click("rail-a", cx);
    click_menu_item(0, cx);
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

    right_click("rail-a", cx);
    click_menu_item(0, cx);
    no_commands(&mut harness);
    assert!(exists("disconnect-warning", cx));

    click("disconnect-cancel", cx);
    no_commands(&mut harness);
    assert!(!exists("disconnect-warning", cx));

    right_click("rail-a", cx);
    click_menu_item(0, cx);
    click("disconnect-confirm", cx);
    assert_eq!(
        harness.commands.try_recv(),
        Ok(ClusterCommand::Disconnect("a".into()))
    );
    assert!(!exists("disconnect-warning", cx));
}

fn open_search(text: &str, cx: &mut VisualTestContext) {
    click("rail-search", cx);
    cx.update(|window, cx| window.input(text, cx));
    cx.run_until_parked();
}

#[gpui_kit::test]
fn searching_five_hundred_clusters_narrows_the_list(cx: &mut TestAppContext) {
    let statuses = (0..500)
        .map(|i| status(&format!("cluster-{i}"), ConnectionState::Disconnected, &[]))
        .collect();
    let mut harness = harness(cx, statuses);
    let (_workspace, cx) = window(cx, &harness.model, None);

    open_search("CLUSTER-499", cx);
    assert!(exists("search-cluster-499", cx));
    assert!(!exists("search-cluster-0", cx));

    click("search-cluster-499", cx);
    assert_eq!(selected(&harness, cx), Some(key("cluster-499")));
    assert!(!exists("cluster-search", cx), "the popover closes");
    no_commands(&mut harness);
    assert!(
        exists("rail-cluster-499", cx),
        "the rail scrolls to the icon"
    );
}

#[gpui_kit::test]
fn double_clicking_a_search_result_connects(cx: &mut TestAppContext) {
    let mut harness = harness(cx, vec![status("a", ConnectionState::Disconnected, &[])]);
    let (_workspace, cx) = window(cx, &harness.model, None);
    open_search("a", cx);
    double_click("search-a", cx);
    assert_eq!(
        harness.commands.try_recv(),
        Ok(ClusterCommand::Connect("a".into()))
    );
    assert_eq!(selected(&harness, cx), Some(key("a")));
}

#[gpui_kit::test]
fn kubeconfig_errors_appear_behind_a_warning_button(cx: &mut TestAppContext) {
    let harness = harness(cx, vec![status("a", ConnectionState::Disconnected, &[])]);
    let (_workspace, cx) = window(cx, &harness.model, None);
    assert!(!exists("rail-errors", cx));

    harness.state.send_replace(Arc::new(ClusterState {
        version: 2,
        clusters: vec![status("a", ConnectionState::Disconnected, &[])],
        errors: vec![FileError {
            path: PathBuf::from("/broken"),
            message: "not yaml".into(),
        }],
    }));
    cx.run_until_parked();

    click("rail-errors", cx);
    assert!(exists("kubeconfig-errors", cx));
}

// Starting clean

fn config(directory: &tempfile::TempDir) -> ConfigPaths {
    ConfigPaths::in_directory(directory.path())
}

#[gpui_kit::test]
fn a_new_window_starts_empty_and_ignores_an_old_layout(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let mut harness = harness(cx, vec![status("a", ConnectionState::Disconnected, &[])]);
    let old_layout = {
        let (workspace, cx) = window(cx, &harness.model, None);
        open(&workspace, "a", cx);
        serde_json::to_string(&layout(&workspace, cx)).unwrap()
    };
    fs::write(directory.path().join("workspace.json"), old_layout).unwrap();
    cx.update(|cx| {
        let shell = cx.new(|_| ShellState::default());
        harness.shell = shell.clone();
        cx.set_global(SharedShellState(shell));
    });

    let (workspace, cx) = window(cx, &harness.model, Some(config(&directory)));

    assert!(open_tabs(&workspace, cx).is_empty());
    assert!(exists("no-tabs-hint", cx));
    assert_eq!(selected(&harness, cx), None);
    no_commands(&mut harness);
}

#[gpui_kit::test]
fn changing_tabs_saves_no_layout(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let harness = harness(cx, vec![status("a", ConnectionState::Disconnected, &[])]);
    let (workspace, cx) = window(cx, &harness.model, Some(config(&directory)));
    let tab = open(&workspace, "a", cx);
    cx.update(|_, cx| tab.update(cx, |tab, cx| tab.pin(cx)));
    cx.executor().advance_clock(Duration::from_secs(5));
    cx.run_until_parked();
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
}

// Closing tabs

fn close_button(id: EntityId) -> String {
    format!("tab-close-{id}")
}

fn close(tab: &Entity<ClusterTab>, cx: &mut VisualTestContext) {
    click(&close_button(tab.entity_id()), cx);
}

#[gpui_kit::test]
fn closing_the_active_tab_activates_its_neighbor(cx: &mut TestAppContext) {
    let harness = harness(
        cx,
        vec![
            status("a", ConnectionState::Disconnected, &[]),
            status("b", ConnectionState::Disconnected, &[]),
        ],
    );
    let (workspace, cx) = window(cx, &harness.model, None);
    let first = open(&workspace, "a", cx);
    let second = open(&workspace, "b", cx);
    assert_eq!(selected(&harness, cx), Some(key("b")));

    close(&second, cx);

    assert_eq!(open_tabs(&workspace, cx), ["a"]);
    assert_eq!(active(&harness, cx), Some(first.entity_id()));
    assert_eq!(selected(&harness, cx), Some(key("a")));
}

#[gpui_kit::test]
fn closing_an_inactive_tab_leaves_the_active_tab_alone(cx: &mut TestAppContext) {
    let harness = harness(
        cx,
        vec![
            status("a", ConnectionState::Disconnected, &[]),
            status("b", ConnectionState::Disconnected, &[]),
        ],
    );
    let (workspace, cx) = window(cx, &harness.model, None);
    let first = open(&workspace, "a", cx);
    let second = open(&workspace, "b", cx);

    close(&first, cx);

    assert_eq!(open_tabs(&workspace, cx), ["b"]);
    assert_eq!(active(&harness, cx), Some(second.entity_id()));
    assert_eq!(selected(&harness, cx), Some(key("b")));
}

#[gpui_kit::test]
fn the_no_tabs_hint_is_inside_the_tab_area(cx: &mut TestAppContext) {
    let harness = harness(cx, vec![]);
    let (_workspace, cx) = window(cx, &harness.model, None);
    let (hint, rail) = cx.update(|window, cx| {
        window.render_frame(cx);
        (
            window.find("no-tabs-hint").bounds(),
            window.find("cluster-rail").bounds(),
        )
    });
    assert_eq!(hint.origin.y, rail.origin.y);
}

fn click_title(tab: &Entity<ClusterTab>, button: MouseButton, cx: &mut VisualTestContext) {
    let id = SharedString::from(format!("tab-title-{}", tab.entity_id()));
    let options = ClickOptions::new().with_button(button);
    cx.update(|window, cx| window.click_with_options(id, options, cx));
    cx.run_until_parked();
}

#[gpui_kit::test]
fn a_middle_click_on_a_tab_closes_it(cx: &mut TestAppContext) {
    let harness = harness(
        cx,
        vec![
            status("a", ConnectionState::Disconnected, &[]),
            status("b", ConnectionState::Disconnected, &[]),
        ],
    );
    let (workspace, cx) = window(cx, &harness.model, None);
    let first = open(&workspace, "a", cx);
    open(&workspace, "b", cx);

    click_title(&first, MouseButton::Middle, cx);

    assert_eq!(open_tabs(&workspace, cx), ["b"]);
}

#[gpui_kit::test]
fn a_right_click_on_a_tab_leaves_it_open(cx: &mut TestAppContext) {
    let harness = harness(cx, vec![status("a", ConnectionState::Disconnected, &[])]);
    let (workspace, cx) = window(cx, &harness.model, None);
    let tab = open(&workspace, "a", cx);

    click_title(&tab, MouseButton::Right, cx);

    assert_eq!(open_tabs(&workspace, cx), ["a"]);
}

#[gpui_kit::test]
fn the_only_tab_can_be_closed(cx: &mut TestAppContext) {
    let harness = harness(cx, vec![status("a", ConnectionState::Disconnected, &[])]);
    let (workspace, cx) = window(cx, &harness.model, None);
    let tab = open(&workspace, "a", cx);

    close(&tab, cx);

    assert!(open_tabs(&workspace, cx).is_empty());
    assert!(exists("no-tabs-hint", cx));
    assert!(exists("cluster-rail", cx));
    assert!(exists("kind-sidebar", cx));
}

#[gpui_kit::test]
fn closing_the_last_tab_of_a_pane_removes_the_pane(cx: &mut TestAppContext) {
    let harness = harness(cx, vec![status("a", ConnectionState::Disconnected, &[])]);
    let (workspace, cx) = window(cx, &harness.model, None);
    open(&workspace, "a", cx);
    let split = open(&workspace, "a", cx);
    split_off_last_tab(&workspace, cx);
    assert_eq!(group_count(&workspace, cx), 2);

    close(&split, cx);

    assert_eq!(group_count(&workspace, cx), 1);
    assert_eq!(open_tabs(&workspace, cx), ["a"]);
}

#[gpui_kit::test]
fn a_closed_preview_tab_leaves_room_for_a_new_one(cx: &mut TestAppContext) {
    let harness = harness(cx, vec![serving("a", kinds())]);
    let (workspace, cx) = window(cx, &harness.model, None);
    select(&harness, "a", cx);
    expand_other_resources(cx);
    click("kind-pods", cx);
    let preview = active(&harness, cx).unwrap();

    click(&close_button(preview), cx);
    assert!(open_tabs(&workspace, cx).is_empty());

    click("kind-pods", cx);
    assert_eq!(tab_states(&workspace, cx), [tab_state("a", "Pod", false)]);
}

#[gpui_kit::test]
fn the_settings_tab_can_be_closed_and_opened_again(cx: &mut TestAppContext) {
    let harness = harness(cx, vec![]);
    let (workspace, cx) = window(cx, &harness.model, None);
    click("rail-settings", cx);
    let tab = cx.update(|_, cx| harness.shell.read(cx).settings_tab().unwrap());

    click(&close_button(tab.entity_id()), cx);
    assert_eq!(settings_open(&workspace, cx), 0);

    click("rail-settings", cx);
    assert_eq!(settings_open(&workspace, cx), 1);
}

#[gpui_kit::test]
fn closing_a_clusters_last_tab_keeps_it_connected_and_releases_the_tab(cx: &mut TestAppContext) {
    let mut harness = harness(cx, vec![serving("a", kinds())]);
    let (workspace, cx) = window(cx, &harness.model, None);
    let tab = open(&workspace, "a", cx);
    let released = tab.downgrade();

    close(&tab, cx);
    drop(tab);
    cx.run_until_parked();

    no_commands(&mut harness);
    assert_eq!(selected(&harness, cx), Some(key("a")));
    assert_eq!(released.upgrade().map(|tab| tab.entity_id()), None);
}

#[gpui_kit::test]
fn a_tab_refuses_to_connect_if_its_context_moved_servers(cx: &mut TestAppContext) {
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

fn open_settings(cx: &mut VisualTestContext) {
    right_click("rail-a", cx);
    click_menu_item(1, cx);
}

#[gpui_kit::test]
fn identity_changes_are_saved(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = config(&directory).clusters;
    let harness = harness(cx, vec![status("a", ConnectionState::Disconnected, &[])]);
    let (_workspace, cx) = window(cx, &harness.model, Some(config(&directory)));
    open_settings(cx);
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
    open_settings(cx);
    click("color-059669", cx);

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
    let stores = StoreConfig {
        clusters: Arc::default(),
        columns: Arc::new(|_| Arc::from([])),
    };
    let runtime = ClusterRuntime::start(vec![path], SessionOptions::default(), stores).unwrap();
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
        command_palette::init(cx);
        cx.set_global(SharedServices(Arc::new(FakeServices::idle())));
        let receiver = runtime.state();
        let model =
            cx.new(|cx| ClusterModel::new(receiver, commands, IdentityStore::default(), cx));
        let shell = cx.new(|_| ShellState::default());
        cx.set_global(SharedShellState(shell));
        settings_tab::init(Preferences::default(), None, cx);
        model
    });
    let (workspace, cx) = window(cx, &model, None);

    let started = std::time::Instant::now();
    double_click("rail-a", cx);
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

fn settings_open(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> usize {
    fn count(state: &PanelState) -> usize {
        usize::from(state.panel_name == settings_tab::PANEL_NAME)
            + state.children.iter().map(count).sum::<usize>()
    }
    count(&layout(workspace, cx).center)
}

#[gpui_kit::test]
fn the_gear_button_opens_one_settings_tab(cx: &mut TestAppContext) {
    let harness = harness(cx, vec![]);
    let (workspace, cx) = window(cx, &harness.model, None);
    click("rail-settings", cx);
    assert!(exists("settings", cx));
    click("rail-settings", cx);
    assert_eq!(settings_open(&workspace, cx), 1, "a second open reuses it");
}

#[gpui_kit::test]
fn the_shortcut_opens_settings_with_nothing_focused(cx: &mut TestAppContext) {
    let harness = harness(cx, vec![]);
    let (workspace, cx) = window(cx, &harness.model, None);
    cx.simulate_keystrokes(settings_tab::SETTINGS_SHORTCUT);
    cx.run_until_parked();
    cx.simulate_keystrokes(settings_tab::SETTINGS_SHORTCUT);
    cx.run_until_parked();
    assert!(exists("settings", cx));
    assert_eq!(settings_open(&workspace, cx), 1);
}

#[gpui_kit::test]
fn an_open_settings_tab_activates_rather_than_duplicates(cx: &mut TestAppContext) {
    let harness = harness(cx, vec![serving("a", kinds())]);
    let (workspace, cx) = window(cx, &harness.model, None);
    click("rail-settings", cx);
    open_kind(&workspace, "a", pod_kind(), cx);
    assert!(!exists("settings", cx), "the kind's tab is in front");
    click("rail-settings", cx);
    assert!(exists("settings", cx));
    assert_eq!(settings_open(&workspace, cx), 1);
}

#[gpui_kit::test]
fn activating_settings_keeps_the_selected_cluster(cx: &mut TestAppContext) {
    let harness = harness(cx, vec![serving("a", kinds())]);
    let (workspace, cx) = window(cx, &harness.model, None);
    open_kind(&workspace, "a", pod_kind(), cx);
    click("rail-settings", cx);
    assert_eq!(selected(&harness, cx), Some(key("a")));
}

#[gpui_kit::test]
fn selecting_a_kind_leaves_a_settings_tab_beside_the_preview(cx: &mut TestAppContext) {
    let harness = harness(cx, vec![serving("a", kinds())]);
    let (workspace, cx) = window(cx, &harness.model, None);
    let show = |kind: ResourceKind, cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.show_kind(key("a"), kind, false, window, cx);
            });
        });
        cx.run_until_parked();
    };
    show(pod_kind(), cx);
    click("rail-settings", cx);
    show(service_kind(), cx);
    assert_eq!(settings_open(&workspace, cx), 1);
    assert_eq!(open_tabs(&workspace, cx).len(), 1, "the preview was reused");
}

#[gpui_kit::test]
fn a_preference_change_is_saved_once_and_read_back(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let config = ConfigPaths::in_directory(directory.path());
    let harness = harness(cx, vec![]);
    let (_workspace, cx) = window(cx, &harness.model, Some(config.clone()));
    let preferences = cx.update(|_, cx| cx.global::<SharedPreferences>().0.clone());
    cx.update(|_, cx| {
        preferences.update(cx, |preferences, cx| {
            preferences.change(|p| p.appearance = Appearance::Dark, cx);
            preferences.change(|p| p.appearance = Appearance::Dark, cx);
        });
    });
    cx.run_until_parked();
    let saved = fs::read_to_string(&config.preferences).unwrap();
    let saved = Preferences::from_json(&saved).unwrap();
    assert_eq!(saved.appearance, Appearance::Dark);
}

fn open_palette(text: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(command_palette::PALETTE_SHORTCUT);
    cx.run_until_parked();
    assert!(exists("command-palette", cx), "the shortcut opens it");
    cx.simulate_input(text);
    cx.run_until_parked();
}

fn confirm_palette(cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(!exists("command-palette", cx), "confirming closes it");
}

/// Services whose one extension contributes one palette command.
fn with_command(cx: &mut TestAppContext) -> Arc<FakeServices> {
    let command = CommandContribution {
        id: "hello".into(),
        title: "Say hello".into(),
    };
    let contributions = Contributions {
        commands: vec![command],
        ..Contributions::default()
    };
    let set = ContributionSet {
        extensions: vec![("example.hello".into(), contributions)],
        ..ContributionSet::default()
    };
    let services = Arc::new(FakeServices::new(Arc::new(ScriptedSource::default()), set));
    let global = services.clone();
    cx.update(|cx| cx.set_global(SharedServices(global)));
    services
}

#[gpui_kit::test]
fn the_palette_opens_a_kind_using_only_the_keyboard(cx: &mut TestAppContext) {
    let mut harness = harness(cx, vec![serving("a", kinds())]);
    let (workspace, cx) = window(cx, &harness.model, None);

    open_palette("servi", cx);
    confirm_palette(cx);

    assert_eq!(
        tab_states(&workspace, cx),
        vec![tab_state("a", "Service", false)]
    );
    assert_eq!(selected(&harness, cx), Some(key("a")));
    no_commands(&mut harness);
}

#[gpui_kit::test]
fn the_palette_lists_the_selected_cluster_first(cx: &mut TestAppContext) {
    let harness = harness(cx, vec![serving("a", kinds()), serving("b", kinds())]);
    let (workspace, cx) = window(cx, &harness.model, None);
    select(&harness, "b", cx);

    open_palette("secret", cx);
    confirm_palette(cx);

    assert_eq!(
        tab_states(&workspace, cx),
        vec![tab_state("b", "Secret", false)]
    );
}

#[gpui_kit::test]
fn the_palette_reaches_a_kind_on_another_cluster_with_the_arrow_keys(cx: &mut TestAppContext) {
    let harness = harness(cx, vec![serving("a", kinds()), serving("b", kinds())]);
    let (workspace, cx) = window(cx, &harness.model, None);
    select(&harness, "a", cx);

    open_palette("secret", cx);
    cx.simulate_keystrokes("down");
    confirm_palette(cx);

    assert_eq!(
        tab_states(&workspace, cx),
        vec![tab_state("b", "Secret", false)]
    );
}

#[gpui_kit::test]
fn the_palette_lists_no_kinds_of_a_disconnected_cluster_and_can_connect_it(
    cx: &mut TestAppContext,
) {
    let mut harness = harness(cx, vec![status("a", ConnectionState::Disconnected, &[])]);
    let (workspace, cx) = window(cx, &harness.model, None);

    open_palette("connect", cx);
    no_commands(&mut harness);
    confirm_palette(cx);

    assert_eq!(
        harness.commands.try_recv(),
        Ok(ClusterCommand::Connect("a".into()))
    );
    assert_eq!(selected(&harness, cx), Some(key("a")));
    assert!(open_tabs(&workspace, cx).is_empty());
}

#[gpui_kit::test]
fn disconnecting_from_the_palette_warns_about_active_work(cx: &mut TestAppContext) {
    let mut busy = serving("a", kinds());
    busy.active_work = vec!["shell in pod web".into()];
    let mut harness = harness(cx, vec![busy]);
    let (_workspace, cx) = window(cx, &harness.model, None);

    open_palette("disconnect", cx);
    confirm_palette(cx);

    assert!(exists("disconnect-warning", cx));
    no_commands(&mut harness);
}

#[gpui_kit::test]
fn the_palette_runs_an_extension_command(cx: &mut TestAppContext) {
    let harness = harness(cx, vec![]);
    let services = with_command(cx);
    let (_workspace, cx) = window(cx, &harness.model, None);

    open_palette("say hello", cx);
    confirm_palette(cx);

    assert_eq!(
        *services.requests.lock().unwrap(),
        vec![(
            "example.hello".to_owned(),
            ExtensionRequest::RunCommand {
                command: "hello".into()
            }
        )]
    );
}

#[gpui_kit::test]
fn escape_closes_the_palette_and_runs_nothing(cx: &mut TestAppContext) {
    let mut harness = harness(cx, vec![serving("a", kinds())]);
    let (workspace, cx) = window(cx, &harness.model, None);

    open_palette("", cx);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();

    assert!(!exists("command-palette", cx));
    assert!(open_tabs(&workspace, cx).is_empty());
    no_commands(&mut harness);
}

#[gpui_kit::test]
fn the_palette_does_not_open_over_another_dialog(cx: &mut TestAppContext) {
    let mut busy = serving("a", kinds());
    busy.active_work = vec!["shell in pod web".into()];
    let harness = harness(cx, vec![busy]);
    let (_workspace, cx) = window(cx, &harness.model, None);
    open_palette("disconnect", cx);
    confirm_palette(cx);

    cx.simulate_keystrokes(command_palette::PALETTE_SHORTCUT);
    cx.run_until_parked();

    assert!(exists("disconnect-warning", cx));
    assert!(!exists("command-palette", cx));
}
