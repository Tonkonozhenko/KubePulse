//! The form inside the cluster settings dialog.

use gpui_kit::{
    component::{
        button::Button,
        input::{Input, InputEvent, InputState},
    },
    *,
};

use rust_i18n::t;

use crate::{
    cluster_model::ClusterModel,
    identity::{ClusterKey, PALETTE},
};

pub struct ClusterSettingsForm {
    key: ClusterKey,
    model: Entity<ClusterModel>,
    name: Entity<InputState>,
    _subscription: Subscription,
}

impl ClusterSettingsForm {
    pub fn new(
        key: ClusterKey,
        model: Entity<ClusterModel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let current = model.read(cx).settings(&key).name.unwrap_or_default();
        let name = cx.new(|cx| {
            let mut input =
                InputState::new(window, cx).placeholder(t!("cluster.display_name").to_string());
            input.set_value(current, window, cx);
            input
        });
        let subscription = cx.subscribe(&name, |this, name, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                let text = name.read(cx).value().trim().to_owned();
                let key = this.key.clone();
                this.model.update(cx, |model, cx| {
                    let value = (!text.is_empty()).then_some(text);
                    model.update_identity(key, |settings| settings.name = value, cx);
                });
            }
        });
        Self {
            key,
            model,
            name,
            _subscription: subscription,
        }
    }
}

impl Render for ClusterSettingsForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let protected = self.model.read(cx).settings(&self.key).protected;
        let swatches = PALETTE.iter().map(|&color| {
            div()
                .id(SharedString::from(format!("color-{color:06x}")))
                .size_4()
                .rounded_full()
                .cursor_pointer()
                .bg(rgb(color))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let key = this.key.clone();
                    this.model.update(cx, |model, cx| {
                        model.update_identity(key, |settings| settings.color = Some(color), cx);
                    });
                }))
                .test_support()
        });
        div()
            .id("cluster-settings")
            .flex()
            .flex_col()
            .gap_2()
            .child(Input::new(&self.name))
            .child(div().flex().gap_1().children(swatches))
            .child(
                Button::new("toggle-protected")
                    .label(
                        if protected {
                            t!("cluster.remove_protection")
                        } else {
                            t!("cluster.protect")
                        }
                        .to_string(),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let key = this.key.clone();
                        this.model.update(cx, |model, cx| {
                            model.update_identity(key, |s| s.protected = !protected, cx);
                        });
                        cx.notify();
                    })),
            )
            .test_support()
    }
}
