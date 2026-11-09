mod support;

use std::{sync::Arc, time::Duration};

use gpui_kit::{
    AppContext as _, Bounds, Entity, Modifiers, MouseButton, Pixels, Point, ScrollDelta,
    SharedString, TestAppContext, VisualTestContext, WindowOptions, point, px,
    test::{ClickOptions, TestWindowExt as _},
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

fn pod(containers: &[&str]) -> Value {
    let spec: Vec<Value> = containers
        .iter()
        .map(|name| json!({"name": name}))
        .collect();
    json!({
        "metadata": {"name": "web", "namespace": "default"},
        "spec": {"containers": spec, "initContainers": [{"name": "setup"}]},
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
    let (fixture, cx) = setup(cx, pod(&["app", "sidecar"]));
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
fn every_container_offers_its_previous_instance(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app", "sidecar"]));
    open(&fixture, cx);

    click("log-previous", cx);
    assert!(requests(&fixture).last().unwrap().previous);

    click("log-container-sidecar", cx);
    assert!(!requests(&fixture).last().unwrap().previous);

    click("log-previous", cx);
    assert!(requests(&fixture).last().unwrap().previous);
}

#[gpui_kit::test]
fn the_view_follows_new_lines_until_the_user_scrolls_up(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"]));
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
    let (fixture, cx) = setup(cx, pod(&["app"]));
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
    let (fixture, cx) = setup(cx, pod(&["app"]));
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
    let (fixture, cx) = setup(cx, pod(&["app"]));
    let tab = open(&fixture, cx);
    send(&fixture, numbered(0..500), cx);

    search(&tab, "line 10", cx);
    wait_until(cx, "the search", |cx| !matches(&tab, cx).is_empty());
    settle(cx);

    assert!(!following(&tab, cx));
}

#[gpui_kit::test]
fn a_new_stream_starts_following_even_after_pausing(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app", "sidecar"]));
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
    let (fixture, cx) = setup(cx, pod(&["app"]));
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
    let (fixture, cx) = setup(cx, pod(&["app"]));
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
    let (fixture, cx) = setup(cx, pod(&["app"]));
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
    let (fixture, cx) = setup(cx, pod(&["app"]));
    let tab = open(&fixture, cx);
    send(&fixture, numbered(0..500), cx);

    scroll_right(px(300.), cx);

    assert_eq!(sideways(&tab, cx), (px(0.), px(0.)));
}

#[gpui_kit::test]
fn moving_the_vertical_scrollbar_up_pauses_following(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"]));
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
    let (fixture, cx) = setup(cx, pod(&["app"]));
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
    let (fixture, cx) = setup(cx, pod(&["app"]));
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
    let (fixture, cx) = setup(cx, pod(&["app"]));
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
    let (fixture, cx) = setup(cx, pod(&["app"]));
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
    let (fixture, cx) = setup(cx, pod(&["app"]));
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
    let (fixture, cx) = setup(cx, pod(&["app"]));
    open(&fixture, cx);

    set_state(&fixture, LogState::Failed("waiting to start".into()), cx);
    assert!(exists("log-failed", cx));

    click("log-retry", cx);
    assert_eq!(requests(&fixture).len(), 2);
}

#[gpui_kit::test]
fn disconnecting_releases_the_lines_and_connecting_restarts_the_stream(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"]));
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
    let (fixture, cx) = setup(cx, pod(&["app"]));
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
    let (fixture, cx) = setup(cx, pod(&["app"]));
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
    let (fixture, cx) = setup(cx, pod(&["app"]));
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
    let (fixture, cx) = setup(cx, pod(&["app"]));
    let first = open(&fixture, cx);
    let second = open(&fixture, cx);
    assert_eq!(first.entity_id(), second.entity_id());
    assert_eq!(requests(&fixture).len(), 1);
}

#[gpui_kit::test]
fn a_context_with_no_such_container_says_so(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"]));
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
    let (fixture, cx) = setup(cx, pod(&["app"]));
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

const COPY: &str = if cfg!(target_os = "macos") {
    "cmd-c"
} else {
    "ctrl-c"
};

fn bounds_of(id: &str, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    let id = SharedString::from(id.to_owned());
    cx.update(|window, cx| {
        window.render_frame(cx);
        window.find(id).bounds()
    })
}

/// A point `x` into the text of a line, halfway down its first row.
fn on_line(seq: u32, x: Pixels, cx: &mut VisualTestContext) -> Point<Pixels> {
    let bounds = bounds_of(&format!("log-line-{seq}"), cx);
    point(bounds.origin.x + x, bounds.origin.y + px(4.))
}

fn drag(from: Point<Pixels>, to: Point<Pixels>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.drag(from, to, cx));
    settle(cx);
}

fn click_line(seq: u32, x: Pixels, count: usize, cx: &mut VisualTestContext) {
    let options = ClickOptions::new()
        .with_offset(point(x, px(4.)))
        .with_count(count);
    let id = SharedString::from(format!("log-line-{seq}"));
    cx.update(|window, cx| window.click_with_options(id, options, cx));
    settle(cx);
}

fn selected(tab: &Entity<LogTab>, cx: &mut VisualTestContext) -> String {
    cx.update(|_, cx| tab.read(cx).selected_text(cx))
}

fn copy(cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|window, cx| window.press(COPY, cx));
    settle(cx);
    cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()))
}

/// Selects from the middle of line 3 to the middle of line 5.
fn select_three_lines(cx: &mut VisualTestContext) {
    let from = on_line(3, px(30.), cx);
    let to = on_line(5, px(30.), cx);
    drag(from, to, cx);
}

fn assert_three_lines(text: &str) {
    let lines: Vec<&str> = text.split('\n').collect();
    assert_eq!(lines.len(), 3, "{text:?}");
    assert!(
        !lines[0].is_empty() && "line 3".ends_with(lines[0]),
        "{text:?}"
    );
    assert_eq!(lines[1], "line 4");
    assert!(
        !lines[2].is_empty() && "line 5".starts_with(lines[2]),
        "{text:?}"
    );
}

#[gpui_kit::test]
fn dragging_across_lines_selects_the_text_between(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"]));
    let tab = open(&fixture, cx);
    send(&fixture, numbered(0..10), cx);

    select_three_lines(cx);

    assert_three_lines(&selected(&tab, cx));
}

#[gpui_kit::test]
fn a_double_click_selects_a_word_and_a_triple_click_a_line(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"]));
    let tab = open(&fixture, cx);
    send(&fixture, numbered(0..10), cx);

    click_line(3, px(4.), 2, cx);
    assert_eq!(selected(&tab, cx), "line");

    click_line(3, px(4.), 3, cx);
    assert_eq!(selected(&tab, cx), "line 3");
}

#[gpui_kit::test]
fn a_new_stream_or_timestamps_clear_the_selection(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app", "sidecar"]));
    let tab = open(&fixture, cx);
    send(&fixture, numbered(0..10), cx);

    select_three_lines(cx);
    click("log-timestamps", cx);
    assert_eq!(selected(&tab, cx), "");

    select_three_lines(cx);
    click("log-previous", cx);
    assert_eq!(selected(&tab, cx), "");

    send(&fixture, numbered(0..10), cx);
    select_three_lines(cx);
    click("log-container-sidecar", cx);
    assert_eq!(selected(&tab, cx), "");
}

#[gpui_kit::test]
fn new_lines_keep_the_same_text_selected(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"]));
    let tab = open(&fixture, cx);
    send(&fixture, numbered(0..10), cx);
    select_three_lines(cx);
    let before = selected(&tab, cx);

    send(&fixture, numbered(10..500), cx);

    assert_eq!(selected(&tab, cx), before);
}

#[gpui_kit::test]
fn dropped_lines_leave_the_selection(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"]));
    *fixture.services.log_limits.lock().unwrap() = Some((400, usize::MAX));
    let tab = open(&fixture, cx);
    send(&fixture, numbered(0..10), cx);
    select_three_lines(cx);

    send(&fixture, numbered(10..404), cx);

    let text = selected(&tab, cx);
    let lines: Vec<&str> = text.split('\n').collect();
    assert_eq!(lines[0], "line 4", "{text:?}");
    assert_eq!(lines.len(), 2, "{text:?}");
}

#[gpui_kit::test]
fn dragging_above_the_view_scrolls_and_stops_following(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"]));
    let tab = open(&fixture, cx);
    send(&fixture, numbered(0..500), cx);
    let list = bounds_of("log-vertical-scrollbar", cx);
    let from = on_line(495, px(30.), cx);
    let above = point(from.x, list.origin.y - px(40.));

    cx.simulate_mouse_move(from, None, Modifiers::none());
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    for step in 1..=8 {
        let y = from.y + (above.y - from.y) * (step as f32 / 8.);
        cx.simulate_mouse_move(point(from.x, y), MouseButton::Left, Modifiers::none());
    }
    for _ in 0..10 {
        cx.executor().advance_clock(Duration::from_millis(50));
        settle(cx);
    }
    cx.simulate_mouse_up(above, MouseButton::Left, Modifiers::none());
    settle(cx);

    assert!(!following(&tab, cx));
    assert!(!exists("log-line-499", cx), "the view moved up");
    assert!(selected(&tab, cx).lines().count() > 5);
}

#[gpui_kit::test]
fn the_copy_shortcut_copies_the_selected_lines(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"]));
    open(&fixture, cx);
    send(&fixture, numbered(0..10), cx);
    select_three_lines(cx);

    let copied = copy(cx).expect("copied text");

    assert_three_lines(&copied);
}

#[gpui_kit::test]
fn copying_in_the_search_field_copies_its_own_text(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"]));
    let tab = open(&fixture, cx);
    send(&fixture, numbered(0..10), cx);
    select_three_lines(cx);
    search(&tab, "needle", cx);
    let input = cx.update(|_, cx| tab.read(cx).search_input().clone());
    cx.update(|window, cx| input.update(cx, |input, cx| input.focus(window, cx)));
    let select_all = if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    };
    cx.update(|window, cx| window.press(select_all, cx));

    assert_eq!(copy(cx).as_deref(), Some("needle"));
}

