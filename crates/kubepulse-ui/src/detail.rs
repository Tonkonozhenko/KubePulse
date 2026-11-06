//! One object: its metadata, status, events, and owners. Secret values stay
//! hidden until revealed, and deleting always names the target first.

use std::{collections::HashSet, sync::Arc};

use base64::Engine as _;
use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme as _, Sizable as _,
        button::{Button, ButtonVariants as _},
        collapsible::Collapsible,
        description_list::DescriptionList,
        tag::Tag,
    },
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
    columns::{columns_for, is_pod, kind_ref},
    declarative::{DeclarativeView, FormSubmitted},
    identity::ClusterKey,
    resource_browser::cell_text,
    services::ClusterServices,
    status::{Tone, condition_tone, event_tone, status_tag, tone_tag},
};

pub enum DetailEvent {
    OpenOwner(ObjectTarget),
    Edit(ObjectTarget),
    Logs(ObjectTarget),
    Shell(ObjectTarget),
    Delete(ObjectTarget),
    Close,
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

pub struct Condition {
    pub kind: String,
    pub status: String,
    pub reason: String,
    pub message: String,
    pub tone: Tone,
}

fn conditions(object: &Value) -> Vec<Condition> {
    let text = |value: &Value| value.as_str().unwrap_or_default().to_owned();
    object["status"]["conditions"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|condition| {
            let (kind, status) = (text(&condition["type"]), text(&condition["status"]));
            Condition {
                tone: condition_tone(&kind, &status),
                kind,
                status,
                reason: text(&condition["reason"]),
                message: text(&condition["message"]),
            }
        })
        .collect()
}

/// The same status the object's table shows. A kind with no status column
/// falls back to its phase, if it has one.
fn status_text(
    target: &ObjectTarget,
    object: &Value,
    services: &dyn ClusterServices,
) -> Option<String> {
    let columns = columns_for(&target.kind, &services.contributions());
    let column = columns.declared.iter().find(|column| column.status);
    match column {
        Some(column) => {
            Some(cell_text(Some(&column.spec.evaluate(object)))).filter(|text| !text.is_empty())
        }
        None => object["status"]["phase"].as_str().map(str::to_owned),
    }
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
    target: ObjectTarget,
    model: Entity<ClusterModel>,
    services: Arc<dyn ClusterServices>,
    object: Load,
    status: Option<String>,
    events: Vec<EventRecord>,
    events_error: Option<String>,
    revealed: HashSet<String>,
    annotations_open: bool,
    tabs: Vec<(String, DetailTabContribution)>,
    extension_tab: Option<Entity<DeclarativeView>>,
    extension_error: Option<String>,
    extension_form: Option<Subscription>,
}

impl DetailView {
    pub fn new(
        key: ClusterKey,
        target: ObjectTarget,
        model: Entity<ClusterModel>,
        services: Arc<dyn ClusterServices>,
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

        let context = key.context;
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
                    Ok(object) => {
                        this.status = status_text(&this.target, &object, &*this.services);
                        Load::Loaded(object)
                    }
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
            target,
            model,
            services,
            object: Load::Loading,
            status: None,
            events: Vec::new(),
            events_error: None,
            revealed: HashSet::new(),
            annotations_open: false,
            tabs,
            extension_tab: None,
            extension_error: None,
            extension_form: None,
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

    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    pub fn conditions(&self) -> Vec<Condition> {
        self.object().map(conditions).unwrap_or_default()
    }

    /// Each key with its value, as shown. A Secret's values stay out, because
    /// an annotation can hold a copy of its data.
    pub fn annotations(&self, object: &Value) -> Vec<(String, Option<String>)> {
        let shown = !self.is_secret();
        object["metadata"]["annotations"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(key, value)| {
                let value = value.as_str().unwrap_or_default().to_owned();
                (key.clone(), shown.then_some(value))
            })
            .collect()
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

    fn is_secret(&self) -> bool {
        self.target.kind.kind == "Secret" && self.target.kind.group.is_empty()
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
}

impl DetailView {
    fn render_header(&self, loaded: bool, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let close = Button::new("close-detail")
            .icon(IconName::X)
            .ghost()
            .xsmall()
            .tab_stop(false)
            .tooltip(t!("detail.close").to_string())
            .on_click(cx.listener(|_, _, _, cx| cx.emit(DetailEvent::Close)));
        let title = div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(format!("{} {}", self.target.kind.kind, self.target.name)),
            )
            .children(self.status.clone().map(status_tag))
            .child(close);
        let logs = (loaded && is_pod(&self.target.kind)).then(|| {
            Button::new("view-logs")
                .icon(IconName::SquareTerminal)
                .small()
                .label(t!("detail.logs").to_string())
                .on_click(cx.listener(|this, _, _, cx| {
                    cx.emit(DetailEvent::Logs(this.target.clone()));
                }))
        });
        let shell = (loaded && is_pod(&self.target.kind)).then(|| {
            Button::new("open-shell")
                .icon(IconName::SquareTerminal)
                .small()
                .label(t!("detail.shell").to_string())
                .on_click(cx.listener(|this, _, _, cx| {
                    cx.emit(DetailEvent::Shell(this.target.clone()));
                }))
        });
        let actions = loaded.then(|| {
            div()
                .flex()
                .gap_2()
                .children(logs)
                .children(shell)
                .child(
                    Button::new("edit-object")
                        .icon(IconName::Pencil)
                        .small()
                        .label(t!("detail.edit").to_string())
                        .on_click(cx.listener(|this, _, _, cx| {
                            cx.emit(DetailEvent::Edit(this.target.clone()));
                        })),
                )
                .child(
                    Button::new("delete-object")
                        .icon(IconName::Trash)
                        .small()
                        .label(t!("detail.delete").to_string())
                        .on_click(cx.listener(|this, _, _, cx| {
                            cx.emit(DetailEvent::Delete(this.target.clone()));
                        })),
                )
        });
        div()
            .id("detail-header")
            .flex()
            .flex_none()
            .flex_col()
            .gap_2()
            .child(title)
            .children(actions)
            .test_support()
    }

    fn render_labels(&self, object: &Value, muted: Hsla) -> Option<impl IntoElement + use<>> {
        let labels = object["metadata"]["labels"]
            .as_object()
            .filter(|labels| !labels.is_empty())?;
        let tags = labels.iter().map(|(key, value)| {
            let value = value.as_str().unwrap_or_default();
            Tag::secondary().small().child(format!("{key}={value}"))
        });
        Some(
            section("detail-labels", t!("detail.labels").to_string(), muted)
                .child(div().flex().flex_wrap().gap_1().children(tags))
                .test_support(),
        )
    }

    fn render_conditions(&self, object: &Value, muted: Hsla) -> Option<impl IntoElement + use<>> {
        let conditions = conditions(object);
        if conditions.is_empty() {
            return None;
        }
        let rows = conditions.into_iter().map(move |condition| {
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_2()
                        .child(tone_tag(condition.tone).child(condition.kind))
                        .child(condition.status)
                        .child(condition.reason),
                )
                .child(div().text_xs().text_color(muted).child(condition.message))
        });
        Some(
            section(
                "detail-conditions",
                t!("detail.conditions").to_string(),
                muted,
            )
            .children(rows)
            .test_support(),
        )
    }

    fn render_annotations(
        &self,
        object: &Value,
        muted: Hsla,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let annotations = self.annotations(object);
        if annotations.is_empty() {
            return None;
        }
        let open = self.annotations_open;
        let rows = annotations
            .into_iter()
            .filter(|_| open)
            .map(move |(key, value)| {
                div()
                    .flex()
                    .flex_col()
                    .child(div().text_xs().text_color(muted).child(key))
                    .children(value)
            });
        Some(
            Collapsible::new()
                .open(open)
                .child(
                    Button::new("toggle-annotations")
                        .icon(if open {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .ghost()
                        .xsmall()
                        .label(t!("detail.annotations").to_string())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.annotations_open = !this.annotations_open;
                            cx.notify();
                        })),
                )
                .content(
                    div()
                        .id("detail-annotations")
                        .flex()
                        .flex_col()
                        .gap_1()
                        .children(rows)
                        .test_support(),
                ),
        )
    }

