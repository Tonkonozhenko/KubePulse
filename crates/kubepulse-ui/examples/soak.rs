//! Holds a table of 10,000 changing pods open for ten minutes in a real window,
//! then checks the memory limit and that a closed kind's watch was released
//! after 5 minutes. Reads memory from `/proc`, so it runs on Linux. Exits
//! non-zero if either check fails.
//!
//! ```bash
//! cargo run -p kubepulse-ui --release --example soak
//! ```

use std::{
    fs, process,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

use gpui_kit::AppContext as _;
use gpui_kit::WindowOptions;
use kubepulse_cluster::{
    ClusterEntry, ConnectionState, EventRecord, ExecRequest, ExecSize, ExecStream, ForwardRequest,
    LogRequest, LogStream, ObjectError, ObjectTarget, PortForward, ResourceKind, UpdateAccess,
    WriteMode,
};
use kubepulse_extension_api::{ExtensionRequest, ExtensionResponse, KindRef};
use kubepulse_extension_host::{CallError, ContributionSet};
use kubepulse_store::{
    ClusterStore, StoreOptions, View, ViewQuery,
    synthetic::{Burst, SyntheticCluster, namespace_kind, pod_columns, pod_kind},
};
use kubepulse_ui::{
    cluster_model::ClusterModel,
    columns::{ColumnDef, KindColumns},
    identity::{ClusterKey, IdentityStore},
    resource_browser::ResourceBrowser,
    runtime::{ClusterState, ClusterStatus},
    services::{ClusterServices, ForwardStartError, Reply},
};
use serde_json::Value;
use tokio::{
    runtime::Runtime,
    sync::{mpsc, watch},
};

const CONTEXT: &str = "soak";
const PODS: usize = 10_000;
const SOAK: Duration = Duration::from_secs(600);
const CLOSE_SECOND_VIEW: Duration = Duration::from_secs(120);
const RELEASE_DELAY: Duration = Duration::from_secs(300);
const RELEASE_MARGIN: Duration = Duration::from_secs(30);
const SAMPLE: Duration = Duration::from_secs(5);
const MEMORY_LIMIT_MIB: u64 = 500;
/// The pods the table shows and the namespaces its picker offers.
const OPEN_WATCHES: usize = 2;

struct Services {
    runtime: Runtime,
    store: ClusterStore,
}

impl ClusterServices for Services {
    fn open_view(&self, _: &str, kind: &ResourceKind, query: ViewQuery) -> Option<View> {
        let _runtime = self.runtime.enter();
        Some(self.store.view(kind, query))
    }

    fn get_object(&self, _: &str, _: ObjectTarget) -> Reply<Value> {
        Box::pin(async { Err(ObjectError::NotConnected) })
    }

    fn events(&self, _: &str, _: Option<String>, _: String) -> Reply<Vec<EventRecord>> {
        Box::pin(async { Err(ObjectError::NotConnected) })
    }

    fn replace_object(&self, _: &str, _: ObjectTarget, _: Value, _: WriteMode) -> Reply<Value> {
        Box::pin(async { Err(ObjectError::NotConnected) })
    }

    fn create_object(&self, _: &str, _: ObjectTarget, _: Value, _: WriteMode) -> Reply<Value> {
        Box::pin(async { Err(ObjectError::NotConnected) })
    }

    fn open_exec(&self, _: &str, _: ExecRequest, _: ExecSize) -> Option<ExecStream> {
        None
    }

    fn open_local_shell(&self, size: ExecSize) -> ExecStream {
        kubepulse_cluster::exec_channel(size).1
    }

    fn start_forward(&self, _: &str, _: ForwardRequest) -> Result<PortForward, ForwardStartError> {
        Err(ForwardStartError::NotConnected)
    }

    fn open_logs(&self, _: &str, _: LogRequest) -> Option<LogStream> {
        None
    }

    fn update_access(&self, _: &str, _: ObjectTarget) -> Reply<UpdateAccess> {
        Box::pin(async { Err(ObjectError::NotConnected) })
    }

    fn delete_object(&self, _: &str, _: ObjectTarget) -> Reply<()> {
        Box::pin(async { Err(ObjectError::NotConnected) })
    }

    fn restart_object(&self, _: &str, _: ObjectTarget) -> Reply<()> {
        Box::pin(async { Err(ObjectError::NotConnected) })
    }

    fn run_cronjob(&self, _: &str, _: ObjectTarget) -> Reply<String> {
        Box::pin(async { Err(ObjectError::NotConnected) })
    }

    fn contributions(&self) -> Arc<ContributionSet> {
        Arc::default()
    }

    fn kind_opened(&self, _: KindRef) {}

    fn extension(&self, _: &str, _: ExtensionRequest) -> Reply<ExtensionResponse, CallError> {
        Box::pin(async { Err(CallError::Unavailable) })
    }
}

/// A field of `/proc/self/status`, in MiB.
fn memory_mib(field: &str) -> u64 {
    let status = fs::read_to_string("/proc/self/status").expect("read process status");
    let line = status
        .lines()
        .find_map(|line| line.strip_prefix(field))
        .expect("the field is listed");
    let kib: u64 = line
        .trim_start_matches(':')
        .trim()
        .trim_end_matches(" kB")
        .parse()
        .expect("a size in kB");
    kib / 1024
}

fn main() {
    let runtime = Runtime::new().expect("start the runtime");
    let burst = Burst {
        changes_per_second: 100,
        duration: SOAK,
    };
    let source = Arc::new(SyntheticCluster::new(PODS, Some(burst)));
    let watches = source.active.clone();
    let store = {
        let _runtime = runtime.enter();
        ClusterStore::new(source, Arc::new(|_| pod_columns()), StoreOptions::default())
    };
    let services = Arc::new(Services { runtime, store });

    let second_kind = ResourceKind {
        kind: "Service".into(),
        plural: "services".into(),
        ..pod_kind()
    };
    let mut second_view = services.open_view(CONTEXT, &second_kind, ViewQuery::default());
    let columns = KindColumns {
        declared: pod_columns()
            .iter()
            .map(|spec| ColumnDef {
                title: spec.id.clone(),
                spec: spec.clone(),
                status: false,
                timestamp: false,
            })
            .collect(),
        computed: Vec::new(),
    };

    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        let status = ClusterStatus {
            entry: ClusterEntry {
                context: CONTEXT.to_owned(),
                cluster: None,
                server: None,
                user: None,
                namespace: None,
            },
            state: ConnectionState::Connected,
            active_work: Vec::new(),
            resources: Arc::new(vec![pod_kind(), namespace_kind()]),
        };
        let state = Arc::new(ClusterState {
            version: 1,
            clusters: vec![status],
            errors: Vec::new(),
        });
        let (_state, state) = watch::channel(state);
        let (commands, _) = mpsc::unbounded_channel();
        let model = cx.new(|cx| ClusterModel::new(state, commands, IdentityStore::default(), cx));
        let key = ClusterKey::new(CONTEXT, None);
        let (_, browser) = gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            ResourceBrowser::open(&key, pod_kind(), columns, model, services, window, cx)
                .expect("the store is open")
        })
        .expect("open the window");

        cx.spawn(async move |cx| {
            let started = Instant::now();
            let mut closed_at = None;
            let mut released_after = None;
            let mut row = 0;
            while started.elapsed() < SOAK {
                cx.background_executor().timer(SAMPLE).await;
                row = (row + 700) % PODS;
                cx.update(|cx| {
                    browser.update(cx, |browser, cx| {
                        browser
                            .table()
                            .update(cx, |table, cx| table.scroll_to_row(row, cx));
                    });
                });

                if closed_at.is_none() && started.elapsed() >= CLOSE_SECOND_VIEW {
                    second_view = None;
                    closed_at = Some(Instant::now());
                }
                if let Some(closed_at) = closed_at
                    && released_after.is_none()
                    && watches.load(Ordering::SeqCst) == OPEN_WATCHES
                {
                    released_after = Some(closed_at.elapsed());
                }
                println!(
                    "{:>4}s rss {} MiB, watches {}",
                    started.elapsed().as_secs(),
                    memory_mib("VmRSS"),
                    watches.load(Ordering::SeqCst),
                );
            }
            drop(second_view);

            let peak = memory_mib("VmHWM");
            let memory_ok = peak <= MEMORY_LIMIT_MIB;
            let release_ok = released_after.is_some_and(|after| {
                after >= RELEASE_DELAY.saturating_sub(SAMPLE)
                    && after <= RELEASE_DELAY + RELEASE_MARGIN
            });
            println!("peak {peak} MiB, limit {MEMORY_LIMIT_MIB} MiB");
            println!("watch released after {released_after:?}, expected about {RELEASE_DELAY:?}");
            process::exit(if memory_ok && release_ok { 0 } else { 1 });
        })
        .detach();
    });
}
