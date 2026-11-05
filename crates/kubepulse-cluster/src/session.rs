//! One connected cluster: its client, connection state, and task tree.
//! Cancelling the tree stops everything the session owns.

use std::{
    collections::{BTreeSet, HashMap},
    future::Future,
    sync::Arc,
    time::Duration,
};

use backon::{BackoffBuilder as _, ExponentialBuilder};
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::{
    Api, Client, Config,
    api::ListParams,
    config::{KubeConfigOptions, Kubeconfig},
    core::discovery::Scope,
    discovery::{ApiGroup, Discovery},
};
use tokio::{sync::watch, time};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::resource_watch::SessionHandle;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Authenticating,
    Connected,
    /// The connection was lost and is being retried. Data is stale.
    Degraded {
        reason: String,
    },
    Failed {
        reason: String,
    },
}

/// A resource kind the cluster serves.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ResourceKind {
    pub group: String,
    pub version: String,
    pub kind: String,
    pub plural: String,
    pub namespaced: bool,
    pub verbs: Vec<String>,
    /// Columns a custom resource definition declares.
    pub printer_columns: Vec<PrinterColumn>,
}

/// One additional printer column from a custom resource definition.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PrinterColumn {
    pub name: String,
    pub path: String,
}

#[derive(Clone, Debug)]
pub struct SessionOptions {
    /// Limit for reaching the API server once credentials are ready.
    pub connect_timeout: Duration,
    /// Limit for a credential helper, which may be waiting on a browser login.
    pub auth_timeout: Duration,
    pub health_interval: Duration,
    pub discovery_interval: Duration,
    /// Limit for one discovery refresh, which can take many requests.
    pub discovery_timeout: Duration,
    pub reconnect_min_delay: Duration,
    pub reconnect_max_delay: Duration,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(15),
            auth_timeout: Duration::from_secs(120),
            health_interval: Duration::from_secs(10),
            discovery_interval: Duration::from_secs(30),
            discovery_timeout: Duration::from_secs(30),
            reconnect_min_delay: Duration::from_secs(1),
            reconnect_max_delay: Duration::from_secs(60),
        }
    }
}

/// A session starts when the user connects and ends only when they disconnect
/// or the application exits.
pub struct ClusterSession {
    state: watch::Receiver<ConnectionState>,
    resources: watch::Receiver<Arc<Vec<ResourceKind>>>,
    client: watch::Receiver<Option<Client>>,
    work: watch::Sender<Vec<String>>,
    cancel: CancellationToken,
    tasks: TaskTracker,
}

/// Marks user-visible work, such as a shell or a port-forward, as running in a
/// session. Dropping it ends the mark.
pub struct WorkGuard {
    work: watch::Sender<Vec<String>>,
    label: String,
}

impl Drop for WorkGuard {
    fn drop(&mut self) {
        self.work.send_modify(|work| {
            if let Some(index) = work.iter().position(|label| *label == self.label) {
                work.remove(index);
            }
        });
    }
}

struct Channels {
    state: watch::Sender<ConnectionState>,
    resources: watch::Sender<Arc<Vec<ResourceKind>>>,
    client: watch::Sender<Option<Client>>,
}

impl ClusterSession {
    /// Starts connecting to `context`. Must be called inside a Tokio runtime.
    pub fn connect(context: &str, kubeconfig: Kubeconfig, options: SessionOptions) -> Self {
        let (state_tx, state) = watch::channel(ConnectionState::Connecting);
        let (resources_tx, resources) = watch::channel(Arc::new(Vec::new()));
        let (client_tx, client) = watch::channel(None);
        let channels = Channels {
            state: state_tx,
            resources: resources_tx,
            client: client_tx,
        };

        let cancel = CancellationToken::new();
        let tasks = TaskTracker::new();
        let context = context.to_owned();
        tasks.spawn(run(
            context,
            kubeconfig,
            options,
            channels,
            cancel.clone(),
            tasks.clone(),
        ));
        let (work, _) = watch::channel(Vec::new());
        Self {
            state,
            resources,
            client,
            work,
            cancel,
            tasks,
        }
    }

    pub fn state(&self) -> ConnectionState {
        self.state.borrow().clone()
    }

    pub fn subscribe_state(&self) -> watch::Receiver<ConnectionState> {
        self.state.clone()
    }

    /// Resource kinds from the latest successful discovery.
    pub fn subscribe_resources(&self) -> watch::Receiver<Arc<Vec<ResourceKind>>> {
        self.resources.clone()
    }

    /// The client, once the session has connected.
    pub fn client(&self) -> Option<Client> {
        self.client.borrow().clone()
    }

    /// Records running work, so the user can be warned before disconnecting.
    pub fn track_work(&self, label: &str) -> WorkGuard {
        self.work.send_modify(|work| work.push(label.to_owned()));
        WorkGuard {
            work: self.work.clone(),
            label: label.to_owned(),
        }
    }

