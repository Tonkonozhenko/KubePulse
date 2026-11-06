# Tasks

## 1. Render icons

- [ ] 1.1 Register `AllAssets` in `app.rs`, and verify in the running app that rail buttons, chevrons, and kit component icons appear
- [ ] 1.2 Switch `kubepulse-ui` to `gpui_kit::assets::IconName`, and replace the `"icons/shield.svg"` string with `IconName::Shield`

## 2. Contract

- [ ] 2.1 Add `ViewIcon` and `ResourceViewContribution.icon`, and bump the contract to 1.2
- [ ] 2.2 Add round-trip tests for a missing icon and an unknown icon name
- [ ] 2.3 Declare the icon for each built-in view in `kubepulse-builtin`
- [ ] 2.4 Document `icon` in `docs/extensions.md`

## 3. Kind icons

- [ ] 3.1 Add `icons.rs` with `view_glyph` and `state_mark`
- [ ] 3.2 Add `icon` to `NavigationEntry` and add `kind_icon`, with unit tests for declared and undeclared kinds
- [ ] 3.3 Show the icon in sidebar entries
- [ ] 3.4 Show the icon in cluster tab titles, and the settings icon in the settings tab title

## 4. Actions and states

- [ ] 4.1 Add icons to connect, disconnect, retry, and cluster context menu actions
- [ ] 4.2 Add icons to delete, reveal, and hide in `detail.rs`
- [ ] 4.3 Add icons to the protection toggle, open folder, restart now, and open in new tab
- [ ] 4.4 Replace the `●` choice marker in `declarative.rs` with a check icon
- [ ] 4.5 Add prefix icons to the filter and label selector inputs, leaving the namespace control alone
- [ ] 4.6 Use `state_mark` in the rail and sidebar header, with colors from `cx.theme()`

## 5. Closing tabs

- [ ] 5.1 Replace `ShellEvent::TabsChanged` with `CloseTab(id)`, add `ShellState::request_close`, store the Settings tab as a `WeakEntity`, and handle the event in `Workspace` with `dock.remove_panel`
- [ ] 5.2 Add the close button to the `ClusterTab` and `SettingsTab` titles, with the `x` glyph and the `tab.close` tooltip, stopping propagation of mouse down and click
- [ ] 5.3 Return `false` from `closable` on both tabs, and verify the dock's overflow menu no longer offers Close
- [ ] 5.4 Add tests for closing the active tab, an inactive tab without activating or pinning it, the last tab, the last tab of a pane, a preview tab, and the Settings tab
- [ ] 5.5 Add tests that closing a cluster's last tab sends no disconnect and keeps it selected, and that the closed tab's entity is released

## 6. Start clean

- [ ] 6.1 Remove layout loading and saving from `Workspace`, keeping `cx.notify()` on `DockEvent::LayoutChanged`, and create the dock with no version
- [ ] 6.2 Remove `ClusterTabState`, `pinned_by_default`, `ClusterTab::register`, and `ClusterTab::dump`, and the `register_panel` call and `dump` in `SettingsTab`
- [ ] 6.3 Remove `ConfigPaths.workspace`, and update the comments in `persistence.rs`, `shell_state.rs`, `cluster_tab.rs`, `settings_tab.rs`, and the `Shared*` globals that mention saved layouts or restart
- [ ] 6.4 Change `settings.restart_warning` to mention closed tabs, and run `python3 scripts/check-i18n.py --write`
- [ ] 6.5 Remove the restore tests and their helpers from `tests/shell.rs`, remove the `ClusterTab::register` calls from the test harnesses, and move helpers that read `dock.dump` to `ShellState::summaries`
- [ ] 6.6 Add tests that a new window has no tabs and no selected cluster with a leftover `workspace.json` present, that changing tabs writes no `workspace.json`, and that a new tab on the same kind has no filter, label selector, or sort
- [ ] 6.7 Confirm `clusters.json` and `settings.json` tests still pass unchanged

## 7. Verify

- [ ] 7.1 Run `cargo test --workspace`, `cargo clippy --workspace --all-targets`, and `cargo fmt --all --check`
- [ ] 7.2 Run `python3 scripts/check-i18n.py` and `python3 scripts/check-crate-graph.py`
- [ ] 7.3 Check icons in the running app in light and dark modes, including long cluster names in tab titles
- [ ] 7.4 Check in the running app that the close button appears and works with one tab, several tabs, and split panes, and that a restart starts empty
- [ ] 7.5 Measure the release binary size before and after, and record it in the commit message
