//! Port-forwards. Connections go to a simulated API server that speaks the
//! port-forward channel protocol over a WebSocket, and target resolution runs
//! against a fake API server.

mod support;

use std::{
    io::ErrorKind,
    net::{Ipv4Addr, UdpSocket},
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

use futures::{SinkExt as _, StreamExt as _};
use kube::{Client, Config};
use kubepulse_cluster::{
    ConnectionState, ForwardFailure, ForwardKind, ForwardRequest, ForwardState, SessionManager,
    bind_local, run_forward, serve_forward,
};
use support::*;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    sync::watch,
    time::timeout,
};
use tokio_tungstenite::{
    accept_hdr_async,
    tungstenite::{
        Message,
        handshake::server::{Request as WsRequest, Response as WsResponse},
    },
};
use wiremock::{
    Mock, MockServer, Request, Respond, ResponseTemplate,
    matchers::{method, path, query_param, query_param_is_missing},
};

/// A simulated cluster whose pods answer each message with their name and port
/// in front of it.
async fn echo_pods() -> Client {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut target = String::new();
                #[allow(clippy::result_large_err, reason = "the library sets this type")]
                let accept = |request: &WsRequest, mut response: WsResponse| {
                    target = request.uri().to_string();
                    response.headers_mut().insert(
                        "Sec-WebSocket-Protocol",
                        "v4.channel.k8s.io".parse().unwrap(),
                    );
                    Ok(response)
                };
                let Ok(mut socket) = accept_hdr_async(socket, accept).await else {
                    return;
                };
                let (pod, port) = pod_and_port(&target);
                for channel in [0, 1] {
                    let mut frame = vec![channel];
                    frame.extend(port.to_le_bytes());
                    socket.send(Message::Binary(frame.into())).await.unwrap();
                }
                while let Some(Ok(Message::Binary(data))) = socket.next().await {
                    let mut reply = vec![0];
                    reply.extend(format!("{pod}:{port}:").as_bytes());
                    reply.extend(&data[1..]);
                    if socket.send(Message::Binary(reply.into())).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    Client::try_from(Config::new(format!("http://{address}").parse().unwrap())).unwrap()
}

/// Reads the pod and port from `/api/v1/namespaces/<ns>/pods/<pod>/portforward?&ports=<port>`.
fn pod_and_port(uri: &str) -> (String, u16) {
    let (route, query) = uri.split_once('?').unwrap();
    let pod = route.trim_end_matches("/portforward").rsplit('/').next();
    let port = query.trim_start_matches('&').trim_start_matches("ports=");
    (pod.unwrap().to_owned(), port.parse().unwrap())
}

fn active(pod: &str, port: u16) -> ForwardState {
    ForwardState::Active {
        pod: pod.into(),
        port,
    }
}

/// Serves connections to wherever `state` points.
async fn serve(state: ForwardState) -> (u16, watch::Sender<ForwardState>) {
    let client = echo_pods().await;
    let listener = bind_local(None).unwrap();
    let port = listener.local_addr().unwrap().port();
    let listener = TcpListener::from_std(listener).unwrap();
    let (sender, receiver) = watch::channel(state);
    tokio::spawn(async move { serve_forward(client, "default", listener, receiver).await });
    (port, sender)
}

async fn exchange(port: u16, message: &str) -> String {
    let mut socket = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
        .await
        .unwrap();
    socket.write_all(message.as_bytes()).await.unwrap();
    let mut reply = String::new();
    timeout(WAIT, async {
        let mut buffer = [0; 256];
        while !reply.ends_with(message) {
            let read = socket.read(&mut buffer).await.unwrap();
            assert_ne!(read, 0, "the connection closed after {reply:?}");
            reply.push_str(&String::from_utf8_lossy(&buffer[..read]));
        }
    })
    .await
    .expect("a reply");
    reply
}

#[tokio::test]
async fn connections_to_the_local_port_reach_the_pod() {
    let (port, _state) = serve(active("web-a", 8080)).await;
    assert_eq!(exchange(port, "hello").await, "web-a:8080:hello");
    assert_eq!(
        exchange(port, "again").await,
        "web-a:8080:again",
        "each connection gets its own stream"
    );
}

#[tokio::test]
async fn new_connections_follow_the_target() {
    let (port, state) = serve(active("web-a", 8080)).await;
    assert_eq!(exchange(port, "one").await, "web-a:8080:one");
    state.send_replace(active("web-b", 8081));
    assert_eq!(exchange(port, "two").await, "web-b:8081:two");
}

#[tokio::test]
async fn a_connection_with_no_target_is_closed() {
    let (port, _state) = serve(ForwardState::NoReadyPod).await;
    let mut socket = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
        .await
        .unwrap();
    let mut buffer = [0; 16];
    let read = timeout(WAIT, socket.read(&mut buffer)).await.unwrap();
    assert!(matches!(read, Ok(0) | Err(_)), "read {read:?}");
}

/// An address of this machine that other machines can reach.
fn outside_address() -> std::net::IpAddr {
    let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
    // Connecting a UDP socket only picks a route, and sends nothing.
    socket.connect("192.0.2.1:9").unwrap();
    socket.local_addr().unwrap().ip()
}

#[tokio::test]
async fn the_listener_is_not_reachable_on_other_addresses() {
    let (port, _state) = serve(active("web-a", 8080)).await;
    let outside = outside_address();
    assert!(!outside.is_loopback());
    let connected = timeout(WAIT, TcpStream::connect((outside, port))).await;
    assert!(
        matches!(connected, Ok(Err(_))),
        "{outside}:{port} accepted a connection"
    );
    assert_eq!(exchange(port, "local").await, "web-a:8080:local");
}

#[test]
fn a_free_port_is_picked_when_none_is_chosen() {
    let listener = bind_local(None).unwrap();
    let address = listener.local_addr().unwrap();
    assert!(address.ip().is_loopback());
    assert_ne!(address.port(), 0);
}

#[test]
fn a_port_in_use_is_reported() {
    let taken = bind_local(None).unwrap();
    let port = taken.local_addr().unwrap().port();
    let error = bind_local(Some(port)).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::AddrInUse);
}

