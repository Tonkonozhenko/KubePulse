//! A tab bound to one cluster for its whole lifetime. Tabs hold no connection,
//! so they survive disconnection and restart, and offer a connect action. A
//! preview tab stays open only until the next kind replaces it.

use std::sync::Arc;

use gpui_kit::{
    component::{
        ActiveTheme as _,
        button::Button,
        dock::{self, PanelBuildContext, PanelEvent, PanelInfo, PanelState, panel_handle},
    },
    prelude::FluentBuilder as _,
    *,
};
use kubepulse_cluster::{ConnectionState, ObjectTarget, ResourceKind};
use kubepulse_extension_api::KindRef;
use serde::{Deserialize, Serialize};

use rust_i18n::t;

use crate::{
    cluster_model::{ClusterModel, SharedClusterModel, can_connect, state_label},
    columns::{columns_for, kind_ref, kind_title},
    detail::{DetailEvent, DetailView},
    identity::ClusterKey,
    resource_browser::{BrowserEvent, ResourceBrowser},
    services::{ClusterServices, SharedServices},
    shell_state::{SharedShellState, ShellState, TabSummary},
};

pub const PANEL_NAME: &str = "ClusterTab";

const DETAIL_WIDTH: f32 = 420.;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClusterTabState {
    pub key: ClusterKey,
    #[serde(default)]
    pub kind: Option<KindRef>,
    #[serde(default = "pinned_by_default")]
    pub pinned: bool,
}

/// Layouts saved before preview tabs existed hold only pinned tabs.
fn pinned_by_default() -> bool {
    true
}

/// The same plural can be served by two groups, so the group is part of it.
pub fn kind_element_id(kind: &ResourceKind) -> String {
    if kind.group.is_empty() {
        format!("kind-{}", kind.plural)
    } else {
        format!("kind-{}.{}", kind.plural, kind.group)
    }
}

pub struct ClusterTab {
    key: ClusterKey,
    kind: Option<KindRef>,
    pinned: bool,
    model: Entity<ClusterModel>,
    shell: Entity<ShellState>,
    services: Arc<dyn ClusterServices>,
    browser: Option<Entity<ResourceBrowser>>,
    detail: Option<Entity<DetailView>>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl ClusterTab {
    pub fn new(
        key: ClusterKey,
        kind: Option<KindRef>,
        pinned: bool,
        model: Entity<ClusterModel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let services = cx.global::<SharedServices>().0.clone();
        let shell = cx.global::<SharedShellState>().0.clone();
        let subscription = cx.observe_in(&model, window, |this, _, window, cx| {
            this.sync_with_connection(window, cx);
            cx.notify();
        });
        Self {
            key,
            kind,
            pinned,
            model,
            shell,
            services,
            browser: None,
            detail: None,
            focus: cx.focus_handle(),
            _subscriptions: vec![subscription],
        }
    }

    pub fn key(&self) -> &ClusterKey {
        &self.key
    }

    pub fn browser(&self) -> Option<&Entity<ResourceBrowser>> {
        self.browser.as_ref()
    }

    pub fn detail(&self) -> Option<&Entity<DetailView>> {
        self.detail.as_ref()
    }

    pub fn pinned(&self) -> bool {
        self.pinned
    }

    pub fn pin(&mut self, cx: &mut Context<Self>) {
        self.pinned = true;
        self.publish(cx);
        cx.notify();
    }

    fn summary(&self, cx: &Context<Self>) -> TabSummary {
        TabSummary {
            id: cx.entity_id(),
            key: self.key.clone(),
            kind: self.kind.clone(),
            pinned: self.pinned,
        }
    }

    fn publish(&self, cx: &mut Context<Self>) {
        let summary = self.summary(cx);
        self.shell
            .update(cx, |shell, cx| shell.update_tab(summary, cx));
    }

    /// Reports this as the active tab. Two panes can show tabs at once, so a
    /// click on a displayed tab counts as well as the dock's notification.
    pub fn activate(&self, cx: &mut Context<Self>) {
        let id = cx.entity_id();
        let key = self
            .same_server(self.model.read(cx))
            .then(|| self.key.clone());
        self.shell
            .update(cx, |shell, cx| shell.activate(id, key, cx));
    }

    pub fn register(cx: &mut App) {
        dock::register_panel(cx, PANEL_NAME, |context: PanelBuildContext, window, cx| {
            let saved: Option<ClusterTabState> = match context.info() {
                PanelInfo::Panel(value) => serde_json::from_value(value.clone())
                    .inspect_err(|error| tracing::warn!(%error, "ignoring saved tab"))
                    .ok(),
                _ => None,
            };
            let (key, kind, pinned) = saved.map_or_else(
                || (ClusterKey::new("", None), None, true),
                |state| (state.key, state.kind, state.pinned),
            );
            let model = cx.global::<SharedClusterModel>().0.clone();
            panel_handle(cx.new(|cx| Self::new(key, kind, pinned, model, window, cx)))
        });
    }

    /// Whether the kubeconfig still points this context at the tab's server. If
    /// it does not, connecting would reach a different cluster.
    fn same_server(&self, model: &ClusterModel) -> bool {
        model.status(&self.key.context).is_some_and(|status| {
            status.entry.server.as_deref().unwrap_or_default() == self.key.server
        })
    }

    /// Opens the remembered kind once connected, and drops cluster data once the
    /// connection ends. A degraded cluster keeps its data, shown as stale.
    fn sync_with_connection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.model.read(cx);
        if can_connect(&model.connection(&self.key.context)) {
            self.browser = None;
            self.detail = None;
            return;
        }
        let served = model
            .status(&self.key.context)
            .map(|status| status.resources.clone());
        if self.browser.is_none()
            && let (Some(wanted), Some(served)) = (self.kind.clone(), served)
            && let Some(kind) = served.iter().find(|kind| kind_ref(kind) == wanted).cloned()
        {
            self.open_kind(kind, window, cx);
        }
    }

