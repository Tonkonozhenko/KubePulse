mod support;

use std::sync::Arc;

use gpui_kit::{
    AppContext as _, ClipboardItem, Entity, Modifiers, MouseButton, Pixels, Point, ScrollDelta,
    SharedString, TestAppContext, VisualTestContext, WindowOptions,
    component::{ActiveTheme as _, Theme, ThemeMode},
    point, px,
    test::TestWindowExt as _,
};
use kubepulse_cluster::{
    ClusterEntry, ConnectionState, ExecRequest, ExecSize, ExecState, ObjectTarget, ResourceKind,
};
use kubepulse_store::synthetic::pod_kind;
use kubepulse_ui::{
    app,
    cluster_model::ClusterModel,
    identity::{ClusterKey, IdentityStore},
    preferences::Preferences,
    runtime::{ClusterState, ClusterStatus},
    services::SharedServices,
    settings_tab,
    shell_state::{SharedShellState, ShellState},
    terminal_tab::{self, TerminalTab},
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

fn pod(running: &[&str], stopped: &[&str]) -> Value {
    let spec: Vec<Value> = running
        .iter()
        .chain(stopped)
        .map(|name| json!({"name": name}))
        .collect();
    let statuses: Vec<Value> = running
        .iter()
        .map(|name| json!({"name": name, "state": {"running": {}}}))
        .collect();
    json!({
        "metadata": {"name": "web", "namespace": "default"},
        "spec": {"containers": spec},
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
        app::init(cx);
        cx.set_reduce_motion(true);
        cx.set_global(SharedServices(shared));
        terminal_tab::init(cx);
        let model = cx.new(|cx| ClusterModel::new(receiver, sender, IdentityStore::default(), cx));
        let shell = cx.new(|_| ShellState::default());
        cx.set_global(SharedShellState(shell.clone()));
        settings_tab::init(Preferences::default(), None, cx);
        (model, shell)
    });
    let (handle, workspace) = cx
        .update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| Workspace::new(model, None, window, cx))
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
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| window.render_frame(cx));
    }
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

fn open_with(
    fixture: &Fixture,
    container: Option<&str>,
    cx: &mut VisualTestContext,
) -> Entity<TerminalTab> {
    let container = container.map(str::to_owned);
    let tab = cx.update(|window, cx| {
        fixture.workspace.update(cx, |workspace, cx| {
            workspace.open_terminal(key(), target(), container, window, cx)
        })
    });
    settle(cx);
    tab
}

/// Opens a shell in a one-container pod and takes it to the running state.
fn open_running(fixture: &Fixture, cx: &mut VisualTestContext) -> Entity<TerminalTab> {
    let tab = open_with(fixture, None, cx);
    wait_for_session(fixture, 1, cx);
    set_state(fixture, ExecState::Running, cx);
    tab
}

fn wait_for_session(fixture: &Fixture, count: usize, cx: &mut VisualTestContext) {
    let services = fixture.services.clone();
    wait_until(cx, "the session", |_| {
        services.exec_feeds.lock().unwrap().len() >= count
    });
    settle(cx);
}

fn requests(fixture: &Fixture) -> Vec<ExecRequest> {
    let feeds = fixture.services.exec_feeds.lock().unwrap();
    feeds.iter().map(|(request, _)| request.clone()).collect()
}

fn set_state(fixture: &Fixture, state: ExecState, cx: &mut VisualTestContext) {
    fixture
        .services
        .exec_feeds
        .lock()
        .unwrap()
        .last()
        .expect("an open session")
        .1
        .set_state(state);
    settle(cx);
}

/// Sends output and waits for the emulator to have read all of it.
fn output(
    fixture: &Fixture,
    tab: &Entity<TerminalTab>,
    text: &[u8],
    last_line: &str,
    cx: &mut VisualTestContext,
) {
    fixture
        .services
        .exec_feeds
        .lock()
        .unwrap()
        .last()
        .expect("an open session")
        .1
        .output(text.to_vec());
    let tab = tab.clone();
    wait_until(cx, "the output", |cx| {
        lines(&tab, cx).iter().any(|l| l == last_line)
    });
    settle(cx);
}