fn visible(id: &str, cx: &mut VisualTestContext) -> bool {
    let id = SharedString::from(id.to_owned());
    cx.update(|window, cx| {
        window.render_frame(cx);
        window.try_find(id).is_some_and(|line| line.visible())
    })
}

fn first_visible(cx: &mut VisualTestContext) -> u32 {
    (0u32..1000)
        .find(|line| visible(&format!("log-line-{line}"), cx))
        .expect("a line in view")
}

fn height(seq: u32, cx: &mut VisualTestContext) -> Pixels {
    bounds_of(&format!("log-line-{seq}"), cx).size.height
}

fn wrapping(tab: &Entity<LogTab>, cx: &mut VisualTestContext) -> bool {
    cx.update(|_, cx| tab.read(cx).wrapping())
}

fn with_wide_line(range: std::ops::Range<u32>, wide: u32) -> Vec<String> {
    let mut lines = numbered(range.clone());
    lines[(wide - range.start) as usize] = "word ".repeat(400);
    lines
}

#[gpui_kit::test]
fn turning_wrapping_on_wraps_wide_lines_and_ends_sideways_scrolling(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"]));
    let tab = open(&fixture, cx);
    send(&fixture, with_wide_line(0..20, 10), cx);
    let row = height(9, cx);
    assert_eq!(height(10, cx), row);
    assert!(exists("log-horizontal-scrollbar", cx));

    click("log-wrap", cx);
    assert!(wrapping(&tab, cx));
    assert!(height(10, cx) > row * 2.);
    assert_eq!(height(9, cx), row);
    assert!(!exists("log-horizontal-scrollbar", cx));

    click("log-wrap", cx);
    assert!(!wrapping(&tab, cx));
    assert_eq!(height(10, cx), row);
    assert!(exists("log-horizontal-scrollbar", cx));
}

