//! What the rail, sidebar, tabs, and workspace share: the selected cluster, the
//! active tab, and a directory of open tabs. Tabs restored by the dock have no
//! reference to the workspace, so they report here themselves.

use gpui_kit::*;
use kubepulse_extension_api::KindRef;

use crate::{cluster_tab::ClusterTab, identity::ClusterKey};

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

pub enum ShellEvent {
    TabsChanged,
}

impl EventEmitter<ShellEvent> for ShellState {}

#[derive(Default)]
pub struct ShellState {
    selected: Option<ClusterKey>,
    active: Option<EntityId>,
    tabs: Vec<Entry>,
    /// Kept apart from `tabs`, because the Settings tab has no cluster or kind.
    settings_tab: Option<EntityId>,
}

/// Lets panels rebuilt from a saved layout find the shell state.
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

    pub fn settings_tab(&self) -> Option<EntityId> {
        self.settings_tab
    }

    pub fn set_settings_tab(&mut self, tab: Option<EntityId>) {
        self.settings_tab = tab;
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
        cx.emit(ShellEvent::TabsChanged);
        cx.notify();
        pinned
    }

    pub fn update_tab(&mut self, summary: TabSummary, cx: &mut Context<Self>) {
        if let Some(entry) = self.tabs.iter_mut().find(|e| e.summary.id == summary.id)
            && entry.summary != summary
        {
            entry.summary = summary;
            cx.emit(ShellEvent::TabsChanged);
            cx.notify();
        }
    }

    pub fn remove_tab(&mut self, id: EntityId, cx: &mut Context<Self>) {
        self.tabs.retain(|entry| entry.summary.id != id);
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
