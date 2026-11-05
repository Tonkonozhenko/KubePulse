//! The thin column of cluster icons. A click selects, a double-click connects,
//! and every other action is on the right-click menu.

use gpui_kit::{
    component::{
        ActiveTheme as _, Icon, IconName, Sizable as _, WindowExt as _,
        button::{Button, ButtonVariants as _},
        input::{Input, InputEvent, InputState},
        menu::{ContextMenuExt as _, PopupMenuItem},
        popover::Popover,
        spinner::Spinner,
        tooltip::Tooltip,
    },
    prelude::FluentBuilder as _,
    *,
};
use kubepulse_cluster::ConnectionState;
use rust_i18n::t;

use crate::{
    cluster_model::{ClusterModel, can_connect, is_live, state_label},
    cluster_settings::ClusterSettingsForm,
    identity::{ClusterIdentity, ClusterKey},
    runtime::ClusterStatus,
    settings_tab::OpenSettings,
    shell_state::{SharedShellState, ShellState},
};

const RAIL_WIDTH: f32 = 48.;
const ICON_SIZE: f32 = 36.;
const ROW_HEIGHT: f32 = 44.;
const SEARCH_WIDTH: f32 = 280.;
const SEARCH_ROW_HEIGHT: f32 = 32.;
const SEARCH_LIST_HEIGHT: f32 = 320.;

pub fn matching(names: &[String], query: &str) -> Vec<usize> {
    let query = query.to_lowercase();
    names
        .iter()
        .enumerate()
        .filter(|(_, name)| name.to_lowercase().contains(&query))
        .map(|(index, _)| index)
        .collect()
}

/// Live clusters first, each set in kubeconfig order. A failed attempt stays in
/// the lower set, so an icon moves only when a connection succeeds.
pub fn rail_order(clusters: &[ClusterStatus]) -> Vec<usize> {
    let (live, rest): (Vec<usize>, Vec<usize>) =
        (0..clusters.len()).partition(|&index| is_live(&clusters[index].state));
    live.into_iter().chain(rest).collect()
}

/// `prod-eu` and `prod-us` become `PE` and `PU` rather than both `PR`.
pub fn initials(name: &str) -> String {
    let segments: Vec<&str> = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|segment| !segment.is_empty())
        .collect();
    let letters: String = match segments.as_slice() {
        [] => String::new(),
        [only] => only.chars().take(2).collect(),
        [first, second, ..] => first
            .chars()
            .take(1)
            .chain(second.chars().take(1))
            .collect(),
    };
    letters.to_uppercase()
}

pub fn tooltip_text(identity: &ClusterIdentity, context: &str, state: &ConnectionState) -> String {
    let label = state_label(state);
    let state = match state {
        ConnectionState::Failed { reason } | ConnectionState::Degraded { reason } => {
            format!("{label}: {reason}")
        }
        _ => label,
    };
    format!("{}\n{context}\n{state}", identity.name)
}

fn choose_cluster(
    context: &str,
    click_count: usize,
    model: &Entity<ClusterModel>,
    shell: &Entity<ShellState>,
    cx: &mut App,
) {
    let key = model.read(cx).key(context);
    shell.update(cx, |shell, cx| shell.select(key, cx));
    let model = model.read(cx);
    if click_count == 2 && can_connect(&model.connection(context)) {
        model.connect(context);
    }
}

pub enum ClusterSearchEvent {
    Chosen(String),
}

impl EventEmitter<ClusterSearchEvent> for ClusterSearch {}

/// What the rail's search popover shows.
pub struct ClusterSearch {
    model: Entity<ClusterModel>,
    shell: Entity<ShellState>,
    input: Entity<InputState>,
    query: String,
    _subscription: Subscription,
}

impl ClusterSearch {
    fn new(
        model: Entity<ClusterModel>,
        shell: Entity<ShellState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("rail.search").to_string()));
        let subscription = cx.subscribe(&input, |this, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.query = input.read(cx).value().to_string();
                cx.notify();
            }
        });
        Self {
            model,
            shell,
            input,
            query: String::new(),
            _subscription: subscription,
        }
    }

    fn render_row(&self, index: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let model = self.model.read(cx);
        let status = &model.state().clusters[index];
        let context = status.entry.context.clone();
        let identity = model.identity(&model.key(&context));
        let label = state_label(&status.state);
        let muted = cx.theme().muted_foreground;
        div()
            .id(SharedString::from(format!("search-{context}")))
            .h(px(SEARCH_ROW_HEIGHT))
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .px_1()
            .cursor_pointer()
            .child(
                div()
                    .size_2()
                    .flex_none()
                    .rounded_full()
                    .bg(rgb(identity.color)),
            )
            .child(div().flex_1().overflow_hidden().child(identity.name))
            .child(div().flex_none().text_xs().text_color(muted).child(label))
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                choose_cluster(&context, event.click_count(), &this.model, &this.shell, cx);
                cx.emit(ClusterSearchEvent::Chosen(context.clone()));
            }))
            .test_support()
    }
}

