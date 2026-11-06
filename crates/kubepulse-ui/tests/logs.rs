mod support;

use std::sync::Arc;

use gpui_kit::{
    AppContext as _, Entity, Pixels, ScrollDelta, SharedString, TestAppContext, VisualTestContext,
    WindowOptions, point, px, test::TestWindowExt as _,
};
use kubepulse_cluster::{
    ClusterEntry, ConnectionState, LogLine, LogRequest, LogState, ObjectTarget, ResourceKind,
};
use kubepulse_store::synthetic::pod_kind;
use kubepulse_ui::{
    cluster_model::ClusterModel,
    identity::{ClusterKey, IdentityStore},
    log_tab::LogTab,
    preferences::Preferences,
    runtime::{ClusterState, ClusterStatus},
    services::SharedServices,
    settings_tab,
    shell_state::{SharedShellState, ShellState},
    workspace::Workspace,
};
use serde_json::{Value, json};
use support::{FakeServices, connected, wait_until};
use tokio::sync::{mpsc, watch};

struct Fixture {
    services: Arc<FakeServices>,
    state: watch::Sender<Arc<ClusterState>>,
    workspace: Entity<Workspace>,
    shell: Entity<ShellState>,
    _commands: mpsc::UnboundedReceiver<kubepulse_ui::runtime::ClusterCommand>,
}

fn kinds() -> Vec<ResourceKind> {
    vec![pod_kind()]
}

fn publish(statuses: Vec<ClusterStatus>) -> Arc<ClusterState> {
    Arc::new(ClusterState {
        version: 1,
        clusters: statuses,
        errors: vec![],
    })
}

fn key() -> ClusterKey {
    ClusterKey::new("a", Some("https://a.example"))
}

fn target() -> ObjectTarget {
    ObjectTarget {
        kind: pod_kind(),
        namespace: Some("default".into()),
        name: "web".into(),
    }
}

fn pod(containers: &[&str], restarted: &[&str]) -> Value {
    let spec: Vec<Value> = containers
        .iter()
        .map(|name| json!({"name": name}))
        .collect();
    let statuses: Vec<Value> = restarted
        .iter()
        .map(|name| json!({"name": name, "restartCount": 1}))
        .collect();
    json!({
        "metadata": {"name": "web", "namespace": "default"},
        "spec": {"containers": spec, "initContainers": [{"name": "setup"}]},
        "status": {"containerStatuses": statuses},
    })
}

fn setup(cx: &mut TestAppContext, object: Value) -> (Fixture, &mut VisualTestContext) {
    cx.executor().allow_parking();
    let (state, receiver) = watch::channel(publish(vec![connected("a", kinds())]));
    let (sender, commands) = mpsc::unbounded_channel();
    let services = Arc::new(FakeServices::idle());
    services.add_object(&pod_kind(), Some("default"), object);
    let shared = services.clone();
    let (model, shell) = cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_reduce_motion(true);
        cx.set_global(SharedServices(shared));
        let model = cx.new(|cx| ClusterModel::new(receiver, sender, IdentityStore::default(), cx));
        let shell = cx.new(|_| ShellState::default());
        cx.set_global(SharedShellState(shell.clone()));
        settings_tab::init(Preferences::default(), None, cx);
        (model, shell)
    });
    let window_model: Entity<ClusterModel> = model;
    let (handle, workspace) = cx
        .update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| Workspace::new(window_model, None, window, cx))
            })
        })
        .unwrap();
    let cx = VisualTestContext::from_window(handle, cx).into_mut();
    cx.simulate_resize(gpui_kit::size(px(1400.), px(900.)));
    cx.run_until_parked();
    let fixture = Fixture {
        services,
        state,
        workspace,
        shell,
        _commands: commands,
    };
    (fixture, cx)
}

fn settle(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.render_frame(cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.render_frame(cx));
    cx.run_until_parked();
}

fn exists(id: &str, cx: &mut VisualTestContext) -> bool {
    let id = SharedString::from(id.to_owned());
    cx.update(|window, cx| {
        window.render_frame(cx);
        window.try_find(id).is_some()
    })
}

fn click(id: &str, cx: &mut VisualTestContext) {
    let id = SharedString::from(id.to_owned());
    cx.update(|window, cx| window.click(id, cx));
    settle(cx);
}

