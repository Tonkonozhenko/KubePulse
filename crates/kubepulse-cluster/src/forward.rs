//! Port-forwards from a loopback port to a pod, or to a service through one of
//! its ready pods. Each local connection gets its own stream to the pod.

use std::{
    collections::BTreeMap,
    future::pending,
    io,
    net::{Ipv4Addr, TcpListener as StdListener},
};

use futures::{StreamExt as _, pin_mut};
use k8s_openapi::api::{
    core::v1::{Pod, Service},
    discovery::v1::EndpointSlice,
};
use kube::{
    Api, Client,
    runtime::{WatchStreamExt as _, watcher, watcher::Event},
};
use tokio::{
    io::copy_bidirectional,
    net::{TcpListener, TcpStream},
    sync::watch,
    task::JoinSet,
};
use tokio_util::sync::{CancellationToken, DropGuard};

use crate::{resource_watch::SessionHandle, session::describe};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForwardKind {
    Pod,
    Service,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForwardRequest {
    pub kind: ForwardKind,
    pub namespace: String,
    pub name: String,
    /// The pod's port, or the service's port.
    pub port: u16,
    /// A free port is picked when this is `None`.
    pub local_port: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ForwardFailure {
    PodDeleted,
    PodStopped,
    /// The service has no port with the requested number.
    NoServicePort,
    /// What the cluster said.
    Cluster(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ForwardState {
    Starting,
    /// New connections go to `port` on `pod`.
    Active {
        pod: String,
        port: u16,
    },
    /// The service has no ready pod. The forward keeps its port and recovers
    /// when one becomes ready.
    NoReadyPod,
    /// The forward has ended and released its port.
    Failed(ForwardFailure),
}

/// A running forward. Dropping it stops the forward and releases its port.
pub struct PortForward {
    local_port: u16,
    state: watch::Receiver<ForwardState>,
    _stop: DropGuard,
}

impl PortForward {
    pub fn local_port(&self) -> u16 {
        self.local_port
    }

    /// Closes when the forward's session ends.
    pub fn subscribe(&self) -> watch::Receiver<ForwardState> {
        self.state.clone()
    }
}

/// The running end of a [`PortForward`].
pub struct ForwardFeed {
    pub state: watch::Sender<ForwardState>,
    stop: CancellationToken,
}

impl ForwardFeed {
    /// Whether the [`PortForward`] was dropped.
    pub fn stopped(&self) -> bool {
        self.stop.is_cancelled()
    }
}

pub fn forward_channel(local_port: u16) -> (ForwardFeed, PortForward) {
    let (state, receiver) = watch::channel(ForwardState::Starting);
    let stop = CancellationToken::new();
    let forward = PortForward {
        local_port,
        state: receiver,
        _stop: stop.clone().drop_guard(),
    };
    (ForwardFeed { state, stop }, forward)
}

/// Listens on the loopback interface only, so no other machine can connect.
pub fn bind_local(port: Option<u16>) -> io::Result<StdListener> {
    let listener = StdListener::bind((Ipv4Addr::LOCALHOST, port.unwrap_or(0)))?;
    listener.set_nonblocking(true)?;
    Ok(listener)
}

impl SessionHandle {
    /// Binds the local port now, so a port in use is reported before anything
    /// starts. The forward runs in this session's tree and ends with it.
    pub fn start_forward(&self, request: ForwardRequest, label: &str) -> io::Result<PortForward> {
        let listener = bind_local(request.local_port)?;
        let (feed, forward) = forward_channel(listener.local_addr()?.port());
        let work = self.track_work(label);
        self.spawn_stream(feed.stop.clone(), async move |client| {
            run_forward(client, request, listener, &feed.state).await;
            drop(work);
            // Its state stays open, so a failed forward is told apart from one
            // whose session ended.
            pending::<()>().await;
        });
        Ok(forward)
    }
}

/// Runs a forward until it fails. Its port is released before the failure is
/// reported, so a restart can bind it again.
pub async fn run_forward(
    client: Client,
    request: ForwardRequest,
    listener: StdListener,
    state: &watch::Sender<ForwardState>,
) {
    let listener = TcpListener::from_std(listener).expect("the listener is non-blocking");
    let namespace = request.namespace.clone();
    let failure = tokio::select! {
        failure = resolve(client.clone(), request, state) => failure,
        never = serve_forward(client, &namespace, listener, state.subscribe()) => match never {},
    };
    state.send_replace(ForwardState::Failed(failure));
}

async fn resolve(
    client: Client,
    request: ForwardRequest,
    state: &watch::Sender<ForwardState>,
) -> ForwardFailure {
    match request.kind {
        ForwardKind::Pod => follow_pod(client, &request, state).await,
        ForwardKind::Service => follow_service(client, &request, state).await,
    }
}

fn watch_failure(error: &watcher::Error) -> Option<ForwardFailure> {
    let (watcher::Error::InitialListFailed(kube::Error::Api(status))
    | watcher::Error::WatchStartFailed(kube::Error::Api(status))) = error
    else {
        tracing::warn!(%error, "port-forward watch interrupted");
        return None;
    };
    (status.code == 403).then(|| ForwardFailure::Cluster(status.message.clone()))
}

/// Follows the pod, and returns once it is gone or has stopped for good.
async fn follow_pod(
    client: Client,
    request: &ForwardRequest,
    state: &watch::Sender<ForwardState>,
) -> ForwardFailure {
    let api: Api<Pod> = Api::namespaced(client, &request.namespace);
    let config = watcher::Config::default().fields(&format!("metadata.name={}", request.name));
    let stream = watcher(api, config).default_backoff();
    pin_mut!(stream);
    let mut listed = false;
    while let Some(event) = stream.next().await {
        let pod = match event {
            Ok(Event::Init) => {
                listed = false;
                continue;
            }
            Ok(Event::InitDone) if !listed => return ForwardFailure::PodDeleted,
            Ok(Event::InitDone) => continue,
            Ok(Event::Delete(_)) => return ForwardFailure::PodDeleted,
            Ok(Event::Apply(pod) | Event::InitApply(pod)) => pod,
            Err(error) => match watch_failure(&error) {
                Some(failure) => return failure,
                None => continue,
            },
        };
        listed = true;
        // A pod being deleted can pass through a final phase first.
        if pod.metadata.deletion_timestamp.is_some() {
            return ForwardFailure::PodDeleted;
        }
        let phase = pod.status.and_then(|status| status.phase);
        match phase.as_deref() {
            Some("Running") => {
                state.send_replace(ForwardState::Active {
                    pod: request.name.clone(),
                    port: request.port,
                });
            }
            Some("Succeeded" | "Failed") => return ForwardFailure::PodStopped,
            _ => {
                state.send_replace(ForwardState::Starting);
            }
        }
    }
    unreachable!("a watcher with backoff never ends")
}

/// Follows the service's ready pods, moving to another one when the current one
/// goes. Returns only if the service cannot be used at all.
async fn follow_service(
    client: Client,
    request: &ForwardRequest,
    state: &watch::Sender<ForwardState>,
) -> ForwardFailure {
    let services: Api<Service> = Api::namespaced(client.clone(), &request.namespace);
    let service = match services.get(&request.name).await {
        Ok(service) => service,
        Err(kube::Error::Api(status)) => return ForwardFailure::Cluster(status.message),
        Err(error) => return ForwardFailure::Cluster(describe(&error)),
    };
    let ports = service.spec.and_then(|spec| spec.ports).unwrap_or_default();
    let Some(port) = ports
        .into_iter()
        .find(|port| port.port == i32::from(request.port))
    else {
        return ForwardFailure::NoServicePort;
    };
    let port_name = port.name.unwrap_or_default();

    let slices: Api<EndpointSlice> = Api::namespaced(client, &request.namespace);
    let selector = format!("kubernetes.io/service-name={}", request.name);
    let stream = watcher(slices, watcher::Config::default().labels(&selector)).default_backoff();
    pin_mut!(stream);
    let mut known = BTreeMap::new();
    let mut listing = None;
    while let Some(event) = stream.next().await {
        match event {
            Ok(Event::Init) => listing = Some(BTreeMap::new()),
            Ok(Event::InitApply(slice)) => {
                let name = slice.metadata.name.clone().unwrap_or_default();
                listing.get_or_insert_default().insert(name, slice);
            }
            Ok(Event::InitDone) => known = listing.take().unwrap_or_default(),
            Ok(Event::Apply(slice)) => {
                known.insert(slice.metadata.name.clone().unwrap_or_default(), slice);
            }
            Ok(Event::Delete(slice)) => {
                known.remove(&slice.metadata.name.unwrap_or_default());
            }
            Err(error) => match watch_failure(&error) {
                Some(failure) => return failure,
                None => continue,
            },
        }
        // A choice waits for a relist to finish, so a partial list never moves
        // a forward.
        if listing.is_some() {
            continue;
        }
        let current = match &*state.borrow() {
            ForwardState::Active { pod, port } => Some((pod.clone(), *port)),
            _ => None,
        };
        let backends = ready_backends(known.values(), &port_name);
        let next = match choose_backend(&backends, current.as_ref()) {
            Some((pod, port)) => ForwardState::Active {
                pod: pod.clone(),
                port: *port,
            },
            None => ForwardState::NoReadyPod,
        };
        state.send_if_modified(|state| {
            let changed = *state != next;
            *state = next;
            changed
        });
    }
    unreachable!("a watcher with backoff never ends")
}

/// The ready pods behind a service port, with the port each one serves it on,
/// in name order.
pub fn ready_backends<'a>(
    slices: impl IntoIterator<Item = &'a EndpointSlice>,
    port_name: &str,
) -> Vec<(String, u16)> {
    let mut backends = Vec::new();
    for slice in slices {
        let port = slice
            .ports
            .iter()
            .flatten()
            .find(|port| port.name.as_deref().unwrap_or_default() == port_name);
        let Some(port) = port.and_then(|port| port.port) else {
            continue;
        };
        let port = u16::try_from(port).expect("ports fit in 16 bits");
        for endpoint in slice.endpoints.iter().flatten() {
            // The API says an unknown readiness should be read as ready.
            let ready = endpoint
                .conditions
                .as_ref()
                .and_then(|conditions| conditions.ready)
                .unwrap_or(true);
            let pod = endpoint
                .target_ref
                .as_ref()
                .filter(|target| target.kind.as_deref() == Some("Pod"))
                .and_then(|target| target.name.clone());
            if let (true, Some(pod)) = (ready, pod) {
                backends.push((pod, port));
            }
        }
    }
    backends.sort();
    backends.dedup();
    backends
}

/// Keeps the current pod while it is ready, so connections are not moved
/// without need.
pub fn choose_backend<'a>(
    backends: &'a [(String, u16)],
    current: Option<&(String, u16)>,
) -> Option<&'a (String, u16)> {
    current
        .and_then(|current| backends.iter().find(|backend| *backend == current))
        .or_else(|| backends.first())
}

/// Accepts connections and sends each one to wherever `state` points now. A
/// connection that arrives while there is no target is closed.
pub async fn serve_forward(
    client: Client,
    namespace: &str,
    listener: TcpListener,
    state: watch::Receiver<ForwardState>,
) -> std::convert::Infallible {
    let api: Api<Pod> = Api::namespaced(client, namespace);
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let socket = match accepted {
                    Ok((socket, _)) => socket,
                    Err(error) => {
                        tracing::warn!(%error, "could not accept a forwarded connection");
                        continue;
                    }
                };
                if let ForwardState::Active { pod, port } = &*state.borrow() {
                    connections.spawn(connect(api.clone(), pod.clone(), *port, socket));
                }
            }
            Some(_) = connections.join_next() => {}
        }
    }
}