const PODS: &str = "/api/v1/namespaces/default/pods";
const SLICES: &str = "/apis/discovery.k8s.io/v1/namespaces/default/endpointslices";

fn pod(name: &str, phase: &str) -> String {
    format!(
        r#"{{"apiVersion":"v1","kind":"Pod","metadata":{{"name":"{name}","namespace":"default","uid":"uid-{name}","resourceVersion":"6"}},"status":{{"phase":"{phase}"}}}}"#
    )
}

fn list(kind: &str, items: &[String]) -> ResponseTemplate {
    json(&format!(
        r#"{{"apiVersion":"v1","kind":"{kind}","metadata":{{"resourceVersion":"10"}},"items":[{}]}}"#,
        items.join(",")
    ))
}

fn slice(name: &str, pods: &[(&str, bool)]) -> String {
    let endpoints: Vec<String> = pods
        .iter()
        .map(|(pod, ready)| {
            format!(
                r#"{{"addresses":["10.0.0.1"],"conditions":{{"ready":{ready}}},"targetRef":{{"kind":"Pod","name":"{pod}","namespace":"default"}}}}"#
            )
        })
        .collect();
    format!(
        r#"{{"apiVersion":"discovery.k8s.io/v1","kind":"EndpointSlice","metadata":{{"name":"{name}","namespace":"default","resourceVersion":"11","labels":{{"kubernetes.io/service-name":"web"}}}},"addressType":"IPv4","ports":[{{"name":"http","port":8080,"protocol":"TCP"}}],"endpoints":[{}]}}"#,
        endpoints.join(",")
    )
}

const SERVICE: &str = r#"{"apiVersion":"v1","kind":"Service","metadata":{"name":"web","namespace":"default"},
"spec":{"ports":[{"name":"http","port":80,"targetPort":"http"}]}}"#;

/// Sends `events` on the first watch, after `delay`, and keeps later watches idle.
struct WatchOnce {
    events: String,
    delay: Duration,
    calls: AtomicUsize,
}

impl Respond for WatchOnce {
    fn respond(&self, _: &Request) -> ResponseTemplate {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            json(&self.events).set_delay(self.delay)
        } else {
            json("").set_delay(Duration::from_secs(30))
        }
    }
}

fn event(kind: &str, object: &str) -> String {
    format!("{{\"type\":\"{kind}\",\"object\":{object}}}\n")
}

