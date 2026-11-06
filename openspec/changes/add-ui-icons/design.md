# Design

## Context

`gpui-kit` 0.9.0 is already a dependency and enables its `assets` feature by default. It includes 1,830 icons (Lucide 1.43.0 plus 12 of its own), licensed ISC, MIT, and Apache-2.0, all allowed by `deny.toml`. The kit offers three ways to register them:

| Source | Contents | Binary cost |
|---|---|---|
| `Assets` | 101 component icons | Baseline |
| `icon_assets!` + `Assets` | Listed extras, through a wrapper source | About 19 KiB for 10 icons |
| `AllAssets` | Full catalog | About 1.02 MiB |

`app.rs` calls no `with_assets`, so GPUI uses the empty `()` source. It returns `Ok(None)` and `paint_svg` draws nothing, with no error. The kit's guidance is `Assets` for component icons, `icon_assets!` for a few extras, and `AllAssets` when most of the catalog is needed. Its docs also say icons take the text color.

The kit's dock can draw a close button through `DockSkin::set_close_button_visible`, but that button and the `ClosePanel` action both go through `TabGroup::close_panel`, which refuses the last visible panel of a lone group. `DockArea::remove_panel` has no such gate, and `the_columns_stay_after_every_tab_closes` already removes the last tab with it. The kit's button also has no tooltip.

What the application stores today, all in the configuration directory:

| File | Contents | After this change |
|---|---|---|
| `workspace.json` | Dock tree: panes, splits, tab order, active tab; each tab's cluster, kind, and pinned flag; the Settings tab | Removed |
| `clusters.json` | Display name, color, and protected flag per cluster | Kept |
| `settings.json` | Appearance, light theme, dark theme, language | Kept |
| `themes/` | The user's theme files, which the application only reads | Kept |

Filters, label selectors, sort order, scroll position, namespace selections, collapsed sidebar groups, the selected cluster, and window size and position are never written to disk. Logs go to stderr.

## Goals / Non-Goals

**Goals:** make icons render; make kinds, actions, and states easier to scan; keep built-ins on the extension contract; let every tab close; start each launch clean.

**Non-Goals:** table cell icons, extension-supplied SVGs, declarative UI icons, and icons for detail tabs, actions, or commands. Each can be added later as a minor contract change where needed. Also out: a close-tab shortcut, a closed-tab history, and a confirmation on close.

## Decisions

### Register `AllAssets`
Call `gpui_kit::application().with_assets(AllAssets)` and use `gpui_kit::assets::IconName` throughout `kubepulse-ui`. This departs from the kit's advice, because this change's bug is an icon that silently draws nothing. With `AllAssets`, any `IconName` that compiles resolves. A selected list needs a wrapper source and can compile with an unlisted icon, which fails only at runtime. The specs found no binary size budget, and the README notes that embedded bytes are static, so the cost is on disk. Replace the `"icons/shield.svg"` string with `IconName::Shield`.

Alternative: `Assets` with about 30 extras through `icon_assets!`, saving about 1 MiB. This is the fallback if binary size becomes a concern, and it would need a test that finds unlisted icons.

### A host-defined `ViewIcon` in the contract
Add `#[serde(default)] pub icon: ViewIcon` to `ResourceViewContribution`, and bump the contract to 1.2. `ViewIcon` is a kebab-case enum whose `Generic` variant is the default and the `#[serde(other)]` catch-all, so no icon and an unknown icon behave the same. Serde requires that variant to be declared last. Keeping names host-defined means catalog renames cannot break the contract. Only `kubepulse-ui` maps `ViewIcon` to `IconName`, in one exhaustive `match`, so the contract stays free of toolkit types. An SVG field was rejected because it adds untrusted image data for no current need.

| `ViewIcon` | Glyph | Built-in kinds |
|---|---|---|
| `generic` | `shapes` | Undeclared kinds |
| `box` | `box` | Pod |
| `layers` | `layers` | Deployment |
| `copy` | `copy` | ReplicaSet |
| `database` | `database` | StatefulSet |
| `server` | `server-cog` | DaemonSet |
| `timer` | `timer` | Job |
| `schedule` | `calendar-clock` | CronJob |
| `network` | `network` | Service |
| `globe` | `globe` | Ingress |
| `shield` | `shield-check` | NetworkPolicy |
| `waypoints` | `waypoints` | EndpointSlice |
| `settings-file` | `file-sliders` | ConfigMap |
| `key` | `key-round` | Secret |
| `disk` | `hard-drive` | PersistentVolume, PersistentVolumeClaim |
| `storage-class` | `layers-2` | StorageClass |
| `account` | `user-cog` | ServiceAccount |
| `role` | `id-card` | Role, ClusterRole |
| `link` | `link` | RoleBinding, ClusterRoleBinding |

### Kind icons flow through existing lookups
`NavigationEntry` gains `icon: ViewIcon`, and a `kind_icon` function sits beside `kind_title`. Group headers keep only their chevrons, to limit visual noise.

