//! Tracks which clusters the user has connected. A context with no session is a
//! plain list entry that holds no client, tasks, or cache.

use std::{collections::HashMap, fmt, sync::Arc};

use tokio::sync::watch;

use crate::{
    kubeconfig::ClusterList,
    session::{ClusterSession, ConnectionState, SessionOptions},
};

#[derive(Debug, PartialEq, Eq)]
pub struct UnknownContext(pub String);

impl fmt::Display for UnknownContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "context {:?} is not in the kubeconfig", self.0)
    }
}

impl std::error::Error for UnknownContext {}

pub struct SessionManager {
    clusters: watch::Receiver<Arc<ClusterList>>,
    options: SessionOptions,
    sessions: HashMap<String, ClusterSession>,
}

impl SessionManager {
    pub fn new(clusters: watch::Receiver<Arc<ClusterList>>, options: SessionOptions) -> Self {
        Self {
            clusters,
            options,
            sessions: HashMap::new(),
        }
    }

    /// Connects `context` and no other. Does nothing if it already has an active
    /// session, and retries if the last attempt failed.
    pub fn connect(&mut self, context: &str) -> Result<(), UnknownContext> {
        let active = self
            .sessions
            .get(context)
            .is_some_and(|session| !matches!(session.state(), ConnectionState::Failed { .. }));
        if active {
            return Ok(());
        }
        let list = self.clusters.borrow().clone();
        if !list.clusters.iter().any(|entry| entry.context == context) {
            return Err(UnknownContext(context.to_owned()));
        }
        let session =
            ClusterSession::connect(context, list.kubeconfig.clone(), self.options.clone());
        self.sessions.insert(context.to_owned(), session);
        Ok(())
    }

    /// Ends the session and everything it owns. A context with no session is
    /// left as it is.
    pub async fn disconnect(&mut self, context: &str) {
        if let Some(session) = self.sessions.remove(context) {
            session.disconnect().await;
        }
    }

    pub fn state(&self, context: &str) -> ConnectionState {
        self.sessions
            .get(context)
            .map_or(ConnectionState::Disconnected, ClusterSession::state)
    }

    pub fn session(&self, context: &str) -> Option<&ClusterSession> {
        self.sessions.get(context)
    }
}
