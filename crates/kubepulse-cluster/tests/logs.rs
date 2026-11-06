mod support;

use std::{fmt::Write as _, time::Duration};

use kubepulse_cluster::{
    ConnectionState, LogRequest, LogState, LogStatus, LogStream, SessionHandle, SessionManager,
};
use serde_json::{Value, json};
use support::*;
use tempfile::TempDir;
use tokio::{sync::watch, time};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, query_param, query_param_is_missing},
};

const LOG_PATH: &str = "/api/v1/namespaces/default/pods/web/log";
const POD_PATH: &str = "/api/v1/namespaces/default/pods/web";

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

fn request(container: &str, previous: bool) -> LogRequest {
    LogRequest {
        namespace: "default".into(),
        pod: "web".into(),
        container: container.into(),
        previous,
    }
}

fn text(body: String) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body, "text/plain")
}

fn pod(running: bool) -> Value {
    let state = if running {
        json!({"running": {}})
    } else {
        json!({"terminated": {"exitCode": 0}})
    };
    json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {"name": "web", "namespace": "default"},
        "status": {"containerStatuses": [{
            "name": "app",
            "state": state,
            "image": "app",
            "imageID": "",
            "ready": true,
            "restartCount": 0,
        }]},
    })
}

async fn finished(status: &mut watch::Receiver<LogStatus>) -> LogStatus {
    let ended = |status: &LogStatus| matches!(status.state, LogState::Ended | LogState::Failed(_));
    let result = time::timeout(WAIT, status.wait_for(ended)).await;
    result.expect("the stream ends").unwrap().clone()
}

fn texts(stream: &LogStream) -> Vec<String> {
    let bounds = stream.buffer().bounds();
    let lines = stream.buffer().lines(bounds);
    lines
        .iter()
        .map(|(_, line)| line.text().to_owned())
        .collect()
}

#[tokio::test]
async fn the_chosen_container_is_read_with_timestamps() {
    let server = api_server().await;
    Mock::given(method("GET"))
        .and(path(LOG_PATH))
        .and(query_param("container", "setup"))
        .and(query_param("timestamps", "true"))
        .and(query_param("follow", "true"))
        .respond_with(text("2026-01-01T00:00:00Z one\n".into()))
        .mount(&server)
        .await;
    get(POD_PATH, json(&pod(false).to_string()))
        .mount(&server)
        .await;
    let connected = connect(&server).await;

    let stream = connected.handle.stream_logs(request("setup", false));
    let status = finished(&mut stream.subscribe()).await;

    assert_eq!(status.state, LogState::Ended);
    assert_eq!(texts(&stream), ["one"]);
}

#[tokio::test]
async fn the_previous_instance_is_read_once_without_following() {
    let server = api_server().await;
    Mock::given(method("GET"))
        .and(path(LOG_PATH))
        .and(query_param("previous", "true"))
        .and(query_param_is_missing("follow"))
        .respond_with(text("2026-01-01T00:00:00Z crashed\n".into()))
        .expect(1)
        .mount(&server)
        .await;
    let connected = connect(&server).await;

    let stream = connected.handle.stream_logs(request("app", true));
    let status = finished(&mut stream.subscribe()).await;

    assert_eq!(status.state, LogState::Ended);
    assert_eq!(texts(&stream), ["crashed"]);
}

#[tokio::test]
async fn a_refused_stream_reports_the_servers_message() {
    let server = api_server().await;
    let denial = json!({
        "kind": "Status",
        "apiVersion": "v1",
        "status": "Failure",
        "reason": "BadRequest",
        "message": "container app is waiting to start",
        "code": 400,
    });
    Mock::given(method("GET"))
        .and(path(LOG_PATH))
        .respond_with(
            ResponseTemplate::new(400).set_body_raw(denial.to_string(), "application/json"),
        )
        .mount(&server)
        .await;
    let connected = connect(&server).await;

    let stream = connected.handle.stream_logs(request("app", false));
    let status = finished(&mut stream.subscribe()).await;

    assert_eq!(
        status.state,
        LogState::Failed("container app is waiting to start".into())
    );
}

