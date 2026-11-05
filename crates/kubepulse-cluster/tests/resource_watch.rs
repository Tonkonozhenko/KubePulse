mod support;

use std::time::Duration;

use kubepulse_cluster::{
    ConnectionState, ResourceKind, SessionManager, SessionWatcher, WatchEvent,
};
use support::*;
use tokio::{sync::mpsc, time};
use tokio_util::sync::CancellationToken;
use wiremock::{
    Mock, MockServer, Request, ResponseTemplate,
    matchers::{method, path, query_param, query_param_is_missing},
};

fn pods() -> ResourceKind {
    ResourceKind {
        group: String::new(),
        version: "v1".into(),
        kind: "Pod".into(),
        plural: "pods".into(),
        namespaced: true,
        verbs: vec!["list".into(), "watch".into()],
    }
}

fn pod(name: &str, namespace: &str) -> String {
    format!(
        r#"{{"apiVersion":"v1","kind":"Pod","metadata":{{"name":"{name}","namespace":"{namespace}","uid":"uid-{name}","resourceVersion":"5",
        "labels":{{"app":"web"}},"managedFields":[{{"manager":"x"}}],
        "ownerReferences":[{{"apiVersion":"apps/v1","kind":"ReplicaSet","name":"rs","uid":"uid-rs"}}]}},"status":{{"phase":"Running"}}}}"#
    )
}

