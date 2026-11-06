//! Asks before deleting. It names the cluster and every object, and on a
//! protected cluster it asks for a typed name. A failed object stays in the list
//! with its error, so confirming again retries only what failed.

use std::{cell::Cell, ops::Range, rc::Rc, sync::Arc};

use futures::{StreamExt as _, stream};
use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme as _, WindowExt as _,
        button::Button,
        dialog::Dialog,
        input::{Input, InputEvent, InputState},
        tooltip::Tooltip,
    },
    prelude::FluentBuilder as _,
    *,
};
use kubepulse_cluster::{ObjectError, ObjectTarget};
use rust_i18n::t;

use crate::{cluster_model::ClusterModel, identity::ClusterKey, services::ClusterServices};

/// Enough to finish quickly without flooding the API server.
const MAX_CONCURRENT_DELETES: usize = 8;
const MAX_LISTED_ROWS: usize = 8;
const ROW_HEIGHT: f32 = 24.;
const ROW_WITH_ERROR_HEIGHT: f32 = 40.;

type OnDeleted = Rc<dyn Fn(Vec<ObjectTarget>, &mut Window, &mut App)>;

struct Item {
    target: ObjectTarget,
    error: Option<String>,
}

impl Item {
    fn label(&self) -> String {
        match &self.target.namespace {
            Some(namespace) => format!("{namespace}/{}", self.target.name),
            None => self.target.name.clone(),
        }
    }
}

pub struct DeleteDialog {
    key: ClusterKey,
    model: Entity<ClusterModel>,
    services: Arc<dyn ClusterServices>,
    items: Vec<Item>,
    /// Fixed when opened, so that a retry of one object still asks for the same
    /// name.
    several: bool,
    running: bool,
    cancelled: bool,
    done: usize,
    total: usize,
    typed_name: Entity<InputState>,
    name_matches: bool,
    on_deleted: OnDeleted,
    list: UniformListScrollHandle,
    _subscription: Subscription,
}

