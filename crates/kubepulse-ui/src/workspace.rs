//! The main window: the cluster rail, the kind sidebar, and a dock of tabs and
//! panes.

use std::{path::PathBuf, sync::Arc};

use gpui_kit::{
    base::dock::PanelView,
    component::{
        ActiveTheme as _, WindowExt as _,
        button::Button,
        command::{Command, CommandState},
        dock::{
            DockArea, DockEvent, DockLayout, DockPlacement, DockSkin, InsertTarget, NodeId,
            PaneRef, PanelId, panel_handle,
        },
        notification::Notification,
    },
    prelude::FluentBuilder as _,
    *,
};
use kubepulse_cluster::{ObjectTarget, ResourceKind};
use kubepulse_extension_api::{
    ChangeOutcome, ChangeRequest, ClusterRef, ExtensionRequest, HostError, KindRef, ObjectRef,
};

use rust_i18n::t;
use tokio::sync::{mpsc, oneshot};

use crate::{
    about_tab::{AboutTab, OpenAbout},
    cluster_model::{ClusterModel, ClusterModelEvent, is_live},
    cluster_rail::{ClusterRail, request_disconnect},
    cluster_tab::ClusterTab,
    columns::kind_ref,
    command_palette::{OpenCommandPalette, PaletteAction, palette_groups},
    forwards::{ForwardRegistry, ForwardsTab, OpenPortForwards, SharedForwards},
    identity::ClusterKey,
    kind_sidebar::{KindSidebar, KindSidebarEvent},
    log_tab::LogTab,
    manifest_tab::{ManifestRequest, ManifestTab},
    persistence::{ConfigPaths, write_atomic},
    preferences::{PreferencesEvent, PreferencesModel, SharedPreferences},
    runtime::HostRequest,
    services::SharedServices,
    settings_tab::{OpenSettings, SettingsTab},
    shell_state::{Placement, SharedShellState, ShellEvent, ShellState, place_kind},
    status_bar::StatusBar,
    terminal_tab::{OpenLocalTerminal, TerminalTab},
    theme::{self, SharedThemes, ThemeCatalog},
};

const DOCK_ID: &str = "workspace";
const PALETTE_WIDTH: f32 = 560.;

pub struct Workspace {
    model: Entity<ClusterModel>,
    shell: Entity<ShellState>,
    preferences: Entity<PreferencesModel>,
    themes: Entity<ThemeCatalog>,
    rail: Entity<ClusterRail>,
    sidebar: Entity<KindSidebar>,
    status_bar: Entity<StatusBar>,
    dock: Entity<DockArea>,
    paths: Option<ConfigPaths>,
    _subscriptions: Vec<Subscription>,
}

