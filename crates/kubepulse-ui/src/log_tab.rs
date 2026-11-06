//! A pod's logs. The list reads only the lines it shows from the bounded buffer,
//! so high log volume costs one small update per frame.

use std::{collections::VecDeque, ops::Range, sync::Arc};

use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme as _, Icon, Selectable as _, Sizable as _,
        button::Button,
        checkbox::Checkbox,
        dock::{self, PanelEvent},
        input::{Input, InputEvent, InputState},
        scroll::{Scrollbar, ScrollbarMode},
    },
    prelude::FluentBuilder as _,
    *,
};
use kubepulse_cluster::{
    ContainerInfo, ContainerKind, LogLine, LogRequest, LogState, LogStatus, LogStream,
    ObjectTarget, match_ranges, pod_containers,
};
use rust_i18n::t;
use unicode_width::UnicodeWidthStr as _;

use crate::{
    cluster_model::{ClusterModel, can_connect, is_live},
    identity::ClusterKey,
    services::{ClusterServices, SharedServices},
    shell_state::{SharedShellState, ShellState, close_button, close_on_middle_click},
    tab_connection::{activate_tab, connection_notes},
};

pub const PANEL_NAME: &str = "LogTab";

/// Longer lines are cut for display, because shaping them stalls a frame. They
/// stay whole in the buffer and in search.
const MAX_SHOWN_BYTES: usize = 4096;
const SEARCH_WIDTH: f32 = 240.;

enum Containers {
    Loading,
    Loaded(Vec<ContainerInfo>),
    Failed(String),
}

fn shown_text(line: &LogLine, with_time: bool) -> (String, usize) {
    let prefix = match line.timestamp() {
        Some(time) if with_time => format!("{time} "),
        _ => String::new(),
    };
    let start = prefix.len();
    let mut text = prefix + line.text();
    if text.len() > MAX_SHOWN_BYTES {
        text.truncate(text.floor_char_boundary(MAX_SHOWN_BYTES));
        text.push('…');
    }
    (text, start)
}

fn shown_width(line: &LogLine) -> usize {
    let text = line.text();
    text[..text.floor_char_boundary(MAX_SHOWN_BYTES)].width()
}

/// Finds the widest retained line at a constant cost per line. It holds only
/// lines wider than every line after them, so the first is the widest.
#[derive(Default)]
struct WidestLines {
    candidates: VecDeque<(u64, usize)>,
}

impl WidestLines {
    fn push(&mut self, seq: u64, width: usize) {
        while self.candidates.back().is_some_and(|(_, w)| *w <= width) {
            self.candidates.pop_back();
        }
        self.candidates.push_back((seq, width));
    }

    fn forget_before(&mut self, seq: u64) {
        while self.candidates.front().is_some_and(|(s, _)| *s < seq) {
            self.candidates.pop_front();
        }
    }

    fn widest(&self) -> Option<u64> {
        self.candidates.front().map(|(seq, _)| *seq)
    }
}