fn lines(tab: &Entity<TerminalTab>, cx: &mut VisualTestContext) -> Vec<String> {
    tab.read_with(cx, |tab, _| tab.emulator().visible_lines())
}

fn typed(fixture: &Fixture, cx: &mut VisualTestContext) -> Vec<u8> {
    settle(cx);
    fixture
        .services
        .exec_feeds
        .lock()
        .unwrap()
        .last_mut()
        .expect("an open session")
        .1
        .typed()
}

fn cell_point(
    tab: &Entity<TerminalTab>,
    line: usize,
    column: usize,
    cx: &mut VisualTestContext,
) -> Point<Pixels> {
    tab.read_with(cx, |tab, _| {
        let (bounds, size) = (tab.grid_bounds(), tab.grid_size());
        let width = bounds.size.width / f32::from(size.cols);
        let height = bounds.size.height / f32::from(size.rows);
        bounds.origin
            + point(
                width * (column as f32 + 0.25),
                height * (line as f32 + 0.25),
            )
    })
}

fn state(tab: &Entity<TerminalTab>, cx: &mut VisualTestContext) -> Option<ExecState> {
    tab.read_with(cx, |tab, _| tab.state().cloned())
}

#[gpui_kit::test]
fn a_single_container_opens_directly_and_looks_for_a_shell(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));

    let tab = open_running(&fixture, cx);

    assert_eq!(
        requests(&fixture),
        [ExecRequest {
            namespace: "default".into(),
            pod: "web".into(),
            container: "app".into(),
            command: None,
        }]
    );
    assert!(exists("terminal-grid", cx));
    assert_eq!(state(&tab, cx), Some(ExecState::Running));
}

#[gpui_kit::test]
fn opening_a_shell_from_the_shell_state_adds_a_tab(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));

    fixture
        .shell
        .update(cx, |shell, cx| shell.open_terminal(key(), target(), cx));
    settle(cx);
    wait_for_session(&fixture, 1, cx);

    assert!(exists("terminal-target", cx));
}

#[gpui_kit::test]
fn each_shell_gets_its_own_session(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));

    open_with(&fixture, None, cx);
    wait_for_session(&fixture, 1, cx);
    open_with(&fixture, None, cx);
    wait_for_session(&fixture, 2, cx);

    assert_eq!(requests(&fixture).len(), 2);
}

#[gpui_kit::test]
fn with_several_containers_nothing_starts_until_one_is_chosen(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app", "sidecar"], &["stopped"]));

    open_with(&fixture, None, cx);

    assert!(requests(&fixture).is_empty());
    assert!(exists("terminal-containers", cx));

    click("terminal-container-stopped", cx);
    assert!(
        requests(&fixture).is_empty(),
        "a stopped container cannot be chosen"
    );

    click("terminal-container-sidecar", cx);
    wait_for_session(&fixture, 1, cx);
    assert_eq!(requests(&fixture)[0].container, "sidecar");
}

#[gpui_kit::test]
fn a_named_container_needs_no_choice(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app", "sidecar"], &[]));

    open_with(&fixture, Some("sidecar"), cx);
    wait_for_session(&fixture, 1, cx);

    assert_eq!(requests(&fixture)[0].container, "sidecar");
}

#[gpui_kit::test]
fn a_container_that_is_not_there_is_reported(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));

    open_with(&fixture, Some("gone"), cx);

    assert!(exists("terminal-load-error", cx));
    assert!(requests(&fixture).is_empty());
}

#[gpui_kit::test]
fn the_target_shows_while_connected_and_after_the_end(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    open_running(&fixture, cx);
    assert!(exists("terminal-target", cx));

    set_state(&fixture, ExecState::Dropped, cx);

    assert!(exists("terminal-target", cx));
}

#[gpui_kit::test]
fn no_shell_asks_for_a_command_and_runs_it(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    let tab = open_running(&fixture, cx);
    set_state(&fixture, ExecState::NoShell, cx);
    assert!(exists("terminal-no-shell", cx));
    assert!(exists("terminal-run", cx));
    assert!(
        !exists("terminal-reconnect", cx),
        "there is nothing to reconnect to"
    );

    let input = tab.read_with(cx, |tab, _| tab.command_input().clone());
    cx.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.set_value("/app/console --user 'a b'", window, cx);
        });
    });
    settle(cx);
    click("terminal-run", cx);
    wait_for_session(&fixture, 2, cx);

    assert_eq!(
        requests(&fixture)[1].command,
        Some(vec![
            "/app/console".to_owned(),
            "--user".to_owned(),
            "a b".to_owned()
        ])
    );
}

