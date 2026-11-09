//! Fakes for UI tests: services with a real store behind them, and helpers to
//! wait for the background threads.
#![allow(dead_code)]

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use gpui_kit::VisualTestContext;
use kubepulse_cluster::{
    ClusterEntry, ConnectionState, EventRecord, ExecFeed, ExecRequest, ExecSize, ExecStream,
    ForwardFailure, ForwardFeed, ForwardRequest, ForwardState, LogBuffer, LogFeed, LogRequest,
    LogStream, ObjectError, ObjectTarget, PortForward, RawObject, ResourceKind, UpdateAccess,
    WatchEvent, WriteMode, bind_local, forward_channel,
};
use kubepulse_extension_api::{ExtensionRequest, ExtensionResponse, KindRef, UiNode};
use kubepulse_extension_host::{CallError, ContributionSet};
use kubepulse_store::{ClusterStore, ColumnResolver, StoreOptions, View, ViewQuery, WatchSource};
use kubepulse_ui::{
    columns::columns_for,
    runtime::ClusterStatus,
    services::{ClusterServices, ForwardStartError, Reply},
};
use serde_json::Value;
use tokio::{
    runtime::{Builder, Runtime},
    sync::{mpsc, watch},
};
use tokio_util::sync::CancellationToken;

const WAIT: Duration = Duration::from_secs(30);