impl DeleteDialog {
    /// Opens the dialog. `on_deleted` hears about every object that went, even
    /// if others failed or the user cancelled.
    pub fn open(
        key: ClusterKey,
        targets: Vec<ObjectTarget>,
        model: Entity<ClusterModel>,
        services: Arc<dyn ClusterServices>,
        on_deleted: impl Fn(Vec<ObjectTarget>, &mut Window, &mut App) + 'static,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let protected = model.read(cx).identity(&key).protected;
        let view = cx.new(|cx: &mut Context<Self>| {
            let several = targets.len() > 1;
            let placeholder = if several {
                t!("delete.type_cluster")
            } else {
                t!("detail.type_name")
            };
            let typed_name =
                cx.new(|cx| InputState::new(window, cx).placeholder(placeholder.to_string()));
            let subscription = cx.subscribe(&typed_name, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let text = input.read(cx).value().to_string();
                    this.type_name(&text, cx);
                }
            });
            Self {
                key,
                model,
                services,
                items: targets
                    .into_iter()
                    .map(|target| Item {
                        target,
                        error: None,
                    })
                    .collect(),
                several,
                running: false,
                cancelled: false,
                done: 0,
                total: 0,
                typed_name,
                name_matches: false,
                on_deleted: Rc::new(on_deleted),
                list: UniformListScrollHandle::new(),
                _subscription: subscription,
            }
        });
        let focus_input = Cell::new(protected);
        let shown = view.clone();
        window.open_dialog(cx, move |dialog, window, cx| {
            if focus_input.replace(false) {
                let input = shown.read(cx).typed_name.clone();
                input.focus_handle(cx).focus(window, cx);
            }
            shown.update(cx, |view, cx| view.dialog(dialog, cx))
        });
        view
    }

    pub fn type_name(&mut self, text: &str, cx: &mut Context<Self>) {
        self.name_matches = text == self.expected_name(cx);
        cx.notify();
    }

    fn expected_name(&self, cx: &App) -> String {
        match &*self.items {
            [item] if !self.several => item.target.name.clone(),
            _ => self.model.read(cx).identity(&self.key).name,
        }
    }

    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.running = true;
        self.done = 0;
        self.total = self.items.len();
        for item in &mut self.items {
            item.error = None;
        }
        cx.notify();

        let services = self.services.clone();
        let context = self.key.context.clone();
        let targets: Vec<_> = self.items.iter().map(|item| item.target.clone()).collect();
        let mut results = stream::iter(targets)
            .map(move |target| {
                let reply = services.delete_object(&context, target.clone());
                async move { (target, reply.await) }
            })
            .buffer_unordered(MAX_CONCURRENT_DELETES);
        // The dialog holds the only other handle, and closing it must not lose
        // what was already deleted.
        let alive = cx.entity();
        cx.spawn_in(window, async move |this, cx| {
            let mut deleted = Vec::new();
            let mut failed = Vec::new();
            while let Some((target, result)) = results.next().await {
                match result {
                    Ok(()) | Err(ObjectError::NotFound(_)) => deleted.push(target),
                    Err(error) => failed.push(Item {
                        target,
                        error: Some(t!("detail.delete_failed", error = error).to_string()),
                    }),
                }
                let cancelled = this.update(cx, |this, cx| {
                    this.done += 1;
                    cx.notify();
                    this.cancelled
                });
                if cancelled.unwrap_or(true) {
                    break;
                }
            }
            let _ = this.update_in(cx, |this, window, cx| {
                this.finish(deleted, failed, window, cx);
            });
            drop(alive);
        })
        .detach();
    }

    fn finish(
        &mut self,
        deleted: Vec<ObjectTarget>,
        failed: Vec<Item>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.running = false;
        let close = !self.cancelled && failed.is_empty();
        if !self.cancelled {
            self.items = failed;
        }
        cx.notify();
        if close {
            window.close_dialog(cx);
        }
        if !deleted.is_empty() {
            (self.on_deleted)(deleted, window, cx);
        }
    }

    fn cancel(&mut self) {
        self.cancelled = true;
    }

    fn row_height(&self) -> Pixels {
        let has_errors = self.items.iter().any(|item| item.error.is_some());
        px(if has_errors {
            ROW_WITH_ERROR_HEIGHT
        } else {
            ROW_HEIGHT
        })
    }

    fn render_rows(&self, range: Range<usize>, cx: &App) -> Vec<AnyElement> {
        let danger = cx.theme().danger;
        let height = self.row_height();
        range
            .filter_map(|index| Some((index, self.items.get(index)?)))
            .map(|(index, item)| {
                let error = item.error.clone();
                div()
                    .id(("delete-item", index))
                    .h(height)
                    .flex()
                    .flex_col()
                    .justify_center()
                    .child(div().truncate().child(item.label()))
                    .when_some(error, |this, error| {
                        let tooltip = SharedString::from(error.clone());
                        this.child(div().truncate().text_xs().text_color(danger).child(error))
                            .tooltip(move |window, cx| {
                                Tooltip::new(tooltip.clone()).build(window, cx)
                            })
                    })
                    .test_support()
                    .into_any_element()
            })
            .collect()
    }

    fn render_list(&self, cx: &Context<Self>) -> impl IntoElement + use<> {
        let view = cx.entity();
        let height = self.row_height() * self.items.len().min(MAX_LISTED_ROWS) as f32;
        div().id("delete-list").h(height).test_support().child(
            uniform_list(
                "delete-items",
                self.items.len(),
                move |range, _, cx: &mut App| view.read(cx).render_rows(range, cx),
            )
            .size_full()
            .track_scroll(&self.list),
        )
    }

    fn title(&self) -> String {
        match &*self.items {
            [item] if !self.several => t!(
                "detail.delete_title",
                kind = item.target.kind.kind,
                name = item.target.name
            ),
            items => t!(
                "delete.title_several",
                kind = items.first().map_or("", |item| &item.target.kind.kind),
                count = items.len()
            ),
        }
        .to_string()
    }

    fn dialog(&self, dialog: Dialog, cx: &mut Context<Self>) -> Dialog {
        let identity = self.model.read(cx).identity(&self.key);
        let needs_name = identity.protected;
        let single = (!self.several).then(|| self.items.first()).flatten();
        let view = cx.weak_entity();
        let confirm = ((!needs_name || self.name_matches) && !self.running).then(|| {
            let view = view.clone();
            Button::new("delete-confirm")
                .icon(IconName::Trash)
                .label(t!("detail.delete").to_string())
                .on_click(move |_, window, cx| {
                    let _ = view.update(cx, |this, cx| this.delete(window, cx));
                })
        });
        let cancel_view = view.clone();
        dialog
            .title(self.title())
            .overlay_closable(false)
            // Enter never deletes, so deleting always takes a deliberate click.
            .on_ok(|_, _, _| false)
            .on_close(move |_, _, cx| {
                let _ = view.update(cx, |this, _| this.cancel());
            })
            .child(
                div()
                    .id("delete-confirmation")
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_color(rgb(identity.color))
                            .child(t!("detail.cluster", name = identity.name).to_string()),
                    )
                    .when_some(single, |this, item| {
                        let namespace = item
                            .target
                            .namespace
                            .clone()
                            .unwrap_or_else(|| t!("detail.cluster_wide").to_string());
                        this.child(t!("detail.namespace_line", namespace = namespace).to_string())
                            .children(
                                item.error.clone().map(|error| {
                                    div().id("delete-error").child(error).test_support()
                                }),
                            )
                    })
                    .when(self.several, |this| this.child(self.render_list(cx)))
                    .children(needs_name.then(|| t!("detail.protected").to_string()))
                    .children(needs_name.then(|| Input::new(&self.typed_name)))
                    .when(self.running, |this| {
                        this.child(
                            div()
                                .id("delete-progress")
                                .text_xs()
                                .child(
                                    t!("delete.progress", done = self.done, total = self.total)
                                        .to_string(),
                                )
                                .test_support(),
                        )
                    })
                    .test_support(),
            )
            .footer(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("delete-cancel")
                            .label(t!("common.cancel").to_string())
                            .on_click(move |_, window, cx| {
                                let _ = cancel_view.update(cx, |this, _| this.cancel());
                                window.close_dialog(cx);
                            }),
                    )
                    .children(confirm),
            )
    }
}