async fn mount(server: &MockServer, route: &str, listed: ResponseTemplate, watched: String) {
    Mock::given(method("GET"))
        .and(path(route))
        .and(query_param_is_missing("watch"))
        .respond_with(listed)
        .mount(server)
        .await;
    let watch = WatchOnce {
        events: watched,
        delay: Duration::from_millis(500),
        calls: AtomicUsize::new(0),
    };
    Mock::given(method("GET"))
        .and(path(route))
        .and(query_param("watch", "true"))
        .respond_with(watch)
        .mount(server)
        .await;
}

fn request(kind: ForwardKind, port: u16, local_port: u16) -> ForwardRequest {
    ForwardRequest {
        kind,
        namespace: "default".into(),
        name: if kind == ForwardKind::Pod {
            "web-a"
        } else {
            "web"
        }
        .into(),
        port,
        local_port: Some(local_port),
    }
}

/// Runs a forward against `server`, and returns its state and local port.
fn forward(
    server: &MockServer,
    kind: ForwardKind,
    port: u16,
) -> (watch::Receiver<ForwardState>, u16) {
    let client = Client::try_from(Config::new(server.uri().parse().unwrap())).unwrap();
    let listener = bind_local(None).unwrap();
    let local_port = listener.local_addr().unwrap().port();
    let (state, receiver) = watch::channel(ForwardState::Starting);
    let request = request(kind, port, local_port);
    tokio::spawn(async move {
        run_forward(client, request, listener, &state).await;
        state.closed().await;
    });
    (receiver, local_port)
}

async fn reach(state: &mut watch::Receiver<ForwardState>, wanted: ForwardState) {
    timeout(WAIT, state.wait_for(|state| *state == wanted))
        .await
        .unwrap_or_else(|_| panic!("expected {wanted:?}"))
        .unwrap();
}

#[tokio::test]
async fn a_deleted_target_pod_marks_the_forward_as_failed_and_releases_the_port() {
    let server = MockServer::start().await;
    let deleted = event("DELETED", &pod("web-a", "Running"));
    mount(
        &server,
        PODS,
        list("PodList", &[pod("web-a", "Running")]),
        deleted,
    )
    .await;
    let (mut state, local_port) = forward(&server, ForwardKind::Pod, 8080);

    reach(&mut state, active("web-a", 8080)).await;
    reach(&mut state, ForwardState::Failed(ForwardFailure::PodDeleted)).await;
    bind_local(Some(local_port)).expect("a restart can bind the port again");
}

#[tokio::test]
async fn a_missing_pod_fails_the_forward() {
    let server = MockServer::start().await;
    mount(&server, PODS, list("PodList", &[]), String::new()).await;
    let (mut state, _) = forward(&server, ForwardKind::Pod, 8080);
    reach(&mut state, ForwardState::Failed(ForwardFailure::PodDeleted)).await;
}

#[tokio::test]
async fn a_pod_that_is_not_running_yet_is_not_active() {
    let server = MockServer::start().await;
    let running = event("MODIFIED", &pod("web-a", "Running"));
    mount(
        &server,
        PODS,
        list("PodList", &[pod("web-a", "Pending")]),
        running,
    )
    .await;
    let (mut state, _) = forward(&server, ForwardKind::Pod, 8080);
    assert_eq!(*state.borrow(), ForwardState::Starting);
    reach(&mut state, active("web-a", 8080)).await;
}

#[tokio::test]
async fn a_service_forward_moves_to_another_ready_pod_when_its_pod_goes() {
    let server = MockServer::start().await;
    get("/api/v1/namespaces/default/services/web", json(SERVICE))
        .mount(&server)
        .await;
    let listed = list(
        "EndpointSliceList",
        &[slice("web-1", &[("web-a", true), ("web-b", false)])],
    );
    let replaced = event("MODIFIED", &slice("web-1", &[("web-b", true)]));
    mount(&server, SLICES, listed, replaced).await;
    let (mut state, _) = forward(&server, ForwardKind::Service, 80);

    reach(&mut state, active("web-a", 8080)).await;
    reach(&mut state, active("web-b", 8080)).await;
}

