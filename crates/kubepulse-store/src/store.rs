//! One background task per cluster and resource kind holds the rows and the
//! watch. Views attach to that task, so any number of tabs share one watch.
//! Changes accumulate in batches, and each view publishes at most once per
//! frame. The watch channel keeps only the latest snapshot, so bursts lower the
//! update rate rather than falling behind.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use kubepulse_cluster::{ResourceKind, SessionWatcher, WatchEvent};
use tokio::{
    sync::{mpsc, watch},
    time::{self, Instant},
};
use tokio_util::sync::CancellationToken;

use crate::{
    query::ViewQuery,
    row::{ColumnSpec, Row},
};

const EVENT_BUFFER: usize = 1024;

/// Starts a watch. Implemented by cluster sessions, and by the synthetic
/// generator for scale tests.
pub trait WatchSource: Send + Sync + 'static {
    fn watch(&self, kind: &ResourceKind, events: mpsc::Sender<WatchEvent>, stop: CancellationToken);
}

impl WatchSource for SessionWatcher {
    fn watch(
        &self,
        kind: &ResourceKind,
        events: mpsc::Sender<WatchEvent>,
        stop: CancellationToken,
    ) {
        SessionWatcher::watch(self, kind, events, stop);
    }
}

/// Supplies the columns for a kind.
pub type ColumnResolver = Arc<dyn Fn(&ResourceKind) -> Arc<[ColumnSpec]> + Send + Sync>;

#[derive(Clone, Debug)]
pub struct StoreOptions {
    /// How long a kind's watch and cache stay after its last view closes.
    pub release_delay: Duration,
    pub frame_interval: Duration,
}

impl Default for StoreOptions {
    fn default() -> Self {
        Self {
            release_delay: Duration::from_secs(300),
            frame_interval: Duration::from_millis(16),
        }
    }
}

/// An immutable result for one view.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    /// Increases with every publication for this view.
    pub version: u64,
    pub rows: Vec<Arc<Row>>,
    /// Objects of this kind before filtering.
    pub total: usize,
    /// False until the first full list completes.
    pub synced: bool,
    /// Why the user cannot list this kind, if they cannot.
    pub denied: Option<String>,
}

enum Command {
    Attach {
        id: u64,
        query: ViewQuery,
        snapshot: watch::Sender<Arc<Snapshot>>,
    },
    SetQuery {
        id: u64,
        query: ViewQuery,
    },
    Detach {
        id: u64,
    },
}

/// One cluster's cache. Dropping it stops every watch and frees the data.
pub struct ClusterStore {
    inner: Arc<Inner>,
    stop: CancellationToken,
}

struct Inner {
    source: Arc<dyn WatchSource>,
    columns: ColumnResolver,
    options: StoreOptions,
    kinds: Mutex<HashMap<String, mpsc::UnboundedSender<Command>>>,
    next_view: AtomicU64,
}

impl ClusterStore {
    pub fn new(
        source: Arc<dyn WatchSource>,
        columns: ColumnResolver,
        options: StoreOptions,
    ) -> Self {
        let inner = Arc::new(Inner {
            source,
            columns,
            options,
            kinds: Mutex::default(),
            next_view: AtomicU64::new(0),
        });
        Self {
            inner,
            stop: CancellationToken::new(),
        }
    }

    /// Opens a view of `kind`, starting its watch if this is the first user.
    /// Must be called inside a Tokio runtime.
    pub fn view(&self, kind: &ResourceKind, query: ViewQuery) -> View {
        let id = self.inner.next_view.fetch_add(1, Ordering::Relaxed);
        let (snapshot_tx, snapshot) = watch::channel(Arc::new(Snapshot::default()));
        let key = format!("{}/{}/{}", kind.group, kind.version, kind.plural);

        // Holding the lock across the send keeps this ordered against a kind
        // releasing itself, so an attach cannot land on a task that is exiting.
        let mut kinds = self.inner.kinds.lock().expect("store lock");
        let attach = Command::Attach {
            id,
            query,
            snapshot: snapshot_tx,
        };
        let commands = kinds.entry(key.clone()).or_insert_with(|| {
            let (commands, receiver) = mpsc::unbounded_channel();
            tokio::spawn(run_kind(
                key,
                kind.clone(),
                Arc::downgrade(&self.inner),
                receiver,
                self.stop.child_token(),
            ));
            commands
        });
        let commands = commands.clone();
        commands
            .send(attach)
            .expect("a registered kind task is running");
        drop(kinds);
        View {
            id,
            commands,
            snapshot,
        }
    }
}

impl Drop for ClusterStore {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

/// A live query. Dropping it detaches from the kind's task.
pub struct View {
    id: u64,
    commands: mpsc::UnboundedSender<Command>,
    snapshot: watch::Receiver<Arc<Snapshot>>,
}

impl View {
    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.snapshot.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<Arc<Snapshot>> {
        self.snapshot.clone()
    }

