//! Port-forwards across all clusters, and the tab that lists them. A forward
//! belongs to its cluster's session, so it outlives the tab that started it and
//! ends when its cluster disconnects.

use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme as _, Icon, Sizable as _,
        button::Button,
        dock::{self, PanelEvent},
    },
    *,
};
use kubepulse_cluster::{ForwardFailure, ForwardKind, ForwardRequest, ForwardState, PortForward};
use rust_i18n::t;

use crate::{
    cluster_model::ClusterModel,
    forward_dialog::ForwardDialog,
    identity::ClusterKey,
    services::{ForwardStartError, SharedServices},
    shell_state::{SharedShellState, ShellState, close_button, close_on_middle_click},
    status::{Tone, tone_tag},
};

pub const PANEL_NAME: &str = "ForwardsTab";

gpui_kit::actions!(kubepulse, [OpenPortForwards]);

pub struct ForwardEntry {
    pub id: u64,
    pub key: ClusterKey,
    /// What was asked for, with the local port that was bound.
    pub request: ForwardRequest,
    pub state: ForwardState,
    _forward: PortForward,
    _follow: Task<()>,
}

#[derive(Default)]
pub struct ForwardRegistry {
    next_id: u64,
    entries: Vec<ForwardEntry>,
}

/// Lets tabs and dialogs find the registry.
pub struct SharedForwards(pub Entity<ForwardRegistry>);

impl Global for SharedForwards {}

impl ForwardRegistry {
    pub fn entries(&self) -> &[ForwardEntry] {
        &self.entries
    }

    pub fn entry(&self, id: u64) -> Option<&ForwardEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    /// Starts a forward and lists it until it is stopped or its cluster
    /// disconnects.
    pub fn start(
        &mut self,
        key: ClusterKey,
        mut request: ForwardRequest,
        cx: &mut Context<Self>,
    ) -> Result<u64, ForwardStartError> {
        let services = cx.global::<SharedServices>().0.clone();
        let forward = services.start_forward(&key.context, request.clone())?;
        request.local_port = Some(forward.local_port());
        let id = self.next_id;
        self.next_id += 1;
        let mut state = forward.subscribe();
        let current = state.borrow_and_update().clone();
        let follow = cx.spawn(async move |this, cx| {
            loop {
                let ended = state.changed().await.is_err();
                let current = state.borrow_and_update().clone();
                let updated = this.update(cx, |this, cx| {
                    if ended {
                        this.entries.retain(|entry| entry.id != id);
                    } else if let Some(entry) = this.entries.iter_mut().find(|e| e.id == id) {
                        entry.state = current;
                    }
                    cx.notify();
                });
                if ended || updated.is_err() {
                    return;
                }
            }
        });
        self.entries.push(ForwardEntry {
            id,
            key,
            request,
            state: current,
            _forward: forward,
            _follow: follow,
        });
        cx.notify();
        Ok(id)
    }

    /// Stops the forward, which releases its port.
    pub fn stop(&mut self, id: u64, cx: &mut Context<Self>) {
        self.entries.retain(|entry| entry.id != id);
        cx.notify();
    }

    /// Starts `request` in place of a failed forward, which has already
    /// released its port. The failed one stays if the new one cannot start.
    pub fn restart(
        &mut self,
        id: u64,
        request: ForwardRequest,
        cx: &mut Context<Self>,
    ) -> Result<u64, ForwardStartError> {
        let key = self
            .entry(id)
            .expect("restarted forwards are listed")
            .key
            .clone();
        let started = self.start(key, request, cx)?;
        let position = self.entries.iter().position(|entry| entry.id == id);
        if let Some(position) = position {
            let restarted = self.entries.pop().expect("the new forward was just added");
            self.entries[position] = restarted;
        }
        Ok(started)
    }
}

pub fn target_text(request: &ForwardRequest) -> String {
    let (namespace, name) = (&request.namespace, &request.name);
    match request.kind {
        ForwardKind::Pod => t!("forward.target_pod", namespace = namespace, name = name),
        ForwardKind::Service => t!("forward.target_service", namespace = namespace, name = name),
    }
    .to_string()
}

pub fn ports_text(request: &ForwardRequest) -> String {
    let local = request.local_port.unwrap_or_default();
    t!("forward.ports", local = local, remote = request.port).to_string()
}

pub fn state_text(state: &ForwardState) -> (Tone, String) {
    match state {
        ForwardState::Starting => (Tone::Neutral, t!("forward.starting").to_string()),
        ForwardState::Active { pod, port } => (
            Tone::Success,
            t!("forward.active", pod = pod, port = port).to_string(),
        ),
        ForwardState::NoReadyPod => (Tone::Warning, t!("forward.no_ready_pod").to_string()),
        ForwardState::Failed(failure) => {
            let reason = match failure {
                ForwardFailure::PodDeleted => t!("forward.pod_deleted").to_string(),
                ForwardFailure::PodStopped => t!("forward.pod_stopped").to_string(),
                ForwardFailure::NoServicePort => t!("forward.no_service_port").to_string(),
                ForwardFailure::Cluster(message) => message.clone(),
            };
            (
                Tone::Danger,
                t!("forward.failed", reason = reason).to_string(),
            )
        }
    }
}

