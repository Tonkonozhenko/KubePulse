# Design

## Context

See `proposal.md` for the motivation and the specs for required behavior. These facts about the current code shape the design:

- `ResourceBrowser` owns the table and a `selected` key. A row click emits `TableEvent::SelectRow`, which opens the detail panel. Selection changes made by code are counted in `expected_selection_events`, so they do not reopen anything.
- Cells render only for visible rows, through `RowsDelegate::render_td`. `leading_columns` and `sort_key` turn a table column index into a declared column.
- A `Row` holds compact cells, and each `ColumnSpec` has one JSON path. Rows never keep the full object.
- The delete dialog lives in `DetailView` and handles one target.
- The kit's table pins columns on the left only. Its row click stops propagation, and a nested element can stop the row seeing a click.
- The kit's `Checkbox::on_click` gives only the new value, not the click's modifiers.
- Zoom comes from the kit's dock. `Panel::zoomable` defaults to true, and the entry sits in the tab bar's ellipsis menu. It fills the dock area, which excludes the rail and sidebar, so a single pane looks the same before and after. `render_toolbar` always draws the entry and only disables it.
- `ActionContribution` exists in the contract, but the UI never renders one.

## Goals / Non-Goals

**Goals:**

- Selection and status rendering add no measurable cost to scrolling at 10k rows.
- One delete dialog serves the detail panel, row actions, and multi-deletion.
- One definition of an object's status serves the table and the detail panel.

**Non-Goals:**

- Rendering extension `ActionContribution`s in rows. That needs its own contract work, and this change records it in `TODO.md`.
- Shell and port-forward row actions, which wait for groups 11 and 12.
- Keyboard selection of checkboxes.
- Coloring numeric columns such as Ready.
- Showing `Terminating` for objects with a deletion timestamp, because a status column reads values and cannot map a timestamp to a word.
- Live updates in the detail panel, or making it resizable.
- A custom dock skin to remove the disabled zoom entry.

## Decisions

### Status columns are a new `ColumnSource` variant

Add `ColumnSource::Status { paths: Vec<String> }`. A single variant carries both the role and the ordered paths, so `ColumnContribution` keeps its shape and existing extensions compile unchanged. `CONTRACT_VERSION` moves to 1.3.

Alternatives: a `role` field on `ColumnContribution` breaks every struct literal, including the compiled guide examples. A computed column would call an extension per row and could not keep the status sortable.

`ColumnSpec` gains `first_of(id, paths)` and holds a list of paths, and `Row::project` keeps the first non-empty cell. `ColumnSpec::evaluate(&Value) -> Cell` is public, so the detail panel can use the same logic.

The built-in Pod column declares `.status.containerStatuses[?(@.state.waiting)].state.waiting.reason` and then `.status.phase`. The PVC and PV Status columns declare `.status.phase`. Printer columns named Status, Phase, or State, in any case, become status columns in `columns_for`.

### Tones live in one UI module

A new `status.rs` holds `Tone { Success, Warning, Danger, Neutral }`, a case-insensitive `status_tone(&str)`, `condition_tone(type, status)`, and a function that turns a tone and text into a kit `Tag`. All colors therefore come from `cx.theme()`.

| Tone | Statuses |
|---|---|
| Success | Running, Succeeded, Completed, Active, Bound, Available, Ready, Healthy |
| Warning | Pending, ContainerCreating, PodInitializing, Terminating, Released, Unknown |
| Danger | Failed, Error, CrashLoopBackOff, ImagePullBackOff, ErrImagePull, CreateContainerConfigError, OOMKilled, Evicted, Lost |

For conditions, the problem types are those ending in `Pressure`, plus `NetworkUnavailable`, `ReplicaFailure`, and `Failed`. On a problem type, True is Danger and False is Success. On any other type, True is Success and False is Warning, because a False such as `Ready` on a finished pod is often expected. Unknown is Warning on every type.

The host chooses tones, so colors never cross the contract.

### Table layout

The table gains two leading columns, both `fixed_left`, not sortable, movable, resizable, or selectable: the checkbox, then the actions. The pinned position is the only one that stays visible, because the kit cannot pin right. Its width depends on the kind, to fit two or three icon buttons.

`leading_columns` grows by two, and `sort_key` and `cell_text` keep working through it. `cell_text` returns an empty string for the two new columns.

### Checkbox clicks

Each checkbox cell is a wrapper `div` with an `on_click` that reads `event.modifiers().shift` and calls `cx.stop_propagation()`. Inside it is a `Checkbox` with no handler, used for display. This is the only way to see Shift, and the wrapper gives a larger click target. The header checkbox uses the same wrapper in `render_th`, so a click there does not select the column.

### Checked rows live beside the rows