fn pod_list(items: &[String], next: Option<&str>) -> ResponseTemplate {
    let continuation = next.map_or(String::new(), |token| format!(r#","continue":"{token}""#));
    json(&format!(
        r#"{{"apiVersion":"v1","kind":"PodList","metadata":{{"resourceVersion":"10"{continuation}}},"items":[{}]}}"#,
        items.join(",")
    ))
}

fn denied() -> ResponseTemplate {
    ResponseTemplate::new(403).set_body_raw(
        r#"{"kind":"Status","apiVersion":"v1","status":"Failure","message":"pods is forbidden","reason":"Forbidden","code":403}"#,
        "application/json",
    )
}

/// Answers list requests on `route`. Watches on it hang open unless mounted first.
async fn mount_list(server: &MockServer, route: &str, response: ResponseTemplate) {
    Mock::given(method("GET"))
        .and(path(route))
        .and(query_param_is_missing("watch"))
        .respond_with(response)
        .mount(server)
        .await;
}

async fn mount_idle_watch(server: &MockServer, route: &str) {
    Mock::given(method("GET"))
        .and(path(route))
        .and(query_param("watch", "true"))
        .respond_with(json("").set_delay(Duration::from_secs(30)))
        .mount(server)
        .await;
}

struct Watch {
    events: mpsc::Receiver<WatchEvent>,
    _sessions: SessionManager,
    _dir: tempfile::TempDir,
    stop: CancellationToken,
}

async fn watch_pods(server: &MockServer, default_namespace: Option<&str>) -> Watch {
    let dir = temp();
    let mut list = (*kubeconfig(dir.path(), &[("a", &server.uri(), Auth::None)])).clone();
    if let Some(namespace) = default_namespace {
        list = with_default_namespace(dir.path(), namespace);
    }
    let mut sessions = SessionManager::new(list_channel(list.into()), fast_options());
    sessions.connect("a").unwrap();
    let session = sessions.session("a").unwrap();
    wait_until(&mut session.subscribe_state(), |s| {
        *s == ConnectionState::Connected
    })
    .await;

    let watcher: SessionWatcher = session.watcher();
    let (sender, events) = mpsc::channel(64);
    let stop = CancellationToken::new();
    watcher.watch(&pods(), sender, stop.clone());
    Watch {
        events,
        _sessions: sessions,
        _dir: dir,
        stop,
    }
}

fn with_default_namespace(
    dir: &std::path::Path,
    namespace: &str,
) -> kubepulse_cluster::ClusterList {
    let path = dir.join("config");
    let contents = std::fs::read_to_string(&path).unwrap();
    let contents = contents.replace(
        "    user: a\n",
        &format!("    user: a\n    namespace: {namespace}\n"),
    );
    std::fs::write(&path, contents).unwrap();
    kubepulse_cluster::load(&[path])
}

async fn next(watch: &mut Watch) -> WatchEvent {
    time::timeout(WAIT, watch.events.recv())
        .await
        .expect("an event")
        .expect("the channel is open")
}

async fn collect_until_synced(watch: &mut Watch, count: usize) -> Vec<(Option<String>, String)> {
    let mut listed = Vec::new();
    let mut synced = 0;
    while synced < count {
        match next(watch).await {
            WatchEvent::Listed { scope, object } => listed.push((scope, object.name)),
            WatchEvent::Synced { .. } => synced += 1,
            _ => {}
        }
    }
    listed.sort();
    listed
}

#[tokio::test(flavor = "multi_thread")]
async fn lists_every_page_then_reports_synced() {
    let server = api_server().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/pods"))
        .and(query_param_is_missing("watch"))
        .and(query_param_is_missing("continue"))
        .respond_with(pod_list(&[pod("one", "x")], Some("page-2")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/pods"))
        .and(query_param("continue", "page-2"))
        .respond_with(pod_list(&[pod("two", "x")], None))
        .mount(&server)
        .await;
    mount_idle_watch(&server, "/api/v1/pods").await;

    let mut watch = watch_pods(&server, None).await;
    assert!(matches!(
        next(&mut watch).await,
        WatchEvent::Restarted { scope: None }
    ));
    let listed = collect_until_synced(&mut watch, 1).await;
    assert_eq!(listed, [(None, "one".to_owned()), (None, "two".to_owned())]);

    let requests = server.received_requests().await.unwrap();
    let first = requests
        .iter()
        .find(|r: &&Request| r.url.path() == "/api/v1/pods")
        .unwrap();
    assert!(
        first.url.query().unwrap().contains("limit=500"),
        "{:?}",
        first.url.query()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn uses_bookmarks_and_reports_live_changes() {
    let server = api_server().await;
    mount_list(&server, "/api/v1/pods", pod_list(&[pod("one", "x")], None)).await;
    let changes = format!(
        "{{\"type\":\"BOOKMARK\",\"object\":{{\"apiVersion\":\"v1\",\"kind\":\"Pod\",\"metadata\":{{\"resourceVersion\":\"11\"}}}}}}\n\
         {{\"type\":\"ADDED\",\"object\":{}}}\n{{\"type\":\"DELETED\",\"object\":{}}}\n",
        pod("two", "x").replace('\n', " "),
        pod("one", "x").replace('\n', " "),
    );
    Mock::given(method("GET"))
        .and(path("/api/v1/pods"))
        .and(query_param("watch", "true"))
        .respond_with(json(&changes))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    mount_idle_watch(&server, "/api/v1/pods").await;

    let mut watch = watch_pods(&server, None).await;
    collect_until_synced(&mut watch, 1).await;

    let WatchEvent::Applied(object) = next(&mut watch).await else {
        panic!("expected a change")
    };
    assert_eq!(object.key, "uid-two");
    assert_eq!(object.namespace.as_deref(), Some("x"));
    assert_eq!(object.labels["app"], "web");
    assert_eq!(object.owners[0].kind, "ReplicaSet");
    assert_eq!(object.data["status"]["phase"], "Running");
    assert_eq!(object.data["metadata"]["name"], "two");
    assert!(object.data["metadata"].get("managedFields").is_none());

    let WatchEvent::Deleted { key } = next(&mut watch).await else {
        panic!("expected a delete")
    };
    assert_eq!(key, "uid-one");

    let requests = server.received_requests().await.unwrap();
    let opened = requests
        .iter()
        .find(|r| r.url.query().is_some_and(|q| q.contains("watch=true")))
        .unwrap();
    assert!(
        opened
            .url
            .query()
            .unwrap()
            .contains("allowWatchBookmarks=true")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn falls_back_to_the_namespaces_the_user_can_access() {
    let server = api_server().await;
    mount_list(&server, "/api/v1/pods", denied()).await;
    get(
        "/api/v1/namespaces",
        json(r#"{"apiVersion":"v1","kind":"NamespaceList","metadata":{"resourceVersion":"1"},"items":[
            {"metadata":{"name":"open"}},{"metadata":{"name":"also-open"}},{"metadata":{"name":"closed"}}]}"#),
    )
    .mount(&server)
    .await;
    mount_list(
        &server,
        "/api/v1/namespaces/open/pods",
        pod_list(&[pod("one", "open")], None),
    )
    .await;
    mount_list(
        &server,
        "/api/v1/namespaces/also-open/pods",
        pod_list(&[pod("two", "also-open")], None),
    )
    .await;
    mount_list(&server, "/api/v1/namespaces/closed/pods", denied()).await;
    mount_idle_watch(&server, "/api/v1/namespaces/open/pods").await;
    mount_idle_watch(&server, "/api/v1/namespaces/also-open/pods").await;

    let mut watch = watch_pods(&server, None).await;
    let listed = collect_until_synced(&mut watch, 2).await;
    assert_eq!(
        listed,
        [
            (Some("also-open".to_owned()), "two".to_owned()),
            (Some("open".to_owned()), "one".to_owned())
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn uses_the_default_namespace_when_namespaces_cannot_be_listed() {
    let server = api_server().await;
    mount_list(&server, "/api/v1/pods", denied()).await;
    get("/api/v1/namespaces", denied()).mount(&server).await;
    mount_list(
        &server,
        "/api/v1/namespaces/mine/pods",
        pod_list(&[pod("one", "mine")], None),
    )
    .await;
    mount_idle_watch(&server, "/api/v1/namespaces/mine/pods").await;

    let mut watch = watch_pods(&server, Some("mine")).await;
    let listed = collect_until_synced(&mut watch, 1).await;
    assert_eq!(listed, [(Some("mine".to_owned()), "one".to_owned())]);
}

#[tokio::test(flavor = "multi_thread")]
async fn reports_denied_when_no_namespace_is_accessible() {
    let server = api_server().await;
    mount_list(&server, "/api/v1/pods", denied()).await;
    get("/api/v1/namespaces", denied()).mount(&server).await;
    mount_list(&server, "/api/v1/namespaces/default/pods", denied()).await;

    let mut watch = watch_pods(&server, None).await;
    loop {
        if let WatchEvent::Denied { reason } = next(&mut watch).await {
            assert_eq!(reason, "pods is forbidden");
            return;
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn stopping_a_watch_closes_its_channel() {
    let server = api_server().await;
    mount_list(&server, "/api/v1/pods", pod_list(&[], None)).await;
    mount_idle_watch(&server, "/api/v1/pods").await;

    let mut watch = watch_pods(&server, None).await;
    collect_until_synced(&mut watch, 1).await;
    watch.stop.cancel();
    let closed = time::timeout(WAIT, async { while watch.events.recv().await.is_some() {} });
    closed.await.expect("the channel should close");
}
