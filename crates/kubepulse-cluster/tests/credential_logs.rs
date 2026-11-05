//! Runs alone in its own process, because it installs a global log subscriber.

mod support;

use std::{
    io,
    sync::{Arc, Mutex},
};

use kubepulse_cluster::{ConnectionState, SessionManager};
use support::*;
use tracing_subscriber::{
    filter::{LevelFilter, Targets},
    fmt,
    layer::SubscriberExt as _,
    util::SubscriberInitExt as _,
};
use wiremock::{
    Mock, MockServer,
    matchers::{header, method, path},
};

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl io::Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn credentials_never_reach_logs_or_failure_reasons() {
    const GOOD_TOKEN: &str = "good-token-5f1c9a";
    const BAD_TOKEN: &str = "bad-token-77ab10";

    let capture = Capture::default();
    let writer = capture.clone();
    // The fake server logs the requests it receives. That is the test talking to
    // itself, so only its target is excluded and everything else stays at TRACE.
    let filter = Targets::new()
        .with_default(LevelFilter::TRACE)
        .with_target("wiremock", LevelFilter::OFF);
    tracing_subscriber::registry()
        .with(
            fmt::layer()
                .with_ansi(false)
                .with_writer(move || writer.clone()),
        )
        .with(filter)
        .init();

    let dir = temp();
    let server = MockServer::start().await;
    mount_core_discovery(&server).await;
    Mock::given(method("GET"))
        .and(path("/version"))
        .and(header(
            "authorization",
            format!("Bearer {GOOD_TOKEN}").as_str(),
        ))
        .respond_with(json(VERSION))
        .mount(&server)
        .await;

    let good = Auth::Helper {
        token: GOOD_TOKEN.into(),
        delay_ms: 0,
        exit_code: 0,
        marker: None,
    };
    let bad = Auth::Helper {
        token: BAD_TOKEN.into(),
        delay_ms: 0,
        exit_code: 3,
        marker: None,
    };
    let list = kubeconfig(
        dir.path(),
        &[("good", &server.uri(), good), ("bad", &server.uri(), bad)],
    );

    let mut sessions = SessionManager::new(list_channel(list), fast_options());
    sessions.connect("good").unwrap();
    sessions.connect("bad").unwrap();
    wait_until(
        &mut sessions.session("good").unwrap().subscribe_state(),
        |s| *s == ConnectionState::Connected,
    )
    .await;
    let failed = wait_until(
        &mut sessions.session("bad").unwrap().subscribe_state(),
        |s| matches!(s, ConnectionState::Failed { .. }),
    )
    .await;
    assert!(!format!("{failed:?}").contains(BAD_TOKEN));

    // Let health checks and discovery log with the token attached.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    sessions.disconnect("good").await;

    let logs = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
    assert!(logs.contains("connected"), "the scan needs real log output");
    assert!(!logs.contains(GOOD_TOKEN), "token found in logs");
    assert!(
        !logs.contains(BAD_TOKEN),
        "failed helper output found in logs"
    );
}