#[gpui_kit::test]
fn a_command_with_broken_quotes_is_not_run(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    let tab = open_running(&fixture, cx);
    set_state(&fixture, ExecState::NoShell, cx);

    let input = tab.read_with(cx, |tab, _| tab.command_input().clone());
    cx.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.set_value("run 'unbalanced", window, cx)
        });
    });
    settle(cx);
    click("terminal-run", cx);

    assert!(exists("terminal-bad-command", cx));
    assert_eq!(requests(&fixture).len(), 1);
}

#[gpui_kit::test]
fn a_denial_is_reported(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    let tab = open_running(&fixture, cx);

    set_state(
        &fixture,
        ExecState::Failed("forbidden by the cluster".into()),
        cx,
    );

    assert!(exists("terminal-failed", cx));
    assert_eq!(
        state(&tab, cx),
        Some(ExecState::Failed("forbidden by the cluster".into()))
    );
}

#[gpui_kit::test]
fn output_reaches_the_grid(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    let tab = open_running(&fixture, cx);

    output(&fixture, &tab, b"hello\r\nworld", "world", cx);

    assert_eq!(lines(&tab, cx)[..2], ["hello", "world"]);
}

#[gpui_kit::test]
fn a_query_from_the_program_is_answered(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    let tab = open_running(&fixture, cx);

    output(&fixture, &tab, b"ready\x1b[6n", "ready", cx);

    assert_eq!(typed(&fixture, cx), b"\x1b[1;6R");
}

#[gpui_kit::test]
fn keys_reach_the_container(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    open_running(&fixture, cx);

    cx.simulate_keystrokes("l s enter");

    assert_eq!(typed(&fixture, cx), b"ls\r");
}

#[gpui_kit::test]
fn keys_the_kit_binds_still_reach_the_container(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    open_running(&fixture, cx);

    cx.simulate_keystrokes("tab shift-tab ctrl-c");

    assert_eq!(typed(&fixture, cx), b"\t\x1b[Z\x03");
}

#[gpui_kit::test]
fn ctrl_q_reaches_the_container_instead_of_quitting(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    open_running(&fixture, cx);

    cx.simulate_keystrokes("ctrl-q");

    assert_eq!(typed(&fixture, cx), b"\x11");
}

#[gpui_kit::test]
fn the_full_screen_program_controls_how_keys_are_sent(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    let tab = open_running(&fixture, cx);
    output(
        &fixture,
        &tab,
        b"\x1b[?1049h\x1b[?1h\x1b[2Jeditor",
        "editor",
        cx,
    );

    cx.simulate_keystrokes("up escape");

    assert_eq!(typed(&fixture, cx), b"\x1bOA\x1b");
}

#[gpui_kit::test]
fn the_wheel_moves_a_full_screen_program_and_not_the_scrollback(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    let tab = open_running(&fixture, cx);
    output(&fixture, &tab, b"\x1b[?1049h\x1b[2Jeditor", "editor", cx);

    let at = cell_point(&tab, 1, 1, cx);
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.simulate_event(gpui_kit::ScrollEvent {
        position: at,
        delta: ScrollDelta::Lines(point(0., 2.)),
        ..Default::default()
    });

    assert_eq!(typed(&fixture, cx), b"\x1b[A\x1b[A");
}

#[gpui_kit::test]
fn the_wheel_moves_through_the_scrollback(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    let tab = open_running(&fixture, cx);
    let rows = usize::from(tab.read_with(cx, |tab, _| tab.grid_size()).rows);
    let text: String = (0..rows + 20).map(|n| format!("line {n}\r\n")).collect();
    let last = format!("line {}", rows + 19);
    output(&fixture, &tab, text.as_bytes(), &last, cx);

    let at = cell_point(&tab, 1, 1, cx);
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.simulate_event(gpui_kit::ScrollEvent {
        position: at,
        delta: ScrollDelta::Lines(point(0., 3.)),
        ..Default::default()
    });
    settle(cx);
    assert_eq!(
        tab.read_with(cx, |tab, _| tab.emulator().display_offset()),
        3
    );

    cx.simulate_keystrokes("a");
    settle(cx);
    assert_eq!(
        tab.read_with(cx, |tab, _| tab.emulator().display_offset()),
        0,
        "typing returns to the end"
    );
}