    /// Labels of the work running now.
    pub fn subscribe_work(&self) -> watch::Receiver<Vec<String>> {
        self.work.subscribe()
    }

    /// A handle for watches and object requests in this session.
    pub fn handle(&self) -> SessionHandle {
        SessionHandle::new(self.client.clone(), self.cancel.clone(), self.tasks.clone())
    }

    /// Runs `task` in this session's tree. It stops when the session disconnects.
    pub fn spawn<F>(&self, task: impl FnOnce(CancellationToken) -> F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let token = self.cancel.child_token();
        let future = task(token.clone());
        self.tasks.spawn(async move {
            token.run_until_cancelled(future).await;
        });
    }

    /// Stops everything the session owns and waits for it to finish.
    ///
    /// A credential helper already running keeps running until it exits, because
    /// the client library gives no handle to stop it. The session no longer
    /// waits for it and ignores its result.
    pub async fn disconnect(self) {
        self.cancel.cancel();
        self.tasks.close();
        self.tasks.wait().await;
    }
}

impl Drop for ClusterSession {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

async fn run(
    context: String,
    kubeconfig: Kubeconfig,
    options: SessionOptions,
    channels: Channels,
    cancel: CancellationToken,
    tasks: TaskTracker,
) {
    let established = cancel
        .run_until_cancelled(establish(&context, kubeconfig, &options, &channels.state))
        .await;
    let client = match established {
        None => return,
        Some(Err(reason)) => {
            tracing::info!(%context, "connection failed");
            channels
                .state
                .send_replace(ConnectionState::Failed { reason });
            return;
        }
        Some(Ok(client)) => client,
    };

    channels.client.send_replace(Some(client.clone()));
    channels.state.send_replace(ConnectionState::Connected);
    tracing::info!(%context, "connected");

    let state = channels.state.subscribe();
    let (discovery_client, resources, token) = (client.clone(), channels.resources, cancel.clone());
    let refresh_options = options.clone();
    tasks.spawn(async move {
        token
            .run_until_cancelled(keep_resources_current(
                discovery_client,
                state,
                resources,
                refresh_options,
            ))
            .await;
    });

    cancel
        .run_until_cancelled(monitor(&context, &client, &options, &channels.state))
        .await;
}

async fn establish(
    context: &str,
    kubeconfig: Kubeconfig,
    options: &SessionOptions,
    state: &watch::Sender<ConnectionState>,
) -> Result<Client, String> {
    state.send_replace(ConnectionState::Connecting);
    let uses_helper = uses_credential_helper(context, &kubeconfig);
    let config_options = KubeConfigOptions {
        context: Some(context.to_owned()),
        ..Default::default()
    };
    let config = Config::from_custom_kubeconfig(kubeconfig, &config_options)
        .await
        .map_err(|_| "the kubeconfig entry for this cluster is not usable".to_owned())?;

    if uses_helper {
        state.send_replace(ConnectionState::Authenticating);
    }
    // Building the client runs any credential helper and blocks until it exits.
    let build = tokio::task::spawn_blocking(move || Client::try_from(config));
    let client = time::timeout(options.auth_timeout, build)
        .await
        .map_err(|_| "the credential helper timed out".to_owned())?
        .map_err(|_| "the credential helper stopped unexpectedly".to_owned())?
        .map_err(|error| describe(&error))?;

    state.send_replace(ConnectionState::Connecting);
    time::timeout(options.connect_timeout, probe(&client))
        .await
        .map_err(|_| "the API server did not respond in time".to_owned())??;
    Ok(client)
}

fn uses_credential_helper(context: &str, kubeconfig: &Kubeconfig) -> bool {
    let user = kubeconfig
        .contexts
        .iter()
        .find(|named| named.name == context)
        .and_then(|named| named.context.as_ref()?.user.clone());
    kubeconfig
        .auth_infos
        .iter()
        .find(|named| Some(&named.name) == user.as_ref())
        .and_then(|named| named.auth_info.as_ref())
        .is_some_and(|auth| auth.exec.is_some() || auth.auth_provider.is_some())
}

/// Confirms the API server answers. Being refused access to `/version` still
/// proves it is reachable, and authorization failures are normal states.
async fn probe(client: &Client) -> Result<(), String> {
    match client.apiserver_version().await {
        Ok(_) => Ok(()),
        Err(kube::Error::Api(status)) if matches!(status.code, 403 | 404) => Ok(()),
        Err(error) => Err(describe(&error)),
    }
}

/// Turns a client error into text for the user. Credential helper failures can
/// carry the helper's output, so those details are dropped.
fn describe(error: &kube::Error) -> String {
    match error {
        kube::Error::Api(status) => format!(
            "the API server responded {}: {}",
            status.code, status.message
        ),
        kube::Error::Auth(_) => "authentication failed".to_owned(),
        kube::Error::Service(source)
            if source.downcast_ref::<kube::client::AuthError>().is_some() =>
        {
            "authentication failed".to_owned()
        }
        kube::Error::Service(_) | kube::Error::HyperError(_) => {
            "the API server is unreachable".to_owned()
        }
        _ => "the request failed".to_owned(),
    }
}

/// Watches a connected cluster and retries with increasing delays if it stops
/// responding.
async fn monitor(
    context: &str,
    client: &Client,
    options: &SessionOptions,
    state: &watch::Sender<ConnectionState>,
) {
    loop {
        time::sleep(options.health_interval).await;
        let Err(reason) = checked_probe(client, options).await else {
            continue;
        };

        tracing::info!(%context, "connection lost");
        state.send_replace(ConnectionState::Degraded { reason });
        let delays = ExponentialBuilder::default()
            .with_min_delay(options.reconnect_min_delay)
            .with_max_delay(options.reconnect_max_delay)
            .with_jitter()
            .without_max_times()
            .build();
        for delay in delays {
            time::sleep(delay).await;
            match checked_probe(client, options).await {
                Ok(()) => break,
                Err(reason) => {
                    state.send_replace(ConnectionState::Degraded { reason });
                }
            }
        }
        tracing::info!(%context, "connection restored");
        state.send_replace(ConnectionState::Connected);
    }
}

async fn checked_probe(client: &Client, options: &SessionOptions) -> Result<(), String> {
    time::timeout(options.connect_timeout, probe(client))
        .await
        .map_err(|_| "the API server did not respond in time".to_owned())?
}

/// Refreshes the served kinds on an interval, because custom resources can
/// appear at any time, and whenever the connection returns.
async fn keep_resources_current(
    client: Client,
    mut state: watch::Receiver<ConnectionState>,
    resources: watch::Sender<Arc<Vec<ResourceKind>>>,
    options: SessionOptions,
) {
    let mut ticks = time::interval(options.discovery_interval);
    ticks.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = ticks.tick() => {}
            changed = state.changed() => {
                if changed.is_err() {
                    return;
                }
                if *state.borrow() != ConnectionState::Connected {
                    continue;
                }
            }
        }
        match time::timeout(options.discovery_timeout, discover(&client)).await {
            Ok(Ok(kinds)) => {
                resources.send_if_modified(|current| {
                    let changed = **current != kinds;
                    if changed {
                        *current = Arc::new(kinds);
                    }
                    changed
                });
            }
            // The last known kinds stay in place until a refresh succeeds.
            Ok(Err(error)) => tracing::debug!(error = %describe(&error), "discovery failed"),
            Err(_) => tracing::debug!("discovery timed out"),
        }
    }
}