impl Workspace {
    pub fn new(
        model: Entity<ClusterModel>,
        paths: Option<ConfigPaths>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let shell = cx.global::<SharedShellState>().0.clone();
        let preferences = cx.global::<SharedPreferences>().0.clone();
        let themes = cx.global::<SharedThemes>().0.clone();
        let rail = cx.new(|cx| ClusterRail::new(model.clone(), window, cx));
        let sidebar = cx.new(|cx| KindSidebar::new(model.clone(), cx));
        let (dock, _skin) = DockSkin::dock_area(DOCK_ID, None, window, cx);
        // Forwards live as long as the window, whatever tabs come and go.
        let forwards = cx.new(|_| ForwardRegistry::default());
        cx.set_global(SharedForwards(forwards));
        let status_bar = cx.new(|cx| StatusBar::new(model.clone(), cx));

        let subscriptions = vec![
            cx.subscribe(&dock, |_, _, event: &DockEvent, cx| {
                if matches!(event, DockEvent::LayoutChanged) {
                    cx.notify();
                }
            }),
            cx.subscribe_in(&shell, window, |this, _, event, window, cx| match event {
                ShellEvent::CloseTab(id) => this.close_tab(*id, window, cx),
                ShellEvent::OpenLogs { key, target } => {
                    let (key, target) = (key.clone(), (**target).clone());
                    this.open_logs(key, target, None, window, cx);
                }
                ShellEvent::OpenTerminal { key, target } => {
                    let (key, target) = (key.clone(), (**target).clone());
                    this.open_terminal(key, target, None, window, cx);
                }
                ShellEvent::OpenEditor { key, target } => {
                    let (key, target) = (key.clone(), (**target).clone());
                    this.open_editor(key, target, ManifestRequest::Edit, None, window, cx);
                }
            }),
            cx.subscribe_in(&sidebar, window, |this, _, event, window, cx| match event {
                KindSidebarEvent::Show { key, kind, pin } => {
                    this.show_kind(key.clone(), kind.clone(), *pin, window, cx);
                }
                KindSidebarEvent::OpenInNewTab { key, kind } => {
                    this.open_tab(key.clone(), kind.clone(), true, window, cx);
                }
            }),
            cx.subscribe(
                &model,
                |this, model, event: &ClusterModelEvent, cx| match event {
                    ClusterModelEvent::IdentitiesChanged => {
                        this.save_identities(&model, cx);
                    }
                    ClusterModelEvent::NamespacesChanged(_) => {}
                },
            ),
            cx.subscribe_in(
                &preferences,
                window,
                |this, _, event: &PreferencesEvent, window, cx| match event {
                    PreferencesEvent::Changed => {
                        this.save_preferences(cx);
                        this.apply_theme(window, cx);
                    }
                },
            ),
            cx.observe_in(&themes, window, |this, _, window, cx| {
                this.apply_theme(window, cx);
            }),
            cx.observe_window_appearance(window, |this, window, cx| {
                this.apply_theme(window, cx);
            }),
        ];
        on_shortcut::<OpenSettings>(Self::open_settings, window, cx);
        on_shortcut::<OpenCommandPalette>(Self::open_command_palette, window, cx);
        on_shortcut::<OpenPortForwards>(Self::open_forwards, window, cx);
        on_shortcut::<OpenAbout>(Self::open_about, window, cx);
        on_shortcut::<OpenLocalTerminal>(
            |this, window, cx| {
                this.open_local_terminal(window, cx);
            },
            window,
            cx,
        );
        Self {
            model,
            shell,
            preferences,
            themes,
            rail,
            sidebar,
            status_bar,
            dock,
            paths,
            _subscriptions: subscriptions,
        }
    }

    pub fn status_bar(&self) -> &Entity<StatusBar> {
        &self.status_bar
    }

    pub fn dock(&self) -> &Entity<DockArea> {
        &self.dock
    }