#[tokio::test]
async fn invalid_utf8_is_kept_as_a_readable_line() {
    let server = api_server().await;
    let body = b"2026-01-01T00:00:00Z bad \xff byte\n".to_vec();
    Mock::given(method("GET"))
        .and(path(LOG_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/plain"))
        .mount(&server)
        .await;
    get(POD_PATH, json(&pod(false).to_string()))
        .mount(&server)
        .await;
    let connected = connect(&server).await;

    let stream = connected.handle.stream_logs(request("app", false));
    finished(&mut stream.subscribe()).await;

    assert_eq!(texts(&stream), ["bad \u{fffd} byte"]);
}

#[tokio::test]
async fn a_high_volume_stream_stays_bounded_and_counts_what_it_dropped() {
    let server = api_server().await;
    let mut body = String::new();
    for number in 0..150_000 {
        writeln!(body, "2026-01-01T00:00:00.{number:09}Z line {number}").unwrap();
    }
    Mock::given(method("GET"))
        .and(path(LOG_PATH))
        .respond_with(text(body))
        .mount(&server)
        .await;
    get(POD_PATH, json(&pod(false).to_string()))
        .mount(&server)
        .await;
    let connected = connect(&server).await;

    let stream = connected.handle.stream_logs(request("app", false));
    finished(&mut stream.subscribe()).await;

    let buffer = stream.buffer();
    assert_eq!(buffer.bounds(), 50_000..150_000);
    assert_eq!(buffer.dropped(), 50_000);
    assert_eq!(buffer.lines(149_999..150_000)[0].1.text(), "line 149999");
}

#[tokio::test]
async fn an_interrupted_stream_resumes_without_duplicates() {
    let server = api_server().await;
    let first = "2026-01-01T00:00:01.100000000Z one\n\
                 2026-01-01T00:00:01.700000000Z two\n\
                 2026-01-01T00:00:01.700000000Z two again\n";
    // Resuming from the whole second replays the lines already delivered.
    let second = "2026-01-01T00:00:01.100000000Z one\n\
                  2026-01-01T00:00:01.700000000Z two\n\
                  2026-01-01T00:00:01.700000000Z two again\n\
                  2026-01-01T00:00:02.000000000Z three\n";
    Mock::given(method("GET"))
        .and(path(LOG_PATH))
        .and(query_param_is_missing("sinceTime"))
        .respond_with(text(first.into()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(LOG_PATH))
        .and(query_param("sinceTime", "2026-01-01T00:00:01Z"))
        .respond_with(text(second.into()))
        .mount(&server)
        .await;
    // Running at the first end, and stopped at the second.
    Mock::given(method("GET"))
        .and(path(POD_PATH))
        .respond_with(json(&pod(true).to_string()))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    get(POD_PATH, json(&pod(false).to_string()))
        .mount(&server)
        .await;
    let connected = connect(&server).await;

    let stream = connected.handle.stream_logs(request("app", false));
    let mut status = stream.subscribe();
    let interrupted = status.wait_for(|status| status.state == LogState::Interrupted);
    time::timeout(WAIT, interrupted).await.unwrap().unwrap();
    let status = finished(&mut stream.subscribe()).await;

    assert_eq!(texts(&stream), ["one", "two", "two again", "three"]);
    assert_eq!(status.interruptions, 1);
}

#[tokio::test]
async fn dropping_the_stream_stops_reading() {
    let server = api_server().await;
    Mock::given(method("GET"))
        .and(path(LOG_PATH))
        .respond_with(text("".into()).set_delay(Duration::from_secs(30)))
        .mount(&server)
        .await;
    let connected = connect(&server).await;

    let stream = connected.handle.stream_logs(request("app", false));
    let mut status = stream.subscribe();
    drop(stream);

    let closed = async { while status.changed().await.is_ok() {} };
    time::timeout(WAIT, closed).await.expect("the task ends");
}

#[tokio::test]
async fn a_failed_resume_after_the_container_stops_still_reads_its_last_lines() {
    let server = api_server().await;
    Mock::given(method("GET"))
        .and(path(LOG_PATH))
        .and(query_param_is_missing("sinceTime"))
        .respond_with(text("2026-01-01T00:00:01Z one\n".into()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(LOG_PATH))
        .and(query_param("sinceTime", "2026-01-01T00:00:01Z"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(LOG_PATH))
        .and(query_param("sinceTime", "2026-01-01T00:00:01Z"))
        .respond_with(text("2026-01-01T00:00:02Z last words\n".into()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(POD_PATH))
        .respond_with(json(&pod(true).to_string()))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    get(POD_PATH, json(&pod(false).to_string()))
        .mount(&server)
        .await;
    let connected = connect(&server).await;

    let stream = connected.handle.stream_logs(request("app", false));
    finished(&mut stream.subscribe()).await;

    assert_eq!(texts(&stream), ["one", "last words"]);
}
