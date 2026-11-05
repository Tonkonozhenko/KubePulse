use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

use kubepulse_cluster::{RawObject, ResourceKind, WatchEvent};
use kubepulse_store::{
    Cell, ClusterStore, ColumnResolver, Snapshot, StoreOptions, ViewQuery, WatchSource,
    synthetic::{Burst, SyntheticCluster, pod, pod_columns, pod_kind},
};
use tokio::{
    sync::{mpsc, watch},
    time::{sleep, timeout},
};
use tokio_util::sync::CancellationToken;

const WAIT: Duration = Duration::from_secs(10);

fn columns() -> ColumnResolver {
    Arc::new(|_| pod_columns())
}

fn options() -> StoreOptions {
    StoreOptions {
        release_delay: Duration::from_millis(100),
        frame_interval: Duration::from_millis(5),
    }
}

fn store(source: Arc<dyn WatchSource>) -> ClusterStore {
    ClusterStore::new(source, columns(), options())
}

async fn wait_for(
    snapshot: &mut watch::Receiver<Arc<Snapshot>>,
    condition: impl FnMut(&Arc<Snapshot>) -> bool,
) -> Arc<Snapshot> {
    timeout(WAIT, snapshot.wait_for(condition))
        .await
        .expect("snapshot")
        .unwrap()
        .clone()
}

