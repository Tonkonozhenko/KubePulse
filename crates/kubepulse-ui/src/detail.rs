//! One object: its metadata, status, events, and owners. Secret values stay
//! hidden until revealed, and deleting always names the target first.

use std::{cell::Cell, collections::HashSet, sync::Arc};

use base64::Engine as _;
use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme as _, WindowExt as _,
        button::Button,
        description_list::DescriptionList,
        dialog::Dialog,
        input::{Input, InputEvent, InputState},
    },
    prelude::FluentBuilder as _,
    *,
};
use kubepulse_cluster::{EventRecord, ObjectTarget, ResourceKind};
use kubepulse_extension_api::{
    ClusterRef, DetailTabContribution, ExtensionRequest, ExtensionResponse, ObjectRef, UiNode,
};
use serde_json::Value;

use rust_i18n::t;

use crate::{
    cluster_model::ClusterModel,
    columns::kind_ref,
    declarative::{DeclarativeView, FormSubmitted},
    identity::ClusterKey,
    services::ClusterServices,
};

pub enum DetailEvent {
    OpenOwner(ObjectTarget),
    Edit(ObjectTarget),
    Logs(ObjectTarget),
    Shell(ObjectTarget),
    Deleted,
}

impl EventEmitter<DetailEvent> for DetailView {}

const HIDDEN_VALUE: &str = "••••••";

enum Load {
    Loading,
    Loaded(Value),
    Failed(String),
}

struct Owner {
    kind: String,
    name: String,
    target: Option<ObjectTarget>,
}

fn is_pod(target: &ObjectTarget) -> bool {
    target.kind.group.is_empty() && target.kind.kind == "Pod"
}

fn decode(value: &str) -> String {
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_else(|| t!("detail.binary_value").to_string())
}

fn owner_target(
    owner: &Value,
    namespace: Option<&str>,
    resources: &[ResourceKind],
) -> Option<ObjectTarget> {
    let api_version = owner["apiVersion"].as_str()?;
    let group = api_version.rsplit_once('/').map_or("", |(group, _)| group);
    let kind = owner["kind"].as_str()?;
    let served = resources
        .iter()
        .find(|served| served.kind == kind && served.group == group)?;
    Some(ObjectTarget {
        kind: served.clone(),
        namespace: served
            .namespaced
            .then(|| namespace.map(str::to_owned))
            .flatten(),
        name: owner["name"].as_str()?.to_owned(),
    })
}

pub struct DetailView {
    context: String,
    key: ClusterKey,
    target: ObjectTarget,
    model: Entity<ClusterModel>,
    services: Arc<dyn ClusterServices>,
    object: Load,
    events: Vec<EventRecord>,
    events_error: Option<String>,
    revealed: HashSet<String>,
    deleting: bool,
    typed_name: Entity<InputState>,
    name_matches: bool,
    delete_error: Option<String>,
    tabs: Vec<(String, DetailTabContribution)>,
    extension_tab: Option<Entity<DeclarativeView>>,
    extension_error: Option<String>,
    extension_form: Option<Subscription>,
    _subscription: Subscription,
}

