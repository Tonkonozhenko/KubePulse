//! Asks before deleting, restarting, or running objects. It names the cluster
//! and every object, and on a protected cluster it asks for a typed name. A
//! failed object stays in the list with its error, so confirming again retries
//! only what failed.

use std::{cell::Cell, ops::Range, rc::Rc, sync::Arc};

use futures::{StreamExt as _, stream};
use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme as _, WindowExt as _,
        button::Button,
        dialog::Dialog,
        input::{Input, InputEvent, InputState},
        notification::Notification,
        tooltip::Tooltip,
    },
    prelude::FluentBuilder as _,
    *,
};
use kubepulse_cluster::{ObjectError, ObjectTarget};
use rust_i18n::t;

use crate::{cluster_model::ClusterModel, identity::ClusterKey, services::ClusterServices};

/// Enough to finish quickly without flooding the API server.
const MAX_CONCURRENT_REQUESTS: usize = 8;
const MAX_LISTED_ROWS: usize = 8;
const ROW_HEIGHT: f32 = 24.;
const ROW_WITH_ERROR_HEIGHT: f32 = 40.;

type OnDone = Rc<dyn Fn(Vec<ObjectTarget>, &mut Window, &mut App)>;

/// What the dialog does to its objects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Delete,
    /// Rolls out new pods for a workload.
    Restart,
    /// Creates a Job from a CronJob.
    Run,
}

impl Operation {
    pub fn icon(self) -> IconName {
        match self {
            Self::Delete => IconName::Trash,
            Self::Restart => IconName::RotateCw,
            Self::Run => IconName::Play,
        }
    }

    pub fn label(self) -> String {
        match self {
            Self::Delete => t!("detail.delete"),
            Self::Restart => t!("restart.action"),
            Self::Run => t!("run.action"),
        }
        .to_string()
    }

    fn title(self, kind: &str, name: &str) -> String {
        match self {
            Self::Delete => t!("detail.delete_title", kind = kind, name = name),
            Self::Restart => t!("restart.title", kind = kind, name = name),
            Self::Run => t!("run.title", kind = kind, name = name),
        }
        .to_string()
    }

    fn title_several(self, kind: &str, count: usize) -> String {
        match self {
            Self::Delete => t!("delete.title_several", kind = kind, count = count),
            Self::Restart => t!("restart.title_several", kind = kind, count = count),
            Self::Run => t!("run.title_several", kind = kind, count = count),
        }
        .to_string()
    }

    fn progress(self, done: usize, total: usize) -> String {
        match self {
            Self::Delete => t!("delete.progress", done = done, total = total),
            Self::Restart => t!("restart.progress", done = done, total = total),
            Self::Run => t!("run.progress", done = done, total = total),
        }
        .to_string()
    }

    fn failed(self, error: &ObjectError) -> String {
        match self {
            Self::Delete => t!("detail.delete_failed", error = error),
            Self::Restart => t!("restart.failed", error = error),
            Self::Run => t!("run.failed", error = error),
        }
        .to_string()
    }

    /// Runs the operation on one object. A run answers with the created Job's
    /// name.
    async fn apply(
        self,
        services: Arc<dyn ClusterServices>,
        context: String,
        target: ObjectTarget,
    ) -> Result<Option<String>, ObjectError> {
        match self {
            Self::Delete => match services.delete_object(&context, target).await {
                Err(ObjectError::NotFound(_)) => Ok(None),
                result => result.map(|()| None),
            },
            Self::Restart => services
                .restart_object(&context, target)
                .await
                .map(|()| None),
            Self::Run => services.run_cronjob(&context, target).await.map(Some),
        }
    }
}

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

pub struct ObjectDialog {
    operation: Operation,
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
    /// Jobs created by runs and not yet announced.
    created: Vec<String>,
    typed_name: Entity<InputState>,
    name_matches: bool,
    on_done: OnDone,
    list: UniformListScrollHandle,
    _subscription: Subscription,
}