#[gpui_kit::test]
fn a_program_that_takes_the_mouse_gets_clicks(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    let tab = open_running(&fixture, cx);
    output(
        &fixture,
        &tab,
        b"\x1b[?1000h\x1b[?1006hclicks",
        "clicks",
        cx,
    );

    let at = cell_point(&tab, 1, 2, cx);
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());

    assert_eq!(typed(&fixture, cx), b"\x1b[<0;3;2M\x1b[<0;3;2m");
}

#[gpui_kit::test]
fn dragging_selects_and_the_shortcut_copies(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    let tab = open_running(&fixture, cx);
    output(&fixture, &tab, b"hello world", "hello world", cx);

    let (start, end) = (cell_point(&tab, 0, 0, cx), cell_point(&tab, 0, 5, cx));
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
    settle(cx);
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-c"
    } else {
        "ctrl-shift-c"
    });

    let copied = cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()));
    assert_eq!(copied.as_deref(), Some("hello"));
    assert!(typed(&fixture, cx).is_empty(), "copying sends nothing");
}

#[gpui_kit::test]
fn the_paste_shortcut_sends_the_clipboard(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    open_running(&fixture, cx);

    cx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string("one\ntwo".to_owned())));
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-v"
    } else {
        "ctrl-shift-v"
    });

    assert_eq!(typed(&fixture, cx), b"one\rtwo");
}

#[gpui_kit::test]
fn the_container_gets_the_size_of_the_pane(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    let tab = open_running(&fixture, cx);

    let wide = tab.read_with(cx, |tab, _| tab.grid_size());
    assert!(wide.cols > 80, "the grid fills the pane: {wide:?}");
    let sent = |fixture: &Fixture| fixture.services.exec_feeds.lock().unwrap()[0].1.size();
    assert_eq!(sent(&fixture), wide);

    cx.simulate_resize(gpui_kit::size(px(900.), px(600.)));
    settle(cx);

    let narrow: ExecSize = tab.read_with(cx, |tab, _| tab.grid_size());
    assert!(narrow.cols < wide.cols && narrow.rows < wide.rows);
    assert_eq!(sent(&fixture), narrow);
}

#[gpui_kit::test]
fn named_colors_are_the_active_themes(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    let tab = open_running(&fixture, cx);
    output(
        &fixture,
        &tab,
        b"\x1b[31mred\x1b[0m \x1b[96mcyan",
        "red cyan",
        cx,
    );

    let color_of = |tab: &Entity<TerminalTab>, cx: &mut VisualTestContext| {
        tab.read_with(cx, |tab, _| {
            let frame = tab.emulator().frame(&tab.palette(), false);
            (frame.spans[0].color, frame.spans[1].color)
        })
    };
    let (dark_red, dark_cyan) = cx.update(|_, cx| (cx.theme().red, cx.theme().cyan_light));
    let shown = color_of(&tab, cx);
    assert_eq!(shown, (dark_red, dark_cyan));

    cx.update(|window, cx| {
        let mode = if cx.theme().mode.is_dark() {
            ThemeMode::Light
        } else {
            ThemeMode::Dark
        };
        Theme::change(mode, Some(window), cx);
    });
    settle(cx);

    let (other_red, other_cyan) = cx.update(|_, cx| (cx.theme().red, cx.theme().cyan_light));
    assert_eq!(color_of(&tab, cx), (other_red, other_cyan));
    assert_ne!(shown.0, other_red, "the two modes define different colors");
}

#[gpui_kit::test]
fn an_exit_shows_its_code_and_offers_to_reconnect(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    open_running(&fixture, cx);

    set_state(&fixture, ExecState::Exited(3), cx);

    assert!(exists("terminal-exited", cx));
    assert!(exists("terminal-reconnect", cx));
}

