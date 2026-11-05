//! The main window: the cluster rail, the kind sidebar, and a dock of tabs and
//! panes.

use std::{path::PathBuf, time::Duration};

use gpui_kit::{
    component::{
        ActiveTheme as _,
        dock::{
            DockArea, DockAreaState, DockEvent, DockLayout, DockPlacement, DockSkin, InsertTarget,
            NodeId, PaneRef, PanelId, panel_handle,
        },
    },
    prelude::FluentBuilder as _,
    *,
};
use kubepulse_cluster::ResourceKind;

use crate::{
    cluster_model::{ClusterModel, ClusterModelEvent},
    cluster_rail::ClusterRail,
    cluster_tab::ClusterTab,
    columns::kind_ref,
    identity::ClusterKey,
    kind_sidebar::{KindSidebar, KindSidebarEvent},
    persistence::{ConfigPaths, read_or_default, write_atomic},
    shell_state::{Placement, SharedShellState, ShellEvent, ShellState, place_kind},
};

const SAVE_DELAY: Duration = Duration::from_millis(500);
const DOCK_ID: &str = "workspace";
/// Bump when saved layouts stop being readable.
const LAYOUT_VERSION: usize = 1;

pub struct Workspace {
    model: Entity<ClusterModel>,
    shell: Entity<ShellState>,
    rail: Entity<ClusterRail>,
    sidebar: Entity<KindSidebar>,
    dock: Entity<DockArea>,
    paths: Option<ConfigPaths>,
    /// Superseded saves give up, so bursts of edits write once.
    save_generation: u64,
    _subscriptions: Vec<Subscription>,
}

impl Workspace {
    pub fn new(
        model: Entity<ClusterModel>,
        paths: Option<ConfigPaths>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let shell = cx.global::<SharedShellState>().0.clone();
        let rail = cx.new(|cx| ClusterRail::new(model.clone(), window, cx));
        let sidebar = cx.new(|cx| KindSidebar::new(model.clone(), cx));
        let (dock, _skin) = DockSkin::dock_area(DOCK_ID, Some(LAYOUT_VERSION), window, cx);

        if let Some(paths) = &paths {
            let saved: Option<DockAreaState> = read_or_default(&paths.workspace, |text| {
                serde_json::from_str(text).map(Some)
            });
            if let Some(saved) = saved.filter(|state| state.version == Some(LAYOUT_VERSION)) {
                dock.update(cx, |dock, cx| {
                    if let Err(error) = dock.load(saved, window, cx) {
                        tracing::warn!(%error, "ignoring saved layout");
                    }
                });
            }
        }

        let subscriptions = vec![
            cx.subscribe(&dock, |this, _, event: &DockEvent, cx| {
                if matches!(event, DockEvent::LayoutChanged) {
                    this.schedule_layout_save(cx);
                    cx.notify();
                }
            }),
            cx.subscribe(&shell, |this, _, event: &ShellEvent, cx| match event {
                ShellEvent::TabsChanged => this.schedule_layout_save(cx),
            }),
            cx.subscribe_in(&sidebar, window, |this, _, event, window, cx| match event {
                KindSidebarEvent::Show { key, kind, pin } => {
                    this.show_kind(key.clone(), kind.clone(), *pin, window, cx);
                }
                KindSidebarEvent::OpenInNewTab { key, kind } => {
                    this.open_tab(key.clone(), kind.clone(), true, window, cx);
                }
            }),
            cx.subscribe(
                &model,
                |this, model, event: &ClusterModelEvent, cx| match event {
                    ClusterModelEvent::IdentitiesChanged => {
                        this.save_identities(&model, cx);
                    }
                },
            ),
        ];
        Self {
            model,
            shell,
            rail,
            sidebar,
            dock,
            paths,
            save_generation: 0,
            _subscriptions: subscriptions,
        }
    }

    pub fn dock(&self) -> &Entity<DockArea> {
        &self.dock
    }