#[gpui_kit::test]
fn wrapped_lines_rewrap_when_the_view_changes_width(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"]));
    open(&fixture, cx);
    send(&fixture, with_wide_line(0..20, 10), cx);
    click("log-wrap", cx);
    let wide = height(10, cx);

    cx.simulate_resize(gpui_kit::size(px(700.), px(900.)));
    settle(cx);

    assert!(height(10, cx) > wide);
}

#[gpui_kit::test]
fn wrapped_lines_follow_until_the_user_scrolls_up(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"]));
    let tab = open(&fixture, cx);
    click("log-wrap", cx);
    send(&fixture, with_wide_line(0..500, 498), cx);
    assert!(visible("log-line-499", cx), "the newest line is in view");
    assert!(following(&tab, cx));

    scroll_up(cx);
    assert!(!following(&tab, cx));

    send(&fixture, numbered(500..600), cx);
    assert!(!visible("log-line-599", cx), "the view stays in place");
    assert!(
        !following(&tab, cx),
        "reaching the end does not follow again"
    );

    click("log-follow", cx);
    assert!(following(&tab, cx));
    assert!(visible("log-line-599", cx));
}

#[gpui_kit::test]
fn moving_the_scrollbar_up_pauses_wrapped_lines(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"]));
    let tab = open(&fixture, cx);
    click("log-wrap", cx);
    send(&fixture, numbered(0..500), cx);

    cx.update(|window, cx| {
        window.click_at("log-vertical-scrollbar", point(px(8.), px(8.)), cx);
    });
    settle(cx);

    assert!(!following(&tab, cx));
    assert!(!visible("log-line-499", cx), "the view moved up");
}

