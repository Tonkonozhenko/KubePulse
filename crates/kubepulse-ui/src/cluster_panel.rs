//! The searchable cluster list. Every context is listed with its connection
//! state, and connecting and disconnecting happen only from explicit actions.

use gpui_kit::{
    component::{
        ActiveTheme as _,
        button::Button,
        input::{Input, InputEvent, InputState},
    },
    *,
};
use kubepulse_cluster::ConnectionState;

use crate::{
    cluster_model::{ClusterModel, state_label},
    identity::{ClusterKey, PALETTE},
};

const ROW_HEIGHT: f32 = 36.;

pub enum ClusterPanelEvent {
    /// The user opened a cluster. This never connects it.
    Open(ClusterKey),
}

impl EventEmitter<ClusterPanelEvent> for ClusterPanel {}

pub struct ClusterPanel {
    model: Entity<ClusterModel>,
    search: Entity<InputState>,
    name: Entity<InputState>,
    query: String,
    /// The cluster waiting on the active-work warning.
    confirming_disconnect: Option<String>,
    editing: Option<ClusterKey>,
    _subscriptions: Vec<Subscription>,
}

pub fn matching(names: &[String], query: &str) -> Vec<usize> {
    let query = query.to_lowercase();
    names
        .iter()
        .enumerate()
        .filter(|(_, name)| name.to_lowercase().contains(&query))
        .map(|(index, _)| index)
        .collect()
}

impl ClusterPanel {
    pub fn new(model: Entity<ClusterModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search clusters"));
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("Display name"));
        let subscriptions = vec![
            cx.observe(&model, |_, _, cx| cx.notify()),
            cx.subscribe(&search, |this, search, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.query = search.read(cx).value().to_string();
                    cx.notify();
                }
            }),
            cx.subscribe(&name, |this, name, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change)
                    && let Some(key) = this.editing.clone()
                {
                    let text = name.read(cx).value().trim().to_owned();
                    this.model.update(cx, |model, cx| {
                        let value = (!text.is_empty()).then_some(text);
                        model.update_identity(key, |settings| settings.name = value, cx);
                    });
                }
            }),
        ];
        Self {
            model,
            search,
            name,
            query: String::new(),
            confirming_disconnect: None,
            editing: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn set_query(&mut self, query: &str, cx: &mut Context<Self>) {
        self.query = query.to_owned();
        cx.notify();
    }

    fn request_disconnect(&mut self, context: String, cx: &mut Context<Self>) {
        let model = self.model.read(cx);
        let busy = model
            .status(&context)
            .is_some_and(|status| !status.active_work.is_empty());
        if busy {
            self.confirming_disconnect = Some(context);
            cx.notify();
        } else {
            model.disconnect(&context);
        }
    }

    fn edit(&mut self, key: ClusterKey, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.model.read(cx).settings(&key).name.unwrap_or_default();
        self.name
            .update(cx, |input, cx| input.set_value(name, window, cx));
        self.editing = if self.editing.as_ref() == Some(&key) {
            None
        } else {
            Some(key)
        };
        cx.notify();
    }

    fn render_row(&self, index: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let model = self.model.read(cx);
        let status = &model.state().clusters[index];
        let context = status.entry.context.clone();
        let key = model.key(&context);
        let identity = model.identity(&key);
        let label = state_label(&status.state);
        let connected = !matches!(
            status.state,
            ConnectionState::Disconnected | ConnectionState::Failed { .. }
        );
        let muted = cx.theme().muted_foreground;

        let (open_key, edit_key, action_context) = (key.clone(), key, context.clone());
        let action = if connected {
            Button::new(format!("disconnect-{context}"))
                .label("Disconnect")
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.request_disconnect(action_context.clone(), cx)
                }))
        } else {
            Button::new(format!("connect-{context}"))
                .label("Connect")
                .on_click(
                    cx.listener(move |this, _, _, cx| this.model.read(cx).connect(&action_context)),
                )
        };

        div()
            .id(SharedString::from(format!("cluster-{context}")))
            .h(px(ROW_HEIGHT))
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .child(
                div()
                    .size_2()
                    .flex_none()
                    .rounded_full()
                    .bg(rgb(identity.color)),
            )
            .child(
                div()
                    .id(SharedString::from(format!("open-{context}")))
                    .flex_1()
                    .overflow_hidden()
                    .cursor_pointer()
                    .child(identity.name)
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(ClusterPanelEvent::Open(open_key.clone()));
                    }))
                    .test_support(),
            )
            .child(
                div()
                    .id(SharedString::from(format!("state-{context}")))
                    .flex_none()
                    .text_xs()
                    .text_color(muted)
                    .child(label),
            )
            .child(action)
            .child(Button::new(format!("edit-{context}")).label("…").on_click(
                cx.listener(move |this, _, window, cx| this.edit(edit_key.clone(), window, cx)),
            ))
    }

    fn render_warning(&self, context: &str, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let model = self.model.read(cx);
        let work = model
            .status(context)
            .map(|status| status.active_work.clone());
        let confirm = context.to_owned();
        div()
            .id("disconnect-warning")
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .child(format!("Disconnecting {context} stops:"))
            .children(
                work.unwrap_or_default()
                    .into_iter()
                    .map(|item| div().child(item)),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Button::new("disconnect-confirm")
                            .label("Disconnect")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.model.read(cx).disconnect(&confirm);
                                this.confirming_disconnect = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("disconnect-cancel")
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.confirming_disconnect = None;
                                cx.notify();
                            })),
                    ),
            )
            .test_support()
    }

    fn render_settings(
        &self,
        key: &ClusterKey,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let settings = self.model.read(cx).settings(key);
        let swatches = PALETTE.iter().map(|&color| {
            let key = key.clone();
            div()
                .id(SharedString::from(format!("color-{color:06x}")))
                .size_4()
                .rounded_full()
                .cursor_pointer()
                .bg(rgb(color))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let key = key.clone();
                    this.model.update(cx, |model, cx| {
                        model.update_identity(key, |settings| settings.color = Some(color), cx);
                    });
                }))
                .test_support()
        });
        let protect_key = key.clone();
        let protected = settings.protected;
        div()
            .id("cluster-settings")
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .child(Input::new(&self.name))
            .child(div().flex().gap_1().children(swatches))
            .child(
                Button::new("toggle-protected")
                    .label(if protected {
                        "Remove protection"
                    } else {
                        "Protect"
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let key = protect_key.clone();
                        this.model.update(cx, |model, cx| {
                            model.update_identity(key, |s| s.protected = !protected, cx);
                        });
                    })),
            )
    }
}