    /// Shows a kind the way a click in the sidebar does: in a tab that already
    /// shows it, or else in the preview tab.
    pub fn show_kind(
        &mut self,
        key: ClusterKey,
        kind: ResourceKind,
        pin: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ClusterTab> {
        let shell = self.shell.read(cx);
        let placement = place_kind(&shell.summaries(), shell.active(), &key, &kind_ref(&kind));
        let tab = match placement {
            Placement::Activate(id) => self.registered_tab(id, cx),
            Placement::Replace(id) => {
                let tab = self.registered_tab(id, cx);
                tab.update(cx, |tab, cx| tab.open_kind(kind, window, cx));
                tab
            }
            Placement::ReplaceAcross(id) => self.replace_across(id, key, kind, window, cx),
            Placement::Open => self.open_tab(key, kind, false, window, cx),
        };
        let panel = PanelId::from(tab.entity_id());
        self.dock
            .update(cx, |dock, cx| dock.select_panel(panel, window, cx));
        tab.update(cx, |tab, cx| {
            if pin {
                tab.pin(cx);
            }
            tab.activate(cx);
        });
        tab
    }

    fn registered_tab(&self, id: EntityId, cx: &App) -> Entity<ClusterTab> {
        self.shell
            .read(cx)
            .tab(id)
            .expect("tabs unregister on removal")
    }

    /// A tab cannot change cluster, so a new tab takes the old one's place.
    fn replace_across(
        &mut self,
        old: EntityId,
        key: ClusterKey,
        kind: ResourceKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ClusterTab> {
        let replaced = self.registered_tab(old, cx);
        let (node, ix) = self.position(PanelId::from(old), cx);
        // Gone from the directory first, so the new tab can be the preview.
        self.shell.update(cx, |shell, cx| shell.remove_tab(old, cx));
        let tab = self.open_tab(key, kind, false, window, cx);
        let panel = PanelId::from(tab.entity_id());
        self.dock.update(cx, |dock, cx| {
            let target = InsertTarget::Tabs {
                node,
                ix: Some(ix),
                activate: true,
            };
            dock.move_panel(panel, target, window, cx);
            dock.remove_panel(replaced, window, cx);
        });
        tab
    }

    fn position(&self, panel: PanelId, cx: &App) -> (NodeId, usize) {
        let dock = self.dock.read(cx);
        let tree = dock
            .layout(DockPlacement::Center)
            .expect("tabs live in the center");
        let node = tree.find_panel_node(panel).expect("the tab is in the dock");
        match tree.find_node(node).map(|node| node.kind()) {
            Some(PaneRef::Tabs { panels, .. }) => {
                let ix = panels.iter().position(|held| *held == panel);
                (node, ix.expect("the node holds the tab"))
            }
            _ => unreachable!("panels are held by tab nodes"),
        }
    }

    /// Opens a new tab bound to this cluster. It does not connect.
    pub fn open_tab(
        &mut self,
        key: ClusterKey,
        kind: ResourceKind,
        pinned: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ClusterTab> {
        let model = self.model.clone();
        let tab = cx.new(|cx| {
            let mut tab = ClusterTab::new(key, None, pinned, model, window, cx);
            tab.open_kind(kind, window, cx);
            tab
        });
        let opened = tab.clone();
        self.dock.update(cx, |dock, cx| {
            if dock.is_empty(DockPlacement::Center, cx) {
                dock.set_center(
                    DockLayout::tabs().panel_view(panel_handle(tab), cx),
                    window,
                    cx,
                );
            } else {
                dock.add_panel_view(panel_handle(tab), DockPlacement::Center, None, window, cx);
            }
        });
        opened
    }

    fn schedule_layout_save(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.paths.as_ref().map(|paths| paths.workspace.clone()) else {
            return;
        };
        self.save_generation += 1;
        let generation = self.save_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            let dump = this.update(cx, |this, cx| {
                (this.save_generation == generation).then(|| this.dock.read(cx).dump(cx))
            });
            if let Ok(Some(state)) = dump {
                save_in_background(path, serde_json::to_string_pretty(&state), cx);
            }
        })
        .detach();
    }

    fn save_identities(&self, model: &Entity<ClusterModel>, cx: &mut Context<Self>) {
        let Some(path) = self.paths.as_ref().map(|paths| paths.clusters.clone()) else {
            return;
        };
        let json = model.read(cx).identities().to_json();
        cx.spawn(async move |_, cx| save_in_background(path, json, cx))
            .detach();
    }
}

fn save_in_background(path: PathBuf, json: serde_json::Result<String>, cx: &AsyncApp) {
    let write = async move {
        let result = json.map_err(std::io::Error::other);
        if let Err(error) = result.and_then(|text| write_atomic(&path, &text)) {
            tracing::warn!(path = %path.display(), %error, "could not save settings");
        }
    };
    cx.background_executor().spawn(write).detach();
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let empty = self.dock.read(cx).is_empty(DockPlacement::Center, cx);
        div()
            .id("workspace")
            .size_full()
            .flex()
            .child(self.rail.clone())
            .child(self.sidebar.clone())
            .child(
                div()
                    .relative()
                    .flex_1()
                    .h_full()
                    .child(self.dock.clone())
                    .when(empty, |this| {
                        this.child(
                            div()
                                .id("no-tabs-hint")
                                .absolute()
                                .size_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_color(cx.theme().muted_foreground)
                                .child("Choose a kind from the sidebar.")
                                .test_support(),
                        )
                    }),
            )
    }
}