### Action and state glyphs
Icons sit in one `icons.rs` in `kubepulse-ui`, with no wrapper type: `view_glyph` for `ViewIcon` and `state_mark` for `ConnectionState`.

| Use | Glyph |
|---|---|
| Connect / Disconnect | `plug` / `unplug` |
| Retry, Restart now | `rotate-cw` |
| Delete | `trash` |
| Reveal / Hide | `eye` / `eye-off` |
| Protect / Remove protection | `shield` / `shield-off` |
| Open folder | `folder-open` |
| Open in new tab | `external-link` |
| Filter / Label selector | `funnel` / `tag` |
| Connected / Disconnected | `circle-check` / `unplug` |
| Degraded / Failed | `triangle-alert` / `circle-x` |
| Connecting, authenticating | Kit `Spinner` |
| Close tab | `x` |

The tab title's "(protected)" text stays, because labels are not replaced. Status colors come from `cx.theme()`.

### A host-owned close button
`ClusterTab` and `SettingsTab` each draw an extra-small ghost icon button at the end of `title()`, with the tooltip `tab.close`. Its click calls `ShellState::request_close(id)`, which emits `ShellEvent::CloseTab(id)`. `Workspace` handles that event by calling `dock.remove_panel`, so tabs still hold no reference to the dock. `ShellEvent::TabsChanged` goes away, because only layout saving listened to it.

`remove_panel` needs a typed entity. `ShellState::tab` already gives one for cluster tabs, so `settings_tab` changes from an `EntityId` to a `WeakEntity<SettingsTab>`.

The button stops propagation of both mouse down and click, so closing an inactive tab does not activate it first and a quick second click does not pin a neighbor. Both tabs return `false` from `closable`, which removes Close from the dock's overflow menu. That entry is a silent no-op on the last tab, and one close path is simpler than two. `remove_panel` does not read `closable`.

Alternative: `set_close_button_visible(true)`. It is one line, but the only tab could not be closed, which is the most common case, and the button has no tooltip, which `Icons accompany labels` requires.

No shortcut is bound. Ctrl+W deletes a word in a shell, and the planned container terminal is host-owned, so taking the key now would cause a conflict later. It can be added with the terminal's key handling in view.

### Remove restore rather than disable it
Delete the code instead of adding a setting, because nothing should depend on it:

- `Workspace`: the load block, `schedule_layout_save`, `save_generation`, `SAVE_DELAY`, and `LAYOUT_VERSION`. The dock is created with no version. The `DockEvent::LayoutChanged` subscription stays, reduced to `cx.notify()`, because the no-tabs hint needs it.
- `ClusterTab`: `ClusterTabState`, `pinned_by_default`, `register`, and `dump`.
- `SettingsTab`: its `register_panel` call and `dump`.
- `ConfigPaths`: the `workspace` field.

`panel_name` stays because the kit's trait requires it. `SharedClusterModel` goes too, because only the restore factory read it. The other `Shared*` globals stay because views read them, so only their comments change. `ClusterTab::same_server` also stays, because the kubeconfig can change while the application runs.

The kit's default `dump` is enough, so test helpers that read tabs from `dock.dump` move to `ShellState::summaries`.

An existing `workspace.json` is left on disk and ignored. Nothing has been released, so a cleanup step would be code with no users. This supersedes the layout part of D16 in `define-app-architecture`.

### Restart now closes tabs
A language change restarts the application, so it now loses the open tabs too. Keeping them for this one path would need the restore code back, so `settings.restart_warning` says so instead: "Restarting disconnects every cluster and closes every tab."

## Risks / Trade-offs

- **Binary grows by about 1 MiB** → Accepted for now; `icon_assets!` is the documented fallback.
- **`#[serde(other)]` on a plain enum is outside serde's documented use, although `serde_derive` accepts it** → A round-trip test pins unknown-name behavior.
- **The rendering bug cannot be seen in test windows, which have no asset source** → Confirm in the running app on Linux.
- **Tab titles get wider** → Use extra-small icons and check with long cluster names.
- **A one-tab pane uses the dock's title bar instead of a tab strip, so the button's placement may differ there** → Check both layouts in the running app.
- **A closed tab that stays alive would keep its watch** → A test confirms the tab entity is released on close.
- **Closing is immediate, so a misclick loses a tab's filter and scroll position** → Accepted. Reopening is one click, and tabs hold no unsaved work yet. The manifest editor's unapplied-changes prompt must hook into this close path when it arrives.
- **`MODIFIED` and `REMOVED` deltas need their target specs, which main specs do not have yet** → Archive `define-app-architecture`, `add-three-column-layout`, and `add-themes-and-i18n` first, as `add-namespace-multiselect` also requires. The `shell/workspace` purpose line still says the layout comes back after a restart, so correct it when this change is archived.
