//! The background half of the application. Sessions live on a Tokio runtime.
//! The UI sends commands down a channel and receives immutable, versioned
//! snapshots, so it never waits on a cluster or the disk.

use std::{
    collections::HashMap,
    future::Future,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use kubepulse_cluster::{
    ClusterEntry, ClusterList, ConnectionState, EventRecord, FileError, KubeconfigWatcher,
    ObjectError, ObjectTarget, ResourceKind, SessionHandle, SessionManager, SessionOptions, load,
};
use kubepulse_extension_api::{
    ClusterRef, ExtensionRequest, ExtensionResponse, HostError, HostOperation, HostResponse,
    KindRef, ObjectRef, ResourceObject,
};
use kubepulse_extension_host::{CallError, ContributionSet, ExtensionHost, HostBackend};
use kubepulse_store::{ClusterStore, ColumnResolver, StoreOptions, View, ViewQuery};
use serde_json::Value;
use tokio::{
    runtime::{Builder, Handle, Runtime},
    sync::{Notify, mpsc, watch},
};

use crate::{
    columns::{columns_for, kind_ref},
    services::{ClusterServices, Reply},
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
    pub active_work: Vec<String>,
    pub resources: Arc<Vec<ResourceKind>>,
}

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

struct ConnectedCluster {
    handle: SessionHandle,
    resources: watch::Receiver<Arc<Vec<ResourceKind>>>,
    store: Arc<ClusterStore>,
}

/// The connected clusters. Entries appear on connect and go on disconnect, and
/// dropping a store stops its watches and frees its data.
#[derive(Default)]
pub struct ConnectedClusters(Mutex<HashMap<String, Arc<ConnectedCluster>>>);

impl ConnectedClusters {
    fn get(&self, context: &str) -> Option<Arc<ConnectedCluster>> {
        self.0
            .lock()
            .expect("cluster registry lock")
            .get(context)
            .cloned()
    }

    fn insert(&self, context: String, cluster: ConnectedCluster) {
        self.0
            .lock()
            .expect("cluster registry lock")
            .insert(context, Arc::new(cluster));
    }

    fn remove(&self, context: &str) {
        self.0
            .lock()
            .expect("cluster registry lock")
            .remove(context);
    }
}

fn host_error(error: ObjectError) -> HostError {
    match error {
        ObjectError::Denied(message) => HostError::ClusterDenied { message },
        ObjectError::NotFound(message) => HostError::NotFound { message },
        other => HostError::Failed {
            message: other.to_string(),
        },
    }
}

/// Carries out extension requests. Nothing here hands an extension a credential.
pub struct ClusterBackend(pub Arc<ConnectedClusters>);

impl ClusterBackend {
    fn cluster(&self, cluster: &ClusterRef) -> Result<Arc<ConnectedCluster>, HostError> {
        self.0.get(&cluster.0).ok_or_else(|| HostError::Failed {
            message: format!("cluster {} is not connected", cluster.0),
        })
    }
}

fn find_kind(cluster: &ConnectedCluster, kind: &KindRef) -> Result<ResourceKind, HostError> {
    let resources = cluster.resources.borrow();
    resources
        .iter()
        .find(|served| kind_ref(served) == *kind)
        .cloned()
        .ok_or_else(|| HostError::NotFound {
            message: format!("the cluster does not serve {}", kind.kind),
        })
}

#[async_trait]
impl HostBackend for ClusterBackend {
    async fn execute(&self, operation: HostOperation) -> Result<HostResponse, HostError> {
        match operation {
            HostOperation::GetObject { target } => {
                let cluster = self.cluster(&target.cluster)?;
                let object_target = ObjectTarget {
                    kind: find_kind(&cluster, &target.kind)?,
                    namespace: target.namespace.clone(),
                    name: target.name.clone(),
                };
                let object = cluster
                    .handle
                    .get_object(&object_target)
                    .await
                    .map_err(host_error)?;
                Ok(HostResponse::Object {
                    object: ResourceObject {
                        reference: target,
                        object,
                    },
                })
            }
            HostOperation::ListObjects {
                cluster: name,
                kind,
                namespace,
                labels,
            } => {
                let cluster = self.cluster(&name)?;
                let served = find_kind(&cluster, &kind)?;
                let selector: Vec<String> = labels
                    .iter()
                    .map(|(key, value)| format!("{key}={value}"))
                    .collect();
                let selector = selector.join(",");
                let labels = (!selector.is_empty()).then_some(selector.as_str());
                let objects = cluster
                    .handle
                    .list_objects(&served, namespace.as_deref(), labels)
                    .await
                    .map_err(host_error)?;
                let objects = objects
                    .into_iter()
                    .map(|object| ResourceObject {
                        reference: ObjectRef {
                            cluster: name.clone(),
                            kind: kind.clone(),
                            namespace: object["metadata"]["namespace"].as_str().map(str::to_owned),
                            name: object["metadata"]["name"]
                                .as_str()
                                .unwrap_or_default()
                                .to_owned(),
                        },
                        object,
                    })
                    .collect();
                Ok(HostResponse::Objects { objects })
            }
            // Changes wait for the review flow, so nothing can bypass it.
            HostOperation::RequestChange { .. } => Err(HostError::Failed {
                message: "cluster changes are not available yet".into(),
            }),
            HostOperation::OpenSurface { .. } => Err(HostError::Failed {
                message: "surfaces are not available yet".into(),
            }),
            HostOperation::Notify { message } => {
                tracing::info!(%message, "extension notification");
                Ok(HostResponse::Done)
            }
        }
    }
}

pub struct RuntimeServices {
    clusters: Arc<ConnectedClusters>,
    host: ExtensionHost,
    runtime: Handle,
}

impl RuntimeServices {
    pub fn new(clusters: Arc<ConnectedClusters>, host: ExtensionHost, runtime: Handle) -> Self {
        Self {
            clusters,
            host,
            runtime,
        }
    }

    fn on_runtime<T: Send + 'static>(
        &self,
        work: impl Future<Output = Result<T, ObjectError>> + Send + 'static,
    ) -> Reply<T> {
        let task = self.runtime.spawn(work);
        Box::pin(async move {
            task.await
                .unwrap_or_else(|_| Err(ObjectError::Failed("the request stopped".into())))
        })
    }

    fn session(&self, context: &str) -> Result<SessionHandle, ObjectError> {
        self.clusters
            .get(context)
            .map(|c| c.handle.clone())
            .ok_or(ObjectError::NotConnected)
    }
}