impl Render for ClusterSearch {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let names: Vec<String> = {
            let model = self.model.read(cx);
            model
                .state()
                .clusters
                .iter()
                .map(|status| {
                    let context = &status.entry.context;
                    let name = model.identity(&model.key(context)).name;
                    format!("{name} {context}")
                })
                .collect()
        };
        let shown = matching(&names, &self.query);
        div()
            .id("cluster-search")
            .w(px(SEARCH_WIDTH))
            .flex()
            .flex_col()
            .gap_2()
            .child(Input::new(&self.input))
            .child(
                uniform_list(
                    "cluster-search-results",
                    shown.len(),
                    cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                        range
                            .map(|row| this.render_row(shown[row], cx))
                            .collect::<Vec<_>>()
                    }),
                )
                .h(px(SEARCH_LIST_HEIGHT))
                .w_full(),
            )
            .test_support()
    }
}

pub struct ClusterRail {
    model: Entity<ClusterModel>,
    shell: Entity<ShellState>,
    search: Entity<ClusterSearch>,
    searching: bool,
    list: UniformListScrollHandle,
    /// Where the selected icon sits, to follow it when the order changes.
    selected_position: Option<usize>,
    _subscriptions: Vec<Subscription>,
}

impl ClusterRail {
    pub fn new(model: Entity<ClusterModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let shell = cx.global::<SharedShellState>().0.clone();
        let search = cx.new(|cx| ClusterSearch::new(model.clone(), shell.clone(), window, cx));
        let subscriptions = vec![
            cx.observe(&model, |this, _, cx| {
                this.follow_selection(cx);
                cx.notify();
            }),
            cx.observe(&shell, |this, _, cx| {
                this.follow_selection(cx);
                cx.notify();
            }),
            cx.subscribe(&search, |this, _, event, cx| match event {
                ClusterSearchEvent::Chosen(_) => {
                    this.searching = false;
                    cx.notify();
                }
            }),
        ];
        Self {
            model,
            shell,
            search,
            searching: false,
            list: UniformListScrollHandle::new(),
            selected_position: None,
            _subscriptions: subscriptions,
        }
    }

    fn follow_selection(&mut self, cx: &mut Context<Self>) {
        let model = self.model.read(cx);
        let clusters = &model.state().clusters;
        let selected = self.shell.read(cx).selected();
        let position = rail_order(clusters).into_iter().position(|index| {
            selected.is_some_and(|key| model.key(&clusters[index].entry.context) == *key)
        });
        if position != self.selected_position {
            self.selected_position = position;
            if let Some(position) = position {
                self.list.scroll_to_item(position, ScrollStrategy::Center);
            }
        }
    }

    fn open_search(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.searching = open;
        if open {
            let input = self.search.read(cx).input.clone();
            input.focus_handle(cx).focus(window, cx);
        }
        cx.notify();
    }

