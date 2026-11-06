# Proposal

## Why

The app never registers an asset source, so every SVG icon draws nothing: the rail's search, settings, and error buttons are blank, as are its state marks, the sidebar's chevrons, and the icons inside kit components. Beyond that bug, the sidebar, tabs, and action buttons are text only, which makes long lists of kinds slow to scan.

Tabs also have no visible way to close. The dock's overflow menu offers Close, but it silently does nothing on the last tab. And every launch brings back the previous session's tabs and panes, when each launch should start clean.

## What Changes

- Register the kit's icon catalog at startup, so existing icons and kit component icons appear.
- Show an icon for each resource kind in the sidebar and in tab titles, with a generic icon for undeclared kinds.
- Add icons beside the labels of common actions, filter inputs, connection states, and protection marks.
- Add an optional `icon` to resource view contributions, so built-in and extension views declare icons the same way. This is contract 1.2, and earlier manifests still load.
- Add a close button to every tab, including the last one in the workspace. Closing a tab never disconnects its cluster.
- **BREAKING:** Stop saving and restoring the workspace. Each launch starts with no tabs, no panes, and no selected cluster. Filters, label selectors, sort order, and namespace selections were already kept only for the session, and this is now required.
- Keep only cluster display settings, preferences, and theme files between launches.
- Warn that Restart now closes every tab, because a language change no longer brings them back.

Not included: icons in table cells, extension-supplied SVGs, icons in declarative UI nodes, icons for detail tabs, actions, or commands, a keyboard shortcut for closing tabs, and a way to reopen closed tabs.

## Capabilities

### New Capabilities
- `shell/icons`: how the application uses icons: rendering, kinds, actions, states, labels, and theme colors.

### Modified Capabilities
- `arch/extension-system`: resource view contributions can declare a host-defined icon. Main specs are still empty, so this is an added requirement, as in `add-themes-and-i18n`.
- `shell/workspace`: tabs can be closed, the workspace starts empty, and `Workspace restore` is removed. Changes the requirements from `define-app-architecture` and `add-three-column-layout`.
- `shell/settings`: the Settings tab no longer returns after a restart. Changes `add-themes-and-i18n`.
- `shell/language`: Restart now no longer restores the layout, and it warns about closing tabs. Changes `add-themes-and-i18n`.

## Impact

- `kubepulse-ui`: startup, `kind_sidebar`, `cluster_tab`, `cluster_rail`, `columns`, `detail`, `settings_tab`, `cluster_settings`, `declarative`, `resource_browser`, `workspace`, `shell_state`, `persistence`, and the restart tests in `tests/shell.rs`.
- `kubepulse-extension-api`: new `ViewIcon` type and contract 1.2.
- `kubepulse-builtin`: an icon for each view.
- `docs/extensions.md`: documents `icon`.
- No new dependencies. The binary grows by about 1 MiB.
- This change must be archived after `define-app-architecture`, `add-three-column-layout`, and `add-themes-and-i18n`, because its deltas change requirements those changes add.