fn open(fixture: &Fixture, cx: &mut VisualTestContext) -> Entity<LogTab> {
    let tab = cx.update(|window, cx| {
        fixture.workspace.update(cx, |workspace, cx| {
            workspace.open_logs(key(), target(), None, window, cx)
        })
    });
    let services = fixture.services.clone();
    wait_until(cx, "the stream", |_| {
        !services.log_feeds.lock().unwrap().is_empty()
    });
    settle(cx);
    tab
}

fn requests(fixture: &Fixture) -> Vec<LogRequest> {
    let feeds = fixture.services.log_feeds.lock().unwrap();
    feeds.iter().map(|(request, _)| request.clone()).collect()
}

/// Delivers lines to the newest stream, then publishes them as one frame would.
fn send(fixture: &Fixture, lines: impl IntoIterator<Item = String>, cx: &mut VisualTestContext) {
    {
        let feeds = fixture.services.log_feeds.lock().unwrap();
        let (_, feed) = feeds.last().expect("an open stream");
        for line in lines {
            feed.push(LogLine::new(&line));
        }
        feed.publish();
    }
    settle(cx);
}

fn numbered(range: std::ops::Range<u32>) -> Vec<String> {
    range
        .map(|number| format!("2026-01-01T00:00:00Z line {number}"))
        .collect()
}

fn set_state(fixture: &Fixture, state: LogState, cx: &mut VisualTestContext) {
    {
        let feeds = fixture.services.log_feeds.lock().unwrap();
        feeds.last().expect("an open stream").1.set_state(state);
    }
    settle(cx);
}

fn scroll_up(cx: &mut VisualTestContext) {
    let delta = ScrollDelta::Pixels(point(px(0.), px(400.)));
    cx.update(|window, cx| window.scroll("log-tab", delta, cx));
    settle(cx);
}

fn scroll_right(distance: Pixels, cx: &mut VisualTestContext) {
    let delta = ScrollDelta::Pixels(point(-distance, px(0.)));
    cx.update(|window, cx| window.scroll("log-tab", delta, cx));
    settle(cx);
}

fn sideways(tab: &Entity<LogTab>, cx: &mut VisualTestContext) -> (Pixels, Pixels) {
    cx.update(|_, cx| tab.read(cx).sideways_scroll())
}

fn search(tab: &Entity<LogTab>, text: &str, cx: &mut VisualTestContext) {
    let input = cx.update(|_, cx| tab.read(cx).search_input().clone());
    cx.update(|window, cx| {
        input.update(cx, |input, cx| input.replace(text, window, cx));
    });
    settle(cx);
}

fn matches(tab: &Entity<LogTab>, cx: &mut VisualTestContext) -> Vec<u64> {
    cx.update(|_, cx| tab.read(cx).matches().to_vec())
}

fn current(tab: &Entity<LogTab>, cx: &mut VisualTestContext) -> Option<u64> {
    cx.update(|_, cx| tab.read(cx).current_match())
}

fn following(tab: &Entity<LogTab>, cx: &mut VisualTestContext) -> bool {
    cx.update(|_, cx| tab.read(cx).following())
}

fn republish(fixture: &Fixture, status: ClusterStatus, cx: &mut VisualTestContext) {
    fixture.state.send_replace(publish(vec![status]));
    settle(cx);
}

#[gpui_kit::test]
fn a_single_container_opens_directly(cx: &mut TestAppContext) {
    let object = json!({
        "spec": {"containers": [{"name": "app"}]},
        "metadata": {"name": "web", "namespace": "default"},
    });
    let (fixture, cx) = setup(cx, object);

    open(&fixture, cx);
    send(&fixture, numbered(0..3), cx);

    assert_eq!(
        requests(&fixture),
        [LogRequest {
            namespace: "default".into(),
            pod: "web".into(),
            container: "app".into(),
            previous: false,
        }]
    );
    assert!(!exists("log-container-app", cx), "nothing to choose");
    assert!(exists("log-line-2", cx));
    assert!(exists("log-target", cx));
}

#[gpui_kit::test]
fn several_containers_can_be_chosen_including_init_containers(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app", "sidecar"], &[]));
    open(&fixture, cx);

    click("log-container-sidecar", cx);
    click("log-container-setup", cx);

    let containers: Vec<String> = requests(&fixture)
        .into_iter()
        .map(|request| request.container)
        .collect();
    assert_eq!(containers, ["app", "sidecar", "setup"]);
}

#[gpui_kit::test]
fn a_restarted_container_offers_its_previous_instance(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app", "sidecar"], &["app"]));
    open(&fixture, cx);
    assert!(exists("log-previous", cx));

    click("log-previous", cx);
    assert!(requests(&fixture).last().unwrap().previous);

    click("log-container-sidecar", cx);
    assert!(!requests(&fixture).last().unwrap().previous);
    assert!(!exists("log-previous", cx), "this one never restarted");
}

