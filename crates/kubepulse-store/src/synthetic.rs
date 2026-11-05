//! A synthetic cluster for scale tests: realistic pods and update bursts, with
//! no network.

use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use kubepulse_cluster::{OwnerReference, RawObject, ResourceKind, WatchEvent};
use serde_json::json;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{row::ColumnSpec, store::WatchSource};

const NAMESPACES: usize = 40;
const NODES: usize = 200;
const BATCHES_PER_SECOND: usize = 100;

pub fn pod_kind() -> ResourceKind {
    ResourceKind {
        group: String::new(),
        version: "v1".into(),
        kind: "Pod".into(),
        plural: "pods".into(),
        namespaced: true,
        verbs: vec!["list".into(), "watch".into()],
    }
}

/// The default pod columns.
pub fn pod_columns() -> Arc<[ColumnSpec]> {
    [
        ("status", ".status.phase"),
        ("restarts", ".status.containerStatuses[0].restartCount"),
        ("node", ".spec.nodeName"),
        ("ip", ".status.podIP"),
        ("image", ".spec.containers[0].image"),
    ]
    .map(|(id, path)| ColumnSpec::new(id, path).expect("valid path"))
    .into()
}

/// Pod number `index`, with `restarts` as its changing field.
pub fn pod(index: usize, restarts: u64) -> RawObject {
    let workload = index % 700;
    let name = format!(
        "workload-{workload}-{:08x}-{:05x}",
        index.wrapping_mul(2_654_435_761),
        index * 40_503
    );
    let namespace = format!("team-{:02}", index % NAMESPACES);
    let node = format!(
        "ip-10-0-{}-{}.compute.internal",
        index % NODES / 16,
        index % NODES
    );
    let ip = format!(
        "10.{}.{}.{}",
        index >> 16 & 255,
        index >> 8 & 255,
        index & 255
    );
    let image = format!(
        "registry.example.com/team/workload-{workload}:1.{}.0",
        workload % 30
    );
    let labels = BTreeMap::from(
        [
            ("app.kubernetes.io/name", format!("workload-{workload}")),
            (
                "app.kubernetes.io/part-of",
                format!("team-{:02}", index % NAMESPACES),
            ),
            ("app.kubernetes.io/managed-by", "deployer".to_owned()),
            ("pod-template-hash", format!("{:08x}", workload * 7919)),
            ("topology.kubernetes.io/zone", format!("zone-{}", index % 3)),
            ("tier", ["front", "back", "data"][index % 3].to_owned()),
        ]
        .map(|(key, value)| (key.to_owned(), value)),
    );
    let data = json!({
        "metadata": {"name": name, "namespace": namespace, "labels": labels},
        "spec": {
            "nodeName": node,
            "serviceAccountName": "default",
            "containers": [{
                "name": "app",
                "image": image,
                "ports": [{"containerPort": 8080, "protocol": "TCP"}],
                "resources": {"requests": {"cpu": "100m", "memory": "128Mi"}},
                "env": [{"name": "LOG_LEVEL", "value": "info"}, {"name": "REGION", "value": "eu-1"}],
            }, {
                "name": "proxy",
                "image": "registry.example.com/mesh/proxy:2.4.1",
                "resources": {"requests": {"cpu": "10m", "memory": "32Mi"}},
            }],
        },
        "status": {
            "phase": "Running",
            "podIP": ip,
            "hostIP": "10.0.0.1",
            "conditions": [
                {"type": "Ready", "status": "True"},
                {"type": "ContainersReady", "status": "True"},
            ],
            "containerStatuses": [{"name": "app", "ready": true, "restartCount": restarts}],
        },
    });
    RawObject {
        key: format!("{index:08x}-3f1c-4b6a-9e21-7c5d{index:08x}"),
        name,
        namespace: Some(namespace),
        labels,
        owners: vec![OwnerReference {
            kind: "ReplicaSet".into(),
            name: format!("workload-{workload}-{:08x}", workload * 7919),
            uid: format!("{workload:08x}-aaaa-4b6a-9e21-7c5d00000000"),
        }],
        created: Some(format!(
            "2026-{:02}-{:02}T08:30:00Z",
            index % 12 + 1,
            index % 28 + 1
        )),
        data,
    }
}

/// A burst of changes after the initial list.
#[derive(Clone, Copy, Debug)]
pub struct Burst {
    pub changes_per_second: usize,
    pub duration: Duration,
}

/// Serves `pods` pods, then runs `burst` if there is one.
pub struct SyntheticCluster {
    pods: usize,
    burst: Option<Burst>,
    /// Watches ever started.
    pub started: AtomicUsize,
    /// Watches running now.
    pub active: Arc<AtomicUsize>,
    /// Updates sent, and so the total `restarts` a view should converge to.
    pub changes_sent: Arc<AtomicUsize>,
}

impl SyntheticCluster {
    pub fn new(pods: usize, burst: Option<Burst>) -> Self {
        Self {
            pods,
            burst,
            started: AtomicUsize::new(0),
            active: Arc::default(),
            changes_sent: Arc::default(),
        }
    }
}

impl WatchSource for SyntheticCluster {
    fn watch(&self, _: &ResourceKind, events: mpsc::Sender<WatchEvent>, stop: CancellationToken) {
        self.started.fetch_add(1, Ordering::SeqCst);
        self.active.fetch_add(1, Ordering::SeqCst);
        let active = self.active.clone();
        let changes = self.changes_sent.clone();
        let (pods, burst) = (self.pods, self.burst);
        tokio::spawn(async move {
            stop.run_until_cancelled(stream(pods, burst, events, changes))
                .await;
            active.fetch_sub(1, Ordering::SeqCst);
        });
    }
}

async fn stream(
    pods: usize,
    burst: Option<Burst>,
    events: mpsc::Sender<WatchEvent>,
    changes: Arc<AtomicUsize>,
) -> Option<()> {
    events
        .send(WatchEvent::Restarted { scope: None })
        .await
        .ok()?;
    for index in 0..pods {
        let object = pod(index, 0);
        events
            .send(WatchEvent::Listed {
                scope: None,
                object,
            })
            .await
            .ok()?;
    }
    events.send(WatchEvent::Synced { scope: None }).await.ok()?;

    if let Some(burst) = burst {
        let mut restarts = vec![0u64; pods];
        let mut cursor = 0usize;
        let per_batch = burst.changes_per_second / BATCHES_PER_SECOND;
        let start = Instant::now();
        let batches = burst.duration.as_secs_f64() * BATCHES_PER_SECOND as f64;
        for batch in 0..batches as u64 {
            for _ in 0..per_batch {
                // A prime step spreads changes across the table.
                cursor = (cursor + 7919) % pods;
                restarts[cursor] += 1;
                let object = pod(cursor, restarts[cursor]);
                events.send(WatchEvent::Applied(object)).await.ok()?;
                changes.fetch_add(1, Ordering::SeqCst);
            }
            let due = start + Duration::from_millis((batch + 1) * 1000 / BATCHES_PER_SECOND as u64);
            tokio::time::sleep(due.saturating_duration_since(Instant::now())).await;
        }
    }
    std::future::pending::<()>().await;
    Some(())
}
