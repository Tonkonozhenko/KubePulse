//! Starts a port-forward to a pod or a service, or restarts a failed one
//! against another target.

use gpui_kit::{
    assets::IconName,
    component::{
        Disableable as _, Sizable as _, WindowExt as _,
        button::{Button, ButtonVariants as _},
        dialog::Dialog,
        input::{Input, InputEvent, InputState},
    },
    *,
};
use kubepulse_cluster::{ForwardKind, ForwardRequest, ObjectTarget};
use rust_i18n::t;
use serde_json::Value;

use crate::{
    cluster_model::ClusterModel,
    forwards::{OpenPortForwards, SharedForwards, start_error_text},
    identity::ClusterKey,
    services::SharedServices,
};

/// The forward kind for objects that can be forwarded to.
pub fn forward_kind(target: &ObjectTarget) -> Option<ForwardKind> {
    match (target.kind.group.as_str(), target.kind.kind.as_str()) {
        ("", "Pod") => Some(ForwardKind::Pod),
        ("", "Service") => Some(ForwardKind::Service),
        _ => None,
    }
}

/// The ports an object declares: a pod's container ports, or a service's ports.
pub fn declared_ports(kind: ForwardKind, object: &Value) -> Vec<u16> {
    let ports: Vec<&Value> = match kind {
        ForwardKind::Pod => object["spec"]["containers"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|container| container["ports"].as_array().into_iter().flatten())
            .map(|port| &port["containerPort"])
            .collect(),
        ForwardKind::Service => object["spec"]["ports"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|port| &port["port"])
            .collect(),
    };
    let mut ports: Vec<u16> = ports
        .into_iter()
        .filter_map(|port| u16::try_from(port.as_u64()?).ok())
        .collect();
    ports.dedup();
    ports
}

fn parse_port(text: &str) -> Option<u16> {
    text.trim().parse().ok().filter(|port| *port != 0)
}

#[derive(Clone, Copy, Debug)]
pub enum ForwardField {
    Name,
    Port,
    LocalPort,
}