async fn discover(client: &Client) -> Result<Vec<ResourceKind>, kube::Error> {
    // Servers without aggregated discovery need one request per group. Some
    // answer the aggregated request with an empty list rather than an error, and
    // every real cluster serves the core group, so its absence also falls back.
    let discovery = match Discovery::new(client.clone()).run_aggregated().await {
        Ok(discovery) if discovery.has_group(ApiGroup::CORE_GROUP) => discovery,
        _ => Discovery::new(client.clone()).run().await?,
    };
    let mut kinds = BTreeSet::new();
    for group in discovery.groups() {
        for (resource, capabilities) in group.recommended_resources() {
            kinds.insert(ResourceKind {
                group: resource.group,
                version: resource.version,
                kind: resource.kind,
                plural: resource.plural,
                namespaced: capabilities.scope == Scope::Namespaced,
                verbs: capabilities.operations,
                printer_columns: Vec::new(),
            });
        }
    }
    let columns = printer_columns(client).await;
    Ok(kinds
        .into_iter()
        .map(|mut kind| {
            let key = (
                kind.group.clone(),
                kind.plural.clone(),
                kind.version.clone(),
            );
            kind.printer_columns = columns.get(&key).cloned().unwrap_or_default();
            kind
        })
        .collect())
}

/// Printer columns by group, plural, and version. Reading definitions can be
/// forbidden, and those kinds then simply show the default columns.
async fn printer_columns(client: &Client) -> HashMap<(String, String, String), Vec<PrinterColumn>> {
    let api: Api<CustomResourceDefinition> = Api::all(client.clone());
    let Ok(definitions) = api.list(&ListParams::default()).await else {
        return HashMap::new();
    };
    let mut columns = HashMap::new();
    for definition in definitions {
        let spec = definition.spec;
        for version in spec.versions {
            let declared = version.additional_printer_columns.unwrap_or_default();
            let declared = declared
                .into_iter()
                .map(|column| PrinterColumn {
                    name: column.name,
                    path: column.json_path,
                })
                .collect();
            let key = (spec.group.clone(), spec.names.plural.clone(), version.name);
            columns.insert(key, declared);
        }
    }
    columns
}
