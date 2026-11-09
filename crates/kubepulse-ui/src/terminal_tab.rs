//! A shell in a container. The emulator lives in the tab rather than in the
//! session, so the scrollback outlasts the connection.

use std::sync::Arc;

use alacritty_terminal::{index::Side, selection::SelectionType, term::TermMode};
use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme as _, Disableable as _, Icon, Selectable as _, Sizable as _,
        button::Button,
        dock::{self, PanelEvent},
        input::{Input, InputEvent, InputState},
    },
    prelude::FluentBuilder as _,
    *,
};
use kubepulse_cluster::{
    ContainerInfo, ContainerKind, ExecRequest, ExecSize, ExecState, ExecStream, ObjectTarget,
    pod_containers,
};
use rust_i18n::t;
use tokio::sync::watch;

use crate::{
    cluster_model::{ClusterModel, is_live},
    identity::ClusterKey,
    services::{ClusterServices, SharedServices},
    shell_state::{SharedShellState, ShellState, close_button, close_on_middle_click},
    tab_connection::{activate_tab, connection_notes},
    terminal::{
        CursorMark, Emulator, Frame, MouseAction, Palette, key_bytes, mouse_report, paste_bytes,
        wheel_keys,
    },
};

pub const PANEL_NAME: &str = "TerminalTab";
const CONTEXT: &str = "Terminal";
const START_SIZE: ExecSize = ExecSize { cols: 80, rows: 24 };
const FONT_REMS: f32 = 0.875;
const LINE_SPACING: f32 = 1.3;
const COMMAND_WIDTH: f32 = 320.;
const CURSOR_STROKE: f32 = 2.;

gpui_kit::actions!(
    terminal,
    [
        SendTab,
        SendBackTab,
        SendInterrupt,
        SendXon,
        CopySelection,
        PasteClipboard
    ]
);

/// The kit binds these keys for moving focus and copying, and Ctrl+Q quits on
/// Linux. Either would keep them from the program in the container.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("tab", SendTab, Some(CONTEXT)),
        KeyBinding::new("shift-tab", SendBackTab, Some(CONTEXT)),
        KeyBinding::new("ctrl-c", SendInterrupt, Some(CONTEXT)),
        KeyBinding::new("ctrl-q", SendXon, Some(CONTEXT)),
    ]);
    if cfg!(target_os = "macos") {
        cx.bind_keys([
            KeyBinding::new("cmd-c", CopySelection, Some(CONTEXT)),
            KeyBinding::new("cmd-v", PasteClipboard, Some(CONTEXT)),
        ]);
    } else {
        cx.bind_keys([
            KeyBinding::new("ctrl-shift-c", CopySelection, Some(CONTEXT)),
            KeyBinding::new("ctrl-shift-v", PasteClipboard, Some(CONTEXT)),
        ]);
    }
}

enum Containers {
    Loading,
    Loaded(Vec<ContainerInfo>),
    Failed(String),
}

#[derive(Clone)]
struct CellMetrics {
    width: Pixels,
    height: Pixels,
    font_size: Pixels,
    font: Font,
}

impl CellMetrics {
    fn measure(window: &Window, cx: &App) -> Option<Self> {
        let font_size = rems(FONT_REMS).to_pixels(window.rem_size());
        let font = font(cx.theme().mono_font_family.clone());
        let system = window.text_system();
        let width = system.em_advance(system.resolve_font(&font), font_size);
        match width {
            Ok(width) => Some(Self {
                width,
                height: (font_size * LINE_SPACING).round(),
                font_size,
                font,
            }),
            Err(error) => {
                tracing::error!(%error, "could not measure the terminal font");
                None
            }
        }
    }

    fn grid_size(&self, bounds: Bounds<Pixels>) -> ExecSize {
        let count = |length: Pixels, cell: Pixels| (length / cell).floor().clamp(1., 1000.) as u16;
        ExecSize {
            cols: count(bounds.size.width, self.width).max(2),
            rows: count(bounds.size.height, self.height),
        }
    }

