//! The background half of the application. Sessions live on a Tokio runtime.
//! The UI sends commands down a channel and receives immutable, versioned
//! snapshots, so it never waits on a cluster or the disk.

use std::{path::PathBuf, sync::Arc};

use kubepulse_cluster::{
    ClusterEntry, ClusterList, ConnectionState, FileError, KubeconfigWatcher, SessionManager,
    SessionOptions, load,
};
use tokio::{
    runtime::{Builder, Runtime},
    sync::{Notify, mpsc, watch},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClusterCommand {
    Connect(String),
    Disconnect(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClusterStatus {
    pub entry: ClusterEntry,
    pub state: ConnectionState,
    /// Running shells, port-forwards, and similar work.
    pub active_work: Vec<String>,
}

/// One published view of every cluster.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClusterState {
    pub version: u64,
    pub clusters: Vec<ClusterStatus>,
    pub errors: Vec<FileError>,
}

impl ClusterState {
    pub fn status(&self, context: &str) -> Option<&ClusterStatus> {
        self.clusters
            .iter()
            .find(|status| status.entry.context == context)
    }
}

pub struct ClusterRuntime {
    commands: mpsc::UnboundedSender<ClusterCommand>,
    state: watch::Receiver<Arc<ClusterState>>,
    runtime: Option<Runtime>,
}

impl ClusterRuntime {
    /// Starts the background threads and returns immediately. Reading the
    /// kubeconfig happens on them, and no cluster is contacted.
    pub fn start(paths: Vec<PathBuf>, options: SessionOptions) -> std::io::Result<Self> {
        let runtime = Builder::new_multi_thread()
            .enable_all()
            .thread_name("kubepulse")
            .build()?;
        let (commands, receiver) = mpsc::unbounded_channel();
        let (publisher, state) = watch::channel(Arc::new(ClusterState::default()));
        runtime.spawn(run(paths, options, receiver, publisher));
        Ok(Self {
            commands,
            state,
            runtime: Some(runtime),
        })
    }

    pub fn commands(&self) -> mpsc::UnboundedSender<ClusterCommand> {
        self.commands.clone()
    }

    pub fn state(&self) -> watch::Receiver<Arc<ClusterState>> {
        self.state.clone()
    }
}

impl Drop for ClusterRuntime {
    /// Exits without waiting, because an abandoned credential helper can hold a
    /// blocking thread until it finishes.
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

async fn run(
    paths: Vec<PathBuf>,
    options: SessionOptions,
    mut commands: mpsc::UnboundedReceiver<ClusterCommand>,
    publisher: watch::Sender<Arc<ClusterState>>,
) {
    let watcher = KubeconfigWatcher::start(paths.clone());
    let (fallback, mut list) = match &watcher {
        Ok(watcher) => (None, watcher.subscribe()),
        Err(error) => {
            // The list still works without live reload.
            tracing::warn!(%error, "kubeconfig changes will not be picked up");
            let (sender, receiver) = watch::channel(Arc::new(load(&paths)));
            (Some(sender), receiver)
        }
    };
    let _fallback = fallback;

    let mut sessions = SessionManager::new(list.clone(), options);
    let changed = Arc::new(Notify::new());
    loop {
        publish(&publisher, &list.borrow(), &sessions);
        tokio::select! {
            command = commands.recv() => match command {
                None => return,
                Some(ClusterCommand::Connect(context)) => {
                    if sessions.connect(&context).is_ok() {
                        forward_changes(&sessions, &context, changed.clone());
                    }
                }
                Some(ClusterCommand::Disconnect(context)) => sessions.disconnect(&context).await,
            },
            result = list.changed() => {
                if result.is_err() {
                    return;
                }
            }
            () = changed.notified() => {}
        }
    }
}

/// Wakes the loop whenever this session's state or work changes.
fn forward_changes(sessions: &SessionManager, context: &str, changed: Arc<Notify>) {
    let Some(session) = sessions.session(context) else {
        return;
    };
    let (mut state, mut work) = (session.subscribe_state(), session.subscribe_work());
    tokio::spawn(async move {
        loop {
            let alive = tokio::select! {
                result = state.changed() => result.is_ok(),
                result = work.changed() => result.is_ok(),
            };
            changed.notify_one();
            if !alive {
                return;
            }
        }
    });
}

fn publish(
    publisher: &watch::Sender<Arc<ClusterState>>,
    list: &ClusterList,
    sessions: &SessionManager,
) {
    let clusters: Vec<ClusterStatus> = list
        .clusters
        .iter()
        .map(|entry| {
            let session = sessions.session(&entry.context);
            ClusterStatus {
                entry: entry.clone(),
                state: sessions.state(&entry.context),
                active_work: session.map_or_else(Vec::new, |s| s.subscribe_work().borrow().clone()),
            }
        })
        .collect();
    publisher.send_if_modified(|current| {
        let changed = current.clusters != clusters || current.errors != list.errors;
        if changed {
            *current = Arc::new(ClusterState {
                version: current.version + 1,
                clusters,
                errors: list.errors.clone(),
            });
        }
        changed
    });
}
