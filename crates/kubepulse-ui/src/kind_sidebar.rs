//! The middle column: the selected cluster and the kinds it serves.

use std::{collections::HashSet, sync::Arc};

use gpui_kit::{
    component::{
        ActiveTheme as _, Icon, IconName, Sizable as _,
        button::Button,
        menu::{ContextMenuExt as _, PopupMenuItem},
    },
    prelude::FluentBuilder as _,
    *,
};
use kubepulse_cluster::{ConnectionState, ResourceKind};

use crate::{
    cluster_model::{ClusterModel, state_label},
    cluster_tab::kind_element_id,
    columns::{NavigationEntry, NavigationGroup, kind_ref, navigation},
    identity::ClusterKey,
    runtime::ClusterStatus,
    services::{ClusterServices, SharedServices},
    shell_state::{SharedShellState, ShellState},
};

const SIDEBAR_WIDTH: f32 = 240.;

pub enum KindSidebarEvent {
    Show {
        key: ClusterKey,
        kind: ResourceKind,
        pin: bool,
    },
    OpenInNewTab {
        key: ClusterKey,
        kind: ResourceKind,
    },
}

impl EventEmitter<KindSidebarEvent> for KindSidebar {}

pub struct KindSidebar {
    model: Entity<ClusterModel>,
    shell: Entity<ShellState>,
    services: Arc<dyn ClusterServices>,
    /// Groups the user flipped from how they start.
    toggled: HashSet<String>,
    _subscriptions: Vec<Subscription>,
}

impl KindSidebar {
    pub fn new(model: Entity<ClusterModel>, cx: &mut Context<Self>) -> Self {
        let shell = cx.global::<SharedShellState>().0.clone();
        let services = cx.global::<SharedServices>().0.clone();
        let subscriptions = vec![
            cx.observe(&model, |_, _, cx| cx.notify()),
            cx.observe(&shell, |_, _, cx| cx.notify()),
        ];
        Self {
            model,
            shell,
            services,
            toggled: HashSet::new(),
            _subscriptions: subscriptions,
        }
    }

    fn is_collapsed(&self, id: &str, by_default: bool) -> bool {
        by_default != self.toggled.contains(id)
    }

    fn render_group_header(
        &self,
        id: &str,
        name: String,
        collapsed: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let toggled = id.to_owned();
        div()
            .id(SharedString::from(format!("group-{id}")))
            .flex()
            .items_center()
            .gap_1()
            .cursor_pointer()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(
                Icon::new(if collapsed {
                    IconName::ChevronRight
                } else {
                    IconName::ChevronDown
                })
                .xsmall(),
            )
            .child(name)
            .on_click(cx.listener(move |this, _, _, cx| {
                if !this.toggled.remove(&toggled) {
                    this.toggled.insert(toggled.clone());
                }
                cx.notify();
            }))
            .test_support()
    }

    fn render_entry(
        &self,
        entry: NavigationEntry,
        key: &ClusterKey,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let NavigationEntry { title, kind } = entry;
        let id = kind_element_id(&kind);
        let active = self.shell.read(cx).active_tab();
        let current = active
            .is_some_and(|tab| tab.key == *key && tab.kind.as_ref() == Some(&kind_ref(&kind)));
        let selection = cx.theme().accent;
        let sidebar = cx.weak_entity();
        let (click_key, click_kind) = (key.clone(), kind.clone());
        let menu_key = key.clone();
        div()
            .id(SharedString::from(id.clone()))
            .pl_4()
            .rounded_sm()
            .cursor_pointer()
            .when(current, |this| this.bg(selection))
            .child(title)
            .when(current, |this| {
                this.child(
                    div()
                        .id(SharedString::from(format!("current-{id}")))
                        .test_support(),
                )
            })
            .on_click(cx.listener(move |_, event: &ClickEvent, _, cx| {
                cx.emit(KindSidebarEvent::Show {
                    key: click_key.clone(),
                    kind: click_kind.clone(),
                    pin: event.click_count() == 2,
                });
            }))
            .test_support()
            .context_menu(move |menu, _, _| {
                let (sidebar, key, kind) = (sidebar.clone(), menu_key.clone(), kind.clone());
                menu.item(
                    PopupMenuItem::new("Open in new tab").on_click(move |_, _, cx| {
                        let (key, kind) = (key.clone(), kind.clone());
                        if let Some(sidebar) = sidebar.upgrade() {
                            sidebar.update(cx, |_, cx| {
                                cx.emit(KindSidebarEvent::OpenInNewTab { key, kind });
                            });
                        }
                    }),
                )
            })
    }