/// What the user sees about a forward that could not start.
pub fn start_error_text(error: &ForwardStartError, local_port: Option<u16>) -> String {
    match error {
        ForwardStartError::NotConnected => t!("forward.not_connected").to_string(),
        ForwardStartError::Port(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            let port = local_port.unwrap_or_default();
            t!("forward.port_in_use", port = port).to_string()
        }
        ForwardStartError::Port(error) => t!("forward.bind_failed", error = error).to_string(),
    }
}

pub struct ForwardsTab {
    registry: Entity<ForwardRegistry>,
    model: Entity<ClusterModel>,
    shell: Entity<ShellState>,
    focus: FocusHandle,
    dialog: WeakEntity<ForwardDialog>,
    _subscriptions: Vec<Subscription>,
}

impl ForwardsTab {
    pub fn new(model: Entity<ClusterModel>, cx: &mut Context<Self>) -> Self {
        let registry = cx.global::<SharedForwards>().0.clone();
        let subscriptions = vec![
            cx.observe(&registry, |_, _, cx| cx.notify()),
            cx.observe(&model, |_, _, cx| cx.notify()),
        ];
        Self {
            registry,
            model,
            shell: cx.global::<SharedShellState>().0.clone(),
            focus: cx.focus_handle(),
            dialog: WeakEntity::new_invalid(),
            _subscriptions: subscriptions,
        }
    }

    pub fn dialog(&self) -> Option<Entity<ForwardDialog>> {
        self.dialog.upgrade()
    }

    fn render_entry(&self, entry: &ForwardEntry, cx: &Context<Self>) -> impl IntoElement {
        let id = entry.id;
        let identity = self.model.read(cx).identity(&entry.key);
        let (tone, status) = state_text(&entry.state);
        let failed = matches!(entry.state, ForwardState::Failed(_));
        let restart = failed.then(|| {
            Button::new(("forward-restart", id as usize))
                .icon(IconName::RotateCcw)
                .small()
                .label(t!("forward.restart").to_string())
                .on_click(cx.listener(move |this, _, window, cx| {
                    let registry = this.registry.read(cx);
                    let Some(entry) = registry.entry(id) else {
                        return;
                    };
                    let (key, request) = (entry.key.clone(), entry.request.clone());
                    let model = this.model.clone();
                    let dialog = ForwardDialog::restart(id, key, &request, model, window, cx);
                    this.dialog = dialog.downgrade();
                }))
        });
        div()
            .id(("forward", id as usize))
            .flex()
            .items_center()
            .gap_3()
            .py_1()
            .child(
                div()
                    .w(px(160.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_none()
                            .size_2()
                            .rounded_full()
                            .bg(rgb(identity.color)),
                    )
                    .child(div().truncate().child(identity.name)),
            )
            .child(
                div()
                    .w(px(260.))
                    .truncate()
                    .child(target_text(&entry.request)),
            )
            .child(
                div()
                    .id(("forward-ports", id as usize))
                    .w(px(180.))
                    .child(ports_text(&entry.request))
                    .test_support(),
            )
            .child(
                div()
                    .id(("forward-status", id as usize))
                    .flex_1()
                    .min_w_0()
                    .child(tone_tag(tone).child(status))
                    .test_support(),
            )
            .children(restart)
            .child(
                Button::new(("forward-stop", id as usize))
                    .icon(IconName::Square)
                    .small()
                    .label(t!("forward.stop").to_string())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.registry
                            .update(cx, |registry, cx| registry.stop(id, cx));
                    })),
            )
            .test_support()
    }
}

impl gpui_kit::base::dock::Panel for ForwardsTab {
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
        let tab = cx.weak_entity();
        self.shell
            .update(cx, |shell, _| shell.set_forwards_tab(Some(tab)));
    }

    fn on_removed(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.shell
            .update(cx, |shell, _| shell.set_forwards_tab(None));
    }
}

impl dock::Panel for ForwardsTab {
    fn title(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("forwards-tab-title")
            .flex()
            .items_center()
            .gap_2()
            .child(Icon::new(IconName::ArrowRightLeft).xsmall())
            .child(t!("forward.title").to_string())
            .on_aux_click(close_on_middle_click(cx.entity_id(), self.shell.clone()))
            .child(close_button(cx.entity_id(), self.shell.clone()))
            .test_support()
    }
}

impl EventEmitter<PanelEvent> for ForwardsTab {}

impl Focusable for ForwardsTab {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ForwardsTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entries = self.registry.read(cx).entries();
        let rows: Vec<AnyElement> = entries
            .iter()
            .map(|entry| self.render_entry(entry, cx).into_any_element())
            .collect();
        let empty = rows.is_empty().then(|| {
            div()
                .id("no-forwards")
                .text_color(cx.theme().muted_foreground)
                .child(t!("forward.none").to_string())
                .test_support()
        });
        div()
            .id("forwards")
            .size_full()
            .p_3()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .track_focus(&self.focus)
            .children(empty)
            .children(rows)
            .test_support()
    }
}
