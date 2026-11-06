//! The UI's copy of cluster state. Views read it and send commands through it.
//! Only an explicit connect command can start a connection, so opening tabs and
//! restoring a layout cannot.

use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};

use gpui_kit::*;
use kubepulse_cluster::ConnectionState;
use rust_i18n::t;
use tokio::sync::{mpsc, watch};

use crate::{
    identity::{ClusterIdentity, ClusterKey, ClusterSettings, IdentityStore},
    runtime::{ClusterCommand, ClusterState, ClusterStatus},
};

pub struct ClusterModel {
    state: Arc<ClusterState>,
    identities: IdentityStore,
    /// Selected namespaces by cluster, kept until exit. Empty means all of them.
    namespaces: HashMap<ClusterKey, BTreeSet<String>>,
    commands: mpsc::UnboundedSender<ClusterCommand>,
}

pub enum ClusterModelEvent {
    IdentitiesChanged,
    NamespacesChanged(ClusterKey),
}

impl EventEmitter<ClusterModelEvent> for ClusterModel {}

impl ClusterModel {
    pub fn new(
        mut state: watch::Receiver<Arc<ClusterState>>,
        commands: mpsc::UnboundedSender<ClusterCommand>,
        identities: IdentityStore,
        cx: &mut Context<Self>,
    ) -> Self {
        let initial = state.borrow_and_update().clone();
        cx.spawn(async move |this, cx| {
            while state.changed().await.is_ok() {
                let latest = state.borrow_and_update().clone();
                let updated = this.update(cx, |this, cx| {
                    this.state = latest;
                    cx.notify();
                });
                if updated.is_err() {
                    return;
                }
            }
        })
        .detach();
        Self {
            state: initial,
            identities,
            namespaces: HashMap::new(),
            commands,
        }
    }

    pub fn state(&self) -> &Arc<ClusterState> {
        &self.state
    }

    pub fn status(&self, context: &str) -> Option<&ClusterStatus> {
        self.state.status(context)
    }

    pub fn connection(&self, context: &str) -> ConnectionState {
        self.status(context)
            .map_or(ConnectionState::Disconnected, |status| status.state.clone())
    }

    pub fn connect(&self, context: &str) {
        self.send(ClusterCommand::Connect(context.to_owned()));
    }

    pub fn disconnect(&self, context: &str) {
        self.send(ClusterCommand::Disconnect(context.to_owned()));
    }

    fn send(&self, command: ClusterCommand) {
        if let Err(error) = self.commands.send(command) {
            tracing::error!(%error, "the background runtime has stopped");
        }
    }

    pub fn key(&self, context: &str) -> ClusterKey {
        let server = self
            .status(context)
            .and_then(|status| status.entry.server.as_deref());
        ClusterKey::new(context, server)
    }

    pub fn identity(&self, key: &ClusterKey) -> ClusterIdentity {
        self.identities.identity(key)
    }

    pub fn settings(&self, key: &ClusterKey) -> ClusterSettings {
        self.identities.settings(key)
    }

    pub fn identities(&self) -> &IdentityStore {
        &self.identities
    }

    pub fn update_identity(
        &mut self,
        key: ClusterKey,
        change: impl FnOnce(&mut ClusterSettings),
        cx: &mut Context<Self>,
    ) {
        self.identities.update(key, change);
        cx.emit(ClusterModelEvent::IdentitiesChanged);
        cx.notify();
    }

    pub fn selected_namespaces(&self, key: &ClusterKey) -> BTreeSet<String> {
        self.namespaces.get(key).cloned().unwrap_or_default()
    }

    pub fn set_namespaces(
        &mut self,
        key: ClusterKey,
        selected: BTreeSet<String>,
        cx: &mut Context<Self>,
    ) {
        if self.selected_namespaces(&key) != selected {
            self.namespaces.insert(key.clone(), selected);
            cx.emit(ClusterModelEvent::NamespacesChanged(key));
        }
    }
}

pub fn state_label(state: &ConnectionState) -> String {
    let label = match state {
        ConnectionState::Disconnected => t!("state.disconnected"),
        ConnectionState::Connecting => t!("state.connecting"),
        ConnectionState::Authenticating => t!("state.authenticating"),
        ConnectionState::Connected => t!("state.connected"),
        ConnectionState::Degraded { .. } => t!("state.degraded"),
        ConnectionState::Failed { .. } => t!("state.failed"),
    };
    label.to_string()
}

/// Whether a connect command would start an attempt.
pub fn can_connect(state: &ConnectionState) -> bool {
    matches!(
        state,
        ConnectionState::Disconnected | ConnectionState::Failed { .. }
    )
}

/// Whether the session is up, even if it has lost contact for now.
pub fn is_live(state: &ConnectionState) -> bool {
    matches!(
        state,
        ConnectionState::Connected | ConnectionState::Degraded { .. }
    )
}