pub struct ForwardDialog {
    key: ClusterKey,
    kind: ForwardKind,
    namespace: String,
    /// The failed forward this one replaces.
    replaces: Option<u64>,
    model: Entity<ClusterModel>,
    name: Entity<InputState>,
    port: Entity<InputState>,
    local_port: Entity<InputState>,
    suggested: Vec<u16>,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl ForwardDialog {
    /// Asks for the ports of a new forward to `target`, suggesting the ports the
    /// object declares.
    pub fn open(
        key: ClusterKey,
        target: ObjectTarget,
        model: Entity<ClusterModel>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let kind = forward_kind(&target).expect("only pods and services are forwarded");
        let request = ForwardRequest {
            kind,
            namespace: target.namespace.clone().unwrap_or_default(),
            name: target.name.clone(),
            port: 0,
            local_port: None,
        };
        let dialog = Self::show(key.clone(), &request, None, model, window, cx);
        let object = cx
            .global::<SharedServices>()
            .0
            .get_object(&key.context, target);
        dialog.update(cx, |_, cx| {
            cx.spawn_in(window, async move |this, cx| {
                let Ok(object) = object.await else {
                    return;
                };
                let _ = this.update_in(cx, |this, window, cx| {
                    this.suggest(declared_ports(kind, &object), window, cx);
                });
            })
            .detach();
        });
        dialog
    }

    /// Asks again for a failed forward, with its target and ports filled in.
    pub fn restart(
        id: u64,
        key: ClusterKey,
        request: &ForwardRequest,
        model: Entity<ClusterModel>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        Self::show(key, request, Some(id), model, window, cx)
    }

    fn show(
        key: ClusterKey,
        request: &ForwardRequest,
        replaces: Option<u64>,
        model: Entity<ClusterModel>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let view = cx.new(|cx: &mut Context<Self>| {
            let mut field = |value: String, placeholder: String, cx: &mut Context<Self>| {
                cx.new(|cx| {
                    let mut input = InputState::new(window, cx).placeholder(placeholder);
                    input.set_value(value, window, cx);
                    input
                })
            };
            let port = (request.port != 0).then(|| request.port.to_string());
            let local_port = request.local_port.map(|port| port.to_string());
            let name = field(request.name.clone(), t!("forward.name").to_string(), cx);
            let port = field(port.unwrap_or_default(), t!("forward.port").to_string(), cx);
            let local_port = field(
                local_port.unwrap_or_default(),
                t!("forward.local_port_automatic").to_string(),
                cx,
            );
            let subscriptions = [&name, &port, &local_port]
                .into_iter()
                .map(|input| {
                    cx.subscribe(input, |this, _, event: &InputEvent, cx| {
                        if matches!(event, InputEvent::Change) {
                            this.error = None;
                            cx.notify();
                        }
                    })
                })
                .collect();
            Self {
                key,
                kind: request.kind,
                namespace: request.namespace.clone(),
                replaces,
                model,
                name,
                port,
                local_port,
                suggested: Vec::new(),
                error: None,
                _subscriptions: subscriptions,
            }
        });
        let shown = view.clone();
        window.open_dialog(cx, move |dialog, _, cx| {
            shown.update(cx, |view, cx| view.dialog(dialog, cx))
        });
        view
    }

    fn suggest(&mut self, ports: Vec<u16>, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(first) = ports.first()
            && self.port.read(cx).value().trim().is_empty()
        {
            let first = first.to_string();
            self.port
                .update(cx, |input, cx| input.set_value(first, window, cx));
        }
        self.suggested = ports;
        cx.notify();
    }

    pub fn set_field(
        &mut self,
        field: ForwardField,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = match field {
            ForwardField::Name => &self.name,
            ForwardField::Port => &self.port,
            ForwardField::LocalPort => &self.local_port,
        };
        let text = text.to_owned();
        input.update(cx, |input, cx| input.set_value(text, window, cx));
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The request the fields describe, if they are valid.
    pub fn request(&self, cx: &App) -> Option<ForwardRequest> {
        let name = self.name.read(cx).value().trim().to_owned();
        let port = parse_port(&self.port.read(cx).value())?;
        let local = self.local_port.read(cx).value().trim().to_owned();
        let local_port = match local.as_str() {
            "" => None,
            text => Some(parse_port(text)?),
        };
        (!name.is_empty()).then(|| ForwardRequest {
            kind: self.kind,
            namespace: self.namespace.clone(),
            name,
            port,
            local_port,
        })
    }

    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(request) = self.request(cx) else {
            return;
        };
        let local_port = request.local_port;
        let registry = cx.global::<SharedForwards>().0.clone();
        let (key, replaces) = (self.key.clone(), self.replaces);
        let started = registry.update(cx, |registry, cx| match replaces {
            Some(id) => registry.restart(id, request, cx),
            None => registry.start(key, request, cx),
        });
        match started {
            Ok(_) => {
                window.close_dialog(cx);
                window.dispatch_action(Box::new(OpenPortForwards), cx);
            }
            Err(error) => {
                self.error = Some(start_error_text(&error, local_port));
                cx.notify();
            }
        }
    }

    fn title(&self) -> String {
        match self.kind {
            ForwardKind::Pod => t!("forward.dialog_title_pod"),
            ForwardKind::Service => t!("forward.dialog_title_service"),
        }
        .to_string()
    }

    fn dialog(&self, dialog: Dialog, cx: &mut Context<Self>) -> Dialog {
        let identity = self.model.read(cx).identity(&self.key);
        let valid = self.request(cx).is_some();
        let view = cx.weak_entity();
        let suggestions: Vec<Button> = self
            .suggested
            .iter()
            .map(|&port| {
                let view = view.clone();
                Button::new(("forward-suggested", port as usize))
                    .label(port.to_string())
                    .xsmall()
                    .outline()
                    .on_click(move |_, window, cx| {
                        let _ = view.update(cx, |this, cx| {
                            let port = port.to_string();
                            this.port
                                .update(cx, |input, cx| input.set_value(port, window, cx));
                        });
                    })
            })
            .collect();
        let start_view = view.clone();
        let start = Button::new("forward-start")
            .icon(IconName::ArrowRightLeft)
            .primary()
            .label(t!("forward.start").to_string())
            .disabled(!valid)
            .on_click(move |_, window, cx| {
                let _ = start_view.update(cx, |this, cx| this.start(window, cx));
            });
        dialog
            .title(self.title())
            .on_ok(move |_, window, cx| {
                let _ = view.update(cx, |this, cx| this.start(window, cx));
                false
            })
            .child(
                div()
                    .id("forward-form")
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_color(rgb(identity.color))
                            .child(t!("detail.cluster", name = identity.name).to_string()),
                    )
                    .child(t!("detail.namespace_line", namespace = self.namespace).to_string())
                    .child(t!("forward.name").to_string())
                    .child(Input::new(&self.name))
                    .child(t!("forward.port").to_string())
                    .child(Input::new(&self.port))
                    .child(div().flex().flex_wrap().gap_1().children(suggestions))
                    .child(t!("forward.local_port").to_string())
                    .child(Input::new(&self.local_port))
                    .children(
                        self.error
                            .clone()
                            .map(|error| div().id("forward-error").child(error).test_support()),
                    )
                    .test_support(),
            )
            .footer(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("forward-cancel")
                            .label(t!("common.cancel").to_string())
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(start),
            )
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{ForwardKind, declared_ports};

    #[test]
    fn a_pod_suggests_its_container_ports() {
        let pod = json!({"spec": {"containers": [
            {"ports": [{"containerPort": 8080}, {"containerPort": 9090}]},
            {"name": "sidecar"},
        ]}});
        assert_eq!(declared_ports(ForwardKind::Pod, &pod), vec![8080, 9090]);
    }

    #[test]
    fn a_service_suggests_its_ports() {
        let service = json!({"spec": {"ports": [{"port": 80, "targetPort": "http"}]}});
        assert_eq!(declared_ports(ForwardKind::Service, &service), vec![80]);
    }
}