    pub fn open_kind(&mut self, kind: ResourceKind, window: &mut Window, cx: &mut Context<Self>) {
        let columns = columns_for(&kind, &self.services.contributions());
        self.kind = Some(kind_ref(&kind));
        self.detail = None;
        let services = self.services.clone();
        self.browser =
            ResourceBrowser::open(&self.key.context, kind, columns, services, window, cx);
        if let Some(browser) = &self.browser {
            let subscription = cx.subscribe_in(
                browser,
                window,
                |this, browser, event, window, cx| match event {
                    BrowserEvent::Open(row) => {
                        let target = ObjectTarget {
                            kind: browser.read(cx).kind().clone(),
                            namespace: row.namespace.as_deref().map(str::to_owned),
                            name: row.name.to_string(),
                        };
                        this.open_object(target, window, cx);
                    }
                },
            );
            self._subscriptions.push(subscription);
        }
        self.publish(cx);
        cx.notify();
    }

    pub fn open_object(
        &mut self,
        target: ObjectTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (key, model, services) = (self.key.clone(), self.model.clone(), self.services.clone());
        let detail = cx.new(|cx| DetailView::new(key, target, model, services, window, cx));
        let subscription =
            cx.subscribe_in(&detail, window, |this, _, event, window, cx| match event {
                DetailEvent::OpenOwner(target) => this.open_object(target.clone(), window, cx),
                DetailEvent::Deleted => {
                    this.detail = None;
                    cx.notify();
                }
            });
        self._subscriptions.push(subscription);
        self.detail = Some(detail);
        cx.notify();
    }
}