#[gpui_kit::test]
fn a_dropped_connection_keeps_the_scrollback_through_a_reconnect(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    let tab = open_running(&fixture, cx);
    let rows = usize::from(tab.read_with(cx, |tab, _| tab.grid_size()).rows);
    let text: String = (0..rows + 20).map(|n| format!("line {n}\r\n")).collect();
    let last = format!("line {}", rows + 19);
    output(&fixture, &tab, text.as_bytes(), &last, cx);
    let kept = tab.read_with(cx, |tab, _| tab.emulator().scrollback_lines());
    assert!(kept >= 20);

    set_state(&fixture, ExecState::Dropped, cx);
    assert!(exists("terminal-dropped", cx));
    assert!(lines(&tab, cx).contains(&last), "the output stays readable");

    click("terminal-reconnect", cx);
    wait_for_session(&fixture, 2, cx);
    set_state(&fixture, ExecState::Running, cx);

    assert!(!exists("terminal-dropped", cx));
    assert_eq!(
        requests(&fixture)[0],
        requests(&fixture)[1],
        "the same target"
    );
    let after = tab.read_with(cx, |tab, _| tab.emulator().scrollback_lines());
    assert!(after >= kept, "{after} of {kept}");
    output(&fixture, &tab, b"new prompt", "new prompt", cx);
    tab.read_with(cx, |tab, _| {
        tab.emulator().scroll(i32::MAX);
        assert_eq!(tab.emulator().visible_lines()[0], "line 0");
    });
}

#[gpui_kit::test]
fn a_reconnect_leaves_a_full_screen_program_behind(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    let tab = open_running(&fixture, cx);
    output(
        &fixture,
        &tab,
        b"prompt\r\n\x1b[?1049h\x1b[?1000h\x1b[2Jeditor",
        "editor",
        cx,
    );

    set_state(&fixture, ExecState::Dropped, cx);
    click("terminal-reconnect", cx);
    wait_for_session(&fixture, 2, cx);

    assert!(lines(&tab, cx).contains(&"prompt".to_owned()));
    let at = cell_point(&tab, 1, 1, cx);
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    assert!(
        typed(&fixture, cx).is_empty(),
        "the old program's mouse mode is gone"
    );
}

#[gpui_kit::test]
fn a_session_taken_away_by_a_disconnect_counts_as_dropped(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    let tab = open_running(&fixture, cx);

    fixture.services.exec_feeds.lock().unwrap().clear();
    settle(cx);

    assert_eq!(state(&tab, cx), Some(ExecState::Dropped));
}

#[gpui_kit::test]
fn a_disconnected_cluster_offers_connect_and_not_reconnect(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    open_running(&fixture, cx);
    let mut disconnected = connected("a", kinds());
    disconnected.state = ConnectionState::Disconnected;
    fixture.state.send_replace(publish(vec![disconnected]));
    set_state(&fixture, ExecState::Dropped, cx);

    assert!(exists("tab-connect", cx));
    assert!(!exists("terminal-reconnect", cx));
    assert_eq!(
        requests(&fixture).len(),
        1,
        "a shell does not reopen by itself"
    );

    fixture
        .state
        .send_replace(publish(vec![connected("a", kinds())]));
    settle(cx);

    assert!(exists("terminal-reconnect", cx));
    assert_eq!(
        requests(&fixture).len(),
        1,
        "reconnecting stays the user's choice"
    );
}

#[gpui_kit::test]
fn a_context_on_another_server_cannot_reconnect(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    open_running(&fixture, cx);
    let mut moved = connected("a", kinds());
    moved.entry = ClusterEntry {
        server: Some("https://elsewhere.example".into()),
        ..moved.entry
    };
    fixture.state.send_replace(publish(vec![moved]));
    set_state(&fixture, ExecState::Dropped, cx);

    assert!(exists("tab-server-changed", cx));
    assert!(!exists("terminal-reconnect", cx));
}

#[gpui_kit::test]
fn closing_the_tab_ends_the_session(cx: &mut TestAppContext) {
    let (fixture, cx) = setup(cx, pod(&["app"], &[]));
    let tab = open_running(&fixture, cx);

    let id = tab.entity_id();
    fixture
        .shell
        .update(cx, |shell, cx| shell.request_close(id, cx));
    settle(cx);

    assert!(fixture.services.exec_feeds.lock().unwrap()[0].1.abandoned());
}
