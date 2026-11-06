mod support;

use kubepulse_cluster::{
    ConnectionState, ObjectError, ObjectTarget, ResourceKind, SessionHandle, SessionManager,
    WriteMode, manifest,
};
use serde_json::{Value, json};
use support::*;
use tempfile::TempDir;
use wiremock::{
    Mock, MockServer, Request, ResponseTemplate,
    matchers::{method, path, query_param, query_param_is_missing},
};

const OBJECT_PATH: &str = "/api/v1/namespaces/default/configmaps/app";

struct Connected {
    handle: SessionHandle,
    _sessions: SessionManager,
    _dir: TempDir,
}

async fn connect(server: &MockServer) -> Connected {
    let dir = temp();
    let list = kubeconfig(dir.path(), &[("a", &server.uri(), Auth::None)]);
    let mut sessions = SessionManager::new(list_channel(list), fast_options());
    sessions.connect("a").unwrap();
    let session = sessions.session("a").unwrap();
    wait_until(&mut session.subscribe_state(), |state| {
        *state == ConnectionState::Connected
    })
    .await;
    Connected {
        handle: session.handle(),
        _sessions: sessions,
        _dir: dir,
    }
}

fn target() -> ObjectTarget {
    ObjectTarget {
        kind: ResourceKind {
            group: String::new(),
            version: "v1".into(),
            kind: "ConfigMap".into(),
            plural: "configmaps".into(),
            namespaced: true,
            verbs: vec!["get".into(), "update".into()],
            printer_columns: vec![],
        },
        namespace: Some("default".into()),
        name: "app".into(),
    }
}

fn live(version: &str, value: &str) -> Value {
    json!({
        "apiVersion": "v1",
        "kind": "ConfigMap",
        "metadata": {"name": "app", "namespace": "default", "resourceVersion": version},
        "data": {"key": value},
    })
}

fn reply(object: &Value) -> ResponseTemplate {
    json(&object.to_string())
}

fn status(code: u16, reason: &str, message: &str) -> ResponseTemplate {
    let body = json!({
        "kind": "Status",
        "apiVersion": "v1",
        "status": "Failure",
        "reason": reason,
        "message": message,
        "code": code,
    });
    ResponseTemplate::new(code).set_body_raw(body.to_string(), "application/json")
}

async fn writes(server: &MockServer) -> Vec<Request> {
    let requests = server.received_requests().await.unwrap_or_default();
    requests
        .into_iter()
        .filter(|request| request.method.as_str() == "PUT")
        .collect()
}

#[tokio::test]
async fn a_read_object_keeps_its_api_version_and_kind() {
    let server = api_server().await;
    get(OBJECT_PATH, reply(&live("1", "old")))
        .mount(&server)
        .await;
    let connected = connect(&server).await;

    let object = connected.handle.get_object(&target()).await.unwrap();

    assert_eq!(object["apiVersion"], "v1");
    assert_eq!(object["kind"], "ConfigMap");
}

#[tokio::test]
async fn a_dry_run_asks_the_server_not_to_persist_and_returns_its_mutations() {
    let server = api_server().await;
    let mut mutated = live("1", "submitted");
    mutated["metadata"]["labels"] = json!({"injected-by": "admission"});
    Mock::given(method("PUT"))
        .and(path(OBJECT_PATH))
        .and(query_param("dryRun", "All"))
        .respond_with(reply(&mutated))
        .mount(&server)
        .await;
    let connected = connect(&server).await;

    let result = connected
        .handle
        .replace_object(&target(), live("1", "submitted"), WriteMode::DryRun)
        .await
        .unwrap();

    assert_eq!(result["metadata"]["labels"]["injected-by"], "admission");
    let diff = manifest::patch(
        &manifest::to_yaml(&manifest::strip(&live("1", "submitted"))),
        &manifest::to_yaml(&manifest::strip(&result)),
    );
    assert!(diff.contains("+  labels:"), "{diff}");
}

#[tokio::test]
async fn applying_sends_the_opened_resource_version_without_a_dry_run() {
    let server = api_server().await;
    Mock::given(method("PUT"))
        .and(path(OBJECT_PATH))
        .and(query_param_is_missing("dryRun"))
        .respond_with(reply(&live("2", "new")))
        .mount(&server)
        .await;
    let connected = connect(&server).await;

    let edited = manifest::restore(manifest::strip(&live("7", "new")), &live("7", "old"));
    connected
        .handle
        .replace_object(&target(), edited, WriteMode::Apply)
        .await
        .unwrap();

    let sent: Value = writes(&server).await[0].body_json().unwrap();
    assert_eq!(sent["metadata"]["resourceVersion"], "7");
    assert_eq!(sent["data"]["key"], "new");
}

#[tokio::test]
async fn a_stale_write_is_a_conflict_and_changes_nothing() {
    let server = api_server().await;
    Mock::given(method("PUT"))
        .and(path(OBJECT_PATH))
        .respond_with(status(409, "Conflict", "the object has been modified"))
        .mount(&server)
        .await;
    let connected = connect(&server).await;

    let result = connected
        .handle
        .replace_object(&target(), live("1", "mine"), WriteMode::Apply)
        .await;

    assert_eq!(
        result,
        Err(ObjectError::Conflict("the object has been modified".into()))
    );
}

#[tokio::test]
async fn a_rejected_dry_run_returns_the_servers_message() {
    let server = api_server().await;
    Mock::given(method("PUT"))
        .and(path(OBJECT_PATH))
        .respond_with(status(422, "Invalid", "spec.replicas: must be positive"))
        .mount(&server)
        .await;
    let connected = connect(&server).await;

    let result = connected
        .handle
        .replace_object(&target(), live("1", "mine"), WriteMode::DryRun)
        .await;

    assert_eq!(
        result,
        Err(ObjectError::Failed(
            "spec.replicas: must be positive".into()
        ))
    );
}

#[tokio::test]
async fn the_access_review_reports_a_denial_and_its_reason() {
    let server = api_server().await;
    let review = json!({
        "apiVersion": "authorization.k8s.io/v1",
        "kind": "SelfSubjectAccessReview",
        "spec": {},
        "status": {"allowed": false, "reason": "no rule allows update"},
    });
    Mock::given(method("POST"))
        .and(path(
            "/apis/authorization.k8s.io/v1/selfsubjectaccessreviews",
        ))
        .respond_with(reply(&review))
        .mount(&server)
        .await;
    let connected = connect(&server).await;

    let access = connected.handle.update_access(&target()).await.unwrap();

    assert!(!access.allowed);
    assert_eq!(access.reason.as_deref(), Some("no rule allows update"));
}
