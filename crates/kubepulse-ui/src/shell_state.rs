//! What the rail, sidebar, tabs, and workspace share: the selected cluster, the
//! active tab, and a directory of open tabs. Tabs have no reference to the
//! workspace, so they report here themselves.

use gpui_kit::{
    assets::IconName,
    component::{
        Sizable as _,
        button::{Button, ButtonVariants as _},
    },
    *,
};
use kubepulse_cluster::ObjectTarget;
use kubepulse_extension_api::KindRef;
use rust_i18n::t;

use crate::{
    about_tab::AboutTab, cluster_tab::ClusterTab, forwards::ForwardsTab, identity::ClusterKey,
    log_tab::LogTab, manifest_tab::ManifestTab, settings_tab::SettingsTab,
    terminal_tab::TerminalTab,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabSummary {
    pub id: EntityId,
    pub key: ClusterKey,
    pub kind: Option<KindRef>,
    pub pinned: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    Activate(EntityId),
    /// The preview tab is on the same cluster, so it can change kind.
    Replace(EntityId),
    /// The preview tab is on another cluster, and a tab cannot change cluster.
    ReplaceAcross(EntityId),
    Open,
}

pub fn place_kind(
    tabs: &[TabSummary],
    active: Option<EntityId>,
    key: &ClusterKey,
    kind: &KindRef,
) -> Placement {
    let showing: Vec<&TabSummary> = tabs
        .iter()
        .filter(|tab| tab.key == *key && tab.kind.as_ref() == Some(kind))
        .collect();
    let existing = showing
        .iter()
        .find(|tab| Some(tab.id) == active)
        .or_else(|| showing.iter().find(|tab| tab.pinned))
        .or_else(|| showing.first());
    if let Some(tab) = existing {
        return Placement::Activate(tab.id);
    }
    match tabs.iter().find(|tab| !tab.pinned) {
        Some(preview) if preview.key == *key => Placement::Replace(preview.id),
        Some(preview) => Placement::ReplaceAcross(preview.id),
        None => Placement::Open,
    }
}

struct Entry {
    summary: TabSummary,
    tab: WeakEntity<ClusterTab>,
}

struct EditorEntry {
    id: EntityId,
    key: ClusterKey,
    target: ObjectTarget,
    tab: WeakEntity<ManifestTab>,
}

struct LogEntry {
    id: EntityId,
    key: ClusterKey,
    target: ObjectTarget,
    tab: WeakEntity<LogTab>,
}

pub enum ShellEvent {
    /// Only the workspace can reach the dock, so tabs ask it to close them.
    CloseTab(EntityId),
    OpenLogs {
        key: ClusterKey,
        target: Box<ObjectTarget>,
    },
    OpenTerminal {
        key: ClusterKey,
        target: Box<ObjectTarget>,
    },
    OpenEditor {
        key: ClusterKey,
        target: Box<ObjectTarget>,
    },
}

impl EventEmitter<ShellEvent> for ShellState {}

#[derive(Default)]
pub struct ShellState {
    selected: Option<ClusterKey>,
    active: Option<EntityId>,
    tabs: Vec<Entry>,
    /// Kept apart from `tabs`, because the Settings tab has no cluster or kind.
    settings_tab: Option<WeakEntity<SettingsTab>>,
    forwards_tab: Option<WeakEntity<ForwardsTab>>,
    about_tab: Option<WeakEntity<AboutTab>>,
    editors: Vec<EditorEntry>,
    log_tabs: Vec<LogEntry>,
    /// One pod can have several shells, so these are found by tab alone.
    terminal_tabs: Vec<(EntityId, WeakEntity<TerminalTab>)>,
}

/// Lets tabs find the shell state.
pub struct SharedShellState(pub Entity<ShellState>);

impl Global for SharedShellState {}

impl ShellState {
    pub fn selected(&self) -> Option<&ClusterKey> {
        self.selected.as_ref()
    }

    pub fn select(&mut self, key: ClusterKey, cx: &mut Context<Self>) {
        self.selected = Some(key);
        cx.notify();
    }

    pub fn active(&self) -> Option<EntityId> {
        self.active
    }

    pub fn settings_tab(&self) -> Option<Entity<SettingsTab>> {
        self.settings_tab.as_ref().and_then(WeakEntity::upgrade)
    }

    pub fn set_settings_tab(&mut self, tab: Option<WeakEntity<SettingsTab>>) {
        self.settings_tab = tab;
    }

    pub fn forwards_tab(&self) -> Option<Entity<ForwardsTab>> {
        self.forwards_tab.as_ref().and_then(WeakEntity::upgrade)
    }

    pub fn set_forwards_tab(&mut self, tab: Option<WeakEntity<ForwardsTab>>) {
        self.forwards_tab = tab;
    }

    pub fn about_tab(&self) -> Option<Entity<AboutTab>> {
        self.about_tab.as_ref().and_then(WeakEntity::upgrade)
    }

    pub fn set_about_tab(&mut self, tab: Option<WeakEntity<AboutTab>>) {
        self.about_tab = tab;
    }

    pub fn add_editor(
        &mut self,
        id: EntityId,
        key: ClusterKey,
        target: ObjectTarget,
        tab: WeakEntity<ManifestTab>,
    ) {
        if !self.editors.iter().any(|entry| entry.id == id) {
            self.editors.push(EditorEntry {
                id,
                key,
                target,
                tab,
            });
        }
    }

    pub fn editor(&self, id: EntityId) -> Option<Entity<ManifestTab>> {
        let entry = self.editors.iter().find(|entry| entry.id == id)?;
        entry.tab.upgrade()
    }

    pub fn editor_of(
        &self,
        key: &ClusterKey,
        target: &ObjectTarget,
    ) -> Option<Entity<ManifestTab>> {
        let entry = self
            .editors
            .iter()
            .find(|entry| entry.key == *key && entry.target == *target)?;
        entry.tab.upgrade()
    }

    pub fn open_editor(&self, key: ClusterKey, target: ObjectTarget, cx: &mut Context<Self>) {
        let target = Box::new(target);
        cx.emit(ShellEvent::OpenEditor { key, target });
    }

    pub fn add_log_tab(
        &mut self,
        id: EntityId,
        key: ClusterKey,
        target: ObjectTarget,
        tab: WeakEntity<LogTab>,
    ) {
        if !self.log_tabs.iter().any(|entry| entry.id == id) {
            self.log_tabs.push(LogEntry {
                id,
                key,
                target,
                tab,
            });
        }
    }

    pub fn log_tab(&self, id: EntityId) -> Option<Entity<LogTab>> {
        let entry = self.log_tabs.iter().find(|entry| entry.id == id)?;
        entry.tab.upgrade()
    }

    pub fn log_tab_of(&self, key: &ClusterKey, target: &ObjectTarget) -> Option<Entity<LogTab>> {
        let entry = self
            .log_tabs
            .iter()
            .find(|entry| entry.key == *key && entry.target == *target)?;
        entry.tab.upgrade()
    }

    pub fn open_logs(&self, key: ClusterKey, target: ObjectTarget, cx: &mut Context<Self>) {
        let target = Box::new(target);
        cx.emit(ShellEvent::OpenLogs { key, target });
    }

    pub fn add_terminal_tab(&mut self, id: EntityId, tab: WeakEntity<TerminalTab>) {
        if !self.terminal_tabs.iter().any(|(known, _)| *known == id) {
            self.terminal_tabs.push((id, tab));
        }
    }

    pub fn terminal_tab(&self, id: EntityId) -> Option<Entity<TerminalTab>> {
        let (_, tab) = self.terminal_tabs.iter().find(|(known, _)| *known == id)?;
        tab.upgrade()
    }

    pub fn open_terminal(&self, key: ClusterKey, target: ObjectTarget, cx: &mut Context<Self>) {
        let target = Box::new(target);
        cx.emit(ShellEvent::OpenTerminal { key, target });
    }

    pub fn active_tab(&self) -> Option<&TabSummary> {
        self.tabs
            .iter()
            .map(|entry| &entry.summary)
            .find(|tab| Some(tab.id) == self.active)
    }

    pub fn summaries(&self) -> Vec<TabSummary> {
        self.tabs.iter().map(|e| e.summary.clone()).collect()
    }

    pub fn tab(&self, id: EntityId) -> Option<Entity<ClusterTab>> {
        self.tabs
            .iter()
            .find(|entry| entry.summary.id == id)
            .and_then(|entry| entry.tab.upgrade())
    }

    /// Returns whether the tab is pinned, which it becomes if a preview tab
    /// already exists. Moving a tab between panes adds it again.
    pub fn add_tab(
        &mut self,
        mut summary: TabSummary,
        tab: WeakEntity<ClusterTab>,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(entry) = self.tabs.iter().find(|e| e.summary.id == summary.id) {
            return entry.summary.pinned;
        }
        summary.pinned |= self.tabs.iter().any(|entry| !entry.summary.pinned);
        let pinned = summary.pinned;
        self.tabs.push(Entry { summary, tab });
        cx.notify();
        pinned
    }

    pub fn update_tab(&mut self, summary: TabSummary, cx: &mut Context<Self>) {
        if let Some(entry) = self.tabs.iter_mut().find(|e| e.summary.id == summary.id)
            && entry.summary != summary
        {
            entry.summary = summary;
            cx.notify();
        }
    }

    pub fn request_close(&self, id: EntityId, cx: &mut Context<Self>) {
        cx.emit(ShellEvent::CloseTab(id));
    }

    pub fn remove_tab(&mut self, id: EntityId, cx: &mut Context<Self>) {
        self.tabs.retain(|entry| entry.summary.id != id);
        self.editors.retain(|entry| entry.id != id);
        self.log_tabs.retain(|entry| entry.id != id);
        self.terminal_tabs.retain(|(known, _)| *known != id);
        if self.active == Some(id) {
            self.active = None;
        }
        cx.notify();
    }

    /// Selects `key` too, when the tab's cluster can still be selected.
    pub fn activate(&mut self, id: EntityId, key: Option<ClusterKey>, cx: &mut Context<Self>) {
        self.active = Some(id);
        if key.is_some() {
            self.selected = key;
        }
        cx.notify();
    }
}

/// Closes tab `id`. It stops the press too, so the tab is not activated first.
pub fn close_button(id: EntityId, shell: Entity<ShellState>) -> impl IntoElement {
    div()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            Button::new(SharedString::from(format!("tab-close-{id}")))
                .icon(IconName::X)
                .ghost()
                .xsmall()
                .tab_stop(false)
                .tooltip(t!("tab.close").to_string())
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    shell.update(cx, |shell, cx| shell.request_close(id, cx));
                }),
        )
}