    fn request_disconnect(&self, context: &str, window: &mut Window, cx: &mut App) {
        let model = self.model.read(cx);
        let work = model
            .status(context)
            .map(|status| status.active_work.clone())
            .unwrap_or_default();
        if work.is_empty() {
            model.disconnect(context);
            return;
        }
        let context = context.to_owned();
        let model = self.model.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let confirm_context = context.clone();
            let confirm_model = model.clone();
            dialog
                .title(t!("rail.disconnect_title", context = context).to_string())
                .child(
                    div()
                        .id("disconnect-warning")
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(t!("rail.disconnect_warning").to_string())
                        .children(work.iter().map(|item| div().child(item.clone())))
                        .test_support(),
                )
                .footer(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            Button::new("disconnect-cancel")
                                .label(t!("common.cancel").to_string())
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("disconnect-confirm")
                                .label(t!("cluster.disconnect").to_string())
                                .on_click(move |_, window, cx| {
                                    confirm_model.read(cx).disconnect(&confirm_context);
                                    window.close_dialog(cx);
                                }),
                        ),
                )
        });
    }

    fn open_settings(&self, key: ClusterKey, window: &mut Window, cx: &mut App) {
        let model = self.model.clone();
        let name = model.read(cx).identity(&key).name;
        let title = t!("rail.cluster_settings", name = name).to_string();
        let form = cx.new(|cx| ClusterSettingsForm::new(key, model, window, cx));
        window.open_dialog(cx, move |dialog, _, _| {
            dialog.title(title.clone()).child(form.clone())
        });
    }

    fn render_icon(&self, index: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let model = self.model.read(cx);
        let status = &model.state().clusters[index];
        let context = status.entry.context.clone();
        let key = model.key(&context);
        let identity = model.identity(&key);
        let state = status.state.clone();
        let selected = self.shell.read(cx).selected() == Some(&key);
        let live = is_live(&state);
        let tooltip = SharedString::from(tooltip_text(&identity, &context, &state));
        let theme = cx.theme();
        let (muted, border, background, accent) = (
            theme.muted_foreground,
            theme.border,
            theme.background,
            theme.foreground,
        );
        let id = |prefix: &str| SharedString::from(format!("{prefix}-{context}"));

        let mark = |id: SharedString, content: AnyElement| {
            div()
                .id(id)
                .absolute()
                .flex()
                .items_center()
                .justify_center()
                .size_3p5()
                .rounded_full()
                .bg(background)
                .child(content)
                .test_support()
        };
        let state_mark = match &state {
            ConnectionState::Connecting | ConnectionState::Authenticating => Some(mark(
                id("rail-attempt"),
                Spinner::new().xsmall().into_any_element(),
            )),
            ConnectionState::Failed { .. } => Some(mark(
                id("rail-failed"),
                Icon::new(IconName::CircleX).xsmall().into_any_element(),
            )),
            ConnectionState::Degraded { .. } => Some(mark(
                id("rail-degraded"),
                Icon::new(IconName::TriangleAlert)
                    .xsmall()
                    .into_any_element(),
            )),
            _ => None,
        }
        .map(|mark| mark.bottom_0().right_0());
        let protected_mark = identity.protected.then(|| {
            mark(
                id("rail-protected"),
                Icon::empty()
                    .path("icons/shield.svg")
                    .xsmall()
                    .into_any_element(),
            )
            .top_0()
            .right_0()
        });

        let rail = cx.weak_entity();
        let menu_context = context.clone();
        let click_context = context.clone();
        div()
            .relative()
            .h(px(ROW_HEIGHT))
            .w_full()
            .flex()
            .items_center()
            .justify_center()
            .when(selected, |this| {
                this.child(
                    div()
                        .id(id("rail-selected"))
                        .absolute()
                        .left_0()
                        .top_2()
                        .bottom_2()
                        .w(px(3.))
                        .bg(accent)
                        .test_support(),
                )
            })
            .child(
                div()
                    .id(id("rail"))
                    .relative()
                    .size(px(ICON_SIZE))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .cursor_pointer()
                    .text_sm()
                    .map(|this| {
                        if live {
                            this.bg(rgb(identity.color)).text_color(white())
                        } else {
                            this.border_2().border_color(border).text_color(muted)
                        }
                    })
                    .child(initials(&identity.name))
                    .children(state_mark)
                    .children(protected_mark)
                    .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                    .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                        choose_cluster(
                            &click_context,
                            event.click_count(),
                            &this.model,
                            &this.shell,
                            cx,
                        );
                    }))
                    .test_support()
                    .context_menu(move |menu, _, _| {
                        let (label, action) = match &state {
                            ConnectionState::Disconnected => {
                                (t!("cluster.connect"), MenuAction::Connect)
                            }
                            ConnectionState::Failed { .. } => {
                                (t!("cluster.retry"), MenuAction::Connect)
                            }
                            ConnectionState::Connecting | ConnectionState::Authenticating => {
                                (t!("common.cancel"), MenuAction::Cancel)
                            }
                            ConnectionState::Connected | ConnectionState::Degraded { .. } => {
                                (t!("cluster.disconnect"), MenuAction::Disconnect)
                            }
                        };
                        let item = |label: String, action: MenuAction| {
                            let rail = rail.clone();
                            let context = menu_context.clone();
                            PopupMenuItem::new(label).on_click(move |_, window, cx| {
                                if let Some(rail) = rail.upgrade() {
                                    rail.update(cx, |rail, cx| {
                                        rail.run(action, &context, window, cx);
                                    });
                                }
                            })
                        };
                        menu.item(item(label.to_string(), action))
                            .item(item(t!("settings.title").to_string(), MenuAction::Settings))
                    }),
            )
    }

    fn run(
        &mut self,
        action: MenuAction,
        context: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            MenuAction::Connect => self.model.read(cx).connect(context),
            // Disconnecting cancels an attempt in progress.
            MenuAction::Cancel => self.model.read(cx).disconnect(context),
            MenuAction::Disconnect => self.request_disconnect(context, window, cx),
            MenuAction::Settings => {
                let key = self.model.read(cx).key(context);
                self.open_settings(key, window, cx);
            }
        }
    }
}

