# Design

## Context

See `proposal.md` for motivation and `specs/` for required behavior.

`Workspace` renders `ClusterPanel` beside a `DockArea`. `ClusterTab` holds its kind navigation, its `ResourceBrowser`, and its `DetailView`. `navigation()` in `columns.rs` already builds the groups, in contribution order, which is load order, so the built-in groups need no contract change.

Three facts shape the design:

- The dock rebuilds restored tabs through a registered factory that has no reference to `Workspace`, which is why `SharedClusterModel` exists as a global.
- The dock reports the active tab to each panel through `Panel::set_active`, delivered on the next tick. `DockEvent` has only `LayoutChanged` and `DragDrop`.
- `add_panel_view` always appends to the first tab group, but `move_panel` with `InsertTarget::Tabs { node, ix, activate }` can place a panel exactly, and `select_panel` activates one by id.

## Goals / Non-Goals

**Goals:**

- Keep every change inside `kubepulse-ui`, with no contract or dependency change.
- Make the ordering, initials, grouping, and preview decisions pure functions, so that most behavior is tested without a window.

**Non-Goals:**

- Keyboard navigation of the rail and sidebar. The command palette (task 8.1 of `define-app-architecture`) is the keyboard route to these actions.
- Resizing or hiding the sidebar, and manual reordering or pinning of clusters in the rail.
- Remembering collapsed groups or the selected cluster across restarts. Selection comes back through the restored active tab.
- Preview behavior for detail views, logs, shells, and editors, which do not exist as separate tabs yet.

## Decisions

### D1. `ShellState` holds selection and the tab directory

A new entity, shared through a `SharedShellState` global like the cluster model, holds the selected `ClusterKey`, the active tab, and a summary and weak handle for every `ClusterTab`. Tabs push their summary, because a tab calling into the directory cannot be read back while it is being updated. The rail, sidebar, tabs, and workspace all read it.

Tabs register themselves in `Panel::on_added_to`, unregister in `Panel::on_removed`, and report activation from `Panel::set_active`. This works the same for tabs the user opens and tabs the dock restores.

Selection is keyed by `ClusterKey`, so a context name reused on another server does not match a stale tab.

*Alternative:* walking the dock's layout dump to find tabs. A dump yields saved state rather than entities, and the workspace would still need another route to learn the active tab.

### D2. Act on each click, and let the second click add to the first

Handlers read `ClickEvent::click_count()`. The first click selects, or shows the preview. A count of exactly 2 then connects, or pins. Nothing waits to see whether a second click follows, so single clicks feel instant, and a double-click's first click does only harmless work.

*Alternative:* delaying the single-click action by the double-click interval. It would slow all browsing to tell the two apart.

### D3. A pure decision for where a kind goes

`place_kind(tabs, active, wanted, intent) -> Placement` is a pure function over a plain summary of the tabs. It returns one of: activate an existing tab, show in the preview tab, replace across clusters, or open a new tab. `Workspace` carries out the result. The rules in `shell/workspace` map directly to its unit tests.

When several tabs match, it prefers the active tab, then a pinned tab, then the first in the directory.

### D4. Replacing the preview tab

- **Same cluster:** call `ClusterTab::open_kind` on the existing tab. The tab keeps its position and pane with no dock operation, and `open_kind` already drops the detail view and starts a new browser with an empty filter.
- **Other cluster:** a tab's binding cannot change, so add a new tab, move it with `move_panel` to the old tab's node and index, then remove the old tab.

*Alternative:* always closing and reopening, even for the same cluster. It is one code path, but it needs the dock work on every browse click and causes a visible tab change.

### D5. Pinned state lives on the tab

`ClusterTabState` gains `pinned`, with a serde default of `true`, so older layouts load pinned and `LAYOUT_VERSION` does not change. `ShellState` enforces the single preview: registering a preview tab while one exists pins the newcomer.