    fn cell_bounds(
        &self,
        bounds: Bounds<Pixels>,
        line: usize,
        column: usize,
        cells: usize,
    ) -> Bounds<Pixels> {
        Bounds::new(
            point(
                bounds.origin.x + self.width * column as f32,
                bounds.origin.y + self.height * line as f32,
            ),
            size(self.width * cells as f32, self.height),
        )
    }
}

fn paint_frame(
    frame: Frame,
    bounds: Bounds<Pixels>,
    metrics: &CellMetrics,
    palette: Palette,
    window: &mut Window,
    cx: &mut App,
) {
    window.paint_quad(fill(bounds, palette.background));
    for block in &frame.blocks {
        let area = metrics.cell_bounds(bounds, block.line, block.column, block.cells);
        window.paint_quad(fill(area, block.color));
    }
    if let Some(cursor) = frame.cursor {
        let cell = metrics.cell_bounds(bounds, cursor.line, cursor.column, 1);
        let stroke = px(CURSOR_STROKE);
        match cursor.mark {
            CursorMark::Filled => {}
            CursorMark::Outline => {
                window.paint_quad(outline(cell, palette.cursor, BorderStyle::Solid));
            }
            CursorMark::Beam => {
                let beam = Bounds::new(cell.origin, size(stroke, cell.size.height));
                window.paint_quad(fill(beam, palette.cursor));
            }
            CursorMark::Underline => {
                let origin = point(cell.origin.x, cell.bottom() - stroke);
                let line = Bounds::new(origin, size(cell.size.width, stroke));
                window.paint_quad(fill(line, palette.cursor));
            }
        }
    }
    for span in frame.spans {
        let mut font = metrics.font.clone();
        if span.bold {
            font.weight = FontWeight::BOLD;
        }
        if span.italic {
            font.style = FontStyle::Italic;
        }
        let run = TextRun {
            len: span.text.len(),
            font,
            color: span.color,
            background_color: None,
            underline: span.underline.then_some(UnderlineStyle {
                thickness: px(1.),
                color: Some(span.color),
                wavy: false,
            }),
            strikethrough: span.strikeout.then_some(StrikethroughStyle {
                thickness: px(1.),
                color: Some(span.color),
            }),
        };
        // Glyphs from a fallback font can have any width, so this holds the text
        // to the grid. A wide character fills its two cells on its own.
        let single_cells = span.text.chars().count() == span.cells;
        let forced = single_cells.then_some(metrics.width);
        let origin = metrics
            .cell_bounds(bounds, span.line, span.column, 1)
            .origin;
        let line =
            window
                .text_system()
                .shape_line(span.text.into(), metrics.font_size, &[run], forced);
        if let Err(error) = line.paint(origin, metrics.height, TextAlign::Left, None, window, cx) {
            tracing::warn!(%error, "could not draw terminal text");
        }
    }
}