    pub fn set_query(&self, query: ViewQuery) {
        let _ = self.commands.send(Command::SetQuery { id: self.id, query });
    }
}

impl Drop for View {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Detach { id: self.id });
    }
}

struct ViewState {
    query: ViewQuery,
    snapshot: watch::Sender<Arc<Snapshot>>,
    version: u64,
    dirty: bool,
}

/// The cached objects for one kind.
#[derive(Default)]
struct Cache {
    rows: HashMap<Arc<str>, Arc<Row>>,
    /// Lists in progress, by scope. Old rows stay until a list completes, and
    /// anything missing from it was deleted while the watch was down.
    lists: HashMap<Option<String>, HashMap<Arc<str>, Arc<Row>>>,
    synced: bool,
    denied: Option<String>,
}

impl Cache {
    fn apply(&mut self, event: WatchEvent, columns: &[ColumnSpec]) {
        match event {
            WatchEvent::Restarted { scope } => {
                self.lists.insert(scope, HashMap::new());
            }
            WatchEvent::Listed { scope, object } => {
                let row = Arc::new(Row::project(&object, columns));
                self.lists
                    .entry(scope)
                    .or_default()
                    .insert(row.key.clone(), row);
            }
            WatchEvent::Synced { scope } => {
                let listed = self.lists.remove(&scope).unwrap_or_default();
                match &scope {
                    None => self.rows = listed,
                    Some(namespace) => {
                        self.rows
                            .retain(|_, row| row.namespace.as_deref() != Some(namespace.as_str()));
                        self.rows.extend(listed);
                    }
                }
                self.synced = true;
            }
            WatchEvent::Applied(object) => {
                let row = Arc::new(Row::project(&object, columns));
                self.rows.insert(row.key.clone(), row);
            }
            WatchEvent::Deleted { key } => {
                self.rows.remove(key.as_str());
            }
            WatchEvent::Denied { reason } => {
                self.denied = Some(reason);
                self.synced = true;
            }
        }
    }
}

async fn run_kind(
    key: String,
    kind: ResourceKind,
    inner: Weak<Inner>,
    mut commands: mpsc::UnboundedReceiver<Command>,
    stop: CancellationToken,
) {
    let (columns, options) = {
        let Some(inner) = inner.upgrade() else { return };
        ((inner.columns)(&kind), inner.options.clone())
    };
    let (events_tx, mut events) = mpsc::channel(EVENT_BUFFER);
    if let Some(inner) = inner.upgrade() {
        inner.source.watch(&kind, events_tx, stop.clone());
    }

    let mut cache = Cache::default();
    let mut views: HashMap<u64, ViewState> = HashMap::new();
    let mut batch = Vec::with_capacity(EVENT_BUFFER);
    let mut release_at: Option<Instant> = None;
    let mut next_frame = Instant::now();
    let mut watching = true;

    loop {
        let dirty = views.values().any(|view| view.dirty);
        tokio::select! {
            biased;
            () = stop.cancelled() => break,
            command = commands.recv() => {
                match command {
                    None => break,
                    Some(Command::Attach { id, query, snapshot }) => {
                        views.insert(id, ViewState { query, snapshot, version: 0, dirty: true });
                        release_at = None;
                    }
                    Some(Command::SetQuery { id, query }) => {
                        if let Some(view) = views.get_mut(&id) {
                            view.query = query;
                            view.dirty = true;
                        }
                    }
                    Some(Command::Detach { id }) => {
                        views.remove(&id);
                        if views.is_empty() {
                            release_at = Some(Instant::now() + options.release_delay);
                        }
                    }
                }
            }
            count = events.recv_many(&mut batch, EVENT_BUFFER), if watching => {
                watching = count > 0;
                for event in batch.drain(..) {
                    cache.apply(event, &columns);
                }
                for view in views.values_mut() {
                    view.dirty = true;
                }
            }
            () = time::sleep_until(next_frame), if dirty => {
                publish(&cache, &mut views);
                next_frame = Instant::now() + options.frame_interval;
            }
            () = wait_until(release_at) => {
                let Some(inner) = inner.upgrade() else { break };
                let mut kinds = inner.kinds.lock().expect("store lock");
                if commands.is_empty() {
                    kinds.remove(&key);
                    break;
                }
                release_at = None;
            }
        }
    }
    stop.cancel();
}

async fn wait_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

fn publish(cache: &Cache, views: &mut HashMap<u64, ViewState>) {
    let rows: Vec<Arc<Row>> = cache.rows.values().cloned().collect();
    for view in views.values_mut().filter(|view| view.dirty) {
        view.dirty = false;
        view.version += 1;
        view.snapshot.send_replace(Arc::new(Snapshot {
            version: view.version,
            rows: view.query.apply(&rows),
            total: rows.len(),
            synced: cache.synced,
            denied: cache.denied.clone(),
        }));
    }
}