    fn render_events(&self, muted: Hsla) -> impl IntoElement + use<> {
        let events: Vec<_> = self
            .events
            .iter()
            .enumerate()
            .map(|(index, event)| {
                let count = t!("detail.event_count", count = event.count).to_string();
                div()
                    .id(("detail-event", index))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_2()
                            .child(tone_tag(event_tone(&event.kind)).child(event.kind.clone()))
                            .child(event.reason.clone())
                            .child(div().text_xs().text_color(muted).child(count)),
                    )
                    .child(event.message.clone())
                    .test_support()
            })
            .collect();
        section("detail-events", t!("detail.events").to_string(), muted)
            .children(events)
            .children(self.events_error.clone().map(|error| {
                div()
                    .id("events-error")
                    .child(t!("detail.events_failed", error = error).to_string())
                    .test_support()
            }))
            .test_support()
    }
}

/// A titled block of the panel.
fn section(id: &'static str, title: String, muted: Hsla) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_color(muted).child(title))
}

impl Render for DetailView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let loaded = matches!(self.object, Load::Loaded(_));
        let root = div()
            .id("detail-view")
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .child(self.render_header(loaded, cx));

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
            .item(t!("detail.uid").to_string(), text(&metadata["uid"]), 1);

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

        root.child(
            div()
                .id("detail-body")
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .gap_3()
                .overflow_y_scroll()
                .child(list)
                .children(self.render_labels(&object, muted))
                .children(self.render_conditions(&object, muted))
                .children(self.render_annotations(&object, muted, cx))
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
                .child(self.render_events(muted))
                .child(div().flex().gap_1().children(tab_buttons))
                .children(self.extension_tab.clone())
                .children(
                    self.extension_error
                        .clone()
                        .map(|error| div().id("detail-tab-error").child(error).test_support()),
                ),
        )
        .test_support()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{Tone, conditions};

    #[test]
    fn conditions_keep_their_text_and_read_their_health() {
        let object = json!({"status": {"conditions": [
            {"type": "Ready", "status": "False", "reason": "Crash", "message": "no"},
            {"type": "DiskPressure", "status": "False"},
        ]}});
        let conditions = conditions(&object);
        assert_eq!(conditions[0].kind, "Ready");
        assert_eq!(conditions[0].status, "False");
        assert_eq!(conditions[0].reason, "Crash");
        assert_eq!(conditions[0].message, "no");
        assert_eq!(conditions[0].tone, Tone::Warning);
        assert_eq!(conditions[1].tone, Tone::Success);
        assert_eq!(conditions[1].reason, "");
    }
}