pub struct TerminalTab {
    key: ClusterKey,
    target: ObjectTarget,
    requested: Option<String>,
    model: Entity<ClusterModel>,
    shell: Entity<ShellState>,
    services: Arc<dyn ClusterServices>,
    containers: Containers,
    selected: Option<String>,
    /// What to run, once the user has entered it in place of a shell.
    command: Option<Vec<String>>,
    command_input: Entity<InputState>,
    command_invalid: bool,
    stream: Option<ExecStream>,
    state: Option<ExecState>,
    emulator: Arc<Emulator>,
    palette: Palette,
    size: ExecSize,
    bounds: Bounds<Pixels>,
    metrics: Option<CellMetrics>,
    /// Wheel travel that has not added up to a line yet.
    wheel: Pixels,
    selecting: bool,
    pressed_button: Option<u8>,
    focus: FocusHandle,
    _parser: Option<Task<()>>,
    _frames: Option<Task<()>>,
    _states: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl TerminalTab {
    pub fn new(
        key: ClusterKey,
        target: ObjectTarget,
        requested: Option<String>,
        model: Entity<ClusterModel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let services = cx.global::<SharedServices>().0.clone();
        let shell = cx.global::<SharedShellState>().0.clone();
        let command_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("shell.command").to_string()));
        let subscriptions = vec![
            cx.subscribe_in(&command_input, window, |this, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.run_entered_command(window, cx);
                }
            }),
            cx.observe_in(&model, window, |this, _, window, cx| {
                this.sync_with_connection(window, cx);
                cx.notify();
            }),
        ];
        let palette = Palette::from_theme(cx.theme());
        let mut tab = Self {
            key,
            target,
            requested,
            model,
            shell,
            services,
            containers: Containers::Loading,
            selected: None,
            command: None,
            command_input,
            command_invalid: false,
            stream: None,
            state: None,
            emulator: Arc::new(Emulator::new(START_SIZE.cols, START_SIZE.rows, palette)),
            palette,
            size: START_SIZE,
            bounds: Bounds::default(),
            metrics: None,
            wheel: px(0.),
            selecting: false,
            pressed_button: None,
            focus: cx.focus_handle(),
            _parser: None,
            _frames: None,
            _states: None,
            _subscriptions: subscriptions,
        };
        tab.load_containers(window, cx);
        tab
    }

    pub fn key(&self) -> &ClusterKey {
        &self.key
    }

    pub fn target(&self) -> &ObjectTarget {
        &self.target
    }

    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    pub fn state(&self) -> Option<&ExecState> {
        self.state.as_ref()
    }

    pub fn emulator(&self) -> &Arc<Emulator> {
        &self.emulator
    }

    pub fn palette(&self) -> Palette {
        self.palette
    }

    pub fn grid_bounds(&self) -> Bounds<Pixels> {
        self.bounds
    }

    pub fn grid_size(&self) -> ExecSize {
        self.size
    }

    pub fn command_input(&self) -> &Entity<InputState> {
        &self.command_input
    }

    pub fn focus_grid(&self, window: &mut Window, cx: &mut App) {
        self.focus.focus(window, cx);
    }

    fn activate(&self, cx: &mut Context<Self>) {
        activate_tab(cx.entity_id(), &self.key, &self.model, &self.shell, cx);
    }

    fn load_containers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.containers = Containers::Loading;
        let reply = self
            .services
            .get_object(&self.key.context, self.target.clone());
        cx.spawn_in(window, async move |this, cx| {
            let pod = reply.await;
            let _ = this.update_in(cx, |this, window, cx| match pod {
                Ok(pod) => this.containers_loaded(pod_containers(&pod), window, cx),
                Err(error) => {
                    this.containers = Containers::Failed(error.to_string());
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// One container, or one the caller named, needs no choice from the user.
    fn containers_loaded(
        &mut self,
        containers: Vec<ContainerInfo>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let chosen = match &self.requested {
            Some(name) if !containers.iter().any(|container| container.name == *name) => {
                let error = t!("log.no_container", name = name).to_string();
                self.containers = Containers::Failed(error);
                cx.notify();
                return;
            }
            Some(name) => Some(name.clone()),
            None => match containers.as_slice() {
                [only] => Some(only.name.clone()),
                _ => None,
            },
        };
        self.containers = Containers::Loaded(containers);
        if let Some(name) = chosen {
            self.choose_container(name, window, cx);
        }
        cx.notify();
    }

    fn choose_container(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = Some(name);
        self.start_session(window, cx);
    }

    fn start_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(container) = self.selected.clone() else {
            return;
        };
        let request = ExecRequest {
            namespace: self.target.namespace.clone().unwrap_or_default(),
            pod: self.target.name.clone(),
            container,
            command: self.command.clone(),
        };
        if self.state.is_some() {
            self.emulator.restore_modes();
        }
        self.stream = None;
        let Some(mut stream) = self
            .services
            .open_exec(&self.key.context, request, self.size)
        else {
            self.state = Some(ExecState::Failed(t!("shell.not_connected").to_string()));
            cx.notify();
            return;
        };
        self.state = Some(ExecState::Connecting);

        let (dirty, mut frames) = watch::channel(());
        let (emulator, input) = (self.emulator.clone(), stream.input());
        self._parser = stream.take_output().map(|mut output| {
            cx.background_spawn(async move {
                while let Some(bytes) = output.recv().await {
                    let replies = emulator.advance(&bytes);
                    if !replies.is_empty() {
                        let _ = input.send(replies);
                    }
                    dirty.send_replace(());
                }
            })
        });
        // Notifying only asks for the next frame, so a burst of output draws once.
        self._frames = Some(cx.spawn(async move |this, cx| {
            while frames.changed().await.is_ok() {
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    return;
                }
            }
        }));
        let mut states = stream.subscribe();
        self._states = Some(cx.spawn(async move |this, cx| {
            loop {
                let state = states.borrow_and_update().clone();
                if this
                    .update(cx, |this, cx| this.apply_state(state, cx))
                    .is_err()
                {
                    return;
                }
                if states.changed().await.is_err() {
                    let _ = this.update(cx, |this, cx| this.session_gone(cx));
                    return;
                }
            }
        }));
        self.stream = Some(stream);
        self.focus_grid(window, cx);
        cx.notify();
    }

    fn apply_state(&mut self, state: ExecState, cx: &mut Context<Self>) {
        self.state = Some(state);
        cx.notify();
    }

    /// A disconnect ends the session without any word on how it ended.
    fn session_gone(&mut self, cx: &mut Context<Self>) {
        if matches!(self.state, Some(ExecState::Connecting | ExecState::Running)) {
            self.apply_state(ExecState::Dropped, cx);
        }
    }

    fn run_entered_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.command_input.read(cx).value().to_string();
        match shlex::split(&text).filter(|words| !words.is_empty()) {
            Some(words) => {
                self.command = Some(words);
                self.command_invalid = false;
                self.start_session(window, cx);
            }
            None => {
                self.command_invalid = true;
                cx.notify();
            }
        }
    }

    /// A shell never comes back on its own, because a new one would not be the
    /// one the user left. Reconnecting is their choice.
    fn sync_with_connection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.model.read(cx);
        let live = is_live(&model.connection(&self.key.context));
        if live && model.same_server(&self.key) && matches!(self.containers, Containers::Failed(_))
        {
            self.load_containers(window, cx);
        }
    }

    fn can_reconnect(&self, cx: &App) -> bool {
        let model = self.model.read(cx);
        model.same_server(&self.key) && is_live(&model.connection(&self.key.context))
    }

    /// Fits the grid to the space it has, and tells the container.
    fn fit(&mut self, bounds: Bounds<Pixels>, metrics: CellMetrics) {
        let size = metrics.grid_size(bounds);
        self.bounds = bounds;
        self.metrics = Some(metrics);
        if size != self.size {
            self.size = size;
            self.emulator.resize(size.cols, size.rows);
            if let Some(stream) = &self.stream {
                stream.resize(size);
            }
        }
    }

    fn send(&self, bytes: Vec<u8>) {
        if let Some(stream) = &self.stream {
            let _ = stream.input().send(bytes);
        }
    }

    fn type_bytes(&self, bytes: Vec<u8>, cx: &mut Context<Self>) {
        self.emulator.scroll_to_end();
        self.emulator.clear_selection();
        self.send(bytes);
        cx.notify();
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(bytes) = key_bytes(&event.keystroke, self.emulator.mode()) {
            cx.stop_propagation();
            self.type_bytes(bytes, cx);
        }
    }

    fn copy(&mut self, _: &CopySelection, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.emulator.selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn paste(&mut self, _: &PasteClipboard, _: &mut Window, cx: &mut Context<Self>) {
        let text = cx.read_from_clipboard().and_then(|item| item.text());
        if let Some(text) = text {
            self.type_bytes(paste_bytes(&text, self.emulator.mode()), cx);
        }
    }

    fn cell_at(&self, position: Point<Pixels>) -> Option<(usize, usize, Side)> {
        let metrics = self.metrics.as_ref()?;
        let local = position - self.bounds.origin;
        let column = (local.x / metrics.width).max(0.);
        let line = (local.y / metrics.height).max(0.) as usize;
        let side = if column.fract() < 0.5 {
            Side::Left
        } else {
            Side::Right
        };
        let last_line = usize::from(self.size.rows) - 1;
        Some((line.min(last_line), column as usize, side))
    }

    /// Whether the program gets the mouse. Holding shift keeps it for selecting.
    fn program_has_mouse(&self, modifiers: Modifiers) -> bool {
        self.emulator.mode().intersects(TermMode::MOUSE_MODE) && !modifiers.shift
    }

    fn report_mouse(&self, action: MouseAction, position: Point<Pixels>) {
        if let Some((line, column, _)) = self.cell_at(position)
            && let Some(bytes) = mouse_report(action, line, column, self.emulator.mode())
        {
            self.send(bytes);
        }
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_grid(window, cx);
        let button = match event.button {
            MouseButton::Left => 0,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
            _ => return,
        };
        if self.program_has_mouse(event.modifiers) {
            self.pressed_button = Some(button);
            self.report_mouse(MouseAction::Press(button), event.position);
            return;
        }
        if event.button != MouseButton::Left {
            return;
        }
        let Some((line, column, side)) = self.cell_at(event.position) else {
            return;
        };
        let kind = match event.click_count {
            1 => SelectionType::Simple,
            2 => SelectionType::Semantic,
            _ => SelectionType::Lines,
        };
        self.emulator.start_selection(kind, line, column, side);
        self.selecting = true;
        cx.notify();
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(button) = self.pressed_button {
            self.report_mouse(MouseAction::Motion(button), event.position);
        } else if self.selecting
            && let Some((line, column, side)) = self.cell_at(event.position)
        {
            self.emulator.extend_selection(line, column, side);
            cx.notify();
        }
    }

    fn mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.selecting = false;
        if let Some(button) = self.pressed_button.take() {
            self.report_mouse(MouseAction::Release(button), event.position);
        }
    }

    fn scroll_wheel(&mut self, event: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(height) = self.metrics.as_ref().map(|metrics| metrics.height) else {
            return;
        };
        self.wheel += event.delta.pixel_delta(height).y;
        let lines = (self.wheel / height).trunc() as i32;
        self.wheel -= height * lines as f32;
        if lines == 0 {
            return;
        }
        let up = lines > 0;
        let mode = self.emulator.mode();
        if self.program_has_mouse(event.modifiers) {
            let action = if up {
                MouseAction::WheelUp
            } else {
                MouseAction::WheelDown
            };
            for _ in 0..lines.abs() {
                self.report_mouse(action, event.position);
            }
        } else if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) {
            self.send(wheel_keys(up, mode).repeat(lines.unsigned_abs() as usize));
        } else {
            self.emulator.scroll(lines);
        }
        cx.notify();
    }

    fn render_grid(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let tab = cx.entity();
        let (emulator, palette, focus) = (self.emulator.clone(), self.palette, self.focus.clone());
        div()
            .id("terminal-grid")
            .track_focus(&self.focus)
            .key_context(CONTEXT)
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_hidden()
            .on_key_down(cx.listener(Self::key_down))
            .on_action(cx.listener(|this, _: &SendTab, _, cx| {
                this.type_bytes(b"\t".to_vec(), cx);
            }))
            .on_action(cx.listener(|this, _: &SendBackTab, _, cx| {
                this.type_bytes(b"\x1b[Z".to_vec(), cx);
            }))
            .on_action(cx.listener(|this, _: &SendXon, _, cx| {
                this.type_bytes(vec![0x11], cx);
            }))
            .on_action(cx.listener(|this, _: &SendInterrupt, _, cx| {
                this.type_bytes(vec![0x03], cx);
            }))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::paste))
            .on_any_mouse_down(cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .capture_any_mouse_up(cx.listener(Self::mouse_up))
            .on_scroll(cx.listener(Self::scroll_wheel))
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let metrics = CellMetrics::measure(window, cx)?;
                        tab.update(cx, |tab, _| tab.fit(bounds, metrics.clone()));
                        let frame = emulator.frame(&palette, focus.is_focused(window));
                        Some((frame, metrics))
                    },
                    move |bounds, painted, window, cx| {
                        if let Some((frame, metrics)) = painted {
                            paint_frame(frame, bounds, &metrics, palette, window, cx);
                        }
                    },
                )
                .size_full(),
            )
            .test_support()
    }

    fn render_containers(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let Containers::Loaded(containers) = &self.containers else {
            return None;
        };
        self.selected.is_none().then(|| {
            div()
                .id("terminal-containers")
                .flex()
                .flex_wrap()
                .items_center()
                .gap_1()
                .child(t!("shell.choose_container").to_string())
                .children(containers.iter().map(|container| {
                    let name = container.name.clone();
                    let label = match container.kind {
                        ContainerKind::Regular => name.clone(),
                        ContainerKind::Init => t!("log.init_container", name = name).to_string(),
                        ContainerKind::Ephemeral => {
                            t!("log.ephemeral_container", name = name).to_string()
                        }
                    };
                    Button::new(SharedString::from(format!("terminal-container-{name}")))
                        .label(label)
                        .small()
                        .selected(self.selected.as_ref() == Some(&name))
                        .disabled(!container.running)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.choose_container(name.clone(), window, cx);
                        }))
                }))
                .test_support()
        })
    }

    fn render_status(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let muted = cx.theme().muted_foreground;
        let note = |id: &'static str, text: String| {
            div()
                .id(id)
                .text_xs()
                .text_color(muted)
                .child(text)
                .test_support()
                .into_any_element()
        };
        let mut notes = Vec::new();
        match &self.state {
            None | Some(ExecState::Running) => {}
            Some(ExecState::Connecting) => {
                notes.push(note(
                    "terminal-connecting",
                    t!("shell.connecting").to_string(),
                ));
            }
            Some(ExecState::Exited(code)) => {
                notes.push(note(
                    "terminal-exited",
                    t!("shell.exited", code = code).to_string(),
                ));
            }
            Some(ExecState::Dropped) => {
                notes.push(note("terminal-dropped", t!("shell.dropped").to_string()));
            }
            Some(ExecState::Failed(reason)) => {
                notes.push(note(
                    "terminal-failed",
                    t!("shell.failed", error = reason).to_string(),
                ));
            }
            Some(ExecState::NoShell) => {
                notes.push(note("terminal-no-shell", t!("shell.no_shell").to_string()));
                notes.push(self.render_command_prompt(cx).into_any_element());
            }
        }
        if self.command_invalid {
            notes.push(note(
                "terminal-bad-command",
                t!("shell.bad_command").to_string(),
            ));
        }
        let ended = matches!(
            self.state,
            Some(ExecState::Exited(_) | ExecState::Dropped | ExecState::Failed(_))
        );
        if ended && self.can_reconnect(cx) {
            notes.push(
                Button::new("terminal-reconnect")
                    .icon(IconName::RotateCw)
                    .label(t!("shell.reconnect").to_string())
                    .small()
                    .on_click(cx.listener(|this, _, window, cx| this.start_session(window, cx)))
                    .into_any_element(),
            );
        }
        notes
    }

    fn render_command_prompt(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .w(px(COMMAND_WIDTH))
                    .child(Input::new(&self.command_input)),
            )
            .child(
                Button::new("terminal-run")
                    .icon(IconName::Play)
                    .label(t!("shell.run").to_string())
                    .small()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.run_entered_command(window, cx);
                    })),
            )
    }
}