pub struct LogTab {
    key: ClusterKey,
    target: ObjectTarget,
    requested: Option<String>,
    model: Entity<ClusterModel>,
    shell: Entity<ShellState>,
    services: Arc<dyn ClusterServices>,
    containers: Containers,
    selected: Option<String>,
    previous: bool,
    stream: Option<LogStream>,
    status: Option<LogStatus>,
    show_times: bool,
    following: bool,
    list: UniformListScrollHandle,
    /// How far down the list was at the last look, to notice the user scrolling
    /// up.
    scrolled: Pixels,
    /// The rows at the last render, which the list measured.
    rows: usize,
    /// The oldest retained line at the last update.
    first: u64,
    widest: WidestLines,
    /// Lines before this one have been measured.
    measured_to: u64,
    search: Entity<InputState>,
    query: String,
    /// Sequence numbers of matching lines, oldest first.
    matches: Vec<u64>,
    current_match: Option<usize>,
    /// Lines before this one have been searched.
    searched_to: u64,
    focus: FocusHandle,
    _updates: Option<Task<()>>,
    _search_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl LogTab {
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
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("log.search").to_string()));
        let subscriptions = vec![
            cx.subscribe(&search, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let query = input.read(cx).value().to_string();
                    this.set_query(query, cx);
                }
            }),
            cx.observe_in(&model, window, |this, _, window, cx| {
                this.sync_with_connection(window, cx);
                cx.notify();
            }),
        ];
        let mut tab = Self {
            key,
            target,
            requested,
            model,
            shell,
            services,
            containers: Containers::Loading,
            selected: None,
            previous: false,
            stream: None,
            status: None,
            show_times: false,
            following: true,
            list: UniformListScrollHandle::new(),
            scrolled: px(0.),
            rows: 0,
            first: 0,
            widest: WidestLines::default(),
            measured_to: 0,
            search,
            query: String::new(),
            matches: Vec::new(),
            current_match: None,
            searched_to: 0,
            focus: cx.focus_handle(),
            _updates: None,
            _search_task: None,
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

    pub fn search_input(&self) -> &Entity<InputState> {
        &self.search
    }

    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    pub fn following(&self) -> bool {
        self.following
    }

    pub fn matches(&self) -> &[u64] {
        &self.matches
    }

    pub fn current_match(&self) -> Option<u64> {
        self.current_match.map(|index| self.matches[index])
    }

    /// How far the view has scrolled sideways, and how far it can.
    pub fn sideways_scroll(&self) -> (Pixels, Pixels) {
        let state = self.list.0.borrow();
        (
            -state.base_handle.offset().x,
            state.base_handle.max_offset().x,
        )
    }

    /// The line count this view knows about.
    pub fn retained(&self) -> Option<Range<u64>> {
        Some(self.stream.as_ref()?.buffer().bounds())
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

    fn containers_loaded(
        &mut self,
        containers: Vec<ContainerInfo>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let wanted = self.requested.as_ref().map_or_else(
            || containers.first().map(|container| container.name.clone()),
            |name| {
                containers
                    .iter()
                    .any(|container| container.name == *name)
                    .then(|| name.clone())
            },
        );
        match wanted {
            Some(name) => {
                self.containers = Containers::Loaded(containers);
                self.selected = Some(name);
                self.start_stream(window, cx);
            }
            None => {
                let name = self.requested.clone().unwrap_or_default();
                let error = t!("log.no_container", name = name).to_string();
                self.containers = Containers::Failed(error);
            }
        }
        cx.notify();
    }

    fn start_stream(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(container) = self.selected.clone() else {
            return;
        };
        self.following = true;
        self.scrolled = px(0.);
        self.list
            .0
            .borrow()
            .base_handle
            .set_offset(point(px(0.), px(0.)));
        self.first = 0;
        self.widest = WidestLines::default();
        self.measured_to = 0;
        self.matches.clear();
        self.current_match = None;
        self.searched_to = 0;
        let request = LogRequest {
            namespace: self.target.namespace.clone().unwrap_or_default(),
            pod: self.target.name.clone(),
            container,
            previous: self.previous,
        };
        self.stream = self.services.open_logs(&self.key.context, request);
        self.status = None;
        self._updates = self.stream.as_ref().map(|stream| {
            let mut updates = stream.subscribe();
            cx.spawn_in(window, async move |this, cx| {
                loop {
                    let status = updates.borrow_and_update().clone();
                    let applied = this.update(cx, |this, cx| this.apply_status(status, cx));
                    if applied.is_err() || updates.changed().await.is_err() {
                        return;
                    }
                }
            })
        });
        cx.notify();
    }

    /// Takes in what arrived since the last frame.
    fn apply_status(&mut self, status: LogStatus, cx: &mut Context<Self>) {
        let Some(buffer) = self.stream.as_ref().map(|stream| stream.buffer().clone()) else {
            return;
        };
        let bounds = buffer.bounds();
        let dropped = usize::try_from(bounds.start - self.first).unwrap_or(usize::MAX);
        self.first = bounds.start;
        self.status = Some(status);
        self.search_new_lines(&bounds, cx);
        for (seq, line) in buffer.lines(self.measured_to..bounds.end) {
            self.widest.push(seq, shown_width(&line));
        }
        self.measured_to = bounds.end;
        self.widest.forget_before(bounds.start);

        if !self.following && dropped > 0 {
            self.keep_place(dropped);
        }
        cx.notify();
    }

    /// Older lines left, so every row moved up. This moves the list by the same
    /// distance, which keeps the same lines in view.
    fn keep_place(&mut self, dropped: usize) {
        let state = self.list.0.borrow();
        let Some(size) = state.last_item_size.filter(|_| self.rows > 0) else {
            return;
        };
        let row_height = size.contents.height / self.rows as f32;
        let offset = state.base_handle.offset();
        let moved = (offset.y + row_height * dropped as f32).min(px(0.));
        state.base_handle.set_offset(point(offset.x, moved));
        self.scrolled = -moved;
    }

    /// Scrolling redraws this view, so a render that finds the list moved up
    /// stops following. It runs before the render pins the list to the end.
    fn notice_scrolling(&mut self) {
        let offset = -self.list.0.borrow().base_handle.offset().y;
        let at_end = self.list.is_scrolled_to_end() != Some(false);
        // Subtracting, because `Pixels` orders -0 below 0.
        if self.following && !at_end && self.scrolled - offset > px(0.) {
            self.following = false;
        }
        self.scrolled = offset;
    }

    fn search_new_lines(&mut self, bounds: &Range<u64>, _: &mut Context<Self>) {
        let kept = self.matches.partition_point(|seq| *seq < bounds.start);
        self.matches.drain(..kept);
        self.current_match = self
            .current_match
            .and_then(|current| current.checked_sub(kept));
        if self.query.is_empty() {
            self.searched_to = bounds.end;
            return;
        }
        let Some(stream) = &self.stream else {
            return;
        };
        for (seq, line) in stream.buffer().lines(self.searched_to..bounds.end) {
            if !match_ranges(line.text(), &self.query).is_empty() {
                self.matches.push(seq);
            }
        }
        self.searched_to = bounds.end;
        if self.current_match.is_none() && !self.matches.is_empty() {
            self.current_match = Some(0);
        }
    }

    /// Searches every retained line away from the UI thread.
    fn set_query(&mut self, query: String, cx: &mut Context<Self>) {
        self.query.clone_from(&query);
        self.matches.clear();
        self.current_match = None;
        let Some(buffer) = self.stream.as_ref().map(|stream| stream.buffer().clone()) else {
            return;
        };
        let bounds = buffer.bounds();
        self.searched_to = bounds.end;
        let task = cx.spawn(async move |this, cx| {
            let lines = bounds.clone();
            let found = cx
                .background_spawn(async move {
                    let lines = buffer.lines(lines);
                    let found = lines
                        .iter()
                        .filter(|(_, line)| !match_ranges(line.text(), &query).is_empty());
                    found.map(|(seq, _)| *seq).collect::<Vec<u64>>()
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                // Lines that arrived meanwhile were appended already.
                let mut matches = found;
                matches.append(&mut this.matches);
                this.matches = matches;
                this.go_to_match(0, cx);
            });
        });
        self._search_task = Some(task);
        cx.notify();
    }

    fn go_to_match(&mut self, index: usize, cx: &mut Context<Self>) {
        self.current_match = (index < self.matches.len()).then_some(index);
        if let Some(seq) = self.current_match() {
            self.following = false;
            let row = usize::try_from(seq.saturating_sub(self.first)).unwrap_or(usize::MAX);
            self.list.scroll_to_item_strict(row, ScrollStrategy::Center);
        }
        cx.notify();
    }

    fn step_match(&mut self, forward: bool, cx: &mut Context<Self>) {
        let count = self.matches.len();
        if count == 0 {
            return;
        }
        let current = self.current_match.unwrap_or(0);
        let next = if forward {
            (current + 1) % count
        } else {
            (current + count - 1) % count
        };
        self.go_to_match(next, cx);
    }

    fn select_container(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = Some(name);
        self.previous = false;
        self.start_stream(window, cx);
    }

    /// A tab holds no connection. Its stream and lines go with the cluster's
    /// session, and come back once the user connects it.
    fn sync_with_connection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.model.read(cx);
        let state = model.connection(&self.key.context);
        if !model.same_server(&self.key) || can_connect(&state) {
            self.stream = None;
            self.status = None;
            self._updates = None;
        } else if is_live(&state) && self.stream.is_none() {
            match self.containers {
                Containers::Loaded(_) => self.start_stream(window, cx),
                Containers::Failed(_) => self.load_containers(window, cx),
                Containers::Loading => {}
            }
        }
    }

    fn render_line(&self, seq: u64, line: &LogLine, cx: &App) -> impl IntoElement + use<> {
        let (text, start) = shown_text(line, self.show_times);
        let highlight = HighlightStyle {
            background_color: Some(cx.theme().warning),
            color: Some(cx.theme().warning_foreground),
            ..HighlightStyle::default()
        };
        let highlights: Vec<(Range<usize>, HighlightStyle)> =
            match_ranges(line.text(), &self.query)
                .into_iter()
                .map(|range| range.start + start..range.end + start)
                .filter(|range| {
                    range.end <= text.len()
                        && text.is_char_boundary(range.start)
                        && text.is_char_boundary(range.end)
                })
                .map(|range| (range, highlight))
                .collect();
        let current = self.current_match() == Some(seq);
        div()
            .id(SharedString::from(format!("log-line-{seq}")))
            .w_full()
            .whitespace_nowrap()
            .overflow_hidden()
            .when(current, |this| this.bg(cx.theme().accent))
            .child(StyledText::new(text).with_highlights(highlights))
            .test_support()
    }

    fn render_lines(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let bounds = self.retained()?;
        let count = usize::try_from(bounds.end - bounds.start).unwrap_or(usize::MAX);
        let widest = self
            .widest
            .widest()
            .map(|seq| usize::try_from(seq.saturating_sub(self.first)).unwrap_or(usize::MAX));
        // The list measures its rows outside its own text style, so the style
        // goes on a parent.
        Some(
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .flex()
                .flex_col()
                .font_family(cx.theme().mono_font_family.clone())
                .text_sm()
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .child(
                            uniform_list(
                                "log-lines",
                                count,
                                cx.processor(move |this, range: Range<usize>, _, cx| {
                                    let first = this.first;
                                    let Some(stream) = &this.stream else {
                                        return Vec::new();
                                    };
                                    let lines = stream.buffer().lines(
                                        first + range.start as u64..first + range.end as u64,
                                    );
                                    lines
                                        .iter()
                                        .map(|(seq, line)| this.render_line(*seq, line, cx))
                                        .collect()
                                }),
                            )
                            .with_horizontal_sizing_behavior(
                                ListHorizontalSizingBehavior::Unconstrained,
                            )
                            .with_width_from_item(widest)
                            .track_scroll(&self.list)
                            .flex_1()
                            .min_w_0()
                            .h_full(),
                        )
                        .child(
                            div()
                                .id("log-vertical-scrollbar")
                                .relative()
                                .flex_none()
                                .h_full()
                                .w(Scrollbar::width())
                                .child(
                                    Scrollbar::vertical(&self.list)
                                        .viewport_from_layout()
                                        .mode(ScrollbarMode::Always)
                                        .max_fps(60),
                                )
                                .test_support(),
                        ),
                )
                .child(
                    div()
                        .relative()
                        .flex_none()
                        .mr(Scrollbar::width())
                        .h(Scrollbar::width())
                        .child(
                            Scrollbar::horizontal(&self.list)
                                .viewport_from_layout()
                                .mode(ScrollbarMode::Always),
                        ),
                ),
        )
    }

    fn render_containers(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let Containers::Loaded(containers) = &self.containers else {
            return None;
        };
        (containers.len() > 1).then(|| {
            div()
                .flex()
                .flex_wrap()
                .gap_1()
                .children(containers.iter().map(|container| {
                    let name = container.name.clone();
                    let label = match container.kind {
                        ContainerKind::Regular => name.clone(),
                        ContainerKind::Init => t!("log.init_container", name = name).to_string(),
                        ContainerKind::Ephemeral => {
                            t!("log.ephemeral_container", name = name).to_string()
                        }
                    };
                    Button::new(SharedString::from(format!("log-container-{name}")))
                        .label(label)
                        .small()
                        .selected(self.selected.as_ref() == Some(&name))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.select_container(name.clone(), window, cx);
                        }))
                }))
        })
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let count = self.matches.len();
        let position = self.current_match.map_or(0, |index| index + 1);
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .w_full()
            .child(
                Checkbox::new("log-timestamps")
                    .label(t!("log.timestamps").to_string())
                    .small()
                    .checked(self.show_times)
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.show_times = *checked;
                        cx.notify();
                    })),
            )
            .child(
                Checkbox::new("log-follow")
                    .label(t!("log.follow").to_string())
                    .small()
                    .checked(self.following)
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.following = *checked;
                        cx.notify();
                    })),
            )
            .when(self.selected.is_some(), |this| {
                this.child(
                    Checkbox::new("log-previous")
                        .label(t!("log.previous").to_string())
                        .small()
                        .checked(self.previous)
                        .on_click(cx.listener(|this, checked: &bool, window, cx| {
                            this.previous = *checked;
                            this.start_stream(window, cx);
                        })),
                )
            })
            .child(
                div()
                    .w(px(SEARCH_WIDTH))
                    .child(Input::new(&self.search).prefix(Icon::new(IconName::Search).xsmall())),
            )
            .when(!self.query.is_empty(), |this| {
                this.child(
                    div()
                        .id("log-match-count")
                        .child(t!("log.matches", current = position, total = count).to_string())
                        .test_support(),
                )
                .child(
                    Button::new("log-match-previous")
                        .icon(IconName::ChevronUp)
                        .small()
                        .tooltip(t!("log.previous_match").to_string())
                        .on_click(cx.listener(|this, _, _, cx| this.step_match(false, cx))),
                )
                .child(
                    Button::new("log-match-next")
                        .icon(IconName::ChevronDown)
                        .small()
                        .tooltip(t!("log.next_match").to_string())
                        .on_click(cx.listener(|this, _, _, cx| this.step_match(true, cx))),
                )
            })
    }

    fn render_status(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut notes = Vec::new();
        let muted = cx.theme().muted_foreground;
        let mut note = |id: &'static str, text: String| {
            notes.push(
                div()
                    .id(id)
                    .text_xs()
                    .text_color(muted)
                    .child(text)
                    .test_support()
                    .into_any_element(),
            );
        };
        let Some(status) = &self.status else {
            return notes;
        };
        match &status.state {
            LogState::Connecting | LogState::Streaming => {}
            LogState::Interrupted => note("log-interrupted", t!("log.interrupted").to_string()),
            LogState::Ended => note("log-ended", t!("log.ended").to_string()),
            LogState::Failed(reason) => {
                note("log-failed", t!("log.failed", error = reason).to_string());
            }
        }
        if status.interruptions > 0 {
            let count = status.interruptions;
            note(
                "log-interruptions",
                t!("log.interruptions", count = count).to_string(),
            );
        }
        if self.first > 0 {
            let count = self.first;
            note("log-dropped", t!("log.dropped", count = count).to_string());
        }
        if matches!(status.state, LogState::Failed(_)) {
            notes.push(
                Button::new("log-retry")
                    .icon(IconName::RotateCw)
                    .label(t!("cluster.retry").to_string())
                    .small()
                    .on_click(cx.listener(|this, _, window, cx| this.start_stream(window, cx)))
                    .into_any_element(),
            );
        }
        notes
    }
}

