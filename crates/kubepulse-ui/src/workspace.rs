//! The main window: the cluster list beside a dock of tabs and panes.

use std::{path::PathBuf, time::Duration};

use gpui_kit::{
    component::dock::{
        DockArea, DockAreaState, DockEvent, DockLayout, DockPlacement, DockSkin, panel_handle,
    },
    *,
};

use crate::{
    cluster_model::{ClusterModel, ClusterModelEvent},
    cluster_panel::{ClusterPanel, ClusterPanelEvent},
    cluster_tab::ClusterTab,
    identity::ClusterKey,
    persistence::{ConfigPaths, read_or_default, write_atomic},
};

const SAVE_DELAY: Duration = Duration::from_millis(500);
const DOCK_ID: &str = "workspace";
/// Bump when saved layouts stop being readable.
const LAYOUT_VERSION: usize = 1;
const PANEL_WIDTH: f32 = 320.;

pub struct Workspace {
    model: Entity<ClusterModel>,
    panel: Entity<ClusterPanel>,
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
        let panel = cx.new(|cx| ClusterPanel::new(model.clone(), window, cx));
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
                }
            }),
            cx.subscribe_in(&panel, window, |this, _, event, window, cx| match event {
                ClusterPanelEvent::Open(key) => {
                    this.open_cluster(key.clone(), window, cx);
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
            panel,
            dock,
            paths,
            save_generation: 0,
            _subscriptions: subscriptions,
        }
    }

    pub fn dock(&self) -> &Entity<DockArea> {
        &self.dock
    }

    pub fn panel(&self) -> &Entity<ClusterPanel> {
        &self.panel
    }

    /// Opens a new tab bound to this cluster. It does not connect.
    pub fn open_cluster(
        &mut self,
        key: ClusterKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ClusterTab> {
        let model = self.model.clone();
        let tab = cx.new(|cx| ClusterTab::new(key, None, model, window, cx));
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

/// Writes off the UI thread.
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
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("workspace")
            .size_full()
            .flex()
            .child(
                div()
                    .w(px(PANEL_WIDTH))
                    .h_full()
                    .flex_none()
                    .child(self.panel.clone()),
            )
            .child(div().flex_1().h_full().child(self.dock.clone()))
    }
}