#[derive(Clone, Copy)]
enum MenuAction {
    Connect,
    Cancel,
    Disconnect,
    Settings,
}

impl Render for ClusterRail {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let model = self.model.read(cx);
        let order = rail_order(&model.state().clusters);
        let errors = model.state().errors.clone();
        let search = self.search.clone();
        let rail = cx.weak_entity();

        div()
            .id("cluster-rail")
            .w(px(RAIL_WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap_1()
            .py_2()
            .border_r_1()
            .border_color(cx.theme().border)
            .child(
                Popover::new("rail-search-popover")
                    .open(self.searching)
                    .on_open_change(move |open, window, cx| {
                        let open = *open;
                        if let Some(rail) = rail.upgrade() {
                            rail.update(cx, |rail, cx| rail.open_search(open, window, cx));
                        }
                    })
                    .trigger(
                        Button::new("rail-search")
                            .icon(IconName::Search)
                            .ghost()
                            .tooltip(t!("rail.search").to_string()),
                    )
                    .content(move |_, _, _| search.clone()),
            )
            .child(
                uniform_list(
                    "cluster-list",
                    order.len(),
                    cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                        range
                            .map(|row| this.render_icon(order[row], cx))
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.list)
                .flex_1()
                .w_full(),
            )
            .when(!errors.is_empty(), |this| {
                this.child(
                    Popover::new("kubeconfig-errors-popover")
                        .trigger(
                            Button::new("rail-errors")
                                .icon(IconName::TriangleAlert)
                                .ghost()
                                .tooltip(t!("rail.kubeconfig_errors").to_string()),
                        )
                        .content(move |_, _, _| {
                            div()
                                .id("kubeconfig-errors")
                                .flex()
                                .flex_col()
                                .gap_1()
                                .text_xs()
                                .children(errors.iter().map(|error| {
                                    div().child(format!(
                                        "{}: {}",
                                        error.path.display(),
                                        error.message
                                    ))
                                }))
                                .test_support()
                        }),
                )
            })
            .child(
                Button::new("rail-settings")
                    .icon(IconName::Settings)
                    .ghost()
                    .tooltip(t!("settings.title").to_string())
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenSettings), cx)),
            )
            .test_support()
    }
}

#[cfg(test)]
mod tests {
    use kubepulse_cluster::ClusterEntry;

    use super::{
        ClusterIdentity, ClusterStatus, ConnectionState, initials, matching, rail_order,
        tooltip_text,
    };

    fn status(context: &str, state: ConnectionState) -> ClusterStatus {
        ClusterStatus {
            entry: ClusterEntry {
                context: context.to_owned(),
                cluster: None,
                server: None,
                user: None,
                namespace: None,
            },
            state,
            active_work: Vec::new(),
            resources: Default::default(),
        }
    }

    #[test]
    fn search_ignores_case_and_keeps_order() {
        let names = ["prod-eu", "dev", "PROD-us"].map(str::to_owned);
        assert_eq!(matching(&names, "prod"), [0, 2]);
        assert_eq!(matching(&names, ""), [0, 1, 2]);
        assert!(matching(&names, "none").is_empty());
    }

    #[test]
    fn live_clusters_come_first_and_each_set_keeps_its_order() {
        let clusters = [
            status("a", ConnectionState::Disconnected),
            status("b", ConnectionState::Connected),
            status("c", ConnectionState::Connecting),
            status("d", ConnectionState::Degraded { reason: "r".into() }),
            status("e", ConnectionState::Failed { reason: "r".into() }),
        ];
        assert_eq!(rail_order(&clusters), [1, 3, 0, 2, 4]);
    }

    #[test]
    fn initials_tell_similar_names_apart() {
        assert_eq!(initials("prod-eu"), "PE");
        assert_eq!(initials("prod-us"), "PU");
        assert_eq!(initials("minikube"), "MI");
        assert_eq!(initials("arn:aws:eks"), "AA");
        assert_eq!(initials("---"), "");
    }

    #[test]
    fn the_tooltip_names_the_cluster_and_gives_the_reason() {
        let identity = ClusterIdentity {
            name: "Production".into(),
            color: 0,
            protected: false,
        };
        let failed = ConnectionState::Failed {
            reason: "timed out".into(),
        };
        assert_eq!(
            tooltip_text(&identity, "prod", &failed),
            "Production\nprod\nFailed: timed out"
        );
    }
}
