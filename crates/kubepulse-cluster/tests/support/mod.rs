//! Fixtures shared by the integration tests.
#![allow(dead_code)]

use std::{
    env, fs,
    net::TcpListener,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use kubepulse_cluster::{ClusterList, ConnectionState, SessionOptions, load};
use tempfile::TempDir;
use tokio::sync::watch;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

pub const WAIT: Duration = Duration::from_secs(20);

pub fn fast_options() -> SessionOptions {
    SessionOptions {
        connect_timeout: Duration::from_secs(2),
        auth_timeout: Duration::from_secs(10),
        health_interval: Duration::from_millis(50),
        discovery_interval: Duration::from_millis(50),
        discovery_timeout: Duration::from_secs(2),
        reconnect_min_delay: Duration::from_millis(20),
        reconnect_max_delay: Duration::from_millis(100),
    }
}

pub fn json(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body.to_owned(), "application/json")
}

pub const VERSION: &str = r#"{"major":"1","minor":"34","gitVersion":"v1.34.0","gitCommit":"x",
"gitTreeState":"clean","buildDate":"2026-01-01T00:00:00Z","goVersion":"go1","compiler":"gc","platform":"linux/amd64"}"#;

pub fn get(route: &str, response: ResponseTemplate) -> Mock {
    Mock::given(method("GET"))
        .and(path(route))
        .respond_with(response)
}

/// A fake API server that answers version and core discovery requests.
pub async fn api_server() -> MockServer {
    let server = MockServer::start().await;
    mount_api(&server).await;
    server
}

pub async fn mount_api(server: &MockServer) {
    get("/version", json(VERSION)).mount(server).await;
    mount_core_discovery(server).await;
}

pub async fn mount_core_discovery(server: &MockServer) {
    get(
        "/api",
        json(r#"{"kind":"APIVersions","versions":["v1"],"serverAddressByClientCIDRs":[]}"#),
    )
    .mount(server)
    .await;
    get(
        "/api/v1",
        json(r#"{"kind":"APIResourceList","apiVersion":"v1","groupVersion":"v1","resources":[
            {"name":"pods","singularName":"pod","namespaced":true,"kind":"Pod","verbs":["get","list","watch"]}]}"#),
    )
    .mount(server)
    .await;
}

/// How a context in a generated kubeconfig authenticates. Helpers are left
/// interactive, as most real kubeconfigs are.
pub enum Auth {
    None,
    Helper {
        token: String,
        delay_ms: u64,
        exit_code: i32,
        message: String,
        marker: Option<PathBuf>,
    },
    /// A helper that runs `command` with no arguments.
    Command(String),
}

pub fn helper_path() -> PathBuf {
    let tests = env::current_exe().unwrap();
    let target = tests.parent().unwrap().parent().unwrap();
    target
        .join("examples")
        .join(format!("fake_credential_helper{}", env::consts::EXE_SUFFIX))
}

fn user_yaml(auth: &Auth) -> String {
    match auth {
        Auth::None => "{}".to_owned(),
        Auth::Helper {
            token,
            delay_ms,
            exit_code,
            message,
            marker,
        } => {
            let mut args = format!("[{token:?}, \"{delay_ms}\", \"{exit_code}\", {message:?}");
            if let Some(marker) = marker {
                args.push_str(&format!(", {:?}", marker.display().to_string()));
            }
            exec_yaml(&helper_path().display().to_string(), &format!("{args}]"))
        }
        Auth::Command(command) => exec_yaml(command, "[]"),
    }
}

fn exec_yaml(command: &str, args: &str) -> String {
    format!(
        "\n    exec:\n      apiVersion: client.authentication.k8s.io/v1\n      command: {command:?}\n      args: {args}"
    )
}

/// Writes a kubeconfig holding one context per `(name, server, auth)`.
pub fn kubeconfig(dir: &Path, contexts: &[(&str, &str, Auth)]) -> Arc<ClusterList> {
    let mut clusters = String::new();
    let mut named = String::new();
    let mut users = String::new();
    for (name, server, auth) in contexts {
        clusters.push_str(&format!(
            "- name: {name}\n  cluster:\n    server: {server}\n"
        ));
        named.push_str(&format!(
            "- name: {name}\n  context:\n    cluster: {name}\n    user: {name}\n"
        ));
        users.push_str(&format!("- name: {name}\n  user: {}\n", user_yaml(auth)));
    }
    let contents = format!(
        "apiVersion: v1\nkind: Config\nclusters:\n{clusters}contexts:\n{named}users:\n{users}"
    );
    let path = dir.join("config");
    fs::write(&path, contents).unwrap();
    Arc::new(load(&[path]))
}

pub fn list_channel(list: Arc<ClusterList>) -> watch::Receiver<Arc<ClusterList>> {
    let (sender, receiver) = watch::channel(list);
    // The tests never update the list, so the sender can outlive them.
    std::mem::forget(sender);
    receiver
}

pub fn temp() -> TempDir {
    tempfile::tempdir().unwrap()
}

pub async fn wait_until(
    state: &mut watch::Receiver<ConnectionState>,
    expected: impl FnMut(&ConnectionState) -> bool,
) -> ConnectionState {
    tokio::time::timeout(WAIT, state.wait_for(expected))
        .await
        .expect("the state should be reached")
        .expect("the session should still exist")
        .clone()
}

/// A port nothing is listening on.
pub fn closed_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}