impl gpui_kit::base::dock::Panel for LogTab {
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
        let (id, key, target) = (cx.entity_id(), self.key.clone(), self.target.clone());
        let tab = cx.weak_entity();
        self.shell
            .update(cx, |shell, _| shell.add_log_tab(id, key, target, tab));
    }

    fn on_removed(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.stream = None;
        let id = cx.entity_id();
        self.shell.update(cx, |shell, cx| shell.remove_tab(id, cx));
    }

    fn set_active(&mut self, active: bool, _: &mut Window, cx: &mut Context<Self>) {
        if active {
            self.activate(cx);
        }
    }
}

impl dock::Panel for LogTab {
    fn title(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let identity = self.model.read(cx).identity(&self.key);
        div()
            .id(SharedString::from(format!("tab-title-{}", cx.entity_id())))
            .flex()
            .items_center()
            .gap_2()
            .child(div().size_2().rounded_full().bg(rgb(identity.color)))
            .child(Icon::new(IconName::Logs).xsmall())
            .child(t!("log.title", name = self.target.name).to_string())
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

impl EventEmitter<PanelEvent> for LogTab {}

impl Focusable for LogTab {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for LogTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.notice_scrolling();
        if self.following {
            self.list.scroll_to_bottom();
        }
        self.rows = self.retained().map_or(0, |bounds| {
            usize::try_from(bounds.end - bounds.start).unwrap_or(usize::MAX)
        });

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
            .id("log-tab")
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
                    .id("log-target")
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
                    .id("log-load-error")
                    .child(t!("detail.load_failed", error = reason).to_string())
                    .test_support()
            }))
            .when(on_its_cluster, |this| {
                this.children(self.render_containers(cx))
                    .child(self.render_toolbar(cx))
                    .children(self.render_status(cx))
                    .children(self.render_lines(cx))
            })
            .test_support()
    }
}

