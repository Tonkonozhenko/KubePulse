# Design

## Context

`gpui-kit` 0.9.0 is already a dependency and enables its `assets` feature by default. It includes 1,830 icons (Lucide 1.43.0 plus 12 of its own), licensed ISC, MIT, and Apache-2.0, all allowed by `deny.toml`. The kit offers three ways to register them:

| Source | Contents | Binary cost |
|---|---|---|
| `Assets` | 101 component icons | Baseline |
| `icon_assets!` + `Assets` | Listed extras, through a wrapper source | About 19 KiB for 10 icons |
| `AllAssets` | Full catalog | About 1.02 MiB |

`app.rs` calls no `with_assets`, so GPUI uses the empty `()` source. It returns `Ok(None)` and `paint_svg` draws nothing, with no error. The kit's guidance is `Assets` for component icons, `icon_assets!` for a few extras, and `AllAssets` when most of the catalog is needed. Its docs also say icons take the text color.

## Goals / Non-Goals

**Goals:** make icons render; make kinds, actions, and states easier to scan; keep built-ins on the extension contract.

**Non-Goals:** table cell icons, extension-supplied SVGs, declarative UI icons, and icons for detail tabs, actions, or commands. Each can be added later as a minor contract change where needed.

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

The tab title's "(protected)" text stays, because labels are not replaced. Status colors come from `cx.theme()`.

## Risks / Trade-offs

- **Binary grows by about 1 MiB** → Accepted for now; `icon_assets!` is the documented fallback.
- **`#[serde(other)]` on a plain enum is outside serde's documented use, although `serde_derive` accepts it** → A round-trip test pins unknown-name behavior.
- **The rendering bug cannot be seen in test windows, which have no asset source** → Confirm in the running app on Linux.
- **Tab titles get wider** → Use extra-small icons and check with long cluster names.
