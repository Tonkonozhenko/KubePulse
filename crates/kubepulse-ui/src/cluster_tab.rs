//! A tab bound to one cluster for its whole lifetime. Tabs hold no connection,
//! so they survive disconnection and restart, and offer a connect action. Once
//! connected, a tab browses the kinds the cluster serves.

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

use crate::{
    cluster_model::{ClusterModel, SharedClusterModel, state_label},
    columns::{NavigationGroup, columns_for, kind_ref, navigation},
    detail::{DetailEvent, DetailView},
    identity::ClusterKey,
    resource_browser::{BrowserEvent, ResourceBrowser},
    services::{ClusterServices, SharedServices},
};

pub const PANEL_NAME: &str = "ClusterTab";

const NAVIGATION_WIDTH: f32 = 200.;
const DETAIL_WIDTH: f32 = 420.;

/// What a saved layout remembers about a tab.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClusterTabState {
    pub key: ClusterKey,
    #[serde(default)]
    pub kind: Option<KindRef>,
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
    model: Entity<ClusterModel>,
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
        model: Entity<ClusterModel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let services = cx.global::<SharedServices>().0.clone();
        let subscription = cx.observe_in(&model, window, |this, _, window, cx| {
            this.sync_with_connection(window, cx);
            cx.notify();
        });
        Self {
            key,
            kind,
            model,
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

    pub fn register(cx: &mut App) {
        dock::register_panel(cx, PANEL_NAME, |context: PanelBuildContext, window, cx| {
            let saved: Option<ClusterTabState> = match context.info() {
                PanelInfo::Panel(value) => serde_json::from_value(value.clone()).ok(),
                _ => None,
            };
            let (key, kind) = saved.map_or_else(
                || (ClusterKey::new("", None), None),
                |state| (state.key, state.kind),
            );
            let model = cx.global::<SharedClusterModel>().0.clone();
            panel_handle(cx.new(|cx| Self::new(key, kind, model, window, cx)))
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
        let state = model.connection(&self.key.context);
        let ended = matches!(
            state,
            ConnectionState::Disconnected | ConnectionState::Failed { .. }
        );
        if ended {
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

    fn render_navigation(
        &self,
        groups: Vec<NavigationGroup>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let muted = cx.theme().muted_foreground;
        let sections = groups.into_iter().map(|group| {
            let entries = group.kinds.into_iter().map(|(title, kind)| {
                let id = kind_element_id(&kind);
                div()
                    .id(SharedString::from(id))
                    .px_1()
                    .cursor_pointer()
                    .child(title)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_kind(kind.clone(), window, cx);
                    }))
                    .test_support()
            });
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_xs().text_color(muted).child(group.name))
                .children(entries.collect::<Vec<_>>())
        });
        div()
            .id("kind-navigation")
            .w(px(NAVIGATION_WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .gap_3()
            .overflow_y_scroll()
            .children(sections.collect::<Vec<_>>())
            .test_support()
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
        };
        state.info = PanelInfo::panel(serde_json::to_value(saved).unwrap_or_default());
        state
    }
}

impl dock::Panel for ClusterTab {
    fn title(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let identity = self.model.read(cx).identity(&self.key);
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(div().size_2().rounded_full().bg(rgb(identity.color)))
            .child(identity.name)
            .when(identity.protected, |this| this.child("(protected)"))
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
        let resources = model
            .status(&self.key.context)
            .map(|status| status.resources.clone());
        let context = self.key.context.clone();
        let connect = matches!(
            state,
            ConnectionState::Disconnected | ConnectionState::Failed { .. }
        );
        let stale = matches!(state, ConnectionState::Degraded { .. });
        let muted = cx.theme().muted_foreground;
        let groups = resources
            .filter(|_| same_server && !connect)
            .map(|served| navigation(&served, &self.services.contributions()))
            .unwrap_or_default();

        div()
            .id("cluster-tab")
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
                            .child("This context is missing or now points to another server.")
                            .test_support(),
                    );
                }
                this.children(reason.map(|reason| div().id("tab-reason").child(reason)))
                    .when(stale, |this| {
                        this.child(
                            div()
                                .id("tab-stale")
                                .child("Showing last known data.")
                                .test_support(),
                        )
                    })
                    .when(connect, |this| {
                        this.child(Button::new("tab-connect").label("Connect").on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.model.read(cx).connect(&context);
                            }),
                        ))
                    })
            })
            .when(!groups.is_empty(), |this| {
                this.child(
                    div()
                        .flex()
                        .flex_1()
                        .w_full()
                        .gap_2()
                        .overflow_hidden()
                        .child(self.render_navigation(groups, cx))
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
