# Proposal

## Why

The app never registers an asset source, so every SVG icon draws nothing: the rail's search, settings, and error buttons are blank, as are its state marks, the sidebar's chevrons, and the icons inside kit components. Beyond that bug, the sidebar, tabs, and action buttons are text only, which makes long lists of kinds slow to scan.

## What Changes

- Register the kit's icon catalog at startup, so existing icons and kit component icons appear.
- Show an icon for each resource kind in the sidebar and in tab titles, with a generic icon for undeclared kinds.
- Add icons beside the labels of common actions, filter inputs, connection states, and protection marks.
- Add an optional `icon` to resource view contributions, so built-in and extension views declare icons the same way. This is contract 1.2, and earlier manifests still load.

Not included: icons in table cells, extension-supplied SVGs, icons in declarative UI nodes, and icons for detail tabs, actions, or commands.

## Capabilities

### New Capabilities
- `shell/icons`: how the application uses icons: rendering, kinds, actions, states, labels, and theme colors.

### Modified Capabilities
- `arch/extension-system`: resource view contributions can declare a host-defined icon. Main specs are still empty, so this is an added requirement, as in `add-themes-and-i18n`.

## Impact

- `kubepulse-ui`: startup, `kind_sidebar`, `cluster_tab`, `cluster_rail`, `columns`, `detail`, `settings_tab`, `cluster_settings`, `declarative`, and `resource_browser`.
- `kubepulse-extension-api`: new `ViewIcon` type and contract 1.2.
- `kubepulse-builtin`: an icon for each view.
- `docs/extensions.md`: documents `icon`.
- No new dependencies. The binary grows by about 1 MiB.