impl ClusterServices for RuntimeServices {
    fn open_view(&self, context: &str, kind: &ResourceKind, query: ViewQuery) -> Option<View> {
        let cluster = self.clusters.get(context)?;
        let _runtime = self.runtime.enter();
        Some(cluster.store.view(kind, query))
    }

    fn get_object(&self, context: &str, target: ObjectTarget) -> Reply<Value> {
        let session = self.session(context);
        self.on_runtime(async move { session?.get_object(&target).await })
    }

    fn events(
        &self,
        context: &str,
        namespace: Option<String>,
        uid: String,
    ) -> Reply<Vec<EventRecord>> {
        let session = self.session(context);
        self.on_runtime(async move { session?.events_for(namespace.as_deref(), &uid).await })
    }

    fn delete_object(&self, context: &str, target: ObjectTarget) -> Reply<()> {
        let session = self.session(context);
        self.on_runtime(async move { session?.delete_object(&target).await })
    }

    fn contributions(&self) -> Arc<ContributionSet> {
        self.host.contributions().borrow().clone()
    }

    fn kind_opened(&self, kind: KindRef) {
        let host = self.host.clone();
        self.runtime.spawn(async move {
            host.trigger(&kubepulse_extension_api::ActivationTrigger::Kind { kind })
                .await;
        });
    }

    fn extension(
        &self,
        id: &str,
        request: ExtensionRequest,
    ) -> Reply<ExtensionResponse, CallError> {
        let (host, id) = (self.host.clone(), id.to_owned());
        let task = self
            .runtime
            .spawn(async move { host.request(&id, request).await });
        Box::pin(async move {
            task.await.unwrap_or_else(|_| {
                Err(CallError::Failed {
                    message: "the request stopped".to_owned(),
                })
            })
        })
    }
}

#[derive(Clone)]
pub struct StoreConfig {
    pub clusters: Arc<ConnectedClusters>,
    pub columns: ColumnResolver,
}

impl StoreConfig {
    pub fn for_host(clusters: Arc<ConnectedClusters>, host: &ExtensionHost) -> Self {
        let contributions = host.contributions();
        let columns: ColumnResolver = Arc::new(move |kind: &ResourceKind| {
            let columns = columns_for(kind, &contributions.borrow());
            columns
                .declared
                .into_iter()
                .map(|column| column.spec)
                .collect()
        });
        Self { clusters, columns }
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
    pub fn start(
        paths: Vec<PathBuf>,
        options: SessionOptions,
        stores: StoreConfig,
    ) -> std::io::Result<Self> {
        let runtime = Builder::new_multi_thread()
            .enable_all()
            .thread_name("kubepulse")
            .build()?;
        let (commands, receiver) = mpsc::unbounded_channel();
        let (publisher, state) = watch::channel(Arc::new(ClusterState::default()));
        runtime.spawn(run(paths, options, stores, receiver, publisher));
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

    pub fn handle(&self) -> Handle {
        self.runtime
            .as_ref()
            .expect("the runtime lives until drop")
            .handle()
            .clone()
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
    stores: StoreConfig,
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
                    let already_active = stores.clusters.get(&context).is_some()
                        && !matches!(sessions.state(&context), ConnectionState::Failed { .. });
                    if !already_active && sessions.connect(&context).is_ok() {
                        forward_changes(&sessions, &context, changed.clone());
                        register_store(&sessions, &context, &stores);
                    }
                }
                Some(ClusterCommand::Disconnect(context)) => {
                    stores.clusters.remove(&context);
                    sessions.disconnect(&context).await;
                }
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

fn register_store(sessions: &SessionManager, context: &str, stores: &StoreConfig) {
    let Some(session) = sessions.session(context) else {
        return;
    };
    let handle = session.handle();
    let source = Arc::new(handle.clone());
    let store = ClusterStore::new(source, stores.columns.clone(), StoreOptions::default());
    let resources = session.subscribe_resources();
    stores.clusters.insert(
        context.to_owned(),
        ConnectedCluster {
            handle,
            resources,
            store: Arc::new(store),
        },
    );
}

/// Wakes the loop whenever this session's state or work changes.
fn forward_changes(sessions: &SessionManager, context: &str, changed: Arc<Notify>) {
    let Some(session) = sessions.session(context) else {
        return;
    };
    let (mut state, mut work) = (session.subscribe_state(), session.subscribe_work());
    let mut resources = session.subscribe_resources();
    tokio::spawn(async move {
        loop {
            let alive = tokio::select! {
                result = state.changed() => result.is_ok(),
                result = work.changed() => result.is_ok(),
                result = resources.changed() => result.is_ok(),
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
                resources: session.map_or_else(Default::default, |s| {
                    s.subscribe_resources().borrow().clone()
                }),
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