async fn connect(api: Api<Pod>, pod: String, port: u16, mut socket: TcpStream) {
    let mut forwarder = match api.portforward(&pod, &[port]).await {
        Ok(forwarder) => forwarder,
        Err(error) => {
            tracing::warn!(error = %describe(&error), %pod, port, "port-forward connection failed");
            return;
        }
    };
    let mut upstream = forwarder.take_stream(port).expect("the port was requested");
    let refused = forwarder.take_error(port).expect("the port was requested");
    tokio::select! {
        copied = copy_bidirectional(&mut socket, &mut upstream) => {
            if let Err(error) = copied {
                tracing::debug!(%error, %pod, port, "forwarded connection ended");
            }
            drop(upstream);
            if let Err(error) = forwarder.join().await {
                tracing::debug!(%error, %pod, port, "forwarded connection ended");
            }
        }
        Some(message) = refused => {
            tracing::warn!(%message, %pod, port, "the pod refused a forwarded connection");
            forwarder.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use k8s_openapi::api::{
        core::v1::ObjectReference,
        discovery::v1::{Endpoint, EndpointConditions, EndpointPort},
    };

    use super::*;

    fn endpoint(pod: &str, ready: Option<bool>) -> Endpoint {
        Endpoint {
            conditions: Some(EndpointConditions {
                ready,
                ..EndpointConditions::default()
            }),
            target_ref: Some(ObjectReference {
                kind: Some("Pod".into()),
                name: Some(pod.into()),
                ..ObjectReference::default()
            }),
            ..Endpoint::default()
        }
    }

    fn slice(ports: &[(Option<&str>, i32)], endpoints: Vec<Endpoint>) -> EndpointSlice {
        EndpointSlice {
            ports: Some(
                ports
                    .iter()
                    .map(|(name, port)| EndpointPort {
                        name: name.map(str::to_owned),
                        port: Some(*port),
                        ..EndpointPort::default()
                    })
                    .collect(),
            ),
            endpoints: Some(endpoints),
            ..EndpointSlice::default()
        }
    }

    fn backend(pod: &str, port: u16) -> (String, u16) {
        (pod.to_owned(), port)
    }

    #[test]
    fn only_ready_pods_are_backends() {
        let slice = slice(
            &[(None, 8080)],
            vec![
                endpoint("b", Some(true)),
                endpoint("a", None),
                endpoint("c", Some(false)),
            ],
        );
        assert_eq!(
            ready_backends([&slice], ""),
            vec![backend("a", 8080), backend("b", 8080)]
        );
    }

    #[test]
    fn a_named_service_port_uses_the_matching_slice_port() {
        let slice = slice(
            &[(Some("metrics"), 9090), (Some("http"), 8080)],
            vec![endpoint("a", Some(true))],
        );
        assert_eq!(ready_backends([&slice], "http"), vec![backend("a", 8080)]);
        assert_eq!(ready_backends([&slice], "grpc"), vec![]);
    }

    #[test]
    fn the_current_pod_is_kept_while_it_is_ready() {
        let backends = [backend("a", 80), backend("b", 80)];
        assert_eq!(
            choose_backend(&backends, Some(&backend("b", 80))),
            Some(&backend("b", 80))
        );
    }

    #[test]
    fn a_pod_that_went_away_is_replaced_by_a_ready_one() {
        let backends = [backend("c", 80)];
        assert_eq!(
            choose_backend(&backends, Some(&backend("b", 80))),
            Some(&backend("c", 80))
        );
        assert_eq!(choose_backend(&[], Some(&backend("b", 80))), None);
    }
}
