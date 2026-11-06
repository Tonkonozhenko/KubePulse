# Tasks

## 1. Render icons

- [x] 1.1 Register `AllAssets` in `app.rs`, and verify in the running app that rail buttons, chevrons, and kit component icons appear
- [x] 1.2 Switch `kubepulse-ui` to `gpui_kit::assets::IconName`, and replace the `"icons/shield.svg"` string with `IconName::Shield`

## 2. Contract

- [x] 2.1 Add `ViewIcon` and `ResourceViewContribution.icon`, and bump the contract to 1.2
- [x] 2.2 Add round-trip tests for a missing icon and an unknown icon name
- [x] 2.3 Declare the icon for each built-in view in `kubepulse-builtin`
- [x] 2.4 Document `icon` in `docs/extensions.md`

## 3. Kind icons

- [x] 3.1 Add `icons.rs` with `view_glyph` and `state_mark`
- [x] 3.2 Add `icon` to `NavigationEntry` and add `kind_icon`, with unit tests for declared and undeclared kinds
- [x] 3.3 Show the icon in sidebar entries
- [x] 3.4 Show the icon in cluster tab titles, and the settings icon in the settings tab title

## 4. Actions and states

- [x] 4.1 Add icons to connect, disconnect, retry, and cluster context menu actions
- [x] 4.2 Add icons to delete, reveal, and hide in `detail.rs`
- [x] 4.3 Add icons to the protection toggle, open folder, restart now, and open in new tab
- [x] 4.4 Replace the `●` choice marker in `declarative.rs` with a check icon
- [x] 4.5 Add prefix icons to the filter and label selector inputs, leaving the namespace control alone
- [x] 4.6 Use `state_mark` in the rail and sidebar header, with colors from `cx.theme()`

## 5. Closing tabs

- [x] 5.1 Replace `ShellEvent::TabsChanged` with `CloseTab(id)`, add `ShellState::request_close`, store the Settings tab as a `WeakEntity`, and handle the event in `Workspace` with `dock.remove_panel`
- [x] 5.2 Add the close button to the `ClusterTab` and `SettingsTab` titles, with the `x` glyph and the `tab.close` tooltip, stopping propagation of mouse down and click
- [ ] 5.3 Return `false` from `closable` on both tabs, and verify the dock's overflow menu no longer offers Close
- [x] 5.4 Add tests for closing the active tab, an inactive tab without activating or pinning it, the last tab, the last tab of a pane, a preview tab, and the Settings tab
- [x] 5.5 Add tests that closing a cluster's last tab sends no disconnect and keeps it selected, and that the closed tab's entity is released

## 6. Start clean

- [x] 6.1 Remove layout loading and saving from `Workspace`, keeping `cx.notify()` on `DockEvent::LayoutChanged`, and create the dock with no version
- [x] 6.2 Remove `ClusterTabState`, `pinned_by_default`, `ClusterTab::register`, and `ClusterTab::dump`, and the `register_panel` call and `dump` in `SettingsTab`, along with the now unused `SharedClusterModel`
- [x] 6.3 Remove `ConfigPaths.workspace`, and update the comments in `persistence.rs`, `shell_state.rs`, `cluster_tab.rs`, `settings_tab.rs`, and the `Shared*` globals that mention saved layouts or restart
- [x] 6.4 Change `settings.restart_warning` to mention closed tabs, and run `python3 scripts/check-i18n.py --write`
- [x] 6.5 Remove the restore tests and their helpers from `tests/shell.rs`, remove the `ClusterTab::register` and `SharedClusterModel` calls from the test harnesses, and move helpers that read `dock.dump` to `ShellState::summaries`
- [x] 6.6 Add tests that a new window has no tabs and no selected cluster with a leftover `workspace.json` present, that changing tabs writes no `workspace.json`, and that a new tab on the same kind has no filter, label selector, or sort
- [x] 6.7 Confirm `clusters.json` and `settings.json` tests still pass unchanged

## 7. Verify

- [x] 7.1 Run `cargo test --workspace`, `cargo clippy --workspace --all-targets`, and `cargo fmt --all --check`
- [x] 7.2 Run `python3 scripts/check-i18n.py` and `python3 scripts/check-crate-graph.py`
- [ ] 7.3 Check icons in the running app in light and dark modes, including long cluster names in tab titles
- [ ] 7.4 Check in the running app that the close button appears and works with one tab, several tabs, and split panes, and that a restart starts empty
- [x] 7.5 Measure the release binary size before and after, and record it in the commit message