impl DetailView {
    pub fn new(
        key: ClusterKey,
        target: ObjectTarget,
        model: Entity<ClusterModel>,
        services: Arc<dyn ClusterServices>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let wanted = kind_ref(&target.kind);
        let tabs = services
            .contributions()
            .extensions
            .iter()
            .flat_map(|(extension, contributed)| {
                contributed
                    .detail_tabs
                    .iter()
                    .map(|tab| (extension.clone(), tab.clone()))
            })
            .filter(|(_, tab)| tab.kind == wanted)
            .collect();

        let typed_name = cx
            .new(|cx| InputState::new(window, cx).placeholder(t!("detail.type_name").to_string()));
        let subscription = cx.subscribe(&typed_name, |this, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                let text = input.read(cx).value().to_string();
                this.type_name(&text, cx);
            }
        });

        let context = key.context.clone();
        let reply = services.get_object(&context, target.clone());
        let event_services = services.clone();
        let event_context = context.clone();
        let namespace = target.namespace.clone();
        cx.spawn(async move |this, cx| {
            let result = reply.await;
            let uid = result
                .as_ref()
                .ok()
                .and_then(|object| object["metadata"]["uid"].as_str())
                .map(str::to_owned);
            let _ = this.update(cx, |this, cx| {
                this.object = match result {
                    Ok(object) => Load::Loaded(object),
                    Err(error) => Load::Failed(t!("detail.load_failed", error = error).to_string()),
                };
                cx.notify();
            });
            if let Some(uid) = uid {
                let events = event_services.events(&event_context, namespace, uid).await;
                let _ = this.update(cx, |this, cx| {
                    match events {
                        Ok(events) => this.events = events,
                        Err(error) => this.events_error = Some(error.to_string()),
                    }
                    cx.notify();
                });
            }
        })
        .detach();

        Self {
            context,
            key,
            target,
            model,
            services,
            object: Load::Loading,
            events: Vec::new(),
            events_error: None,
            revealed: HashSet::new(),
            deleting: false,
            typed_name,
            name_matches: false,
            delete_error: None,
            tabs,
            extension_tab: None,
            extension_error: None,
            extension_form: None,
            _subscription: subscription,
        }
    }

    pub fn target(&self) -> &ObjectTarget {
        &self.target
    }

    pub fn object(&self) -> Option<&Value> {
        match &self.object {
            Load::Loaded(object) => Some(object),
            _ => None,
        }
    }

    pub fn events(&self) -> &[EventRecord] {
        &self.events
    }

    pub fn extension_tab(&self) -> Option<&Entity<DeclarativeView>> {
        self.extension_tab.as_ref()
    }

    pub fn extension_error(&self) -> Option<&str> {
        self.extension_error.as_deref()
    }

    /// What the view shows for a Secret key: its mask until revealed.
    pub fn secret_text(&self, key: &str) -> Option<String> {
        let value = self.object()?["data"].get(key)?;
        Some(if self.revealed.contains(key) {
            decode(value.as_str().unwrap_or_default())
        } else {
            HIDDEN_VALUE.to_owned()
        })
    }

    pub fn type_name(&mut self, text: &str, cx: &mut Context<Self>) {
        self.name_matches = text == self.target.name;
        cx.notify();
    }

    fn is_secret(&self) -> bool {
        self.target.kind.kind == "Secret" && self.target.kind.group.is_empty()
    }

    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.deleting = true;
        self.delete_error = None;
        cx.notify();
        let reply = self
            .services
            .delete_object(&self.context, self.target.clone());
        cx.spawn_in(window, async move |this, cx| {
            let result = reply.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.deleting = false;
                match result {
                    Ok(()) => {
                        window.close_dialog(cx);
                        cx.emit(DetailEvent::Deleted);
                    }
                    Err(error) => {
                        this.delete_error =
                            Some(t!("detail.delete_failed", error = error).to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn open_tab(
        &mut self,
        extension: String,
        tab: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = ObjectRef {
            cluster: ClusterRef(self.context.clone()),
            kind: kind_ref(&self.target.kind),
            namespace: self.target.namespace.clone(),
            name: self.target.name.clone(),
        };
        let request = ExtensionRequest::RenderDetailTab { tab, target };
        let reply = self.services.extension(&extension, request);
        cx.spawn_in(window, async move |this, cx| {
            let result = reply.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(ExtensionResponse::Ui { root }) => {
                        this.show_extension_ui(extension, root, window, cx);
                    }
                    Ok(_) => this.extension_error = Some(t!("detail.extension_no_ui").to_string()),
                    Err(error) => {
                        this.extension_error =
                            Some(t!("detail.extension_failed", error = error).to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn show_extension_ui(
        &mut self,
        extension: String,
        root: UiNode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.new(|cx| DeclarativeView::new(root, window, cx));
        self.extension_form = Some(cx.subscribe_in(
            &view,
            window,
            move |this, _, submitted: &FormSubmitted, window, cx| {
                this.submit_form(extension.clone(), submitted, window, cx);
            },
        ));
        self.extension_error = None;
        self.extension_tab = Some(view);
    }

    /// Sends a form to its extension. If it answers with an interface, that
    /// replaces the form.
    fn submit_form(
        &mut self,
        extension: String,
        submitted: &FormSubmitted,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let request = ExtensionRequest::SubmitForm {
            form: submitted.form.clone(),
            values: submitted.values.clone(),
        };
        let reply = self.services.extension(&extension, request);
        cx.spawn_in(window, async move |this, cx| {
            let result = reply.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(ExtensionResponse::Ui { root }) => {
                        this.show_extension_ui(extension, root, window, cx);
                    }
                    Ok(_) => this.extension_error = None,
                    Err(error) => {
                        this.extension_error =
                            Some(t!("detail.extension_failed", error = error).to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn owners(&self, object: &Value, cx: &App) -> Vec<Owner> {
        let resources = self
            .model
            .read(cx)
            .status(&self.context)
            .map(|status| status.resources.clone())
            .unwrap_or_default();
        let namespace = self.target.namespace.as_deref();
        object["metadata"]["ownerReferences"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|owner| Owner {
                kind: owner["kind"].as_str().unwrap_or_default().to_owned(),
                name: owner["name"].as_str().unwrap_or_default().to_owned(),
                target: owner_target(owner, namespace, &resources),
            })
            .collect()
    }

    fn render_secret(&self, object: &Value, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let rows: Vec<_> = object["data"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(key, _)| {
                let shown = self.revealed.contains(key);
                let text = self.secret_text(key).unwrap_or_default();
                let toggle = key.clone();
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(key.clone())
                    .child(
                        div()
                            .id(SharedString::from(format!("secret-value-{key}")))
                            .child(text)
                            .test_support(),
                    )
                    .child(
                        Button::new(format!("reveal-{key}"))
                            .icon(if shown {
                                IconName::EyeOff
                            } else {
                                IconName::Eye
                            })
                            .label(
                                if shown {
                                    t!("detail.hide")
                                } else {
                                    t!("detail.reveal")
                                }
                                .to_string(),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.revealed.remove(&toggle) {
                                    this.revealed.insert(toggle.clone());
                                }
                                cx.notify();
                            })),
                    )
            })
            .collect();
        div()
            .id("secret-values")
            .flex()
            .flex_col()
            .gap_1()
            .children(rows)
            .test_support()
    }

    /// Asks before deleting. Each opening starts clean, whichever way the last
    /// one closed.
    fn open_delete_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.delete_error = None;
        self.name_matches = false;
        self.typed_name
            .update(cx, |input, cx| input.set_value("", window, cx));
        let view = cx.weak_entity();
        let input = self.typed_name.clone();
        let focus_input = Cell::new(self.model.read(cx).identity(&self.key).protected);
        window.open_dialog(cx, move |dialog, window, cx| {
            if focus_input.replace(false) {
                input.focus_handle(cx).focus(window, cx);
            }
            match view.upgrade() {
                Some(view) => view.read(cx).delete_dialog(dialog, view.downgrade(), cx),
                // The cluster disconnected behind the dialog.
                None => dialog.title(t!("detail.gone").to_string()),
            }
        });
    }

    fn delete_dialog(&self, dialog: Dialog, view: WeakEntity<Self>, cx: &App) -> Dialog {
        let identity = self.model.read(cx).identity(&self.key);
        let namespace = self
            .target
            .namespace
            .clone()
            .unwrap_or_else(|| t!("detail.cluster_wide").to_string());
        let needs_name = identity.protected;
        let confirm = ((!needs_name || self.name_matches) && !self.deleting).then(|| {
            Button::new("delete-confirm")
                .icon(IconName::Trash)
                .label(t!("detail.delete").to_string())
                .on_click(move |_, window, cx| {
                    let _ = view.update(cx, |this, cx| this.delete(window, cx));
                })
        });
        dialog
            .title(
                t!(
                    "detail.delete_title",
                    kind = self.target.kind.kind,
                    name = self.target.name
                )
                .to_string(),
            )
            .overlay_closable(false)
            // Enter never deletes, so deleting always takes a deliberate click.
            .on_ok(|_, _, _| false)
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
                    .child(t!("detail.namespace_line", namespace = namespace).to_string())
                    .children(needs_name.then(|| t!("detail.protected").to_string()))
                    .children(needs_name.then(|| Input::new(&self.typed_name)))
                    .children(
                        self.delete_error
                            .clone()
                            .map(|error| div().id("delete-error").child(error).test_support()),
                    )
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
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .children(confirm),
            )
    }
}

impl Render for DetailView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let root = div()
            .id("detail-view")
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .overflow_y_scroll()
            .child(format!("{} {}", self.target.kind.kind, self.target.name));

        let object = match &self.object {
            Load::Loading => return root.child(t!("common.loading").to_string()).test_support(),
            Load::Failed(message) => {
                return root
                    .child(
                        div()
                            .id("detail-error")
                            .child(message.clone())
                            .test_support(),
                    )
                    .test_support();
            }
            Load::Loaded(object) => object.clone(),
        };

        let metadata = &object["metadata"];
        let text = |value: &Value| value.as_str().unwrap_or("-").to_owned();
        let phase = object["status"]["phase"].as_str().map(str::to_owned);
        let list = DescriptionList::new()
            .columns(1)
            .item(
                t!("column.namespace").to_string(),
                text(&metadata["namespace"]),
                1,
            )
            .item(
                t!("column.created").to_string(),
                text(&metadata["creationTimestamp"]),
                1,
            )
            .item(t!("detail.uid").to_string(), text(&metadata["uid"]), 1)
            .item(
                t!("detail.status").to_string(),
                phase.unwrap_or_else(|| "-".to_owned()),
                1,
            );

        let labels: Vec<String> = metadata["labels"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(key, value)| format!("{key}={}", value.as_str().unwrap_or_default()))
            .collect();
        let conditions: Vec<String> = object["status"]["conditions"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| format!("{}: {}", text(&c["type"]), text(&c["status"])))
            .collect();

        let owners = self.owners(&object, cx);
        let owner_rows: Vec<AnyElement> = owners
            .into_iter()
            .map(|owner| match owner.target {
                Some(target) => Button::new(format!("owner-{}", owner.name))
                    .label(format!("{} {}", owner.kind, owner.name))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(DetailEvent::OpenOwner(target.clone()));
                    }))
                    .into_any_element(),
                None => div()
                    .child(format!("{} {}", owner.kind, owner.name))
                    .into_any_element(),
            })
            .collect();

        let events: Vec<_> = self
            .events
            .iter()
            .map(|event| {
                div().child(
                    t!(
                        "detail.event",
                        kind = event.kind,
                        reason = event.reason,
                        count = event.count,
                        message = event.message
                    )
                    .to_string(),
                )
            })
            .collect();

        let tab_buttons: Vec<_> = self
            .tabs
            .iter()
            .map(|(extension, tab)| {
                let (extension, id) = (extension.clone(), tab.id.clone());
                Button::new(format!("detail-tab-{}", tab.id))
                    .label(tab.title.clone())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_tab(extension.clone(), id.clone(), window, cx);
                    }))
            })
            .collect();

        let secret = self.is_secret().then(|| self.render_secret(&object, cx));

        root.child(list)
            .children((!labels.is_empty()).then(|| {
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_color(muted)
                            .child(t!("detail.labels").to_string()),
                    )
                    .children(labels.into_iter().map(|label| div().child(label)))
            }))
            .children((!conditions.is_empty()).then(|| {
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_color(muted)
                            .child(t!("detail.conditions").to_string()),
                    )
                    .children(
                        conditions
                            .into_iter()
                            .map(|condition| div().child(condition)),
                    )
            }))
            .children((!owner_rows.is_empty()).then(|| {
                div()
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap_1()
                    .child(
                        div()
                            .text_color(muted)
                            .child(t!("detail.owners").to_string()),
                    )
                    .children(owner_rows)
            }))
            .children(secret)
            .child(
                div()
                    .id("detail-events")
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_color(muted)
                            .child(t!("detail.events").to_string()),
                    )
                    .children(events)
                    .children(self.events_error.clone().map(|error| {
                        div()
                            .id("events-error")
                            .child(t!("detail.events_failed", error = error).to_string())
                            .test_support()
                    }))
                    .test_support(),
            )
            .child(div().flex().gap_1().children(tab_buttons))
            .children(self.extension_tab.clone())
            .children(
                self.extension_error
                    .clone()
                    .map(|error| div().id("detail-tab-error").child(error).test_support()),
            )
            .when(is_pod(&self.target), |this| {
                this.child(
                    Button::new("view-logs")
                        .icon(IconName::SquareTerminal)
                        .label(t!("detail.logs").to_string())
                        .on_click(cx.listener(|this, _, _, cx| {
                            cx.emit(DetailEvent::Logs(this.target.clone()));
                        })),
                )
                .child(
                    Button::new("open-shell")
                        .icon(IconName::SquareTerminal)
                        .label(t!("detail.shell").to_string())
                        .on_click(cx.listener(|this, _, _, cx| {
                            cx.emit(DetailEvent::Shell(this.target.clone()));
                        })),
                )
            })
            .child(
                Button::new("edit-object")
                    .icon(IconName::Pencil)
                    .label(t!("detail.edit").to_string())
                    .on_click(cx.listener(|this, _, _, cx| {
                        cx.emit(DetailEvent::Edit(this.target.clone()));
                    })),
            )
            .child(
                Button::new("delete-object")
                    .icon(IconName::Trash)
                    .label(t!("detail.delete").to_string())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_delete_dialog(window, cx);
                    })),
            )
            .test_support()
    }
}
