# Proposal

## Why

The window has one wide cluster panel beside the dock, and each tab repeats its own kind navigation. The panel takes 320 pixels for a list of clusters, every tab loses another 200 pixels to navigation, and browsing leaves a trail of tabs. Separating cluster choice, kind choice, and content into three columns gives the table more room and makes browsing cheap.

## What Changes

- Replace the cluster panel with a thin, icon-only cluster rail on the far left. It lists every context, shows connection state on each icon, and keeps the connected clusters at the top.
- Single-clicking a rail icon selects the cluster and never connects it. Double-clicking a disconnected or failed cluster connects it.
- Move all cluster actions to the rail's right-click menu: connect, disconnect, cancel, retry, and cluster settings. The disconnect warning and the settings form become dialogs.
- Add a search button to the rail that opens a popover for finding a cluster by name, because an icon column has no room for a search box.
- Add a left sidebar that lists the selected cluster's kinds in collapsible groups. Kinds no extension claims are grouped by API group.
- **BREAKING:** remove kind navigation from inside tabs. Tabs show content only, and the sidebar is the one place to pick a kind.
- Add preview tabs. A single click on a kind shows it in the one preview tab, replacing what that tab held. A double-click pins the content, so the next selection opens a new tab. At most one preview tab exists.
- Keep the selected cluster in step with the active tab, so the sidebar always matches what the user is looking at after switching tabs.
- Show the kind and the cluster in tab titles, and mark the preview tab.

## Capabilities

### New Capabilities

- `shell/cluster-rail`: the icon-only cluster list, including cluster selection, connection state, double-click connect, the right-click menu, and search.
- `shell/kind-navigation`: the sidebar that lists a cluster's kinds in groups, and what it shows for clusters that are not connected.

### Modified Capabilities

- `shell/workspace`: adds the three-column layout, preview and pinned tabs, and tab titles. Existing requirements stay as written. This capability is defined by the in-flight change `define-app-architecture` and is not yet in `openspec/specs/`.

## Impact

- **Code:** all in `kubepulse-ui`. `cluster_panel.rs` is replaced, `workspace.rs` and `cluster_tab.rs` change substantially, and `columns.rs` gains API-group grouping. The extension contract does not change, and no dependency is added.
- **Saved layouts:** tab state gains a pinned flag. Layouts saved without it load with their tabs pinned.
- **Tests:** `tests/shell.rs` and `tests/browsing.rs` drive the panel's buttons and the in-tab navigation, so they move to the rail and sidebar.
- **Dependencies between changes:** this builds on the uncommitted resource-browsing work (group 7) in `define-app-architecture`, and that change must be archived first, because this change adds to its `shell/workspace` spec.
