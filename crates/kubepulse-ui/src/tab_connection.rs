//! What every cluster-bound tab shows and does about its connection. A tab holds
//! no connection, so these all read the cluster model.

use gpui_kit::{
    assets::IconName,
    component::{ActiveTheme as _, button::Button},
    *,
};
use kubepulse_cluster::ConnectionState;
use rust_i18n::t;

use crate::{
    cluster_model::{ClusterModel, can_connect, state_label},
    icons::state_mark,
    identity::ClusterKey,
    shell_state::ShellState,
};

/// Reports the tab as active. Two panes can show tabs at once, so a click on a
/// displayed tab counts as well as the dock's notification. A context that moved
/// to another server is a different cluster, so it is not selected.
pub fn activate_tab(
    id: EntityId,
    key: &ClusterKey,
    model: &Entity<ClusterModel>,
    shell: &Entity<ShellState>,
    cx: &mut App,
) {
    let key = model.read(cx).same_server(key).then(|| key.clone());
    shell.update(cx, |shell, cx| shell.activate(id, key, cx));
}

/// Which connection notes a tab shows.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Notes {
    /// The state, why, and the way to reconnect.
    All,
    /// Only what the user must act on in the tab, because the status bar
    /// shows the state and why.
    Actions,
}

/// The cluster's state, why it is in that state, and the way to reconnect.
pub fn connection_notes(
    key: &ClusterKey,
    model: &Entity<ClusterModel>,
    shown: Notes,
    cx: &App,
) -> Vec<AnyElement> {
    let cluster = model.read(cx);
    let state = cluster.connection(&key.context);
    let all = shown == Notes::All;
    let mut notes = Vec::new();
    if all {
        notes.push(
            div()
                .id("tab-state")
                .flex()
                .items_center()
                .gap_1()
                .text_color(cx.theme().muted_foreground)
                .child(state_mark(&state, cx))
                .child(state_label(&state))
                .into_any_element(),
        );
    }
    if !cluster.same_server(key) {
        notes.push(
            div()
                .id("tab-server-changed")
                .child(t!("tab.server_changed").to_string())
                .test_support()
                .into_any_element(),
        );
        return notes;
    }
    if all && let ConnectionState::Failed { reason } | ConnectionState::Degraded { reason } = &state
    {
        notes.push(
            div()
                .id("tab-reason")
                .child(reason.clone())
                .into_any_element(),
        );
    }
    if all && matches!(state, ConnectionState::Degraded { .. }) {
        notes.push(
            div()
                .id("tab-stale")
                .child(t!("tab.stale").to_string())
                .test_support()
                .into_any_element(),
        );
    }
    if can_connect(&state) {
        let (model, context) = (model.clone(), key.context.clone());
        notes.push(
            Button::new("tab-connect")
                .icon(IconName::Plug)
                .label(t!("cluster.connect").to_string())
                .on_click(move |_, _, cx| model.read(cx).connect(&context))
                .into_any_element(),
        );
    }
    notes
}
