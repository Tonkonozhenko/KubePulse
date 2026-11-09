# Tasks

## 1. Shared shell state

- [x] 1.1 Add `ShellState` and its global from D1 with selection, active tab, and the tab directory, register it in `app.rs` and the test harnesses, and verify with unit tests for selection and for tabs registering and unregistering
- [x] 1.2 Register `ClusterTab` from `on_added_to`, `on_removed`, and `set_active`, and verify with a UI test that activating a tab selects its cluster and selecting a cluster changes no tab
- [x] 1.3 Skip selection for a tab whose context is missing or on another server, and verify with a UI test for each case

## 2. Preview and pinned tabs

- [x] 2.1 Add `pinned` to `ClusterTabState` with the single-preview rule from D5, emit a change event, and save the layout on it, and verify with tests for a restart, a layout with no `pinned` field, and a layout with two previews
- [x] 2.2 Implement `place_kind` from D3, and verify with unit tests for each scenario in `shell/workspace`, including the reuse tie-break and the same kind on another cluster
- [x] 2.3 Show the kind, cluster identity, and italics in `Panel::title`, with double-click to pin, and verify with UI tests that a double-click pins and that dragging a tab still moves it
- [x] 2.4 Implement `Workspace::show_kind`, including the replacement paths in D4, and verify with UI tests that Pods, Services, Deployments use one tab, that the preview's pane is used, and that a cross-cluster replace keeps the position and tab count

## 3. Kind sidebar

- [x] 3.1 Extend `navigation()` with API-group sections, sorted by name, and verify with unit tests in `columns.rs` for two API groups and for empty groups
- [x] 3.2 Build `KindSidebar` with the cluster header, collapsible groups, and the current-kind marker from D8, and verify with UI tests for group order, an empty group, collapsing across a cluster switch, and the marker on and off the active tab's cluster
- [x] 3.3 Connect kind clicks to `show_kind`, with count 2 pinning and a right-click action that opens a new pinned tab, and verify with UI tests for single click, double-click, and two tabs of one kind keeping separate filters
- [x] 3.4 Add the no-cluster, disconnected, attempt, failed, and degraded states with connect and retry actions, and verify with UI tests for each state, including that showing a disconnected cluster sends no command
- [x] 3.5 Verify with a UI test that a kind added while connected appears without changing collapsed groups or the sidebar scroll position, and tick the matching item in `TODO.md`

## 4. Cluster rail

- [x] 4.1 Implement `rail_order` and `initials` from D6, and verify with unit tests for stable order, degraded clusters, and `prod-eu` against `prod-us`
- [x] 4.2 Build `ClusterRail` on `uniform_list` with state rendering, selection marker, protection badge, and tooltips, and verify with UI tests for each connection state and with 500 contexts
- [x] 4.3 Add single-click select and count-2 connect, and verify with UI tests that a single click sends no command, a double-click sends one connect, a failed cluster retries, and a cluster in an attempt or connected sends nothing
- [x] 4.4 Add the context menu with state-dependent actions, and verify with UI tests of each state's items, of Cancel stopping an attempt, and that right-clicking leaves the selection unchanged
- [x] 4.5 Move the disconnect warning and cluster settings into dialogs per D7, and verify by porting `disconnecting_with_active_work_warns_first`, `identity_changes_are_saved`, and `choosing_a_color_applies_to_the_cluster_and_is_saved`
- [x] 4.6 Add the search popover, including scrolling to the chosen icon, and verify by porting `searching_five_hundred_clusters_narrows_the_list` and adding tests for single-click and double-click on a result
- [x] 4.7 Add the kubeconfig warning button, and verify with UI tests for a failing file and for none

## 5. Three-column layout

- [x] 5.1 Render the rail, sidebar, and dock in `Workspace`, add the no-tabs hint, and delete `cluster_panel.rs`, and verify with UI tests for the column order and for the rail and sidebar staying after every tab closes
- [x] 5.2 Remove the navigation from `ClusterTab`, keeping its connect action, stale notice, and server-changed notice, and verify that `tests/shell.rs` covers tabs surviving disconnection and restored tabs through the rail and sidebar
- [x] 5.3 Move `tests/browsing.rs` and the remaining `tests/shell.rs` tests to the sidebar, and verify `cargo test --workspace` passes
- [x] 5.4 Update `docs/extensions.md` to describe `category` and its place in the sidebar, and `docs/architecture/cluster-sessions.md` for the rail, and verify the documented behavior matches the UI tests

## 6. Checks

- [x] 6.1 Run `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, `python3 scripts/check-crate-graph.py`, and `openspec validate add-three-column-layout --strict`, and verify each passes
- [x] 6.2 Run the startup check, and verify the window still appears within 1 second with unreachable clusters
- [x] 6.3 Launch the app against a real cluster, and verify the rail, sidebar, preview, pin, double-click connect, and right-click menu work on Linux
- [x] 6.4 Measure memory after clicking through every kind on a 10k-pod synthetic cluster, and verify it stays within the limits in `general/resource-browsing`, or record the result and a mitigation in `design.md`

## Workflow follow-up

- Archive `define-app-architecture` before archiving this change, because this change adds to its `shell/workspace` spec.
- Repeat task 6.3 on macOS and Windows when those builds run.
