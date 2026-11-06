//! A tab bound to one cluster for its whole lifetime. Tabs hold no connection,
//! so they survive disconnection and offer a connect action. A preview tab stays
//! open only until the next kind replaces it.

use std::sync::Arc;

use gpui_kit::{
    component::{
        Icon, Sizable as _,
        dock::{self, PanelEvent},
    },
    prelude::FluentBuilder as _,
    *,
};
use kubepulse_cluster::{ObjectTarget, ResourceKind};
use kubepulse_extension_api::KindRef;

use rust_i18n::t;

use crate::{
    cluster_model::{ClusterModel, can_connect},
    columns::{columns_for, kind_icon, kind_ref, kind_title},
    detail::{DetailEvent, DetailView},
    icons::view_glyph,
    identity::ClusterKey,
    resource_browser::{BrowserEvent, ResourceBrowser},
    services::{ClusterServices, SharedServices},
    shell_state::{SharedShellState, ShellState, TabSummary, close_button, close_on_middle_click},
    tab_connection::{activate_tab, connection_notes},
};

pub const PANEL_NAME: &str = "ClusterTab";

const DETAIL_WIDTH: f32 = 420.;

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

    pub fn activate(&self, cx: &mut Context<Self>) {
        activate_tab(cx.entity_id(), &self.key, &self.model, &self.shell, cx);
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
        let (model, services) = (self.model.clone(), self.services.clone());
        self.browser = ResourceBrowser::open(&self.key, kind, columns, model, services, window, cx);
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
                DetailEvent::Logs(target) => {
                    let (key, target) = (this.key.clone(), target.clone());
                    this.shell
                        .update(cx, |shell, cx| shell.open_logs(key, target, cx));
                }
                DetailEvent::Edit(target) => {
                    let (key, target) = (this.key.clone(), target.clone());
                    this.shell
                        .update(cx, |shell, cx| shell.open_editor(key, target, cx));
                }
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

    fn closable(&self, _: &App) -> bool {
        false
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
        let title = self.kind.as_ref().map(|kind| {
            (
                kind_icon(kind, &contributions),
                kind_title(kind, &contributions),
            )
        });
        div()
            .id(SharedString::from(format!("tab-title-{}", cx.entity_id())))
            .flex()
            .items_center()
            .gap_2()
            .when(!self.pinned, |this| this.italic())
            .child(div().size_2().rounded_full().bg(rgb(identity.color)))
            .when_some(title, |this, (icon, title)| {
                this.child(Icon::new(view_glyph(icon)).xsmall())
                    .child(title)
            })
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
            .on_aux_click(close_on_middle_click(cx.entity_id(), self.shell.clone()))
            .child(close_button(cx.entity_id(), self.shell.clone()))
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
        let same_server = model.same_server(&self.key);
        let connect = can_connect(&model.connection(&self.key.context));

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
            .child(identity.name)
            .children(connection_notes(&self.key, &self.model, cx))
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