`ShellState` emits a change event when a tab's kind or pinned state changes, and `Workspace` schedules a layout save. Today a kind change alone does not save, because `DockEvent::LayoutChanged` does not fire for it.

Double-clicking a title is handled by a wrapper inside `Panel::title`. It must not take the press that starts a tab drag, and task 2.3 verifies this.

### D6. Rail rendering

The rail stays a `uniform_list`, so 500 contexts cost only the visible icons. Two pure functions drive it, both unit-tested:

- `rail_order(clusters)`: a stable partition, connected first. Degraded counts as connected, because the session is alive. Connecting, authenticating, failed, and disconnected stay in the lower set, so an icon moves only once when the connection succeeds.
- `initials(name)`: the first character of the first two alphanumeric segments, upper-cased, so `prod-eu` and `prod-us` become `PE` and `PU` rather than both `PR`. A single-segment name uses its first two characters.

Connected icons are filled with the identity color, and disconnected icons are an outline with muted text. Badges from the kit's icon set mark failed, degraded, and protected clusters, and a spinner marks an attempt in progress. The kit's `Tooltip` and `ContextMenuExt` supply hover and the right-click menu. Element ids use the context name rather than the row index, so an open menu survives a reorder.

Cancel, Disconnect, and the double-click are all built on existing commands: `ClusterCommand::Disconnect` already cancels an attempt.

### D7. Dialogs and popovers replace panel regions

The disconnect warning and settings form move from inline panel regions to the kit's dialog, reusing the existing logic and the `InputState` for the name. `open_window` already wraps content in the kit's root, which dialogs need. Search and kubeconfig errors use the kit's `Popover`, and `matching()` moves unchanged from `cluster_panel.rs`.

### D8. The sidebar renders its own rows

The kit's `SidebarMenu` derives element ids from row position, and it keeps open state per row. The spec needs collapsed groups to survive cluster switches and newly installed definitions, and the tests rely on the stable `kind-<plural>.<group>` ids. So `KindSidebar` renders plain rows, keeps a set of collapsed group names itself, and uses `ContextMenuExt` for the row menu.

*Alternative:* the kit's `SidebarMenu`, which the reuse rule prefers. Inserting a kind above a group would shift the ids and with them the open state.

`NavigationGroup` gains nested sections for undeclared kinds, keyed by API group, with the core group labelled `core`. Sorting within a section happens in `navigation()`.

## Risks / Trade-offs

- **Clicking through kinds opens many watches** → Each preview opens a shared watch, and each stays for the 5-minute release delay, so browsing every kind on a large cluster holds more data than before. The store's reference counting and release delay already bound this. Task 6.4 measures it against the memory limits.
- **Connected-first order moves an icon as the user reaches for it** → An icon moves only when a connection succeeds, and the rail scrolls to the selected icon after a move.
- **Preview content is lost on replace** → This is what a preview tab means, but a filter typed into a preview tab disappears on the next click. The title's italics signal it, and the user pins with a double-click. Making interaction pin the tab is a small later change to `place_kind` callers.
- **`set_active` arrives a tick late** → A kind clicked in that window could use a stale active tab. The only affected outcome is the tie-break in D3, and the next render corrects the sidebar.
- **Right-click-only actions are hard to discover and need a mouse** → The tooltip states the connection state, the sidebar offers connect and retry, and the command palette adds keyboard access later.
- **Two rail icons can share initials and color** → The tooltip gives the full name, and the settings dialog can change the name or color.
- **The kit's title and drag handling may not allow a double-click on a tab title** → If task 2.3 fails, keep the sidebar double-click and add a pin action to the panel's dropdown menu, then update the spec.

## Migration Plan

`ClusterPanel` and its region are removed in the same commit that adds the rail and sidebar. Saved layouts stay readable through the serde default in D5. Rollback is a revert, and a layout saved with `pinned` still loads on the older build, because serde ignores unknown fields.