#[cfg(test)]
mod tests {
    use super::{LogLine, MAX_SHOWN_BYTES, WidestLines, shown_text, shown_width};

    const AT: &str = "2026-01-01T00:00:00Z";

    #[test]
    fn a_wider_line_takes_over_from_narrower_ones() {
        let mut lines = WidestLines::default();
        lines.push(0, 5);
        lines.push(1, 9);
        lines.push(2, 3);
        assert_eq!(lines.widest(), Some(1));
    }

    #[test]
    fn the_next_widest_line_takes_over_when_the_widest_leaves() {
        let mut lines = WidestLines::default();
        lines.push(0, 9);
        lines.push(1, 3);
        lines.push(2, 5);
        lines.push(3, 4);
        lines.forget_before(1);
        assert_eq!(lines.widest(), Some(2));
        lines.forget_before(3);
        assert_eq!(lines.widest(), Some(3));
        lines.forget_before(4);
        assert_eq!(lines.widest(), None);
    }

    #[test]
    fn width_counts_display_columns_and_not_bytes() {
        assert!(shown_width(&LogLine::new(&"é".repeat(4))) < shown_width(&LogLine::new("abcdef")));
        assert_eq!(shown_width(&LogLine::new("日本")), 4);
    }

    #[test]
    fn width_stops_at_the_display_cut() {
        let line = LogLine::new(&"a".repeat(MAX_SHOWN_BYTES * 2));
        assert_eq!(shown_width(&line), MAX_SHOWN_BYTES);
    }

    #[test]
    fn timestamps_show_only_when_turned_on() {
        let line = LogLine::new(&format!("{AT} hello"));
        assert_eq!(shown_text(&line, false), ("hello".to_owned(), 0));
        assert_eq!(
            shown_text(&line, true),
            (format!("{AT} hello"), AT.len() + 1)
        );
    }

    #[test]
    fn a_very_long_line_is_cut_on_a_character_boundary() {
        let line = LogLine::new(&"é".repeat(MAX_SHOWN_BYTES));
        let (text, _) = shown_text(&line, false);
        assert!(text.len() <= MAX_SHOWN_BYTES + '…'.len_utf8());
        assert!(text.ends_with('…'));
    }
}
