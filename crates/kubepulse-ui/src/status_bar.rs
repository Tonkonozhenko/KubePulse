//! The bar along the bottom of the window, about the selected cluster: who it
//! is, whether it is connected, and what is running on it.

use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme as _, Icon, Sizable as _,
        button::{Button, ButtonVariants as _},
        tooltip::Tooltip,
    },
    prelude::FluentBuilder as _,
    *,
};
use kubepulse_cluster::{ConnectionState, ForwardState};
use rust_i18n::t;

use crate::{
    cluster_model::{ClusterModel, state_label},
    forwards::{ForwardRegistry, OpenPortForwards, SharedForwards},
    icons::state_mark,
    identity::ClusterIdentity,
    shell_state::{SharedShellState, ShellState},
};

/// What the bar shows about the selected cluster.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    pub identity: ClusterIdentity,
    pub state: ConnectionState,
    /// The shells and forwards a disconnect would stop.
    pub work: Vec<String>,
    pub forwards: usize,
}

impl Summary {
    pub fn state_label(&self) -> String {
        state_label(&self.state)
    }

    /// Why the cluster is not connected as it should be, as received.
    pub fn reason(&self) -> Option<&str> {
        match &self.state {
            ConnectionState::Failed { reason } | ConnectionState::Degraded { reason } => {
                Some(reason)
            }
            _ => None,
        }
    }

    pub fn stale(&self) -> bool {
        matches!(self.state, ConnectionState::Degraded { .. })
    }
}

pub struct StatusBar {
    model: Entity<ClusterModel>,
    shell: Entity<ShellState>,
    forwards: Entity<ForwardRegistry>,
    _subscriptions: Vec<Subscription>,
}

impl StatusBar {
    pub fn new(model: Entity<ClusterModel>, cx: &mut Context<Self>) -> Self {
        let shell = cx.global::<SharedShellState>().0.clone();
        let forwards = cx.global::<SharedForwards>().0.clone();
        let subscriptions = vec![
            cx.observe(&model, |_, _, cx| cx.notify()),
            cx.observe(&shell, |_, _, cx| cx.notify()),
            cx.observe(&forwards, |_, _, cx| cx.notify()),
        ];
        Self {
            model,
            shell,
            forwards,
            _subscriptions: subscriptions,
        }
    }

    /// The selected cluster's details, or `None` when nothing is selected.
    pub fn summary(&self, cx: &App) -> Option<Summary> {
        let key = self.shell.read(cx).selected()?.clone();
        let model = self.model.read(cx);
        let forwards = self
            .forwards
            .read(cx)
            .entries()
            .iter()
            .filter(|entry| entry.key == key && !matches!(entry.state, ForwardState::Failed(_)))
            .count();
        Some(Summary {
            identity: model.identity(&key),
            state: model.connection(&key.context),
            work: model
                .status(&key.context)
                .map(|status| status.active_work.clone())
                .unwrap_or_default(),
            forwards,
        })
    }
}

impl Render for StatusBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let bar = div()
            .id("status-bar")
            .w_full()
            .h_6()
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .px_2()
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.background)
            .text_xs()
            .text_color(theme.muted_foreground)
            .test_support();
        let Some(summary) = self.summary(cx) else {
            return bar;
        };
        let identity = &summary.identity;
        let cluster = div()
            .id("status-cluster")
            .flex()
            .flex_none()
            .items_center()
            .gap_1()
            .text_color(cx.theme().foreground)
            .child(div().size_2().rounded_full().bg(rgb(identity.color)))
            .child(identity.name.clone())
            .children(
                identity
                    .protected
                    .then(|| Icon::new(IconName::Shield).xsmall()),
            )
            .children(
                identity
                    .protected
                    .then(|| t!("cluster.protected_tag").to_string()),
            )
            .test_support();
        let state = div()
            .id("status-state")
            .flex()
            .flex_none()
            .items_center()
            .gap_1()
            .child(state_mark(&summary.state, cx))
            .child(summary.state_label())
            .test_support();
        let reason = summary.reason().map(|reason| {
            let full = SharedString::from(reason.to_owned());
            div()
                .id("status-reason")
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .child(full.clone())
                .tooltip(move |window, cx| Tooltip::new(full.clone()).build(window, cx))
                .test_support()
        });
        let stale = summary.stale().then(|| {
            div()
                .id("status-stale")
                .flex_none()
                .child(t!("tab.stale").to_string())
                .test_support()
        });
        let work = (!summary.work.is_empty()).then(|| {
            let list = SharedString::from(summary.work.join("\n"));
            div()
                .id("status-work")
                .flex_none()
                .child(t!("status.work", count = summary.work.len()).to_string())
                .tooltip(move |window, cx| Tooltip::new(list.clone()).build(window, cx))
                .test_support()
        });
        let forwards = (summary.forwards > 0).then(|| {
            Button::new("status-forwards")
                .icon(IconName::ArrowRightLeft)
                .label(t!("status.forwards", count = summary.forwards).to_string())
                .ghost()
                .xsmall()
                .on_click(|_, window, cx| {
                    window.dispatch_action(Box::new(OpenPortForwards), cx);
                })
        });
        bar.child(cluster)
            .child(state)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_3()
                    .children(reason)
                    .children(stale),
            )
            .children(work)
            .when_some(forwards, |this, forwards| this.child(forwards))
    }
}