#[gpui_kit::test]
fn the_view_follows_new_lines_until_the_user_scrolls_up(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    let tab = open(&fixture, cx);

    send(&fixture, numbered(0..500), cx);
    assert!(exists("log-line-499", cx), "the newest line is in view");
    assert!(following(&tab, cx));

    scroll_up(cx);
    assert!(!following(&tab, cx));

    send(&fixture, numbered(500..600), cx);
    assert!(!exists("log-line-599", cx), "the view stays in place");
    assert_eq!(
        cx.update(|_, cx| tab.read(cx).retained()),
        Some(0..600),
        "the stream continues"
    );

    click("log-follow", cx);
    assert!(following(&tab, cx));
    assert!(exists("log-line-599", cx));
}

#[gpui_kit::test]
fn the_newest_line_stays_in_view_when_a_quiet_view_gets_shorter(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    let tab = open(&fixture, cx);
    send(&fixture, numbered(0..500), cx);
    assert!(exists("log-line-499", cx));

    cx.simulate_resize(gpui_kit::size(px(1400.), px(400.)));
    settle(cx);

    assert!(exists("log-line-499", cx), "no new line had to arrive");
    assert!(following(&tab, cx));
}

#[gpui_kit::test]
fn the_follow_checkbox_pauses_and_resumes_while_the_stream_continues(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    let tab = open(&fixture, cx);
    send(&fixture, numbered(0..500), cx);

    click("log-follow", cx);
    assert!(!following(&tab, cx));
    send(&fixture, numbered(500..600), cx);
    assert!(exists("log-line-499", cx), "the view stays in place");
    assert!(!exists("log-line-599", cx));
    assert_eq!(cx.update(|_, cx| tab.read(cx).retained()), Some(0..600));

    click("log-follow", cx);
    assert!(following(&tab, cx));
    assert!(exists("log-line-599", cx));
}

#[gpui_kit::test]
fn moving_to_a_search_match_pauses_following(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    let tab = open(&fixture, cx);
    send(&fixture, numbered(0..500), cx);

    search(&tab, "line 10", cx);
    wait_until(cx, "the search", |cx| !matches(&tab, cx).is_empty());
    settle(cx);

    assert!(!following(&tab, cx));
}

#[gpui_kit::test]
fn a_new_stream_starts_following_even_after_pausing(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app", "sidecar"], &[]));
    let tab = open(&fixture, cx);
    send(&fixture, numbered(0..500), cx);
    click("log-follow", cx);
    assert!(!following(&tab, cx));

    click("log-container-sidecar", cx);
    assert!(following(&tab, cx));
    send(&fixture, numbered(0..500), cx);
    assert!(exists("log-line-499", cx));
}

#[gpui_kit::test]
fn a_wide_line_far_from_the_first_row_can_be_reached(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    let tab = open(&fixture, cx);
    let mut lines = numbered(0..500);
    lines[300] = "x".repeat(2000);
    send(&fixture, lines, cx);

    assert!(sideways(&tab, cx).1 > px(5000.), "the width covers it");

    scroll_right(px(1000.), cx);
    assert_eq!(sideways(&tab, cx).0, px(1000.));
}

#[gpui_kit::test]
fn the_sideways_range_shrinks_when_the_widest_line_is_dropped(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    *fixture.services.log_limits.lock().unwrap() = Some((400, usize::MAX));
    let tab = open(&fixture, cx);
    let mut lines = numbered(0..300);
    lines[0] = "x".repeat(2000);
    lines[1] = "x".repeat(1000);
    send(&fixture, lines, cx);
    let full = sideways(&tab, cx).1;
    scroll_right(full, cx);

    send(&fixture, numbered(300..401), cx);

    let (offset, range) = sideways(&tab, cx);
    assert!(range > px(0.) && range < full);
    assert!(offset <= range, "the view is still within the lines");
}

#[gpui_kit::test]
fn scrolling_sideways_keeps_following_and_keeps_its_position(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    let tab = open(&fixture, cx);
    let mut lines = numbered(0..500);
    lines[0] = "x".repeat(2000);
    send(&fixture, lines, cx);

    scroll_right(px(300.), cx);
    assert!(following(&tab, cx));

    send(&fixture, numbered(500..600), cx);
    assert!(following(&tab, cx));
    assert!(exists("log-line-599", cx));
    assert_eq!(sideways(&tab, cx).0, px(300.));
}

