//! Watches one resource kind and reports plain events, so that client library
//! types stay in this crate. Paging, bookmarks, and retries come from the
//! client library's watcher.

use std::{collections::BTreeMap, future::Future, time::Duration};

use futures::{StreamExt as _, future::join_all, pin_mut};
use k8s_openapi::api::core::v1::Namespace;
use kube::{
    Api, Client,
    api::{ApiResource, DynamicObject, ListParams},
    runtime::{WatchStreamExt as _, watcher, watcher::Event},
};
use tokio::{
    sync::{mpsc, watch},
    time,
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::session::{ResourceKind, describe};

const PAGE_SIZE: u32 = 500;
const POLL_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnerReference {
    pub kind: String,
    pub name: String,
    pub uid: String,
}

#[derive(Clone, Debug)]
pub struct RawObject {
    /// The UID, or `namespace/name` for the few served objects that have none.
    pub key: String,
    pub name: String,
    pub namespace: Option<String>,
    pub labels: BTreeMap<String, String>,
    pub owners: Vec<OwnerReference>,
    pub created: Option<String>,
    /// The whole object without `managedFields`, for evaluating column paths.
    pub data: serde_json::Value,
}

/// What a watch reports. `scope` is the namespace of a namespace-scoped watch,
/// or `None` for a cluster-wide one.
#[derive(Clone, Debug)]
pub enum WatchEvent {
    /// A full list is starting, and `Listed` objects will replace what `scope` holds.
    Restarted {
        scope: Option<String>,
    },
    Listed {
        scope: Option<String>,
        object: RawObject,
    },
    Synced {
        scope: Option<String>,
    },
    Applied(RawObject),
    Deleted {
        key: String,
    },
    /// The user cannot list this kind. This is final for the watch.
    Denied {
        reason: String,
    },
}

/// Starts watches that belong to a session, so disconnecting stops them.
#[derive(Clone)]
pub struct SessionHandle {
    client: watch::Receiver<Option<Client>>,
    cancel: CancellationToken,
    tasks: TaskTracker,
}

impl SessionHandle {
    pub(crate) fn new(
        client: watch::Receiver<Option<Client>>,
        cancel: CancellationToken,
        tasks: TaskTracker,
    ) -> Self {
        Self {
            client,
            cancel,
            tasks,
        }
    }

    pub(crate) async fn client(&self) -> Option<Client> {
        let mut client = self.client.clone();
        let ready = client.wait_for(Option::is_some).await.ok()?;
        ready.clone()
    }

    /// Runs `work` in the session's tree until `stop` fires or the session
    /// ends. It starts once the session has a client.
    pub(crate) fn spawn_stream<F>(
        &self,
        stop: CancellationToken,
        work: impl FnOnce(Client) -> F + Send + 'static,
    ) where
        F: Future<Output = ()> + Send + 'static,
    {
        let mut client = self.client.clone();
        let session = self.cancel.child_token();
        self.tasks.spawn(async move {
            let run = async move {
                let client = client
                    .wait_for(Option::is_some)
                    .await
                    .map(|ready| ready.clone());
                if let Ok(Some(client)) = client {
                    work(client).await;
                }
            };
            session
                .run_until_cancelled(stop.run_until_cancelled(run))
                .await;
        });
    }

    /// Sends events for `kind` until `stop` fires, the session ends, or
    /// `events` closes. Starts once the session has a client.
    pub fn watch(
        &self,
        kind: &ResourceKind,
        events: mpsc::Sender<WatchEvent>,
        stop: CancellationToken,
    ) {
        let kind = kind.clone();
        let mut client = self.client.clone();
        let session = self.cancel.child_token();
        self.tasks.spawn(async move {
            let run = async move {
                let client = client
                    .wait_for(Option::is_some)
                    .await
                    .map(|ready| ready.clone());
                if let Ok(Some(client)) = client {
                    run(client, kind, events).await;
                }
            };
            session
                .run_until_cancelled(stop.run_until_cancelled(run))
                .await;
        });
    }
}

async fn run(client: Client, kind: ResourceKind, events: mpsc::Sender<WatchEvent>) {
    let resource = api_resource(&kind);
    let all: Api<DynamicObject> = Api::all_with(client.clone(), &resource);
    if !kind.verbs.iter().any(|verb| verb == "watch") {
        poll(all, &events).await;
        return;
    }
    let Ended::Denied(reason) = follow(all, None, &events).await else {
        return;
    };

    if !kind.namespaced {
        let _ = events.send(WatchEvent::Denied { reason }).await;
        return;
    }
    tracing::debug!(kind = %kind.kind, "cluster-wide list forbidden, using namespaces");
    let namespaces = accessible_namespaces(&client).await;
    let watches = namespaces.into_iter().map(|namespace| {
        let api = Api::namespaced_with(client.clone(), &namespace, &resource);
        let events = &events;
        async move { follow(api, Some(namespace), events).await }
    });
    // Each watch ends only if it is denied or the store goes away.
    let ended = join_all(watches).await;
    if ended.iter().all(|end| matches!(end, Ended::Denied(_))) {
        let _ = events.send(WatchEvent::Denied { reason }).await;
    }
}

/// The namespaces to try, or the context's default one if they cannot be listed.
async fn accessible_namespaces(client: &Client) -> Vec<String> {
    let api: Api<Namespace> = Api::all(client.clone());
    match api.list(&ListParams::default()).await {
        Ok(list) => list
            .items
            .into_iter()
            .filter_map(|item| item.metadata.name)
            .collect(),
        Err(error) => {
            tracing::info!(error = %describe(&error), "could not list namespaces");
            vec![client.default_namespace().to_owned()]
        }
    }
}

pub(crate) fn api_resource(kind: &ResourceKind) -> ApiResource {
    let api_version = match kind.group.as_str() {
        "" => kind.version.clone(),
        group => format!("{group}/{}", kind.version),
    };
    ApiResource {
        group: kind.group.clone(),
        version: kind.version.clone(),
        api_version,
        kind: kind.kind.clone(),
        plural: kind.plural.clone(),
    }
}

/// Lists a kind that cannot be watched, again on every tick.
async fn poll(api: Api<DynamicObject>, events: &mpsc::Sender<WatchEvent>) {
    let mut ticks = time::interval(POLL_INTERVAL);
    loop {
        ticks.tick().await;
        match api.list(&ListParams::default()).await {
            Ok(list) => {
                let listed = list.items.into_iter().map(|object| WatchEvent::Listed {
                    scope: None,
                    object: raw(object),
                });
                let restarted = WatchEvent::Restarted { scope: None };
                let synced = WatchEvent::Synced { scope: None };
                for event in [restarted].into_iter().chain(listed).chain([synced]) {
                    if events.send(event).await.is_err() {
                        return;
                    }
                }
            }
            Err(kube::Error::Api(status)) if status.code == 403 => {
                let reason = status.message;
                let _ = events.send(WatchEvent::Denied { reason }).await;
                return;
            }
            Err(error) => tracing::warn!(error = %describe(&error), "list failed"),
        }
    }
}

enum Ended {
    Denied(String),
    Closed,
}

async fn follow(
    api: Api<DynamicObject>,
    scope: Option<String>,
    events: &mpsc::Sender<WatchEvent>,
) -> Ended {
    let stream = watcher(api, watcher::Config::default().page_size(PAGE_SIZE)).default_backoff();
    pin_mut!(stream);
    while let Some(item) = stream.next().await {
        let event = match item {
            Ok(Event::Init) => WatchEvent::Restarted {
                scope: scope.clone(),
            },
            Ok(Event::InitApply(object)) => WatchEvent::Listed {
                scope: scope.clone(),
                object: raw(object),
            },
            Ok(Event::InitDone) => WatchEvent::Synced {
                scope: scope.clone(),
            },
            Ok(Event::Apply(object)) => WatchEvent::Applied(raw(object)),
            Ok(Event::Delete(object)) => WatchEvent::Deleted { key: key(&object) },
            Err(error) => {
                if let Some(reason) = forbidden(&error) {
                    return Ended::Denied(reason);
                }
                // The stream retries with backoff, and the session reports outages.
                tracing::warn!(error = %describe_watch(&error), "watch interrupted");
                continue;
            }
        };
        if events.send(event).await.is_err() {
            return Ended::Closed;
        }
    }
    Ended::Closed
}

fn describe_watch(error: &watcher::Error) -> String {
    match error {
        watcher::Error::InitialListFailed(error)
        | watcher::Error::WatchStartFailed(error)
        | watcher::Error::WatchFailed(error) => describe(error),
        other => other.to_string(),
    }
}

fn forbidden(error: &watcher::Error) -> Option<String> {
    let (watcher::Error::InitialListFailed(kube::Error::Api(status))
    | watcher::Error::WatchStartFailed(kube::Error::Api(status))) = error
    else {
        return None;
    };
    (status.code == 403).then(|| status.message.clone())
}

fn key(object: &DynamicObject) -> String {
    let meta = &object.metadata;
    meta.uid.clone().unwrap_or_else(|| {
        format!(
            "{}/{}",
            meta.namespace.as_deref().unwrap_or(""),
            meta.name.as_deref().unwrap_or("")
        )
    })
}

fn raw(mut object: DynamicObject) -> RawObject {
    let key = key(&object);
    object.metadata.managed_fields = None;
    let metadata = serde_json::to_value(&object.metadata).expect("object metadata serializes");
    let meta = object.metadata;
    let mut data = object.data;
    if let Some(fields) = data.as_object_mut() {
        fields.insert("metadata".to_owned(), metadata);
    }
    RawObject {
        key,
        name: meta.name.unwrap_or_default(),
        namespace: meta.namespace,
        labels: meta.labels.unwrap_or_default(),
        owners: meta
            .owner_references
            .unwrap_or_default()
            .into_iter()
            .map(|owner| OwnerReference {
                kind: owner.kind,
                name: owner.name,
                uid: owner.uid,
            })
            .collect(),
        created: meta.creation_timestamp.map(|time| time.0.to_string()),
        data,
    }
}