    /// Shows a kind the way a click in the sidebar does: in a tab that already
    /// shows it, or else in the preview tab.
    pub fn show_kind(
        &mut self,
        key: ClusterKey,
        kind: ResourceKind,
        pin: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ClusterTab> {
        let shell = self.shell.read(cx);
        let placement = place_kind(&shell.summaries(), shell.active(), &key, &kind_ref(&kind));
        let tab = match placement {
            Placement::Activate(id) => self.registered_tab(id, cx),
            Placement::Replace(id) => {
                let tab = self.registered_tab(id, cx);
                tab.update(cx, |tab, cx| tab.open_kind(kind, window, cx));
                tab
            }
            Placement::ReplaceAcross(id) => self.replace_across(id, key, kind, window, cx),
            Placement::Open => self.open_tab(key, kind, false, window, cx),
        };
        let panel = PanelId::from(tab.entity_id());
        self.dock
            .update(cx, |dock, cx| dock.select_panel(panel, window, cx));
        tab.update(cx, |tab, cx| {
            if pin {
                tab.pin(cx);
            }
            tab.activate(cx);
        });
        tab
    }

    fn registered_tab(&self, id: EntityId, cx: &App) -> Entity<ClusterTab> {
        self.shell
            .read(cx)
            .tab(id)
            .expect("tabs unregister on removal")
    }

    /// A tab cannot change cluster, so a new tab takes the old one's place.
    fn replace_across(
        &mut self,
        old: EntityId,
        key: ClusterKey,
        kind: ResourceKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ClusterTab> {
        let replaced = self.registered_tab(old, cx);
        let (node, ix) = self.position(PanelId::from(old), cx);
        // Gone from the directory first, so the new tab can be the preview.
        self.shell.update(cx, |shell, cx| shell.remove_tab(old, cx));
        let tab = self.open_tab(key, kind, false, window, cx);
        let panel = PanelId::from(tab.entity_id());
        self.dock.update(cx, |dock, cx| {
            let target = InsertTarget::Tabs {
                node,
                ix: Some(ix),
                activate: true,
            };
            dock.move_panel(panel, target, window, cx);
            dock.remove_panel(replaced, window, cx);
        });
        tab
    }

    fn position(&self, panel: PanelId, cx: &App) -> (NodeId, usize) {
        let dock = self.dock.read(cx);
        let tree = dock
            .layout(DockPlacement::Center)
            .expect("tabs live in the center");
        let node = tree.find_panel_node(panel).expect("the tab is in the dock");
        match tree.find_node(node).map(|node| node.kind()) {
            Some(PaneRef::Tabs { panels, .. }) => {
                let ix = panels.iter().position(|held| *held == panel);
                (node, ix.expect("the node holds the tab"))
            }
            _ => unreachable!("panels are held by tab nodes"),
        }
    }

    /// Activates the Settings tab, opening it if needed.
    pub fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self
            .shell
            .read(cx)
            .settings_tab()
            .map(|tab| tab.entity_id())
        {
            self.dock
                .update(cx, |dock, cx| dock.select_panel(tab.into(), window, cx));
            return;
        }
        let tab = cx.new(SettingsTab::new);
        self.add_to_dock(panel_handle(tab), window, cx);
    }

    /// Activates the Port forwards tab, opening it if needed.
    pub fn open_forwards(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.shell.read(cx).forwards_tab() {
            self.dock.update(cx, |dock, cx| {
                dock.select_panel(tab.entity_id().into(), window, cx);
            });
            return;
        }
        let model = self.model.clone();
        let tab = cx.new(|cx| ForwardsTab::new(model, cx));
        self.add_to_dock(panel_handle(tab), window, cx);
    }

    /// Activates the About tab, opening it if needed.
    pub fn open_about(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.shell.read(cx).about_tab() {
            self.dock.update(cx, |dock, cx| {
                dock.select_panel(tab.entity_id().into(), window, cx);
            });
            return;
        }
        let tab = cx.new(AboutTab::new);
        self.add_to_dock(panel_handle(tab), window, cx);
    }

    /// Lists what the application offers at this moment. A dialog on screen
    /// keeps the keyboard, so the palette does not open over it.
    pub fn open_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) {
            return;
        }
        let contributions = cx.global::<SharedServices>().0.contributions();
        let selected = self.shell.read(cx).selected();
        let groups = palette_groups(self.model.read(cx), selected, &contributions);
        let state = cx.new(|cx| CommandState::new(window, cx));
        let palette = state.clone();
        let workspace = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let (chosen, workspace) = (groups.clone(), workspace.clone());
            let command = Command::new(&palette)
                .placeholder(t!("palette.search").to_string())
                .empty(|_, _, _| t!("palette.no_matches").to_string())
                .bordered(false)
                .on_confirm(move |index, window, cx| {
                    let action = chosen[index.section].entries[index.row].action.clone();
                    window.close_dialog(cx);
                    let ran = workspace.update(cx, |workspace, cx| {
                        workspace.run_palette_action(action, window, cx);
                    });
                    if let Err(error) = ran {
                        tracing::warn!(%error, "could not run a palette entry");
                    }
                });
            let command = groups.iter().fold(command, |command, group| {
                command.group(group.command_group())
            });
            dialog
                .close_button(false)
                .w(px(PALETTE_WIDTH))
                .child(div().id("command-palette").child(command).test_support())
        });
        state.update(cx, |state, cx| state.focus(window, cx));
    }

    fn run_palette_action(
        &mut self,
        action: PaletteAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            PaletteAction::OpenKind { key, kind } => {
                self.show_kind(key, kind, false, window, cx);
            }
            PaletteAction::RunCommand { extension, command } => {
                let request = ExtensionRequest::RunCommand { command };
                let reply = cx
                    .global::<SharedServices>()
                    .0
                    .extension(&extension, request);
                cx.spawn_in(window, async move |_, cx| {
                    if let Err(error) = reply.await {
                        let message = t!("palette.command_failed", error = error).to_string();
                        tracing::warn!(%extension, %error, "a palette command failed");
                        // The window can close while the command runs.
                        let _ = cx.update(|window, cx| {
                            window.push_notification(Notification::error(message), cx);
                        });
                    }
                })
                .detach();
            }
            PaletteAction::Connect(context) => {
                let model = self.model.read(cx);
                model.connect(&context);
                let key = model.key(&context);
                self.shell.update(cx, |shell, cx| shell.select(key, cx));
            }
            PaletteAction::Disconnect(context) => {
                request_disconnect(&self.model, &context, window, cx);
            }
            PaletteAction::OpenLocalTerminal => {
                self.open_local_terminal(window, cx);
            }
            PaletteAction::OpenSettings => self.open_settings(window, cx),
            PaletteAction::OpenForwards => self.open_forwards(window, cx),
            PaletteAction::OpenAbout => self.open_about(window, cx),
        }
    }

    /// Opens a new tab bound to this cluster. It does not connect.
    pub fn open_tab(
        &mut self,
        key: ClusterKey,
        kind: ResourceKind,
        pinned: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ClusterTab> {
        let model = self.model.clone();
        let tab = cx.new(|cx| {
            let mut tab = ClusterTab::new(key, None, pinned, model, window, cx);
            tab.open_kind(kind, window, cx);
            tab
        });
        self.add_to_dock(panel_handle(tab.clone()), window, cx);
        tab
    }

    fn add_to_dock(
        &mut self,
        panel: Arc<dyn PanelView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dock.update(cx, |dock, cx| {
            if dock.is_empty(DockPlacement::Center, cx) {
                dock.set_center(DockLayout::tabs().panel_view(panel, cx), window, cx);
            } else {
                dock.add_panel_view(panel, DockPlacement::Center, None, window, cx);
            }
        });
    }

    fn close_tab(&mut self, id: EntityId, window: &mut Window, cx: &mut Context<Self>) {
        let shell = self.shell.read(cx);
        let (tab, editor) = (shell.tab(id), shell.editor(id));
        let logs = shell.log_tab(id);
        let terminal = shell.terminal_tab(id);
        let settings = shell.settings_tab().filter(|tab| tab.entity_id() == id);
        let forwards = shell.forwards_tab().filter(|tab| tab.entity_id() == id);
        let about = shell.about_tab().filter(|tab| tab.entity_id() == id);
        if let Some(editor) = &editor
            && editor.read(cx).has_unapplied_changes(cx)
        {
            self.confirm_discard(editor.clone(), window, cx);
            return;
        }
        self.dock.update(cx, |dock, cx| {
            if let Some(tab) = tab {
                dock.remove_panel(tab, window, cx);
            } else if let Some(editor) = editor {
                dock.remove_panel(editor, window, cx);
            } else if let Some(logs) = logs {
                dock.remove_panel(logs, window, cx);
            } else if let Some(terminal) = terminal {
                dock.remove_panel(terminal, window, cx);
            } else if let Some(settings) = settings {
                dock.remove_panel(settings, window, cx);
            } else if let Some(forwards) = forwards {
                dock.remove_panel(forwards, window, cx);
            } else if let Some(about) = about {
                dock.remove_panel(about, window, cx);
            }
        });
    }

    fn confirm_discard(
        &self,
        editor: Entity<ManifestTab>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = editor.read(cx).target().name.clone();
        let dock = self.dock.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let (editor, dock) = (editor.clone(), dock.clone());
            dialog
                .title(t!("manifest.discard_title", name = name).to_string())
                .child(
                    div()
                        .id("discard-confirmation")
                        .child(t!("manifest.discard_message").to_string())
                        .test_support(),
                )
                .footer(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            Button::new("discard-cancel")
                                .label(t!("manifest.keep_editing").to_string())
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("discard-confirm")
                                .label(t!("manifest.discard").to_string())
                                .on_click(move |_, window, cx| {
                                    let editor = editor.clone();
                                    dock.update(cx, |dock, cx| {
                                        dock.remove_panel(editor, window, cx);
                                    });
                                    window.close_dialog(cx);
                                }),
                        ),
                )
        });
    }

    /// Opens a manifest tab. The user's own edits of one object share a tab, and
    /// each extension request gets its own, so its reply stays with its review.
    pub fn open_editor(
        &mut self,
        key: ClusterKey,
        target: ObjectTarget,
        request: ManifestRequest,
        reply: Option<oneshot::Sender<ChangeOutcome>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ManifestTab> {
        if request == ManifestRequest::Edit
            && let Some(open) = self.shell.read(cx).editor_of(&key, &target)
        {
            let panel = PanelId::from(open.entity_id());
            self.dock
                .update(cx, |dock, cx| dock.select_panel(panel, window, cx));
            return open;
        }
        let model = self.model.clone();
        let tab = cx.new(|cx| ManifestTab::new(key, target, request, reply, model, window, cx));
        self.add_to_dock(panel_handle(tab.clone()), window, cx);
        tab
    }

    /// Opens a pod's logs, in the tab that already shows them if there is one.
    pub fn open_logs(
        &mut self,
        key: ClusterKey,
        target: ObjectTarget,
        container: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<LogTab> {
        if let Some(open) = self.shell.read(cx).log_tab_of(&key, &target) {
            let panel = PanelId::from(open.entity_id());
            self.dock
                .update(cx, |dock, cx| dock.select_panel(panel, window, cx));
            return open;
        }
        let model = self.model.clone();
        let tab = cx.new(|cx| LogTab::new(key, target, container, model, window, cx));
        self.add_to_dock(panel_handle(tab.clone()), window, cx);
        tab
    }

    /// Opens a shell. Each gets a tab of its own, because several shells in one
    /// pod are a normal way to work.
    pub fn open_terminal(
        &mut self,
        key: ClusterKey,
        target: ObjectTarget,
        container: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<TerminalTab> {
        let model = self.model.clone();
        let tab = cx.new(|cx| TerminalTab::new(key, target, container, model, window, cx));
        self.add_to_dock(panel_handle(tab.clone()), window, cx);
        tab.update(cx, |tab, cx| tab.focus_grid(window, cx));
        tab
    }

    /// Opens a shell on this machine in a tab of its own.
    pub fn open_local_terminal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<TerminalTab> {
        let tab = cx.new(|cx| TerminalTab::local(window, cx));
        self.add_to_dock(panel_handle(tab.clone()), window, cx);
        tab.update(cx, |tab, cx| tab.focus_grid(window, cx));
        tab
    }

    /// Serves what extensions ask of the interface, for the life of the window.
    pub fn listen(
        &self,
        mut requests: mpsc::UnboundedReceiver<HostRequest>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        cx.spawn_in(window, async move |this, cx| {
            while let Some(request) = requests.recv().await {
                let served = this.update_in(cx, |this, window, cx| {
                    this.serve(request, window, cx);
                });
                if served.is_err() {
                    return;
                }
            }
        })
        .detach();
    }

    fn resolve_pod(
        &self,
        target: ObjectRef,
        not_a_pod: &str,
        cx: &App,
    ) -> Result<(ClusterKey, ObjectTarget), HostError> {
        let ObjectRef {
            cluster,
            kind,
            namespace,
            name,
        } = target;
        let (key, target) = self.resolve(&cluster, &kind, namespace, name, cx)?;
        if target.kind.group.is_empty() && target.kind.kind == "Pod" {
            Ok((key, target))
        } else {
            Err(HostError::Failed {
                message: not_a_pod.to_owned(),
            })
        }
    }

    fn serve(&mut self, request: HostRequest, window: &mut Window, cx: &mut Context<Self>) {
        match request {
            HostRequest::Change { request, reply } => {
                let (cluster, kind, namespace, name, change) = change_target(request);
                match self.resolve(&cluster, &kind, namespace, name, cx) {
                    Ok((key, target)) => {
                        self.open_editor(key, target, change, Some(reply), window, cx);
                    }
                    // The extension may have stopped waiting.
                    Err(error) => {
                        let reason = error.to_string();
                        let _ = reply.send(ChangeOutcome::Failed { reason });
                    }
                }
            }
            HostRequest::OpenLogs {
                target,
                container,
                reply,
            } => {
                let opened =
                    self.resolve_pod(target, "only pods have logs", cx)
                        .map(|(key, target)| {
                            self.open_logs(key, target, container, window, cx);
                        });
                let _ = reply.send(opened);
            }
            HostRequest::OpenTerminal {
                target,
                container,
                reply,
            } => {
                let opened =
                    self.resolve_pod(target, "only pods have shells", cx)
                        .map(|(key, target)| {
                            self.open_terminal(key, target, container, window, cx);
                        });
                let _ = reply.send(opened);
            }
            HostRequest::OpenEditor { target, reply } => {
                let ObjectRef {
                    cluster,
                    kind,
                    namespace,
                    name,
                } = target;
                let resolved = self.resolve(&cluster, &kind, namespace, name, cx);
                let opened = resolved.map(|(key, target)| {
                    self.open_editor(key, target, ManifestRequest::Edit, None, window, cx);
                });
                let _ = reply.send(opened);
            }
        }
    }

    fn resolve(
        &self,
        cluster: &ClusterRef,
        kind: &KindRef,
        namespace: Option<String>,
        name: String,
        cx: &App,
    ) -> Result<(ClusterKey, ObjectTarget), HostError> {
        let model = self.model.read(cx);
        let status = model
            .status(&cluster.0)
            .filter(|status| is_live(&status.state))
            .ok_or_else(|| HostError::Failed {
                message: format!("cluster {} is not connected", cluster.0),
            })?;
        let served = status
            .resources
            .iter()
            .find(|served| kind_ref(served) == *kind);
        let kind = served.cloned().ok_or_else(|| HostError::NotFound {
            message: format!("the cluster does not serve {}", kind.kind),
        })?;
        let target = ObjectTarget {
            kind,
            namespace,
            name,
        };
        Ok((model.key(&cluster.0), target))
    }

    /// The window's value, because the application-wide one is unreliable on
    /// Linux.
    fn apply_theme(&self, window: &Window, cx: &mut App) {
        let preferences = self.preferences.read(cx).get().clone();
        let themes = self.themes.clone();
        themes.update(cx, |themes, cx| {
            theme::apply(&preferences, themes, window.appearance(), cx);
        });
    }

    fn save_identities(&self, model: &Entity<ClusterModel>, cx: &mut Context<Self>) {
        let Some(path) = self.paths.as_ref().map(|paths| paths.clusters.clone()) else {
            return;
        };
        let json = model.read(cx).identities().to_json();
        cx.spawn(async move |_, cx| save_in_background(path, json, cx))
            .detach();
    }

    fn save_preferences(&self, cx: &mut Context<Self>) {
        let Some(path) = self.paths.as_ref().map(|paths| paths.preferences.clone()) else {
            return;
        };
        let json = self.preferences.read(cx).get().to_json();
        cx.spawn(async move |_, cx| save_in_background(path, json, cx))
            .detach();
    }
}

/// An application action, because a shortcut must work with nothing in the
/// workspace focused.
fn on_shortcut<A: Action>(
    run: fn(&mut Workspace, &mut Window, &mut Context<Workspace>),
    window: &Window,
    cx: &mut Context<Workspace>,
) {
    let workspace = cx.weak_entity();
    let handle = window.window_handle();
    App::on_action(cx, move |_: &A, cx| {
        let workspace = workspace.clone();
        // The window is busy dispatching this action until it returns.
        cx.defer(move |cx| {
            let ran = handle.update(cx, |_, window, cx| {
                workspace.update(cx, |workspace, cx| run(workspace, window, cx))
            });
            if let Err(error) = ran.and_then(|result| result) {
                tracing::warn!(%error, "could not run a shortcut");
            }
        });
    });
}

fn save_in_background(path: PathBuf, json: serde_json::Result<String>, cx: &AsyncApp) {
    let write = async move {
        let result = json.map_err(std::io::Error::other);
        if let Err(error) = result.and_then(|text| write_atomic(&path, &text)) {
            tracing::warn!(path = %path.display(), %error, "could not save settings");
        }
    };
    cx.background_executor().spawn(write).detach();
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let empty = self.dock.read(cx).is_empty(DockPlacement::Center, cx);
        let columns = div()
            .flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(self.rail.clone())
            .child(self.sidebar.clone())
            .child(
                div()
                    .relative()
                    .flex_1()
                    .h_full()
                    .child(self.dock.clone())
                    .when(empty, |this| {
                        this.child(
                            div()
                                .id("no-tabs-hint")
                                .absolute()
                                .top_0()
                                .left_0()
                                .size_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_color(cx.theme().muted_foreground)
                                .child(t!("workspace.no_tabs").to_string())
                                .test_support(),
                        )
                    }),
            );
        div()
            .id("workspace")
            .size_full()
            .flex()
            .flex_col()
            .child(columns)
            .child(self.status_bar.clone())
    }
}

fn change_target(
    request: ChangeRequest,
) -> (ClusterRef, KindRef, Option<String>, String, ManifestRequest) {
    let metadata_text = |manifest: &serde_json::Value, field: &str| {
        manifest["metadata"][field].as_str().map(str::to_owned)
    };
    match request {
        ChangeRequest::Create {
            cluster,
            kind,
            manifest,
        } => (
            cluster,
            kind,
            metadata_text(&manifest, "namespace"),
            metadata_text(&manifest, "name").unwrap_or_default(),
            ManifestRequest::Create(manifest),
        ),
        ChangeRequest::Update { target, manifest } => (
            target.cluster,
            target.kind,
            target.namespace,
            target.name,
            ManifestRequest::Update(manifest),
        ),
        ChangeRequest::Delete { target } => (
            target.cluster,
            target.kind,
            target.namespace,
            target.name,
            ManifestRequest::Delete,
        ),
    }
}