/// Runs the UI until `condition` holds. Needs `allow_parking`, because the
/// store answers from other threads.
pub fn wait_until(
    cx: &mut VisualTestContext,
    what: &str,
    mut condition: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let started = Instant::now();
    loop {
        cx.run_until_parked();
        if condition(cx) {
            return;
        }
        assert!(started.elapsed() < WAIT, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

pub fn kind(group: &str, name: &str, plural: &str, namespaced: bool) -> ResourceKind {
    ResourceKind {
        group: group.to_owned(),
        version: "v1".into(),
        kind: name.to_owned(),
        plural: plural.to_owned(),
        namespaced,
        verbs: vec!["get".into(), "list".into(), "update".into(), "watch".into()],
        printer_columns: vec![],
    }
}

pub fn connected(context: &str, resources: Vec<ResourceKind>) -> ClusterStatus {
    ClusterStatus {
        entry: ClusterEntry {
            context: context.to_owned(),
            cluster: Some(context.to_owned()),
            server: Some(format!("https://{context}.example")),
            user: None,
            namespace: None,
        },
        state: ConnectionState::Connected,
        active_work: vec![],
        resources: Arc::new(resources),
    }
}

/// An object as a watch delivers it.
pub fn raw(name: &str, namespace: Option<&str>, data: Value) -> RawObject {
    RawObject {
        key: format!("uid-{name}"),
        name: name.to_owned(),
        namespace: namespace.map(str::to_owned),
        labels: Default::default(),
        owners: vec![],
        created: Some("2026-01-01T00:00:00Z".into()),
        data,
    }
}

/// Lists fixed objects per kind, then stays open for live changes.
#[derive(Default)]
pub struct ScriptedSource {
    objects: Mutex<HashMap<String, Vec<RawObject>>>,
    live: Mutex<Vec<mpsc::Sender<WatchEvent>>>,
}

impl ScriptedSource {
    pub fn with(self, kind: &str, objects: Vec<RawObject>) -> Self {
        self.objects
            .lock()
            .unwrap()
            .insert(kind.to_owned(), objects);
        self
    }

    /// Sends a change to every open watch.
    pub fn send(&self, event: WatchEvent) {
        for watch in self.live.lock().unwrap().iter() {
            watch.try_send(event.clone()).expect("the watch has room");
        }
    }
}

impl WatchSource for ScriptedSource {
    fn watch(&self, kind: &ResourceKind, events: mpsc::Sender<WatchEvent>, _: CancellationToken) {
        let objects = self
            .objects
            .lock()
            .unwrap()
            .get(&kind.kind)
            .cloned()
            .unwrap_or_default();
        self.live.lock().unwrap().push(events.clone());
        tokio::spawn(async move {
            let _ = events.send(WatchEvent::Restarted { scope: None }).await;
            for object in objects {
                let scope = None;
                let _ = events.send(WatchEvent::Listed { scope, object }).await;
            }
            let _ = events.send(WatchEvent::Synced { scope: None }).await;
        });
    }
}

/// A forward that holds a real loopback port, so a port in use is refused as it
/// would be.
pub struct FakeForward {
    pub request: ForwardRequest,
    pub feed: ForwardFeed,
    listener: Option<std::net::TcpListener>,
}

impl FakeForward {
    pub fn set_state(&self, state: ForwardState) {
        self.feed.state.send_replace(state);
    }

    /// Releases the port, then reports the failure, as a real forward does.
    pub fn fail(&mut self, failure: ForwardFailure) {
        self.listener = None;
        self.set_state(ForwardState::Failed(failure));
    }
}

/// The services a view sees, over a real store and a fixed set of objects.
pub struct FakeServices {
    runtime: Runtime,
    store: Option<ClusterStore>,
    contributions: Arc<ContributionSet>,
    pub objects: Mutex<Vec<(ObjectTarget, Value)>>,
    pub events: Mutex<Vec<EventRecord>>,
    pub deleted: Mutex<Vec<ObjectTarget>>,
    /// How deleting the object of this name fails.
    pub delete_errors: Mutex<HashMap<String, ObjectError>>,
    /// Holds delete replies back until it has permits.
    pub delete_gate: Mutex<Option<Arc<tokio::sync::Semaphore>>>,
    /// Every write, in order.
    pub writes: Mutex<Vec<(WriteMode, Value)>>,
    /// What the next writes answer, oldest first. With none left, a write
    /// echoes its object.
    pub write_results: Mutex<Vec<Result<Value, ObjectError>>>,
    /// One feed per opened log stream, newest last.
    pub log_feeds: Mutex<Vec<(LogRequest, LogFeed)>>,
    pub exec_feeds: Mutex<Vec<(ExecRequest, ExecFeed)>>,
    /// One per started local shell, oldest first.
    pub local_feeds: Mutex<Vec<ExecFeed>>,
    /// One per started forward, oldest first.
    pub forwards: Mutex<Vec<FakeForward>>,
    /// Limits for the buffers of streams opened from now on.
    pub log_limits: Mutex<Option<(usize, usize)>>,
    /// Makes `open_logs` report a disconnected cluster.
    pub logs_disconnected: Mutex<bool>,
    pub access: Mutex<Option<UpdateAccess>>,
    pub access_failure: Mutex<Option<String>>,
    /// Why the next delete fails.
    pub delete_failure: Mutex<Option<String>>,
    pub requests: Mutex<Vec<(String, ExtensionRequest)>>,
    pub tab_ui: Mutex<Option<UiNode>>,
    /// Extension replies wait while this holds false.
    gate: watch::Sender<bool>,
}

impl FakeServices {
    /// Services for tests that open no views.
    pub fn idle() -> Self {
        Self::build(None, ContributionSet::default())
    }

    pub fn new(source: Arc<dyn WatchSource>, contributions: ContributionSet) -> Self {
        Self::build(Some(source), contributions)
    }

    fn build(source: Option<Arc<dyn WatchSource>>, contributions: ContributionSet) -> Self {
        let runtime = Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("a runtime");
        let resolved = contributions.clone();
        let columns: ColumnResolver = Arc::new(move |kind: &ResourceKind| {
            columns_for(kind, &resolved)
                .declared
                .into_iter()
                .map(|column| column.spec)
                .collect()
        });
        let store = source.map(|source| {
            let _runtime = runtime.enter();
            ClusterStore::new(source, columns, StoreOptions::default())
        });
        Self {
            runtime,
            store,
            contributions: Arc::new(contributions),
            objects: Mutex::default(),
            events: Mutex::default(),
            deleted: Mutex::default(),
            delete_errors: Mutex::default(),
            delete_gate: Mutex::default(),
            writes: Mutex::default(),
            write_results: Mutex::default(),
            log_feeds: Mutex::default(),
            exec_feeds: Mutex::default(),
            local_feeds: Mutex::default(),
            forwards: Mutex::default(),
            log_limits: Mutex::default(),
            logs_disconnected: Mutex::default(),
            access: Mutex::default(),
            access_failure: Mutex::default(),
            delete_failure: Mutex::default(),
            requests: Mutex::default(),
            tab_ui: Mutex::default(),
            gate: watch::channel(true).0,
        }
    }

    pub fn add_object(&self, kind: &ResourceKind, namespace: Option<&str>, object: Value) {
        let target = ObjectTarget {
            kind: kind.clone(),
            namespace: namespace.map(str::to_owned),
            name: object["metadata"]["name"]
                .as_str()
                .expect("a name")
                .to_owned(),
        };
        self.objects.lock().unwrap().push((target, object));
    }

    fn write(&self, object: Value, mode: WriteMode) -> Reply<Value> {
        self.writes.lock().unwrap().push((mode, object.clone()));
        let mut results = self.write_results.lock().unwrap();
        let result = if results.is_empty() {
            Ok(object)
        } else {
            results.remove(0)
        };
        Box::pin(async move { result })
    }

    /// Makes extensions stop answering until `release_extensions`.
    pub fn stall_extensions(&self) {
        self.gate.send_replace(false);
    }

    pub fn release_extensions(&self) {
        self.gate.send_replace(true);
    }
}

impl ClusterServices for FakeServices {
    fn open_view(&self, _: &str, kind: &ResourceKind, query: ViewQuery) -> Option<View> {
        let _runtime = self.runtime.enter();
        self.store.as_ref().map(|store| store.view(kind, query))
    }

    fn get_object(&self, _: &str, target: ObjectTarget) -> Reply<Value> {
        let found = self
            .objects
            .lock()
            .unwrap()
            .iter()
            .find(|(candidate, _)| *candidate == target)
            .map(|(_, object)| object.clone());
        Box::pin(async move { found.ok_or_else(|| ObjectError::NotFound("not found".into())) })
    }

    fn events(&self, _: &str, _: Option<String>, _: String) -> Reply<Vec<EventRecord>> {
        let events = self.events.lock().unwrap().clone();
        Box::pin(async move { Ok(events) })
    }

    fn delete_object(&self, _: &str, target: ObjectTarget) -> Reply<()> {
        let named = self.delete_errors.lock().unwrap().remove(&target.name);
        self.deleted.lock().unwrap().push(target);
        let failure = self.delete_failure.lock().unwrap().take();
        let gate = self.delete_gate.lock().unwrap().clone();
        Box::pin(async move {
            if let Some(gate) = gate {
                let _permit = gate.acquire().await;
            }
            if let Some(error) = named {
                return Err(error);
            }
            failure.map_or(Ok(()), |message| Err(ObjectError::Failed(message)))
        })
    }

    fn replace_object(
        &self,
        _: &str,
        _: ObjectTarget,
        object: Value,
        mode: WriteMode,
    ) -> Reply<Value> {
        self.write(object, mode)
    }

    fn create_object(
        &self,
        _: &str,
        _: ObjectTarget,
        object: Value,
        mode: WriteMode,
    ) -> Reply<Value> {
        self.write(object, mode)
    }

    fn update_access(&self, _: &str, _: ObjectTarget) -> Reply<UpdateAccess> {
        let access = self.access.lock().unwrap().clone().unwrap_or(UpdateAccess {
            allowed: true,
            reason: None,
        });
        let failure = self.access_failure.lock().unwrap().take();
        Box::pin(
            async move { failure.map_or(Ok(access), |message| Err(ObjectError::Failed(message))) },
        )
    }

    fn open_logs(&self, _: &str, request: LogRequest) -> Option<LogStream> {
        if *self.logs_disconnected.lock().unwrap() {
            return None;
        }
        let buffer = match *self.log_limits.lock().unwrap() {
            Some((lines, bytes)) => LogBuffer::with_limits(lines, bytes),
            None => LogBuffer::default(),
        };
        let (feed, stream) = kubepulse_cluster::log_channel(buffer);
        self.log_feeds.lock().unwrap().push((request, feed));
        Some(stream)
    }

    fn open_exec(&self, _: &str, request: ExecRequest, size: ExecSize) -> Option<ExecStream> {
        let (feed, stream) = kubepulse_cluster::exec_channel(size);
        self.exec_feeds.lock().unwrap().push((request, feed));
        Some(stream)
    }

    fn open_local_shell(&self, size: ExecSize) -> ExecStream {
        let (feed, stream) = kubepulse_cluster::exec_channel(size);
        self.local_feeds.lock().unwrap().push(feed);
        stream
    }

    fn start_forward(
        &self,
        _: &str,
        request: ForwardRequest,
    ) -> Result<PortForward, ForwardStartError> {
        let listener = bind_local(request.local_port).map_err(ForwardStartError::Port)?;
        let (feed, forward) = forward_channel(listener.local_addr().unwrap().port());
        self.forwards.lock().unwrap().push(FakeForward {
            request,
            feed,
            listener: Some(listener),
        });
        Ok(forward)
    }

    fn contributions(&self) -> Arc<ContributionSet> {
        self.contributions.clone()
    }

    fn kind_opened(&self, _: KindRef) {}

    fn extension(
        &self,
        id: &str,
        request: ExtensionRequest,
    ) -> Reply<ExtensionResponse, CallError> {
        self.requests
            .lock()
            .unwrap()
            .push((id.to_owned(), request.clone()));
        let mut gate = self.gate.subscribe();
        let ui = self.tab_ui.lock().unwrap().clone();
        Box::pin(async move {
            let _ = gate.wait_for(|open| *open).await;
            match request {
                ExtensionRequest::ComputeColumn { targets, .. } => Ok(ExtensionResponse::Cells {
                    cells: targets
                        .iter()
                        .map(|target| format!("computed {}", target.name))
                        .collect(),
                }),
                _ => ui.map_or_else(
                    || {
                        Err(CallError::Failed {
                            message: "no interface".into(),
                        })
                    },
                    |root| Ok(ExtensionResponse::Ui { root }),
                ),
            }
        })
    }
}