impl gpui_kit::base::dock::Panel for TerminalTab {
    fn panel_name(&self) -> &'static str {
        PANEL_NAME
    }

    fn zoomable(&self, _: &App) -> bool {
        false
    }

    fn closable(&self, _: &App) -> bool {
        false
    }

    fn on_added_to(
        &mut self,
        _: WeakEntity<gpui_kit::base::dock::TabGroup>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (id, tab) = (cx.entity_id(), cx.weak_entity());
        self.shell
            .update(cx, |shell, _| shell.add_terminal_tab(id, tab));
    }

    fn on_removed(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.stream = None;
        self._parser = None;
        self._frames = None;
        self._states = None;
        let id = cx.entity_id();
        self.shell.update(cx, |shell, cx| shell.remove_tab(id, cx));
    }

    fn set_active(&mut self, active: bool, window: &mut Window, cx: &mut Context<Self>) {
        if active {
            self.activate(cx);
            self.focus_grid(window, cx);
        }
    }
}

impl dock::Panel for TerminalTab {
    fn title(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let identity = self.model.read(cx).identity(&self.key);
        div()
            .id(SharedString::from(format!("tab-title-{}", cx.entity_id())))
            .flex()
            .items_center()
            .gap_2()
            .child(div().size_2().rounded_full().bg(rgb(identity.color)))
            .child(Icon::new(IconName::SquareTerminal).xsmall())
            .child(t!("shell.title", name = self.target.name).to_string())
            .child(identity.name)
            .when(identity.protected, |this| {
                this.child(t!("cluster.protected_tag").to_string())
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.activate(cx)),
            )
            .on_aux_click(close_on_middle_click(cx.entity_id(), self.shell.clone()))
            .child(close_button(cx.entity_id(), self.shell.clone()))
            .test_support()
    }
}