#[tokio::test]
async fn a_service_with_no_ready_pod_is_not_active() {
    let server = MockServer::start().await;
    get("/api/v1/namespaces/default/services/web", json(SERVICE))
        .mount(&server)
        .await;
    let listed = list("EndpointSliceList", &[slice("web-1", &[("web-a", false)])]);
    let ready = event("MODIFIED", &slice("web-1", &[("web-a", true)]));
    mount(&server, SLICES, listed, ready).await;
    let (mut state, _) = forward(&server, ForwardKind::Service, 80);

    reach(&mut state, ForwardState::NoReadyPod).await;
    reach(&mut state, active("web-a", 8080)).await;
}

#[tokio::test]
async fn a_service_port_that_does_not_exist_fails_the_forward() {
    let server = MockServer::start().await;
    get("/api/v1/namespaces/default/services/web", json(SERVICE))
        .mount(&server)
        .await;
    let (mut state, _) = forward(&server, ForwardKind::Service, 443);
    reach(
        &mut state,
        ForwardState::Failed(ForwardFailure::NoServicePort),
    )
    .await;
}

#[tokio::test]
async fn forwards_stop_when_their_cluster_disconnects() {
    let server = api_server().await;
    mount(
        &server,
        PODS,
        list("PodList", &[pod("web-a", "Running")]),
        String::new(),
    )
    .await;
    let dir = temp();
    let list = kubeconfig(dir.path(), &[("a", &server.uri(), Auth::None)]);
    let mut sessions = SessionManager::new(list_channel(list), fast_options());
    sessions.connect("a").unwrap();
    let session = sessions.session("a").unwrap();
    wait_until(&mut session.subscribe_state(), |s| {
        *s == ConnectionState::Connected
    })
    .await;
    let work = session.subscribe_work();

    let forward = session
        .handle()
        .start_forward(request(ForwardKind::Pod, 8080, 0), "forward web-a")
        .unwrap();
    let local_port = forward.local_port();
    let mut state = forward.subscribe();
    reach(&mut state, active("web-a", 8080)).await;
    assert_eq!(*work.borrow(), vec!["forward web-a".to_owned()]);

    sessions.disconnect("a").await;
    timeout(WAIT, state.changed())
        .await
        .unwrap()
        .expect_err("the forward ended with its session");
    bind_local(Some(local_port)).expect("the port was released");
}

#[tokio::test]
async fn stopping_a_forward_releases_its_port() {
    let server = api_server().await;
    mount(
        &server,
        PODS,
        list("PodList", &[pod("web-a", "Running")]),
        String::new(),
    )
    .await;
    let dir = temp();
    let list = kubeconfig(dir.path(), &[("a", &server.uri(), Auth::None)]);
    let mut sessions = SessionManager::new(list_channel(list), fast_options());
    sessions.connect("a").unwrap();
    let handle = sessions.session("a").unwrap().handle();

    let forward = handle
        .start_forward(request(ForwardKind::Pod, 8080, 0), "forward web-a")
        .unwrap();
    let local_port = forward.local_port();
    let mut state = forward.subscribe();
    reach(&mut state, active("web-a", 8080)).await;
    drop(forward);
    timeout(WAIT, async {
        while bind_local(Some(local_port)).is_err() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the port was released");
}

#[tokio::test]
async fn a_pod_that_starts_terminating_fails_the_forward_as_deleted() {
    let server = MockServer::start().await;
    let terminating = pod("web-a", "Succeeded").replace(
        r#""resourceVersion":"6""#,
        r#""resourceVersion":"7","deletionTimestamp":"2026-01-01T00:00:00Z""#,
    );
    let modified = event("MODIFIED", &terminating);
    mount(
        &server,
        PODS,
        list("PodList", &[pod("web-a", "Running")]),
        modified,
    )
    .await;
    let (mut state, _) = forward(&server, ForwardKind::Pod, 8080);
    reach(&mut state, active("web-a", 8080)).await;
    reach(&mut state, ForwardState::Failed(ForwardFailure::PodDeleted)).await;
}

#[tokio::test]
async fn a_pod_that_completed_fails_the_forward_as_stopped() {
    let server = MockServer::start().await;
    let completed = event("MODIFIED", &pod("web-a", "Succeeded"));
    mount(
        &server,
        PODS,
        list("PodList", &[pod("web-a", "Running")]),
        completed,
    )
    .await;
    let (mut state, _) = forward(&server, ForwardKind::Pod, 8080);
    reach(&mut state, ForwardState::Failed(ForwardFailure::PodStopped)).await;
}