#[gpui_kit::test]
fn wrapped_lines_keep_their_place_when_old_lines_are_dropped(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"]));
    *fixture.services.log_limits.lock().unwrap() = Some((400, usize::MAX));
    let tab = open(&fixture, cx);
    click("log-wrap", cx);
    send(&fixture, numbered(0..400), cx);
    scroll_up(cx);
    assert!(!following(&tab, cx));
    let before = first_visible(cx);

    send(&fixture, numbered(400..450), cx);

    let after = first_visible(cx);
    assert!(
        after.abs_diff(before) <= 1,
        "the same lines stay in view: {before} then {after}"
    );
}

#[gpui_kit::test]
fn moving_to_a_match_shows_it_in_wrapped_lines(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"]));
    let tab = open(&fixture, cx);
    click("log-wrap", cx);
    send(&fixture, numbered(0..500), cx);

    search(&tab, "line 10", cx);
    wait_until(cx, "the search", |cx| !matches(&tab, cx).is_empty());
    settle(cx);

    assert!(!following(&tab, cx));
    assert!(visible("log-line-10", cx));
}

#[gpui_kit::test]
fn toggling_wrapping_while_paused_keeps_the_top_line(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"]));
    let tab = open(&fixture, cx);
    send(&fixture, numbered(0..500), cx);
    scroll_up(cx);
    scroll_up(cx);
    let top = first_visible(cx);

    click("log-wrap", cx);
    assert!(first_visible(cx).abs_diff(top) <= 1, "wrapping on");
    assert!(!following(&tab, cx));

    scroll_up(cx);
    let top = first_visible(cx);
    click("log-wrap", cx);
    assert!(first_visible(cx).abs_diff(top) <= 1, "wrapping off");
    assert!(!following(&tab, cx));
}

#[gpui_kit::test]
fn toggling_wrapping_while_following_keeps_following(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"]));
    let tab = open(&fixture, cx);
    send(&fixture, numbered(0..500), cx);

    click("log-wrap", cx);
    assert!(following(&tab, cx));
    assert!(visible("log-line-499", cx));
    send(&fixture, numbered(500..510), cx);
    assert!(visible("log-line-509", cx));

    click("log-wrap", cx);
    assert!(following(&tab, cx));
    assert!(visible("log-line-509", cx));
}

#[gpui_kit::test]
fn another_container_keeps_wrapping(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app", "sidecar"]));
    let tab = open(&fixture, cx);
    click("log-wrap", cx);

    click("log-container-sidecar", cx);
    send(&fixture, with_wide_line(0..20, 10), cx);

    assert!(wrapping(&tab, cx));
    assert!(height(10, cx) > height(9, cx) * 2.);
    assert!(following(&tab, cx));
}

#[gpui_kit::test]
fn copying_wrapped_lines_breaks_only_between_lines(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"]));
    let tab = open(&fixture, cx);
    send(&fixture, with_wide_line(0..10, 4), cx);
    click("log-wrap", cx);

    select_three_lines(cx);

    let text = selected(&tab, cx);
    let lines: Vec<&str> = text.split('\n').collect();
    assert_eq!(lines.len(), 3, "{text:?}");
    assert_eq!(lines[1], "word ".repeat(400));
}

#[gpui_kit::test]
fn a_high_volume_stream_stays_bounded_while_wrapping(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"]));
    let tab = open(&fixture, cx);
    click("log-wrap", cx);

    for frame in 0..60 {
        send(&fixture, numbered(frame * 3334..(frame + 1) * 3334), cx);
    }
    send(&fixture, numbered(200_040..300_000), cx);

    let retained = cx.update(|_, cx| tab.read(cx).retained()).unwrap();
    assert_eq!(retained.end - retained.start, 100_000);
    assert!(following(&tab, cx));
    assert!(visible("log-line-299999", cx));
}