impl ObjectDialog {
    /// Opens the dialog. `on_done` hears about every object the operation
    /// succeeded on, even if others failed or the user cancelled.
    #[expect(clippy::too_many_arguments, reason = "each is a separate input")]
    pub fn open(
        operation: Operation,
        key: ClusterKey,
        targets: Vec<ObjectTarget>,
        model: Entity<ClusterModel>,
        services: Arc<dyn ClusterServices>,
        on_done: impl Fn(Vec<ObjectTarget>, &mut Window, &mut App) + 'static,
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
                operation,
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
                created: Vec::new(),
                typed_name,
                name_matches: false,
                on_done: Rc::new(on_done),
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

    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.running = true;
        self.done = 0;
        self.total = self.items.len();
        for item in &mut self.items {
            item.error = None;
        }
        cx.notify();

        let operation = self.operation;
        let services = self.services.clone();
        let context = self.key.context.clone();
        let targets: Vec<_> = self.items.iter().map(|item| item.target.clone()).collect();
        let mut results = stream::iter(targets)
            .map(move |target| {
                let reply = operation.apply(services.clone(), context.clone(), target.clone());
                async move { (target, reply.await) }
            })
            .buffer_unordered(MAX_CONCURRENT_REQUESTS);
        // The dialog holds the only other handle, and closing it must not lose
        // what was already done.
        let alive = cx.entity();
        cx.spawn_in(window, async move |this, cx| {
            let mut succeeded = Vec::new();
            let mut failed = Vec::new();
            while let Some((target, result)) = results.next().await {
                let cancelled = this.update(cx, |this, cx| {
                    match result {
                        Ok(created) => {
                            this.created.extend(created);
                            succeeded.push(target);
                        }
                        Err(error) => failed.push(Item {
                            target,
                            error: Some(this.operation.failed(&error)),
                        }),
                    }
                    this.done += 1;
                    cx.notify();
                    this.cancelled
                });
                if cancelled.unwrap_or(true) {
                    break;
                }
            }
            let _ = this.update_in(cx, |this, window, cx| {
                this.finish(succeeded, failed, window, cx);
            });
            drop(alive);
        })
        .detach();
    }

    fn finish(
        &mut self,
        succeeded: Vec<ObjectTarget>,
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
        if close || self.cancelled {
            self.announce(window, cx);
        }
        if !succeeded.is_empty() {
            (self.on_done)(succeeded, window, cx);
        }
    }

    /// Names the Jobs created so far, once.
    fn announce(&mut self, window: &mut Window, cx: &mut App) {
        if self.created.is_empty() {
            return;
        }
        let names = std::mem::take(&mut self.created).join(", ");
        let message = t!("run.created", names = names).to_string();
        window.push_notification(Notification::success(message), cx);
    }

    /// Stops starting new requests. When nothing is running, the Jobs already
    /// created are announced now, because no finish will follow.
    fn cancel(&mut self, window: &mut Window, cx: &mut App) {
        self.cancelled = true;
        if !self.running {
            self.announce(window, cx);
        }
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
                    .id(("object-item", index))
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
        div().id("object-list").h(height).test_support().child(
            uniform_list(
                "object-items",
                self.items.len(),
                move |range, _, cx: &mut App| view.read(cx).render_rows(range, cx),
            )
            .size_full()
            .track_scroll(&self.list),
        )
    }

    fn title(&self) -> String {
        match &*self.items {
            [item] if !self.several => self
                .operation
                .title(&item.target.kind.kind, &item.target.name),
            items => self.operation.title_several(
                items.first().map_or("", |item| &item.target.kind.kind),
                items.len(),
            ),
        }
    }

    fn dialog(&self, dialog: Dialog, cx: &mut Context<Self>) -> Dialog {
        let identity = self.model.read(cx).identity(&self.key);
        let needs_name = identity.protected;
        let single = (!self.several).then(|| self.items.first()).flatten();
        let view = cx.weak_entity();
        let confirm = ((!needs_name || self.name_matches) && !self.running).then(|| {
            let view = view.clone();
            Button::new("object-confirm")
                .icon(self.operation.icon())
                .label(self.operation.label())
                .on_click(move |_, window, cx| {
                    let _ = view.update(cx, |this, cx| this.start(window, cx));
                })
        });
        let cancel_view = view.clone();
        dialog
            .title(self.title())
            .overlay_closable(false)
            // Enter never confirms, so every operation takes a deliberate click.
            .on_ok(|_, _, _| false)
            .on_close(move |_, window, cx| {
                let _ = view.update(cx, |this, cx| this.cancel(window, cx));
            })
            .child(
                div()
                    .id("object-confirmation")
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
                                    div().id("object-error").child(error).test_support()
                                }),
                            )
                    })
                    .when(self.several, |this| this.child(self.render_list(cx)))
                    .children(needs_name.then(|| t!("detail.protected").to_string()))
                    .children(needs_name.then(|| Input::new(&self.typed_name)))
                    .when(self.running, |this| {
                        this.child(
                            div()
                                .id("object-progress")
                                .text_xs()
                                .child(self.operation.progress(self.done, self.total))
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
                        Button::new("object-cancel")
                            .label(t!("common.cancel").to_string())
                            .on_click(move |_, window, cx| {
                                let _ = cancel_view.update(cx, |this, cx| this.cancel(window, cx));
                                window.close_dialog(cx);
                            }),
                    )
                    .children(confirm),
            )
    }
}