impl EventEmitter<PanelEvent> for TerminalTab {}

impl Focusable for TerminalTab {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TerminalTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::from_theme(cx.theme());
        if palette != self.palette {
            self.palette = palette;
            self.emulator.set_palette(palette);
        }

        let identity = self.model.read(cx).identity(&self.key);
        let on_its_cluster = self.model.read(cx).same_server(&self.key);
        let failure = match &self.containers {
            Containers::Failed(reason) => Some(reason.clone()),
            _ => None,
        };
        let namespace = self.target.namespace.clone().unwrap_or_default();
        let container = self.selected.clone().unwrap_or_default();
        let target = t!("log.target", pod = self.target.name, container = container);
        div()
            .id("terminal-tab")
            .capture_any_mouse_down(cx.listener(|this, _, _, cx| this.activate(cx)))
            .size_full()
            .flex()
            .flex_col()
            .items_start()
            .gap_2()
            .p_2()
            .child(div().h_1().w_full().flex_none().bg(rgb(identity.color)))
            .child(
                div()
                    .id("terminal-target")
                    .flex()
                    .flex_wrap()
                    .gap_3()
                    .child(
                        div()
                            .text_color(rgb(identity.color))
                            .child(t!("detail.cluster", name = identity.name).to_string()),
                    )
                    .when(identity.protected, |this| {
                        this.child(t!("cluster.protected_tag").to_string())
                    })
                    .child(t!("detail.namespace_line", namespace = namespace).to_string())
                    .child(target.to_string())
                    .test_support(),
            )
            .children(connection_notes(&self.key, &self.model, cx))
            .children(failure.map(|reason| {
                div()
                    .id("terminal-load-error")
                    .child(t!("detail.load_failed", error = reason).to_string())
                    .test_support()
            }))
            .when(on_its_cluster, |this| {
                this.children(self.render_containers(cx))
                    .children(self.render_status(cx))
            })
            .child(self.render_grid(cx))
            .test_support()
    }
}