#[gpui_kit::test]
fn lines_that_fit_do_not_scroll_sideways(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    let tab = open(&fixture, cx);
    send(&fixture, numbered(0..500), cx);

    scroll_right(px(300.), cx);

    assert_eq!(sideways(&tab, cx), (px(0.), px(0.)));
}

#[gpui_kit::test]
fn moving_the_vertical_scrollbar_up_pauses_following(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    let tab = open(&fixture, cx);
    send(&fixture, numbered(0..500), cx);
    assert!(following(&tab, cx));

    cx.update(|window, cx| {
        window.click_at("log-vertical-scrollbar", point(px(8.), px(8.)), cx);
    });
    settle(cx);

    assert!(!following(&tab, cx));
    assert!(!exists("log-line-499", cx), "the view moved up");
}

#[gpui_kit::test]
fn dropped_lines_are_reported_and_the_reader_keeps_their_place(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    *fixture.services.log_limits.lock().unwrap() = Some((400, usize::MAX));
    let tab = open(&fixture, cx);
    send(&fixture, numbered(0..400), cx);
    assert!(!exists("log-dropped", cx));
    scroll_up(cx);
    assert!(!following(&tab, cx));
    let visible = |cx: &mut VisualTestContext| {
        (0u32..700)
            .find(|line| exists(&format!("log-line-{line}"), cx))
            .expect("a line in view")
    };
    let before = visible(cx);

    send(&fixture, numbered(400..450), cx);

    assert!(exists("log-dropped", cx));
    let after = visible(cx);
    assert!(
        after.abs_diff(before) <= 1,
        "the same lines stay in view: {before} then {after}"
    );
}

#[gpui_kit::test]
fn a_high_volume_stream_keeps_the_interface_bounded(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    let tab = open(&fixture, cx);

    // A second of output at 10,000 lines per second, one frame at a time.
    for frame in 0..60 {
        send(&fixture, numbered(frame * 3334..(frame + 1) * 3334), cx);
    }
    send(&fixture, numbered(200_040..300_000), cx);

    let retained = cx.update(|_, cx| tab.read(cx).retained()).unwrap();
    assert_eq!(retained.end - retained.start, 100_000);
    assert!(exists("log-dropped", cx));
    assert!(following(&tab, cx));
}

#[gpui_kit::test]
fn search_finds_retained_lines_and_steps_through_them(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    let tab = open(&fixture, cx);
    let lines = vec![
        "2026-01-01T00:00:00Z ok".to_owned(),
        "2026-01-01T00:00:00Z ERROR one".to_owned(),
        "2026-01-01T00:00:00Z ok".to_owned(),
        "2026-01-01T00:00:00Z error two".to_owned(),
    ];
    send(&fixture, lines, cx);

    search(&tab, "error", cx);
    wait_until(cx, "the search", |cx| matches(&tab, cx) == [1, 3]);
    assert_eq!(current(&tab, cx), Some(1));
    assert!(exists("log-match-count", cx));

    click("log-match-next", cx);
    assert_eq!(current(&tab, cx), Some(3));
    click("log-match-next", cx);
    assert_eq!(current(&tab, cx), Some(1), "it wraps around");
    click("log-match-previous", cx);
    assert_eq!(current(&tab, cx), Some(3));
}

#[gpui_kit::test]
fn search_follows_lines_arriving_while_it_is_active(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    *fixture.services.log_limits.lock().unwrap() = Some((3, usize::MAX));
    let tab = open(&fixture, cx);
    send(&fixture, ["2026-01-01T00:00:00Z error old".to_owned()], cx);
    search(&tab, "error", cx);
    wait_until(cx, "the search", |cx| matches(&tab, cx) == [0]);

    let new = vec![
        "2026-01-01T00:00:00Z ok".to_owned(),
        "2026-01-01T00:00:00Z error new".to_owned(),
    ];
    send(&fixture, new, cx);
    assert_eq!(matches(&tab, cx), [0, 2]);

    send(&fixture, ["2026-01-01T00:00:00Z ok".to_owned()], cx);
    assert_eq!(matches(&tab, cx), [2], "the dropped line left the matches");
    assert_eq!(current(&tab, cx), Some(2));
}

#[gpui_kit::test]
fn an_interruption_is_shown_and_then_counted(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    open(&fixture, cx);
    set_state(&fixture, LogState::Streaming, cx);

    set_state(&fixture, LogState::Interrupted, cx);
    assert!(exists("log-interrupted", cx));

    set_state(&fixture, LogState::Streaming, cx);
    assert!(!exists("log-interrupted", cx));
    assert!(exists("log-interruptions", cx), "it stays on record");
}

