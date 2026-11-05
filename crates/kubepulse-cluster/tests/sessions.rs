mod support;

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use kubepulse_cluster::{ConnectionState, SessionManager};
use support::*;
use tokio::{net::TcpListener, time};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{header, method, path},
};

fn manager(list: Arc<kubepulse_cluster::ClusterList>) -> SessionManager {
    SessionManager::new(list_channel(list), fast_options())
}

fn is_connected(state: &ConnectionState) -> bool {
    *state == ConnectionState::Connected
}

fn is_failed(state: &ConnectionState) -> bool {
    matches!(state, ConnectionState::Failed { .. })
}

#[tokio::test(flavor = "multi_thread")]
async fn listing_contexts_makes_no_requests_and_runs_no_helpers() {
    let dir = temp();
    let server = api_server().await;
    let marker = dir.path().join("helper-ran");
    let auth = Auth::Helper {
        token: "t".into(),
        delay_ms: 0,
        exit_code: 0,
        marker: Some(marker.clone()),
    };
    let list = kubeconfig(dir.path(), &[("a", &server.uri(), auth)]);

    let sessions = manager(list.clone());
    assert_eq!(list.clusters.len(), 1);
    assert_eq!(sessions.state("a"), ConnectionState::Disconnected);

    time::sleep(Duration::from_millis(200)).await;
    assert!(!marker.exists());
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn connecting_one_cluster_leaves_the_others_disconnected() {
    let dir = temp();
    let (one, two) = (api_server().await, api_server().await);
    let list = kubeconfig(
        dir.path(),
        &[
            ("one", &one.uri(), Auth::None),
            ("two", &two.uri(), Auth::None),
        ],
    );

    let mut sessions = manager(list);
    sessions.connect("one").unwrap();
    wait_until(
        &mut sessions.session("one").unwrap().subscribe_state(),
        is_connected,
    )
    .await;

    assert_eq!(sessions.state("two"), ConnectionState::Disconnected);
    assert!(two.received_requests().await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn two_clusters_stay_connected_together() {
    let dir = temp();
    let (one, two) = (api_server().await, api_server().await);
    let list = kubeconfig(
        dir.path(),
        &[
            ("one", &one.uri(), Auth::None),
            ("two", &two.uri(), Auth::None),
        ],
    );

    let mut sessions = manager(list);
    sessions.connect("one").unwrap();
    sessions.connect("two").unwrap();
    wait_until(
        &mut sessions.session("one").unwrap().subscribe_state(),
        is_connected,
    )
    .await;
    wait_until(
        &mut sessions.session("two").unwrap().subscribe_state(),
        is_connected,
    )
    .await;

    assert_eq!(sessions.state("one"), ConnectionState::Connected);
    assert_eq!(sessions.state("two"), ConnectionState::Connected);
}

#[tokio::test(flavor = "multi_thread")]
async fn reports_connecting_while_the_server_is_slow() {
    let dir = temp();
    let server = MockServer::start().await;
    mount_core_discovery(&server).await;
    get(
        "/version",
        json(VERSION).set_delay(Duration::from_millis(500)),
    )
    .mount(&server)
    .await;
    let list = kubeconfig(dir.path(), &[("a", &server.uri(), Auth::None)]);

    let mut sessions = manager(list);
    sessions.connect("a").unwrap();
    assert_eq!(sessions.state("a"), ConnectionState::Connecting);
    wait_until(
        &mut sessions.session("a").unwrap().subscribe_state(),
        is_connected,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn runs_the_helper_while_authenticating_and_sends_its_token() {
    let dir = temp();
    let server = MockServer::start().await;
    mount_core_discovery(&server).await;
    Mock::given(method("GET"))
        .and(path("/version"))
        .and(header("authorization", "Bearer helper-token"))
        .respond_with(json(VERSION))
        .mount(&server)
        .await;
    let auth = Auth::Helper {
        token: "helper-token".into(),
        delay_ms: 500,
        exit_code: 0,
        marker: None,
    };
    let list = kubeconfig(dir.path(), &[("a", &server.uri(), auth)]);

    let mut sessions = manager(list);
    sessions.connect("a").unwrap();
    let mut state = sessions.session("a").unwrap().subscribe_state();
    wait_until(&mut state, |s| *s == ConnectionState::Authenticating).await;
    wait_until(&mut state, is_connected).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn rejected_credentials_fail_and_a_retry_can_succeed() {
    let dir = temp();
    let server = MockServer::start().await;
    get("/version", ResponseTemplate::new(401))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    let list = kubeconfig(dir.path(), &[("a", &server.uri(), Auth::None)]);

    let mut sessions = manager(list);
    sessions.connect("a").unwrap();
    let failed = wait_until(
        &mut sessions.session("a").unwrap().subscribe_state(),
        is_failed,
    )
    .await;
    let ConnectionState::Failed { reason } = failed else {
        unreachable!()
    };
    assert!(reason.contains("401"), "{reason}");

    mount_api(&server).await;
    sessions.connect("a").unwrap();
    wait_until(
        &mut sessions.session("a").unwrap().subscribe_state(),
        is_connected,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn being_forbidden_from_version_still_connects() {
    let dir = temp();
    let server = MockServer::start().await;
    mount_core_discovery(&server).await;
    get(
        "/version",
        ResponseTemplate::new(403).set_body_raw(
            r#"{"kind":"Status","apiVersion":"v1","status":"Failure","message":"forbidden","reason":"Forbidden","code":403}"#,
            "application/json",
        ),
    )
    .mount(&server)
    .await;
    let list = kubeconfig(dir.path(), &[("a", &server.uri(), Auth::None)]);

    let mut sessions = manager(list);
    sessions.connect("a").unwrap();
    wait_until(
        &mut sessions.session("a").unwrap().subscribe_state(),
        is_connected,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn unreachable_cluster_fails_without_affecting_another() {
    let dir = temp();
    let good = api_server().await;
    let dead = format!("http://127.0.0.1:{}", closed_port());
    let list = kubeconfig(
        dir.path(),
        &[
            ("dead", &dead, Auth::None),
            ("good", &good.uri(), Auth::None),
        ],
    );

    let mut sessions = manager(list);
    sessions.connect("dead").unwrap();
    sessions.connect("good").unwrap();
    wait_until(
        &mut sessions.session("dead").unwrap().subscribe_state(),
        is_failed,
    )
    .await;
    wait_until(
        &mut sessions.session("good").unwrap().subscribe_state(),
        is_connected,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stalled_server_times_out() {
    let dir = temp();
    let server = MockServer::start().await;
    get("/version", json(VERSION).set_delay(Duration::from_secs(30)))
        .mount(&server)
        .await;
    let list = kubeconfig(dir.path(), &[("a", &server.uri(), Auth::None)]);

    let mut sessions = SessionManager::new(
        list_channel(list),
        kubepulse_cluster::SessionOptions {
            connect_timeout: Duration::from_millis(200),
            ..fast_options()
        },
    );
    sessions.connect("a").unwrap();
    wait_until(
        &mut sessions.session("a").unwrap().subscribe_state(),
        is_failed,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_slow_helper_times_out() {
    let dir = temp();
    let server = api_server().await;
    let auth = Auth::Helper {
        token: "t".into(),
        delay_ms: 3_000,
        exit_code: 0,
        marker: None,
    };
    let list = kubeconfig(dir.path(), &[("a", &server.uri(), auth)]);

    let mut sessions = SessionManager::new(
        list_channel(list),
        kubepulse_cluster::SessionOptions {
            auth_timeout: Duration::from_millis(200),
            ..fast_options()
        },
    );
    sessions.connect("a").unwrap();
    let failed = wait_until(
        &mut sessions.session("a").unwrap().subscribe_state(),
        is_failed,
    )
    .await;
    assert_eq!(
        failed,
        ConnectionState::Failed {
            reason: "the credential helper timed out".into()
        }
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_slow_helper_can_be_cancelled_without_blocking_other_sessions() {
    let dir = temp();
    let slow = api_server().await;
    let good = api_server().await;
    let auth = Auth::Helper {
        token: "t".into(),
        delay_ms: 5_000,
        exit_code: 0,
        marker: None,
    };
    let list = kubeconfig(
        dir.path(),
        &[
            ("slow", &slow.uri(), auth),
            ("good", &good.uri(), Auth::None),
        ],
    );

    let mut sessions = manager(list);
    sessions.connect("slow").unwrap();
    let mut state = sessions.session("slow").unwrap().subscribe_state();
    wait_until(&mut state, |s| *s == ConnectionState::Authenticating).await;

    sessions.connect("good").unwrap();
    wait_until(
        &mut sessions.session("good").unwrap().subscribe_state(),
        is_connected,
    )
    .await;

    let started = Instant::now();
    sessions.disconnect("slow").await;
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "took {:?}",
        started.elapsed()
    );
    assert_eq!(sessions.state("slow"), ConnectionState::Disconnected);
    assert!(slow.received_requests().await.unwrap().is_empty());
    assert_eq!(sessions.state("good"), ConnectionState::Connected);
}

#[tokio::test(flavor = "multi_thread")]
async fn disconnecting_stops_tasks_the_session_owns() {
    let dir = temp();
    let server = api_server().await;
    let list = kubeconfig(dir.path(), &[("a", &server.uri(), Auth::None)]);

    let mut sessions = manager(list);
    sessions.connect("a").unwrap();
    let session = sessions.session("a").unwrap();
    wait_until(&mut session.subscribe_state(), is_connected).await;

    let saw_cancel = Arc::new(AtomicBool::new(false));
    let guard = SetOnDrop(saw_cancel.clone());
    session.spawn(move |token| async move {
        let _guard = guard;
        token.cancelled().await;
    });
    // A task ignoring its token is still stopped by the session.
    let forever_stopped = Arc::new(AtomicBool::new(false));
    let guard = SetOnDrop(forever_stopped.clone());
    session.spawn(move |_| async move {
        let _guard = guard;
        std::future::pending::<()>().await;
    });

    sessions.disconnect("a").await;
    assert!(saw_cancel.load(Ordering::SeqCst));
    assert!(forever_stopped.load(Ordering::SeqCst));

    let before = server.received_requests().await.unwrap().len();
    time::sleep(Duration::from_millis(200)).await;
    assert_eq!(server.received_requests().await.unwrap().len(), before);
}

struct SetOnDrop(Arc<AtomicBool>);

impl Drop for SetOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_idle_cluster_stays_connected() {
    let dir = temp();
    let server = api_server().await;
    let list = kubeconfig(dir.path(), &[("a", &server.uri(), Auth::None)]);

    let mut sessions = manager(list);
    sessions.connect("a").unwrap();
    wait_until(
        &mut sessions.session("a").unwrap().subscribe_state(),
        is_connected,
    )
    .await;
    time::sleep(Duration::from_millis(500)).await;
    assert_eq!(sessions.state("a"), ConnectionState::Connected);
}

async fn server_on(listener: std::net::TcpListener) -> MockServer {
    let server = MockServer::builder().listener(listener).start().await;
    mount_api(&server).await;
    server
}

#[tokio::test(flavor = "multi_thread")]
async fn an_outage_degrades_the_session_and_it_recovers() {
    let dir = temp();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = server_on(listener).await;
    let list = kubeconfig(dir.path(), &[("a", &server.uri(), Auth::None)]);

    let mut sessions = manager(list);
    sessions.connect("a").unwrap();
    let session = sessions.session("a").unwrap();
    let mut state = session.subscribe_state();
    wait_until(&mut state, is_connected).await;
    let client = session.client();

    drop(server);
    let degraded = wait_until(&mut state, |s| {
        matches!(s, ConnectionState::Degraded { .. })
    })
    .await;
    let ConnectionState::Degraded { reason } = degraded else {
        unreachable!()
    };
    assert!(!reason.is_empty());
    assert!(client.is_some());

    let listener = loop {
        match TcpListener::bind(address).await {
            Ok(listener) => break listener.into_std().unwrap(),
            Err(_) => time::sleep(Duration::from_millis(20)).await,
        }
    };
    listener.set_nonblocking(true).unwrap();
    let _restored = server_on(listener).await;
    wait_until(&mut state, is_connected).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_custom_kind_appears_without_reconnecting() {
    let dir = temp();
    let server = api_server().await;
    get(
        "/apis",
        json(r#"{"kind":"APIGroupList","apiVersion":"v1","groups":[]}"#),
    )
    .with_priority(5)
    .mount(&server)
    .await;
    let list = kubeconfig(dir.path(), &[("a", &server.uri(), Auth::None)]);

    let mut sessions = manager(list);
    sessions.connect("a").unwrap();
    let session = sessions.session("a").unwrap();
    wait_until(&mut session.subscribe_state(), is_connected).await;

    let mut resources = session.subscribe_resources();
    time::timeout(
        WAIT,
        resources.wait_for(|kinds| kinds.iter().any(|k| k.kind == "Pod")),
    )
    .await
    .expect("core kinds should appear")
    .unwrap();
    assert!(!resources.borrow().iter().any(|k| k.kind == "Widget"));

    server.reset().await;
    mount_api(&server).await;
    get(
        "/apis",
        json(
            r#"{"kind":"APIGroupList","apiVersion":"v1","groups":[{"name":"example.com",
            "versions":[{"groupVersion":"example.com/v1","version":"v1"}],
            "preferredVersion":{"groupVersion":"example.com/v1","version":"v1"}}]}"#,
        ),
    )
    .mount(&server)
    .await;
    get(
        "/apis/example.com/v1",
        json(r#"{"kind":"APIResourceList","apiVersion":"v1","groupVersion":"example.com/v1","resources":[
            {"name":"widgets","singularName":"widget","namespaced":true,"kind":"Widget","verbs":["get","list"]}]}"#),
    )
    .mount(&server)
    .await;

    let kinds = time::timeout(
        WAIT,
        resources.wait_for(|kinds| kinds.iter().any(|k| k.kind == "Widget")),
    )
    .await
    .expect("the new kind should appear")
    .unwrap();
    let widget = kinds.iter().find(|k| k.kind == "Widget").unwrap();
    assert_eq!(
        (
            widget.group.as_str(),
            widget.plural.as_str(),
            widget.namespaced
        ),
        ("example.com", "widgets", true)
    );
    assert_eq!(sessions.state("a"), ConnectionState::Connected);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stalled_discovery_request_does_not_stop_later_refreshes() {
    let dir = temp();
    let server = MockServer::start().await;
    get("/version", json(VERSION)).mount(&server).await;
    get("/api", json("{}").set_delay(Duration::from_secs(60)))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    mount_core_discovery(&server).await;
    get(
        "/apis",
        json(r#"{"kind":"APIGroupList","apiVersion":"v1","groups":[]}"#),
    )
    .mount(&server)
    .await;
    let list = kubeconfig(dir.path(), &[("a", &server.uri(), Auth::None)]);

    let options = kubepulse_cluster::SessionOptions {
        discovery_timeout: Duration::from_millis(200),
        ..fast_options()
    };
    let mut sessions = SessionManager::new(list_channel(list), options);
    sessions.connect("a").unwrap();
    let session = sessions.session("a").unwrap();
    wait_until(&mut session.subscribe_state(), is_connected).await;

    let mut resources = session.subscribe_resources();
    time::timeout(
        WAIT,
        resources.wait_for(|kinds| kinds.iter().any(|k| k.kind == "Pod")),
    )
    .await
    .expect("a later refresh should succeed")
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn tracked_work_is_listed_until_its_guard_drops() {
    let dir = temp();
    let server = api_server().await;
    let list = kubeconfig(dir.path(), &[("a", &server.uri(), Auth::None)]);
    let mut sessions = manager(list);
    sessions.connect("a").unwrap();
    let session = sessions.session("a").unwrap();
    let work = session.subscribe_work();

    let shell = session.track_work("shell in web-1");
    let forward = session.track_work("port-forward to web-2");
    assert_eq!(*work.borrow(), ["shell in web-1", "port-forward to web-2"]);

    drop(shell);
    assert_eq!(*work.borrow(), ["port-forward to web-2"]);
    drop(forward);
    assert!(work.borrow().is_empty());
}
