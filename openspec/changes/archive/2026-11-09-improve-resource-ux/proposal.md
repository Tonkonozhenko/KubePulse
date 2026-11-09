# Proposal

## Why

Resource tables and the detail panel show plain text, so problems are hard to spot and common work takes too many steps. A crash-looping pod reads `Running`, every action needs the detail panel open, objects can only be deleted one at a time, and the panel cannot be closed. The tab menu also offers a Zoom In entry that visibly does nothing in the usual single-pane layout.

## What Changes

- Show statuses in tables and the detail panel as colored tags, with the status text always visible.
- Make a pod's Status show the first waiting container's reason, such as `CrashLoopBackOff`, and otherwise its phase.
- Show labels as tags. Show conditions with their health, reason, and message. Show events by severity. Add annotations as a collapsed section, with values withheld on Secrets.
- Move the detail panel's title, status, and actions to its top, so they do not scroll out of reach.
- Add a pinned actions column to every table with Logs and Shell (pods only), Edit, and Delete, and offer the same actions from a right-click menu on the row.
- Add a pinned checkbox column. A click toggles one row, Shift+click selects every row between the last clicked row and this one, and the header checkbox selects every row shown.
- Add deleting several objects at once, with a confirmation that lists every object, and reports each failure.
- Add a close button to the detail panel. Escape in the table also closes it.
- Stop every tab from zooming. The UI kit always draws the Zoom In menu entry, so it stays visible but disabled.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

`openspec/specs/` is empty until the in-flight changes are archived, so these paths are the ones `define-app-architecture` and `add-three-column-layout` introduce. Each delta only adds requirements.

- `general/resource-browsing`: status colors, row actions, multi-selection, multi-deletion, and the detail panel's layout and closing
- `shell/workspace`: tabs cannot be zoomed
- `arch/extension-system`: a column can declare itself a status, with paths tried in order

## Impact

- **Contract:** `kubepulse-extension-api` gains `ColumnSource::Status`, which is additive, so `CONTRACT_VERSION` moves to 1.3. `docs/extensions.md` needs a matching section.
- **Store:** `kubepulse-store` gains multi-path columns and a way to evaluate a column on one object.
- **Built-in views:** `kubepulse-builtin` declares its Status columns with the new source.
- **UI:** `resource_browser.rs`, `cluster_tab.rs`, `detail.rs`, and `columns.rs` change, and the tab panels stop zooming. New modules hold the status tones and the delete dialog, which moves out of `detail.rs`.
- **Dependencies:** `kubepulse-ui` uses the workspace's existing `futures` dependency. Nothing new is added to the workspace.
- **Locale:** `locales/en.json` gains new keys, and `en-XA` is regenerated.
- **Tests:** table tests that index columns must account for the two new leading columns.