impl Render for ClusterPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let names: Vec<String> = {
            let model = self.model.read(cx);
            model
                .state()
                .clusters
                .iter()
                .map(|s| s.entry.context.clone())
                .collect()
        };
        let shown = matching(&names, &self.query);
        let errors = self.model.read(cx).state().errors.clone();
        let warning = self.confirming_disconnect.clone();
        let editing = self.editing.clone();
        let count = shown.len();

        div()
            .id("cluster-panel")
            .h_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .border_r_1()
            .border_color(cx.theme().border)
            .child(Input::new(&self.search))
            .children(errors.into_iter().map(|error| {
                div()
                    .text_xs()
                    .child(format!("{}: {}", error.path.display(), error.message))
            }))
            .child(
                uniform_list(
                    "cluster-list",
                    count,
                    cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                        range
                            .map(|row| this.render_row(shown[row], cx))
                            .collect::<Vec<_>>()
                    }),
                )
                .flex_1()
                .w_full(),
            )
            .children(warning.map(|context| self.render_warning(&context, cx)))
            .children(editing.map(|key| self.render_settings(&key, cx)))
    }
}

#[cfg(test)]
mod tests {
    use super::matching;

    #[test]
    fn search_ignores_case_and_keeps_order() {
        let names = ["prod-eu", "dev", "PROD-us"].map(str::to_owned);
        assert_eq!(matching(&names, "prod"), [0, 2]);
        assert_eq!(matching(&names, ""), [0, 1, 2]);
        assert!(matching(&names, "none").is_empty());
    }
}