/// The click listener that closes tab `id` from its title on a middle click.
pub fn close_on_middle_click(
    id: EntityId,
    shell: Entity<ShellState>,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |event, _, cx| {
        if event.is_middle_click() {
            shell.update(cx, |shell, cx| shell.request_close(id, cx));
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::EntityId;
    use kubepulse_extension_api::KindRef;

    use super::{ClusterKey, Placement, TabSummary, place_kind};

    fn tab(id: u64, context: &str, kind: &str, pinned: bool) -> TabSummary {
        TabSummary {
            id: EntityId::from(id),
            key: ClusterKey::new(context, None),
            kind: Some(KindRef::new("", kind)),
            pinned,
        }
    }

    fn place(tabs: &[TabSummary], active: Option<u64>, context: &str, kind: &str) -> Placement {
        place_kind(
            tabs,
            active.map(EntityId::from),
            &ClusterKey::new(context, None),
            &KindRef::new("", kind),
        )
    }

    fn id(id: u64) -> EntityId {
        EntityId::from(id)
    }

    #[test]
    fn with_no_preview_a_new_tab_opens() {
        assert_eq!(place(&[], None, "a", "Pod"), Placement::Open);
        let pinned = [tab(1, "a", "Service", true)];
        assert_eq!(place(&pinned, Some(1), "a", "Pod"), Placement::Open);
    }

    #[test]
    fn a_preview_on_the_same_cluster_is_replaced() {
        let tabs = [tab(1, "a", "Service", true), tab(2, "a", "Secret", false)];
        assert_eq!(
            place(&tabs, Some(1), "a", "Pod"),
            Placement::Replace(id(2)),
            "the preview is used even when another tab is active"
        );
    }

    #[test]
    fn a_preview_on_another_cluster_is_replaced_across() {
        let tabs = [tab(1, "a", "Pod", false)];
        assert_eq!(
            place(&tabs, Some(1), "b", "Pod"),
            Placement::ReplaceAcross(id(1)),
            "the same kind on another cluster is not reused"
        );
    }

    #[test]
    fn a_kind_already_shown_is_activated() {
        let tabs = [tab(1, "a", "Pod", true), tab(2, "a", "Secret", false)];
        assert_eq!(place(&tabs, None, "a", "Pod"), Placement::Activate(id(1)));
        assert_eq!(
            place(&tabs, None, "a", "Secret"),
            Placement::Activate(id(2))
        );
    }

    #[test]
    fn among_several_matches_the_active_tab_then_a_pinned_tab_wins() {
        let tabs = [
            tab(1, "a", "Pod", false),
            tab(2, "a", "Pod", true),
            tab(3, "a", "Pod", true),
        ];
        assert_eq!(
            place(&tabs, Some(3), "a", "Pod"),
            Placement::Activate(id(3))
        );
        assert_eq!(place(&tabs, None, "a", "Pod"), Placement::Activate(id(2)));
    }
}
