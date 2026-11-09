//! Browses a real cluster through the real runtime, host, and store. It creates
//! and deletes objects, so it refuses any context but the throwaway
//! `kubepulse-e2e` minikube profile. `CLAUDE.md` shows how to run it.

use std::{
    env,
    future::Future,
    io::{ErrorKind, Read as _, Write as _},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use kubepulse_cluster::{
    ConnectionState, ExecRequest, ExecSize, ExecState, ExecStream, ForwardFailure, ForwardKind,
    ForwardRequest, ForwardState, ObjectTarget, PortForward, ResourceKind, SessionOptions,
    bind_local,
};
use kubepulse_extension_host::{ContributionSet, ExtensionHost, HostOptions};
use kubepulse_store::{Row, View, ViewQuery};
use kubepulse_ui::{
    columns::{NavigationGroup, columns_for, navigation},
    runtime::{
        ClusterBackend, ClusterCommand, ClusterRuntime, ClusterState, ConnectedClusters,
        RuntimeServices, StoreConfig,
    },
    services::{ClusterServices, ForwardStartError},
    terminal::{Emulator, Palette},
};
use tokio::{
    sync::{mpsc, watch},
    time,
};

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

    fn within<T>(&self, work: impl Future<Output = T>) -> Option<T> {
        self.wait(async { time::timeout(WAIT, work).await.ok() })
    }

    fn shell(&self, pod: &str, command: Option<Vec<String>>) -> ExecStream {
        let request = ExecRequest {
            namespace: NAMESPACE.to_owned(),
            pod: pod.to_owned(),
            container: pod.to_owned(),
            command,
        };
        let size = ExecSize { cols: 80, rows: 24 };
        self.services
            .open_exec(CONTEXT, request, size)
            .expect("the cluster is connected")
    }

    fn forward(&self, kind: ForwardKind, name: &str, port: u16) -> PortForward {
        self.services
            .start_forward(CONTEXT, request(kind, name, port))
            .expect("the forward starts")
    }

    #[track_caller]
    fn wait_for_forward(
        &self,
        forward: &PortForward,
        reached: impl FnMut(&ForwardState) -> bool,
    ) -> ForwardState {
        let mut states = forward.subscribe();
        self.within(async { states.wait_for(reached).await.map(|state| state.clone()) })
            .expect("the forward reaches the state")
            .expect("the forward is still open")
    }

    #[track_caller]
    fn wait_for_state(
        &self,
        shell: &ExecStream,
        reached: impl FnMut(&ExecState) -> bool,
    ) -> ExecState {
        let mut states = shell.subscribe();
        self.within(async { states.wait_for(reached).await.map(|state| state.clone()) })
            .expect("the shell reaches the state")
            .expect("the shell is still open")
    }

    fn show_until(
        &self,
        shell: &ExecStream,
        output: &mut mpsc::Receiver<Vec<u8>>,
        screen: &Emulator,
        wanted: &str,
    ) {
        let shown = || {
            screen
                .visible_lines()
                .iter()
                .any(|line| line.contains(wanted))
        };
        self.within(async {
            while !shown() {
                let bytes = output.recv().await.expect("the output is open");
                let replies = screen.advance(&bytes);
                if !replies.is_empty() {
                    shell.input().send(replies).expect("the shell is open");
                }
            }
        })
        .unwrap_or_else(|| panic!("no {wanted:?} in {:?}", screen.visible_lines()));
    }

    fn read_until(&self, output: &mut mpsc::Receiver<Vec<u8>>, wanted: &str) -> String {
        let mut text = String::new();
        self.within(async {
            while !text.contains(wanted) {
                let bytes = output.recv().await.expect("the output is open");
                text.push_str(&String::from_utf8_lossy(&bytes));
            }
        })
        .unwrap_or_else(|| panic!("no {wanted:?} in {text:?}"));
        text
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
        "run",
        "unpullable",
        "--image=kubepulse.invalid/none:1",
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
    let backend = Arc::new(ClusterBackend::new(clusters.clone()).0);
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

    // A pod whose phase is Pending shows why its container is waiting.
    wait_for("the waiting reason", WAIT, || {
        let unpullable = row(&table, "unpullable")?;
        let status = cell(&pods, &contributions, &unpullable, "Status");
        (status.contains("ErrImagePull") || status.contains("ImagePullBackOff")).then_some(())
    });

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

    // Real container runtimes decide what a missing shell looks like.
    for (pod, image) in [
        (
            "shelly",
            "--image=registry.k8s.io/e2e-test-images/busybox:1.36.1-1",
        ),
        ("silent", "--image=registry.k8s.io/pause:3.9"),
    ] {
        kubectl.run(&["run", pod, image, "-n", NAMESPACE, "--", "sleep", "3600"]);
    }
    kubectl.run(&[
        "wait",
        "pod/shelly",
        "--for=condition=Ready",
        "--timeout=120s",
        "-n",
        NAMESPACE,
    ]);

    let mut shell = browser.shell("shelly", None);
    let mut output = shell.take_output().expect("the output is free");
    browser.wait_for_state(&shell, |state| *state == ExecState::Running);
    assert!(
        browser.read_until(&mut output, "#").contains('#'),
        "busybox has no bash, so ash or sh opened"
    );
    wait_for("the shell to count as work", WAIT, || {
        let work = state.borrow().status(CONTEXT)?.active_work.clone();
        (work.len() == 1).then_some(())
    });
    let type_line = |shell: &ExecStream, line: &str| {
        shell
            .input()
            .send(line.as_bytes().to_vec())
            .expect("the shell is open");
    };
    type_line(&shell, "echo pulse-$((40+2))\n");
    browser.read_until(&mut output, "pulse-42");
    shell.resize(ExecSize {
        cols: 101,
        rows: 33,
    });
    // The size travels apart from what is typed, so it must land first.
    thread::sleep(Duration::from_secs(1));
    type_line(&shell, "stty size\n");
    browser.read_until(&mut output, "33 101");
    type_line(&shell, "echo term=$TERM.\n");
    let term = browser.read_until(&mut output, "term=xterm");
    assert!(term.contains("term=xterm"), "{term:?}");
    type_line(&shell, "exit 7\n");
    assert_eq!(
        browser.wait_for_state(&shell, |state| *state != ExecState::Running),
        ExecState::Exited(7)
    );

    // Full-screen programs run through the real emulator, which answers their
    // terminal queries the way a tab does.
    let vi = vec!["vi".to_owned(), "/tmp/pulse.txt".to_owned()];
    let mut editor = browser.shell("shelly", Some(vi));
    let mut output = editor.take_output().expect("the output is free");
    let screen = Emulator::new(80, 24, Palette::default());
    browser.show_until(&editor, &mut output, &screen, "~");
    // Sent together, an escape and what follows it read as one special key.
    type_line(&editor, "ipulse-from-vi");
    thread::sleep(Duration::from_secs(1));
    type_line(&editor, "\x1b");
    thread::sleep(Duration::from_secs(1));
    type_line(&editor, ":wq\r");
    assert_eq!(
        browser.wait_for_state(&editor, |state| !matches!(
            state,
            ExecState::Connecting | ExecState::Running
        )),
        ExecState::Exited(0)
    );
    let mut reader = browser.shell(
        "shelly",
        Some(vec!["cat".to_owned(), "/tmp/pulse.txt".to_owned()]),
    );
    let mut output = reader.take_output().expect("the output is free");
    browser.read_until(&mut output, "pulse-from-vi");

    let mut top = browser.shell("shelly", Some(vec!["top".to_owned()]));
    let mut output = top.take_output().expect("the output is free");
    let screen = Emulator::new(80, 24, Palette::default());
    browser.show_until(&top, &mut output, &screen, "PID");
    assert!(
        screen
            .visible_lines()
            .iter()
            .any(|line| line.contains("top")),
        "{:?}",
        screen.visible_lines()
    );
    type_line(&top, "q");
    assert_eq!(
        browser.wait_for_state(&top, |state| !matches!(
            state,
            ExecState::Connecting | ExecState::Running
        )),
        ExecState::Exited(0)
    );

    let no_shell = browser.shell("silent", None);
    assert_eq!(
        browser.wait_for_state(&no_shell, |state| *state != ExecState::Connecting),
        ExecState::NoShell
    );
    let missing = browser.shell("silent", Some(vec!["/not/here".to_owned()]));
    let ended = browser.wait_for_state(&missing, |state| {
        !matches!(state, ExecState::Connecting | ExecState::Running)
    });
    assert!(
        matches!(ended, ExecState::Failed(_) | ExecState::Exited(1..)),
        "{ended:?}"
    );

    // Forwards reach the pod that answers, which `/hostname` names.
    kubectl.run(&[
        "create",
        "deployment",
        "echo",
        "--image=registry.k8s.io/e2e-test-images/agnhost:2.47",
        "--replicas=2",
        "-n",
        NAMESPACE,
        "--",
        "/agnhost",
        "netexec",
        "--http-port=8080",
    ]);
    kubectl.run(&[
        "expose",
        "deployment",
        "echo",
        "--port=80",
        "--target-port=8080",
        "-n",
        NAMESPACE,
    ]);
    kubectl.run(&[
        "wait",
        "--for=condition=Available",
        "deployment/echo",
        "-n",
        NAMESPACE,
        "--timeout=120s",
    ]);
    let echo_pods = || {
        let names = kubectl.run(&[
            "get",
            "pods",
            "-n",
            NAMESPACE,
            "-l",
            "app=echo",
            "--field-selector=status.phase=Running",
            "-o",
            "jsonpath={.items[*].metadata.name}",
        ]);
        names
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    let target = echo_pods()[0].clone();
    let pod_forward = browser.forward(ForwardKind::Pod, &target, 8080);
    let mut pod_states = pod_forward.subscribe();
    browser.wait_for_forward(&pod_forward, |state| {
        matches!(state, ForwardState::Active { .. })
    });
    assert_eq!(hostname(pod_forward.local_port()), target);
    assert!(
        std::net::TcpStream::connect((outside_address(), pod_forward.local_port())).is_err(),
        "only loopback connections are accepted"
    );
    let taken = browser.services.start_forward(
        CONTEXT,
        ForwardRequest {
            local_port: Some(pod_forward.local_port()),
            ..request(ForwardKind::Pod, &target, 8080)
        },
    );
    assert!(
        matches!(taken, Err(ForwardStartError::Port(ref error)) if error.kind() == ErrorKind::AddrInUse),
        "a port in use starts nothing"
    );

    let service_forward = browser.forward(ForwardKind::Service, "echo", 80);
    let ForwardState::Active { pod: backend, .. } = browser
        .wait_for_forward(&service_forward, |state| {
            matches!(state, ForwardState::Active { .. })
        })
    else {
        unreachable!()
    };
    assert_eq!(hostname(service_forward.local_port()), backend);

    kubectl.run(&["delete", "pod", &backend, "-n", NAMESPACE, "--wait=false"]);
    let moved = browser.wait_for_forward(
        &service_forward,
        |state| matches!(state, ForwardState::Active { pod, .. } if *pod != backend),
    );
    let ForwardState::Active {
        pod: replacement, ..
    } = moved
    else {
        unreachable!()
    };
    assert_eq!(hostname(service_forward.local_port()), replacement);
    if backend != target {
        kubectl.run(&["delete", "pod", &target, "-n", NAMESPACE, "--wait=false"]);
    }
    let failed = ForwardState::Failed(ForwardFailure::PodDeleted);
    browser.wait_for_forward(&pod_forward, |state| *state == failed);
    bind_local(Some(pod_forward.local_port())).expect("a failed forward releases its port");
    let mut service_states = service_forward.subscribe();

    let open_shell = browser.shell("shelly", None);
    browser.wait_for_state(&open_shell, |state| *state == ExecState::Running);
    let mut shell_states = open_shell.subscribe();

    commands
        .send(ClusterCommand::Disconnect(CONTEXT.to_owned()))
        .expect("the runtime is running");
    wait_for("the disconnect", WAIT, || {
        (state.borrow().status(CONTEXT)?.state == ConnectionState::Disconnected).then_some(())
    });
    assert!(
        browser
            .within(async { while shell_states.changed().await.is_ok() {} })
            .is_some(),
        "disconnecting ends the shell"
    );
    for states in [&mut pod_states, &mut service_states] {
        assert!(
            browser
                .within(async { while states.changed().await.is_ok() {} })
                .is_some(),
            "disconnecting ends forwards"
        );
    }
    bind_local(Some(service_forward.local_port())).expect("the forward's port was released");
}

fn request(kind: ForwardKind, name: &str, port: u16) -> ForwardRequest {
    ForwardRequest {
        kind,
        namespace: NAMESPACE.to_owned(),
        name: name.to_owned(),
        port,
        local_port: None,
    }
}

/// An address of this machine that other machines can reach.
fn outside_address() -> std::net::IpAddr {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").expect("a UDP socket");
    // Connecting a UDP socket only picks a route, and sends nothing.
    socket.connect("192.0.2.1:9").expect("a route");
    socket.local_addr().expect("a local address").ip()
}

/// The pod that answers `/hostname` through the local port.
fn hostname(port: u16) -> String {
    wait_for("an answer through the forward", WAIT, || {
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).ok()?;
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
        stream
            .write_all(b"GET /hostname HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .ok()?;
        let mut response = Vec::new();
        let mut chunk = [0; 1024];
        // The response is read by its length, because a forwarded connection
        // need not close when the pod closes it.
        loop {
            let read = stream.read(&mut chunk).ok()?;
            if read == 0 {
                return None;
            }
            response.extend_from_slice(&chunk[..read]);
            let text = String::from_utf8_lossy(&response);
            let Some((head, body)) = text.split_once("\r\n\r\n") else {
                continue;
            };
            let length = head.lines().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())?
            })?;
            if body.len() >= length {
                return Some(body[..length].to_owned());
            }
        }
    })
}