    fn render_group(
        &self,
        group: NavigationGroup,
        key: &ClusterKey,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let NavigationGroup {
            name,
            kinds,
            sections,
        } = group;
        let collapsed = self.is_collapsed(&name, !sections.is_empty());
        let mut body = Vec::new();
        if !collapsed {
            for entry in kinds {
                body.push(self.render_entry(entry, key, cx).into_any_element());
            }
            for section in sections {
                let id = format!("{name}/{}", section.name);
                let closed = self.is_collapsed(&id, false);
                let header = self.render_group_header(&id, section.name, closed, cx);
                body.push(div().pl_2().child(header).into_any_element());
                if !closed {
                    for entry in section.kinds {
                        let row = self.render_entry(entry, key, cx);
                        body.push(div().pl_2().child(row).into_any_element());
                    }
                }
            }
        }
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(self.render_group_header(&name.clone(), name, collapsed, cx))
            .children(body)
    }

    fn render_header(
        &self,
        key: &ClusterKey,
        state: &ConnectionState,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let identity = self.model.read(cx).identity(key);
        let muted = cx.theme().muted_foreground;
        div()
            .id("sidebar-cluster")
            .flex()
            .flex_col()
            .pb_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .size_2()
                            .flex_none()
                            .rounded_full()
                            .bg(rgb(identity.color)),
                    )
                    .child(div().overflow_hidden().child(identity.name)),
            )
            .child(
                div()
                    .id("sidebar-state")
                    .text_xs()
                    .text_color(muted)
                    .child(state_label(state))
                    .test_support(),
            )
            .when(identity.protected, |this| {
                this.child(
                    div()
                        .id("sidebar-protected")
                        .text_xs()
                        .child("Protected")
                        .test_support(),
                )
            })
            .test_support()
    }

    fn render_connect(
        &self,
        id: &'static str,
        label: &'static str,
        context: String,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        Button::new(id)
            .label(label)
            .on_click(cx.listener(move |this, _, _, cx| this.model.read(cx).connect(&context)))
    }

    fn render_body(
        &self,
        key: &ClusterKey,
        status: &ClusterStatus,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let context = status.entry.context.clone();
        match &status.state {
            ConnectionState::Disconnected => {
                vec![
                    self.render_connect("sidebar-connect", "Connect", context, cx)
                        .into_any_element(),
                ]
            }
            ConnectionState::Failed { reason } => vec![
                div()
                    .id("sidebar-reason")
                    .text_xs()
                    .child(reason.clone())
                    .test_support()
                    .into_any_element(),
                self.render_connect("sidebar-retry", "Retry", context, cx)
                    .into_any_element(),
            ],
            ConnectionState::Connecting | ConnectionState::Authenticating => Vec::new(),
            ConnectionState::Connected | ConnectionState::Degraded { .. } => {
                let groups = navigation(&status.resources, &self.services.contributions());
                let stale = matches!(status.state, ConnectionState::Degraded { .. }).then(|| {
                    div()
                        .id("sidebar-stale")
                        .text_xs()
                        .child("Showing last known kinds.")
                        .test_support()
                        .into_any_element()
                });
                stale
                    .into_iter()
                    .chain(
                        groups
                            .into_iter()
                            .map(|group| self.render_group(group, key, cx).into_any_element()),
                    )
                    .collect()
            }
        }
    }
}

impl Render for KindSidebar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let model = self.model.read(cx);
        // A context can be removed or point at another server after selection.
        let selected = self.shell.read(cx).selected().and_then(|key| {
            let status = model
                .status(&key.context)
                .filter(|_| model.key(&key.context) == *key)?;
            Some((key.clone(), status.clone()))
        });
        let content: Vec<AnyElement> = match selected {
            Some((key, status)) => {
                let header = self
                    .render_header(&key, &status.state, cx)
                    .into_any_element();
                let body = self.render_body(&key, &status, cx);
                std::iter::once(header).chain(body).collect()
            }
            None => vec![
                div()
                    .id("sidebar-no-cluster")
                    .text_color(cx.theme().muted_foreground)
                    .child("Select a cluster.")
                    .test_support()
                    .into_any_element(),
            ],
        };
        div()
            .id("kind-sidebar")
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .gap_3()
            .p_2()
            .overflow_y_scroll()
            .border_r_1()
            .border_color(cx.theme().border)
            .children(content)
            .test_support()
    }
}