async fn until(mut condition: impl FnMut() -> bool) {
    timeout(WAIT, async {
        while !condition() {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("condition");
}

#[tokio::test]
async fn two_views_share_one_watch() {
    let source = Arc::new(SyntheticCluster::new(10, None));
    let store = store(source.clone());

    let first = store.view(&pod_kind(), ViewQuery::default());
    let second = store.view(
        &pod_kind(),
        ViewQuery {
            text: "workload-1-".into(),
            ..Default::default()
        },
    );
    wait_for(&mut first.subscribe(), |s| s.synced && s.rows.len() == 10).await;
    let filtered = wait_for(&mut second.subscribe(), |s| s.synced).await;

    assert_eq!(filtered.rows.len(), 1);
    assert_eq!(filtered.total, 10);
    assert_eq!(source.started.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn different_kinds_get_their_own_watches() {
    let source = Arc::new(SyntheticCluster::new(1, None));
    let store = store(source.clone());
    let other = ResourceKind {
        plural: "pods2".into(),
        ..pod_kind()
    };
    let _one = store.view(&pod_kind(), ViewQuery::default());
    let _two = store.view(&other, ViewQuery::default());
    until(|| source.started.load(Ordering::SeqCst) == 2).await;
}

#[tokio::test]
async fn watch_stops_after_the_release_delay_and_restarts_on_next_use() {
    let source = Arc::new(SyntheticCluster::new(10, None));
    let store = store(source.clone());

    let view = store.view(&pod_kind(), ViewQuery::default());
    wait_for(&mut view.subscribe(), |s| s.synced).await;
    drop(view);
    assert_eq!(
        source.active.load(Ordering::SeqCst),
        1,
        "the watch outlives the view for the delay"
    );
    until(|| source.active.load(Ordering::SeqCst) == 0).await;

    let view = store.view(&pod_kind(), ViewQuery::default());
    wait_for(&mut view.subscribe(), |s| s.synced && s.rows.len() == 10).await;
    assert_eq!(source.started.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn reopening_within_the_delay_keeps_the_watch_and_its_data() {
    let source = Arc::new(SyntheticCluster::new(10, None));
    let store = ClusterStore::new(
        source.clone(),
        columns(),
        StoreOptions {
            release_delay: Duration::from_secs(60),
            ..options()
        },
    );

    let view = store.view(&pod_kind(), ViewQuery::default());
    wait_for(&mut view.subscribe(), |s| s.synced).await;
    drop(view);

    let view = store.view(&pod_kind(), ViewQuery::default());
    let snapshot = wait_for(&mut view.subscribe(), |s| s.synced).await;
    assert_eq!(snapshot.rows.len(), 10);
    assert_eq!(source.started.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn dropping_the_store_stops_its_watches() {
    let source = Arc::new(SyntheticCluster::new(10, None));
    let store = store(source.clone());
    let view = store.view(&pod_kind(), ViewQuery::default());
    wait_for(&mut view.subscribe(), |s| s.synced).await;
    drop(store);
    until(|| source.active.load(Ordering::SeqCst) == 0).await;
}

#[tokio::test]
async fn changing_a_query_republishes() {
    let store = store(Arc::new(SyntheticCluster::new(100, None)));
    let view = store.view(&pod_kind(), ViewQuery::default());
    let mut snapshots = view.subscribe();
    wait_for(&mut snapshots, |s| s.synced && s.rows.len() == 100).await;

    view.set_query(ViewQuery {
        namespaces: ["team-01".to_owned()].into(),
        ..Default::default()
    });
    let filtered = wait_for(&mut snapshots, |s| s.rows.len() < 100).await;
    assert_eq!(filtered.rows.len(), 3);
    assert!(
        filtered
            .rows
            .iter()
            .all(|row| row.namespace.as_deref() == Some("team-01"))
    );
    assert_eq!(filtered.total, 100);
}

/// Sends exactly the events a test gives it.
struct Scripted(std::sync::Mutex<Option<mpsc::Sender<WatchEvent>>>);

impl Scripted {
    fn new() -> Arc<Self> {
        Arc::new(Self(Default::default()))
    }

    async fn send(&self, event: WatchEvent) {
        let sender = timeout(WAIT, async {
            loop {
                if let Some(sender) = self.0.lock().unwrap().clone() {
                    return sender;
                }
                sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
        sender.send(event).await.unwrap();
    }
}

impl WatchSource for Scripted {
    fn watch(&self, _: &ResourceKind, events: mpsc::Sender<WatchEvent>, _: CancellationToken) {
        *self.0.lock().unwrap() = Some(events);
    }
}

fn listed(scope: Option<&str>, object: RawObject) -> WatchEvent {
    WatchEvent::Listed {
        scope: scope.map(str::to_owned),
        object,
    }
}

fn in_namespace(index: usize, namespace: &str) -> RawObject {
    RawObject {
        namespace: Some(namespace.to_owned()),
        ..pod(index, 0)
    }
}

fn synced(scope: Option<&str>) -> WatchEvent {
    WatchEvent::Synced {
        scope: scope.map(str::to_owned),
    }
}

fn restarted(scope: Option<&str>) -> WatchEvent {
    WatchEvent::Restarted {
        scope: scope.map(str::to_owned),
    }
}

#[tokio::test]
async fn nothing_counts_as_synced_until_the_list_finishes() {
    let source = Scripted::new();
    let store = store(source.clone());
    let view = store.view(&pod_kind(), ViewQuery::default());
    let mut snapshots = view.subscribe();

    source.send(restarted(None)).await;
    source.send(listed(None, pod(1, 0))).await;
    sleep(Duration::from_millis(30)).await;
    assert!(!view.snapshot().synced);
    assert!(view.snapshot().rows.is_empty());

    source.send(synced(None)).await;
    let snapshot = wait_for(&mut snapshots, |s| s.synced).await;
    assert_eq!(snapshot.rows.len(), 1);
}

#[tokio::test]
async fn live_changes_replace_and_remove_rows_and_keys_stay_stable() {
    let source = Scripted::new();
    let store = store(source.clone());
    let view = store.view(&pod_kind(), ViewQuery::default());
    let mut snapshots = view.subscribe();
    source.send(restarted(None)).await;
    source.send(listed(None, pod(1, 0))).await;
    source.send(synced(None)).await;
    let before = wait_for(&mut snapshots, |s| s.synced).await.rows[0]
        .key
        .clone();

    source.send(WatchEvent::Applied(pod(1, 9))).await;
    let changed = wait_for(&mut snapshots, |s| {
        s.rows
            .first()
            .is_some_and(|r| r.cells[1] == Cell::Number(9))
    })
    .await;
    assert_eq!(changed.rows[0].key, before);

    source.send(WatchEvent::Applied(pod(2, 0))).await;
    wait_for(&mut snapshots, |s| s.rows.len() == 2).await;

    source
        .send(WatchEvent::Deleted {
            key: before.to_string(),
        })
        .await;
    let remaining = wait_for(&mut snapshots, |s| s.rows.len() == 1).await;
    assert_ne!(remaining.rows[0].key, before);
}

#[tokio::test]
async fn a_relist_keeps_rows_until_it_completes_and_drops_what_vanished() {
    let source = Scripted::new();
    let store = store(source.clone());
    let view = store.view(&pod_kind(), ViewQuery::default());
    let mut snapshots = view.subscribe();
    source.send(restarted(None)).await;
    source.send(listed(None, pod(1, 0))).await;
    source.send(listed(None, pod(2, 0))).await;
    source.send(synced(None)).await;
    wait_for(&mut snapshots, |s| s.rows.len() == 2).await;

    source.send(restarted(None)).await;
    source.send(listed(None, pod(2, 0))).await;
    sleep(Duration::from_millis(30)).await;
    assert_eq!(
        view.snapshot().rows.len(),
        2,
        "stale rows stay visible during a relist"
    );

    source.send(synced(None)).await;
    wait_for(&mut snapshots, |s| s.rows.len() == 1).await;
}

#[tokio::test]
async fn namespace_watches_refresh_only_their_own_rows() {
    let source = Scripted::new();
    let store = store(source.clone());
    let view = store.view(&pod_kind(), ViewQuery::default());
    let mut snapshots = view.subscribe();
    for (index, namespace) in [(1, "a"), (2, "b")] {
        source.send(restarted(Some(namespace))).await;
        source
            .send(listed(Some(namespace), in_namespace(index, namespace)))
            .await;
        source.send(synced(Some(namespace))).await;
    }
    wait_for(&mut snapshots, |s| s.rows.len() == 2).await;

    source.send(restarted(Some("a"))).await;
    source.send(synced(Some("a"))).await;
    let snapshot = wait_for(&mut snapshots, |s| s.rows.len() == 1).await;
    assert_eq!(snapshot.rows[0].namespace.as_deref(), Some("b"));
}

#[tokio::test]
async fn denied_kinds_are_reported_in_the_snapshot() {
    let source = Scripted::new();
    let store = store(source.clone());
    let view = store.view(&pod_kind(), ViewQuery::default());
    let mut snapshots = view.subscribe();
    source
        .send(WatchEvent::Denied {
            reason: "pods is forbidden".into(),
        })
        .await;
    let snapshot = wait_for(&mut snapshots, |s| s.synced).await;
    assert_eq!(snapshot.denied.as_deref(), Some("pods is forbidden"));
}

#[tokio::test]
async fn bursts_publish_no_more_than_once_per_frame() {
    let frame = Duration::from_millis(50);
    let burst = Burst {
        changes_per_second: 2_000,
        duration: Duration::from_millis(500),
    };
    let source = Arc::new(SyntheticCluster::new(100, Some(burst)));
    let store = ClusterStore::new(
        source.clone(),
        columns(),
        StoreOptions {
            frame_interval: frame,
            ..options()
        },
    );
    let view = store.view(&pod_kind(), ViewQuery::default());
    let mut snapshots = view.subscribe();
    wait_for(&mut snapshots, |s| s.synced).await;

    let start = view.snapshot().version;
    let begun = tokio::time::Instant::now();
    sleep(Duration::from_millis(400)).await;
    let published = view.snapshot().version - start;
    let frames = begun.elapsed().as_millis() as u64 / frame.as_millis() as u64;
    assert!(published > 0, "the burst should publish");
    assert!(
        published <= frames + 1,
        "{published} publications in {frames} frames"
    );
}

#[tokio::test]
async fn snapshots_list_every_namespace_whatever_the_view_shows() {
    let source = Scripted::new();
    let store = store(source.clone());
    let view = store.view(
        &pod_kind(),
        ViewQuery {
            namespaces: ["one".to_owned()].into(),
            ..Default::default()
        },
    );
    let mut snapshots = view.subscribe();
    source.send(restarted(None)).await;
    source.send(listed(None, in_namespace(1, "one"))).await;
    source.send(listed(None, in_namespace(2, "two"))).await;
    source.send(listed(None, in_namespace(3, "two"))).await;
    source.send(synced(None)).await;

    let listed = wait_for(&mut snapshots, |s| s.synced).await;
    assert_eq!(listed.rows.len(), 1);
    assert_eq!(*listed.namespaces, ["one", "two"]);

    for index in [2, 3] {
        source
            .send(WatchEvent::Deleted {
                key: in_namespace(index, "two").key,
            })
            .await;
    }
    let emptied = wait_for(&mut snapshots, |s| s.total == 1).await;
    assert_eq!(*emptied.namespaces, ["one"]);
}