impl gpui_kit::base::dock::Panel for ClusterTab {
    fn panel_name(&self) -> &'static str {
        PANEL_NAME
    }

    fn dump(&self, _: &App) -> PanelState {
        let mut state = PanelState::new(PANEL_NAME);
        let saved = ClusterTabState {
            key: self.key.clone(),
            kind: self.kind.clone(),
            pinned: self.pinned,
        };
        state.info = PanelInfo::panel(serde_json::to_value(saved).expect("tab state serializes"));
        state
    }

    fn on_added_to(
        &mut self,
        _: WeakEntity<gpui_kit::base::dock::TabGroup>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let summary = self.summary(cx);
        let tab = cx.weak_entity();
        self.pinned = self
            .shell
            .update(cx, |shell, cx| shell.add_tab(summary, tab, cx));
    }

    fn on_removed(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let id = cx.entity_id();
        self.shell.update(cx, |shell, cx| shell.remove_tab(id, cx));
    }

    fn set_active(&mut self, active: bool, _: &mut Window, cx: &mut Context<Self>) {
        if active {
            self.activate(cx);
        }
    }
}

impl dock::Panel for ClusterTab {
    fn title(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let identity = self.model.read(cx).identity(&self.key);
        let contributions = self.services.contributions();
        let title = self
            .kind
            .as_ref()
            .map(|kind| kind_title(kind, &contributions));
        div()
            .id(SharedString::from(format!("tab-title-{}", cx.entity_id())))
            .flex()
            .items_center()
            .gap_2()
            .when(!self.pinned, |this| this.italic())
            .child(div().size_2().rounded_full().bg(rgb(identity.color)))
            .children(title)
            .child(identity.name)
            .when(identity.protected, |this| {
                this.child(t!("cluster.protected_tag").to_string())
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.activate(cx)),
            )
            .on_click(cx.listener(|this, event: &ClickEvent, _, cx| {
                if event.click_count() == 2 {
                    this.pin(cx);
                }
            }))
            .test_support()
    }
}

impl EventEmitter<PanelEvent> for ClusterTab {}

impl Focusable for ClusterTab {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ClusterTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let model = self.model.read(cx);
        let identity = model.identity(&self.key);
        let same_server = self.same_server(model);
        let state = model.connection(&self.key.context);
        let reason = match &state {
            ConnectionState::Failed { reason } | ConnectionState::Degraded { reason } => {
                Some(reason.clone())
            }
            _ => None,
        };
        let context = self.key.context.clone();
        let connect = can_connect(&state);
        let stale = matches!(state, ConnectionState::Degraded { .. });
        let muted = cx.theme().muted_foreground;

        div()
            .id("cluster-tab")
            .capture_any_mouse_down(cx.listener(|this, _, _, cx| this.activate(cx)))
            .size_full()
            .flex()
            .flex_col()
            .items_start()
            .gap_2()
            .p_2()
            .child(div().h_1().w_full().flex_none().bg(rgb(identity.color)))
            .child(
                div().flex().gap_2().child(identity.name).child(
                    div()
                        .id("tab-state")
                        .text_color(muted)
                        .child(state_label(&state)),
                ),
            )
            .map(|this| {
                if !same_server {
                    return this.child(
                        div()
                            .id("tab-server-changed")
                            .child(t!("tab.server_changed").to_string())
                            .test_support(),
                    );
                }
                this.children(reason.map(|reason| div().id("tab-reason").child(reason)))
                    .when(stale, |this| {
                        this.child(
                            div()
                                .id("tab-stale")
                                .child(t!("tab.stale").to_string())
                                .test_support(),
                        )
                    })
                    .when(connect, |this| {
                        this.child(
                            Button::new("tab-connect")
                                .label(t!("cluster.connect").to_string())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.model.read(cx).connect(&context);
                                })),
                        )
                    })
            })
            .when(same_server && !connect, |this| {
                this.child(
                    div()
                        .flex()
                        .flex_1()
                        .w_full()
                        .gap_2()
                        .overflow_hidden()
                        .child(
                            div()
                                .flex_1()
                                .h_full()
                                .overflow_hidden()
                                .children(self.browser.clone()),
                        )
                        .children(self.detail.clone().map(|detail| {
                            div().w(px(DETAIL_WIDTH)).h_full().flex_none().child(detail)
                        })),
                )
            })
    }
}
