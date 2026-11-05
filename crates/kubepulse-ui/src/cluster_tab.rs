//! A tab bound to one cluster for its whole lifetime. Tabs hold no connection,
//! so they survive disconnection and restart, and offer a connect action.

use gpui_kit::{
    component::{
        ActiveTheme as _,
        button::Button,
        dock::{self, PanelBuildContext, PanelEvent, PanelInfo, PanelState, panel_handle},
    },
    prelude::FluentBuilder as _,
    *,
};
use kubepulse_cluster::ConnectionState;
use serde::{Deserialize, Serialize};

use crate::{
    cluster_model::{ClusterModel, SharedClusterModel, state_label},
    identity::ClusterKey,
};

pub const PANEL_NAME: &str = "ClusterTab";

/// What a saved layout remembers about a tab.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClusterTabState {
    pub key: ClusterKey,
}

pub struct ClusterTab {
    key: ClusterKey,
    model: Entity<ClusterModel>,
    focus: FocusHandle,
}

impl ClusterTab {
    pub fn new(key: ClusterKey, model: Entity<ClusterModel>, cx: &mut Context<Self>) -> Self {
        cx.observe(&model, |_, _, cx| cx.notify()).detach();
        Self {
            key,
            model,
            focus: cx.focus_handle(),
        }
    }

    pub fn key(&self) -> &ClusterKey {
        &self.key
    }

    pub fn register(cx: &mut App) {
        dock::register_panel(cx, PANEL_NAME, |context: PanelBuildContext, _, cx| {
            let saved = match context.info() {
                PanelInfo::Panel(value) => serde_json::from_value(value.clone()).ok(),
                _ => None,
            };
            let key = saved.map_or_else(
                || ClusterKey::new("", None),
                |state: ClusterTabState| state.key,
            );
            let model = cx.global::<SharedClusterModel>().0.clone();
            panel_handle(cx.new(|cx| Self::new(key, model, cx)))
        });
    }

    /// Whether the kubeconfig still points this context at the tab's server. If
    /// it does not, connecting would reach a different cluster.
    fn same_server(&self, model: &ClusterModel) -> bool {
        model.status(&self.key.context).is_some_and(|status| {
            status.entry.server.as_deref().unwrap_or_default() == self.key.server
        })
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
        let context = self.key.context.clone();
        let connect = matches!(
            state,
            ConnectionState::Disconnected | ConnectionState::Failed { .. }
        );
        let muted = cx.theme().muted_foreground;

        div()
            .id("cluster-tab")
            .size_full()
            .flex()
            .flex_col()
            .items_start()
            .gap_2()
            .p_4()
            .child(div().h_1().w_full().bg(rgb(identity.color)))
            .child(identity.name)
            .map(|this| {
                if !same_server {
                    return this.child(
                        div()
                            .id("tab-server-changed")
                            .child("This context is missing or now points to another server.")
                            .test_support(),
                    );
                }
                this.child(
                    div()
                        .id("tab-state")
                        .text_color(muted)
                        .child(state_label(&state)),
                )
                .children(reason.map(|reason| div().id("tab-reason").child(reason)))
                .when(connect, |this| {
                    this.child(
                        Button::new("tab-connect")
                            .label("Connect")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.model.read(cx).connect(&context);
                            })),
                    )
                })
            })
    }
}
