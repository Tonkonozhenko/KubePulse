//! Browses a real cluster through the real runtime, host, and store. It creates
//! and deletes objects, so it refuses any context but the throwaway
//! `kubepulse-e2e` minikube profile. `CLAUDE.md` shows how to run it.

use std::{
    env,
    future::Future,
    io::Write as _,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use kubepulse_cluster::{ConnectionState, ObjectTarget, ResourceKind, SessionOptions};
use kubepulse_extension_host::{ContributionSet, ExtensionHost, HostOptions};
use kubepulse_store::{Row, View, ViewQuery};
use kubepulse_ui::{
    columns::{NavigationGroup, columns_for, navigation},
    runtime::{
        ClusterBackend, ClusterCommand, ClusterRuntime, ClusterState, ConnectedClusters,
        RuntimeServices, StoreConfig,
    },
    services::ClusterServices,
};
use tokio::sync::watch;

const CONTEXT: &str = "kubepulse-e2e";
const NAMESPACE: &str = "kubepulse-e2e";
const CRD: &str = "widgets.example.com";
const WAIT: Duration = Duration::from_secs(120);
/// The spec's limit for a table reflecting a change.
const LIVE_LIMIT: Duration = Duration::from_secs(2);
// "hunter2", plain and as the API returns it.
const SECRET: &str = "hunter2";
const ENCODED_SECRET: &str = "aHVudGVyMg==";

const WIDGET_DEFINITION: &str = r#"
apiVersion: apiextensions.k8s.io/v1
kind: CustomResourceDefinition
metadata:
  name: widgets.example.com
spec:
  group: example.com
  scope: Namespaced
  names: {plural: widgets, singular: widget, kind: Widget}
  versions:
  - name: v1
    served: true
    storage: true
    schema:
      openAPIV3Schema:
        type: object
        properties:
          spec:
            type: object
            properties:
              size: {type: integer}
    additionalPrinterColumns:
    - name: Size
      type: integer
      jsonPath: .spec.size
"#;

const WIDGET: &str = r#"
apiVersion: example.com/v1
kind: Widget
metadata:
  name: big
  namespace: kubepulse-e2e
spec:
  size: 7
"#;

struct Kubectl(PathBuf);

impl Kubectl {
    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new("kubectl");
        command.env("KUBECONFIG", &self.0).args(args);
        command
    }

    fn run(&self, args: &[&str]) -> String {
        let output = self.command(args).output().expect("run kubectl");
        assert!(
            output.status.success(),
            "kubectl {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn apply(&self, manifest: &str) {
        let mut child = self
            .command(&["apply", "-f", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .expect("run kubectl");
        child
            .stdin
            .take()
            .expect("a pipe")
            .write_all(manifest.as_bytes())
            .expect("write the manifest");
        assert!(child.wait().expect("wait for kubectl").success());
    }

    fn remove_fixtures(&self) {
        for (kind, name) in [("namespace", NAMESPACE), ("crd", CRD)] {
            self.run(&["delete", kind, name, "--ignore-not-found", "--wait=false"]);
        }
    }
}

impl Drop for Kubectl {
    fn drop(&mut self) {
        self.remove_fixtures();
    }
}

fn wait_for<T>(what: &str, limit: Duration, mut found: impl FnMut() -> Option<T>) -> T {
    let started = Instant::now();
    loop {
        if let Some(value) = found() {
            return value;
        }
        assert!(started.elapsed() < limit, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(50));
    }
}

fn row(view: &View, name: &str) -> Option<Arc<Row>> {
    let snapshot = view.snapshot();
    snapshot.rows.iter().find(|row| &*row.name == name).cloned()
}

fn served(state: &watch::Receiver<Arc<ClusterState>>, name: &str) -> Option<ResourceKind> {
    let state = state.borrow();
    let resources = state.status(CONTEXT)?.resources.clone();
    resources.iter().find(|kind| kind.kind == name).cloned()
}

fn every_kind(groups: &[NavigationGroup]) -> Vec<ResourceKind> {
    groups
        .iter()
        .flat_map(|group| {
            let nested = group.sections.iter().flat_map(|section| &section.kinds);
            group.kinds.iter().chain(nested)
        })
        .map(|entry| entry.kind.clone())
        .collect()
}

fn cell(kind: &ResourceKind, contributions: &ContributionSet, row: &Row, title: &str) -> String {
    let columns = columns_for(kind, contributions).declared;
    let index = columns
        .iter()
        .position(|column| &*column.title == title)
        .unwrap_or_else(|| panic!("{} has no {title} column", kind.kind));
    format!("{:?}", row.cells[index])
}

struct Browser {
    services: RuntimeServices,
    runtime: ClusterRuntime,
}

impl Browser {
    fn view(&self, kind: &ResourceKind) -> View {
        self.services
            .open_view(CONTEXT, kind, ViewQuery::default())
            .expect("the cluster is connected")
    }

    fn wait<T>(&self, reply: impl Future<Output = T>) -> T {
        self.runtime.handle().block_on(reply)
    }
}

#[test]
#[ignore = "needs the kubepulse-e2e minikube profile"]
fn browses_a_real_cluster() {
    let kubeconfig = env::var_os("KUBEPULSE_E2E_KUBECONFIG")
        .map(PathBuf::from)
        .expect("KUBEPULSE_E2E_KUBECONFIG names the throwaway cluster's kubeconfig");
    let kubectl = Kubectl(Path::new(&kubeconfig).to_owned());
    assert_eq!(
        kubectl.run(&["config", "current-context"]),
        CONTEXT,
        "this test changes the cluster, so it only runs on {CONTEXT}"
    );
    kubectl.remove_fixtures();
    wait_for("the old fixtures to go", WAIT, || {
        kubectl
            .run(&["get", "namespace", NAMESPACE, "--ignore-not-found"])
            .is_empty()
            .then_some(())
    });
    kubectl.run(&["create", "namespace", NAMESPACE]);
    kubectl.run(&[
        "create",
        "deployment",
        "web",
        "--image=registry.k8s.io/pause:3.9",
        "-n",
        NAMESPACE,
    ]);
    kubectl.run(&[
        "create",
        "secret",
        "generic",
        "db",
        &format!("--from-literal=password={SECRET}"),
        "-n",
        NAMESPACE,
    ]);

    let clusters = Arc::new(ConnectedClusters::default());
    let backend = Arc::new(ClusterBackend(clusters.clone()));
    let host = ExtensionHost::new(backend, HostOptions::default());
    host.load(kubepulse_builtin::extensions());
    let contributions = host.contributions().borrow().clone();
    let stores = StoreConfig::for_host(clusters.clone(), &host);
    let runtime = ClusterRuntime::start(vec![kubeconfig], SessionOptions::default(), stores)
        .expect("start the runtime");
    let state = runtime.state();
    let services = RuntimeServices::new(clusters, host, runtime.handle());
    let browser = Browser { services, runtime };

    wait_for("the context list", WAIT, || {
        state.borrow().status(CONTEXT).map(|_| ())
    });
    let commands = browser.runtime.commands();
    commands
        .send(ClusterCommand::Connect(CONTEXT.to_owned()))
        .expect("the runtime is running");
    let pods = wait_for("the connection", WAIT, || served(&state, "Pod"));
    assert_eq!(
        state.borrow().status(CONTEXT).unwrap().state,
        ConnectionState::Connected
    );

    // Every kind in the navigation lists, built-in or not.
    let resources = state.borrow().status(CONTEXT).unwrap().resources.clone();
    let groups = navigation(&resources, &contributions);
    let names: Vec<&str> = groups.iter().map(|group| group.name.as_str()).collect();
    for category in [
        "Workloads",
        "Network",
        "Config",
        "Storage",
        "Access control",
    ] {
        assert!(
            names.contains(&category),
            "{category} is missing: {names:?}"
        );
    }
    let kinds = every_kind(&groups);
    assert!(kinds.len() > 21, "only {} kinds", kinds.len());
    for kind in &kinds {
        let view = browser.view(kind);
        wait_for(&format!("{} to list", kind.plural), WAIT, || {
            view.snapshot().synced.then_some(())
        });
        assert_eq!(view.snapshot().denied, None, "{}", kind.plural);
    }

    // A Deployment-owned pod, with its columns, owner, and events.
    let table = browser.view(&pods);
    let pod = wait_for("the running pod", WAIT, || {
        let snapshot = table.snapshot();
        snapshot
            .rows
            .iter()
            .find(|row| {
                row.namespace.as_deref() == Some(NAMESPACE)
                    && cell(&pods, &contributions, row, "Status").contains("Running")
            })
            .cloned()
    });
    assert!(pod.name.starts_with("web-"), "{}", pod.name);
    assert!(!cell(&pods, &contributions, &pod, "Created").contains("Empty"));
    let target = ObjectTarget {
        kind: pods.clone(),
        namespace: Some(NAMESPACE.to_owned()),
        name: pod.name.to_string(),
    };
    let object = browser
        .wait(browser.services.get_object(CONTEXT, target))
        .expect("the pod opens");
    let owner = &object["metadata"]["ownerReferences"][0];
    assert_eq!(owner["kind"], "ReplicaSet");
    let uid = object["metadata"]["uid"]
        .as_str()
        .expect("a UID")
        .to_owned();
    wait_for("the pod's events", WAIT, || {
        let reply = browser
            .services
            .events(CONTEXT, Some(NAMESPACE.to_owned()), uid.clone());
        let events = browser.wait(reply).expect("events load");
        (!events.is_empty()).then_some(())
    });

    // Secret values stay out of the table's rows.
    let secrets = served(&state, "Secret").expect("Secrets are served");
    let table = browser.view(&secrets);
    let secret = wait_for("the Secret", WAIT, || row(&table, "db"));
    let shown = format!("{secret:?}");
    assert!(
        !shown.contains(SECRET) && !shown.contains(ENCODED_SECRET),
        "{shown}"
    );

    // The table follows changes, including its own delete.
    let config_maps = served(&state, "ConfigMap").expect("ConfigMaps are served");
    let table = browser.view(&config_maps);
    wait_for("the ConfigMaps", WAIT, || {
        table.snapshot().synced.then_some(())
    });
    kubectl.run(&["create", "configmap", "live", "-n", NAMESPACE]);
    wait_for("the new object", LIVE_LIMIT, || row(&table, "live"));
    let target = ObjectTarget {
        kind: config_maps,
        namespace: Some(NAMESPACE.to_owned()),
        name: "live".to_owned(),
    };
    browser
        .wait(browser.services.delete_object(CONTEXT, target))
        .expect("the delete succeeds");
    wait_for("the deleted object to go", LIVE_LIMIT, || {
        row(&table, "live").is_none().then_some(())
    });

    // A definition installed now joins the navigation with no reconnect.
    assert!(served(&state, "Widget").is_none());
    kubectl.apply(WIDGET_DEFINITION);
    let widgets = wait_for("the new kind", WAIT, || served(&state, "Widget"));
    let resources = state.borrow().status(CONTEXT).unwrap().resources.clone();
    assert!(every_kind(&navigation(&resources, &contributions)).contains(&widgets));
    assert_eq!(
        state.borrow().status(CONTEXT).unwrap().state,
        ConnectionState::Connected
    );
    kubectl.apply(WIDGET);
    let table = browser.view(&widgets);
    let widget = wait_for("the widget", WAIT, || row(&table, "big"));
    assert!(cell(&widgets, &contributions, &widget, "Size").contains('7'));

    commands
        .send(ClusterCommand::Disconnect(CONTEXT.to_owned()))
        .expect("the runtime is running");
    wait_for("the disconnect", WAIT, || {
        (state.borrow().status(CONTEXT)?.state == ConnectionState::Disconnected).then_some(())
    });
}