#[gpui_kit::test]
fn a_failed_stream_shows_why_and_can_be_retried(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    open(&fixture, cx);

    set_state(&fixture, LogState::Failed("waiting to start".into()), cx);
    assert!(exists("log-failed", cx));

    click("log-retry", cx);
    assert_eq!(requests(&fixture).len(), 2);
}

#[gpui_kit::test]
fn disconnecting_releases_the_lines_and_connecting_restarts_the_stream(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    let tab = open(&fixture, cx);
    send(&fixture, numbered(0..3), cx);

    let mut disconnected = connected("a", kinds());
    disconnected.state = ConnectionState::Disconnected;
    republish(&fixture, disconnected, cx);

    assert_eq!(cx.update(|_, cx| tab.read(cx).retained()), None);
    assert!(exists("tab-connect", cx), "the tab offers a connect action");
    assert_eq!(requests(&fixture).len(), 1, "it does not connect by itself");

    republish(&fixture, connected("a", kinds()), cx);
    assert_eq!(requests(&fixture).len(), 2);
}

#[gpui_kit::test]
fn a_degraded_cluster_keeps_the_lines_and_marks_them_stale(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    let tab = open(&fixture, cx);
    send(&fixture, numbered(0..3), cx);

    let mut degraded = connected("a", kinds());
    degraded.state = ConnectionState::Degraded {
        reason: "timed out".into(),
    };
    republish(&fixture, degraded, cx);

    assert!(exists("tab-stale", cx));
    assert_eq!(cx.update(|_, cx| tab.read(cx).retained()), Some(0..3));
}

fn repointed() -> ClusterStatus {
    let mut moved = connected("a", kinds());
    moved.entry = ClusterEntry {
        server: Some("https://other.example".into()),
        ..moved.entry
    };
    moved
}

#[gpui_kit::test]
fn a_context_moved_to_another_server_stops_the_stream_and_is_not_selected(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    let tab = open(&fixture, cx);
    send(&fixture, numbered(0..3), cx);

    republish(&fixture, repointed(), cx);

    assert!(exists("tab-server-changed", cx));
    assert_eq!(cx.update(|_, cx| tab.read(cx).retained()), None);
    assert!(!exists("log-timestamps", cx));
    assert_eq!(
        requests(&fixture).len(),
        1,
        "it never reads the other server"
    );

    cx.update(|_, cx| {
        fixture.shell.update(cx, |shell, cx| {
            shell.select(ClusterKey::new("b", None), cx);
        })
    });
    click("log-tab", cx);
    assert_eq!(
        cx.update(|_, cx| fixture.shell.read(cx).selected().cloned()),
        Some(ClusterKey::new("b", None)),
        "the context of the same name is not selected"
    );
}

#[gpui_kit::test]
fn clicking_the_tab_selects_its_cluster(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    open(&fixture, cx);
    cx.update(|_, cx| {
        fixture.shell.update(cx, |shell, cx| {
            shell.select(ClusterKey::new("b", None), cx);
        })
    });

    click("log-tab", cx);

    assert_eq!(
        cx.update(|_, cx| fixture.shell.read(cx).selected().cloned()),
        Some(key())
    );
}

#[gpui_kit::test]
fn opening_a_pods_logs_twice_shares_a_tab(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    let first = open(&fixture, cx);
    let second = open(&fixture, cx);
    assert_eq!(first.entity_id(), second.entity_id());
    assert_eq!(requests(&fixture).len(), 1);
}

#[gpui_kit::test]
fn a_context_with_no_such_container_says_so(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    cx.update(|window, cx| {
        fixture.workspace.update(cx, |workspace, cx| {
            workspace.open_logs(key(), target(), Some("gone".into()), window, cx);
        });
    });
    wait_until(cx, "the error", |cx| exists("log-load-error", cx));
    assert!(requests(&fixture).is_empty());
}

#[gpui_kit::test]
fn closing_the_tab_stops_the_stream_and_leaves_the_cluster_connected(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    let tab = open(&fixture, cx);
    let id = tab.entity_id();

    cx.update(|_, cx| {
        fixture
            .shell
            .update(cx, |shell, cx| shell.request_close(id, cx))
    });
    settle(cx);

    assert!(cx.update(|_, cx| fixture.shell.read(cx).log_tab(id).is_none()));
    assert_eq!(cx.update(|_, cx| tab.read(cx).retained()), None);
}