`RowsDelegate` holds `checked: HashSet<Arc<str>>` and `anchor: Option<Arc<str>>`, keyed by `Row::key`, so `render_td` can read them and rows can be reordered freely. The anchor is the key of the last checkbox clicked without Shift. A Shift+click finds both positions in the current rows, inserts the range, and leaves the anchor where it is.

`apply_snapshot` retains only keys present in the new rows. It skips this when nothing is checked, so the idle cost is zero, and otherwise it is one linear pass per published snapshot, at most once per frame.

Dropping rows that leave the table is deliberate: a person deleting from a filtered view should never delete objects that view does not show.

### One delete dialog

Move the dialog from `detail.rs` into a new `delete_dialog.rs`, as a `DeleteDialog` entity holding `Vec<ObjectTarget>`. It keeps the current element ids (`delete-confirmation`, `delete-confirm`, `delete-cancel`), so existing tests carry over. `ClusterTab` opens it for the detail panel, the row actions, and the selection bar, and passes a callback that hears which objects went. A callback rather than an event, because the dialog is often closed by the time the results arrive.

With one target, a protected cluster still asks for the object's name. With several, it asks for the cluster's name, because one object's name would not confirm the whole list. Every target is listed in a `uniform_list`, so a list of 10,000 stays cheap.

Deletion runs with `futures::stream::buffer_unordered(8)` over `services.delete_object`, in a task on the UI executor that only awaits replies. Eight keeps a large deletion from flooding the API server without making it slow. After each result the task updates the progress text, and a cancelled flag stops it taking more from the stream. `ObjectError::NotFound` counts as deleted, which matches the goal of the user's request. After the run, failures replace the target list, so confirming again retries only those.

The callback runs after every run, including a cancelled or partly failed one. `ClusterTab` then closes the detail panel if its target is in the list, and the browser removes the deleted rows from `checked`.

### Row actions as events

`BrowserEvent` gains `Logs(ObjectTarget)`, `Edit(ObjectTarget)`, `Delete(Vec<ObjectTarget>)`, and `Close`. `ClusterTab` routes them the way it routes `DetailEvent`. The right-click menu is built in `TableDelegate::context_menu` with `PopupMenuItem::on_click`, and its closures capture the row's `Arc<Row>`, not its index, so a row that moves while the menu is open keeps the right target. Button handlers call `cx.stop_propagation()`, as `close_button` does.

### Closing the panel

`DetailEvent` gains `Close` and `Delete`, and loses `Deleted`. `ClusterTab::close_detail` drops the detail and calls `ResourceBrowser::clear_open`, which clears `selected` and runs `sync_selection`, and the existing expected-event count keeps that from reopening anything. A deleted open object uses the same path. A user `ClearSelection` from Escape emits `BrowserEvent::Close`. The table holds no selection while the open row is scrolled off screen, so it passes Escape on, and a key listener around the table handles that case the same way. The kit does not export its `Cancel` action, so an action handler is not an option.

### Detail layout

The header becomes a fixed block above a scrolling body. It holds the title, status tag, close button, and the Logs, Edit, and Delete buttons, which keep their ids. The status comes from the kind's status column through `ColumnSpec::evaluate`. A kind with no such column uses `.status.phase` if present.

Labels use a wrapping row of secondary `Tag`s. Conditions and events are rows of a tag, a reason, and a message. Annotations sit in a collapsed `Collapsible`, and for Secrets they show keys only, because `last-applied-configuration` holds the Secret's data.

### Zoom

Every tab panel (`ClusterTab`, `LogTab`, `ManifestTab`, `SettingsTab`, `TerminalTab`) overrides `zoomable` to return false. The base dock refuses the zoom however it is asked for, so one override covers the action too.

Alternatives: enabling zoom only when split needs each panel to track the pane count for a rarely used feature. Hiding the rail and sidebar too is new layout behavior nobody asked for.

## Risks / Trade-offs

- **Disabled menu entry stays** → The kit always draws it, and for tabs with no other entries the ellipsis menu holds only that item. Removing it needs a custom skin or a kit change, so this is accepted for now.
- **Tags increase row render cost** → Only visible rows render a tag. The 10k scroll test and the manual window check confirm this holds.
- **Actions column pushes names right** → It keeps actions reachable when the table is wide. Users cannot reorder it, and the column stays narrow by using icons with tooltips.
- **A name can be reused between confirming and deleting** → Delete targets by name, so a recreated StatefulSet pod could be deleted as well. This matches single deletion today, and a UID precondition can be added later without changing the dialog.
- **A deletion of 10,000 objects takes time** → Progress shows throughout, Cancel stops further requests, and the UI thread only awaits replies.
- **Status list misses a reason** → An unrecognized value shows in neutral, as received, so nothing is hidden. New reasons are one line to add.
- **Filtering clears checks unexpectedly** → It is the safe direction, and the selected count updates at once.

## Migration Plan

None. Nothing persisted changes, and `ColumnSource::Status` is additive. Rollback is a revert.
