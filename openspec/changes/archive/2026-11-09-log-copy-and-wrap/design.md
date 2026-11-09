# Design

## Context

`LogTab` (`crates/kubepulse-ui/src/log_tab.rs`) paints each row as a `StyledText` inside a `uniform_list`. Rows are read from the bounded `LogBuffer` (100,000 lines) only for the visible range. Several behaviors rely on every row having the same height:

- `keep_place` moves the offset by `row_height * dropped` when retention drops lines while paused.
- `notice_scrolling` compares the vertical offset between renders to notice the user scrolling up.
- `go_to_match` calls `scroll_to_item_strict` with a row index.
- The scrollable width comes from `with_width_from_item(widest)`, which `WidestLines` keeps current.

The toolkit already provides what both features need:

- **Window text selection** (`gpui_base::TextSelection`, `TextSelectionHandle`). `Root` renders the `TextSelectionLayer` and binds `cmd-c` and `ctrl-c` to copying the window selection in its key context, and `KubePulse` already wraps its window in `Root`. Participants report laid-out runs each frame. Virtualized participants identify positions with a `TextSelectionContentKey`, resolve keys through `resolve_content_key_with`, and build copied text through `copy_with`. The toolkit's own diff view uses this to select and copy across rows that are not painted. Drag auto-scroll arrives as `TextSelectionEvent::AutoScroll`.
- **Variable-height list** (`gpui::list` with `ListState`). It measures rows as they render, adjusts its logical scroll position on `splice`, and works with the toolkit's `Scrollbar` through `ScrollbarHandle for ListState`. The diff view uses it for soft wrap.

See proposal.md for motivation and `specs/pod/log/spec.md` for the required behavior.

## Goals / Non-Goals

**Goals:**
- Select and copy across any retained lines, independent of which rows are painted.
- Wrap without degrading the unwrapped path or its cost per frame at 10,000 lines per second.

**Non-Goals:**
- Remembering the wrap setting across tabs or restarts. Each tab starts unwrapped.
- A copy button, other items in the right-click menu (such as select all or copy line), or copying all retained lines at once. Triple-click and drag cover lines, and the shortcut and the menu's Copy item are the copy paths for now.
- Selecting text while a match is being stepped to. Search keeps its own highlight.

## Decisions

### Rows join the window text selection with content keys in shown-text coordinates

`LogTab` owns one `TextSelectionHandle`. Each row is a small custom element that lays out its `StyledText`, registers with the handle using `document_order = seq`, and reports one `TextSelectionRun`. The returned projection gives the selected byte range for the row, painted with `cx.theme().selection` beneath the search highlights.

A content key encodes `(seq, offset)` as `seq << 16 | offset`, where `offset` is a byte offset into the row's shown text. Shown text is at most `MAX_SHOWN_BYTES` plus the cut marker and an optional timestamp prefix, so 16 bits hold it. `resolve_content_key_with` turns a key into the row's position when that row is painted, so a drag that starts on a row that has scrolled out of view still works.

Offsets refer to shown text rather than the received line, because shown text is what the user drags across. That is why turning timestamps on or off clears the selection: the same offset would point at different characters. Starting a new stream clears it too, because sequence numbers restart.

The alternative was a line-based selection model of our own, with click and shift-click on whole lines. It would duplicate drag, word, and auto-scroll handling the toolkit already has, and would not select part of a line.

### Copy reads from the buffer, not from painted rows

`copy_with` receives the selection's anchor and cursor keys, orders them, and reads the lines from the buffer. For each line it rebuilds the shown text, takes the selected range, and joins lines with `\n`. When the range on a line reaches the cut marker, it takes the rest of the received line instead, so a cut line is copied whole. When the start line has already been dropped, copying starts at the oldest retained line, which matches the highlight the user sees.

Copying is bound by `Root`'s existing `cmd-c` and `ctrl-c` bindings. The search `Input` has a deeper key context with its own copy binding, so it keeps copying its own text while it has focus. The log view gets a focus handle tracked on its list area, so a click on the lines moves focus out of the search field.

### The right-click menu copies the text selected when it opens

The line area, the element that holds the rows and the vertical scrollbar in both wrap modes, gets the toolkit's `context_menu`, as the kind sidebar and cluster rail already have. The menu has one item, **Copy** (`log.copy`, with the copy icon).

The window text selection reacts only to the left button, so a right-click keeps the selection. But it clears the selection in the capture phase of every left-button press in the window, including a press on a menu item, and it exempts only touch-selection UI. By the time the item's click handler runs, the selection is gone. So the menu builder, which runs when the menu opens, reads `TextSelection::selected_text` and trims it as `Root`'s copy handler does. The item's `on_click` writes that text to the clipboard. As a result:

- The copied text is what the shortcut would copy, because both go through `copy_with` and the same trimming.
- The item is disabled when that text is empty.
- The item carries Root's `Copy` action with `action_context(focus)`, but only so it shows the platform shortcut from the existing key bindings. The click handler takes precedence over the action.
- The search field's focus does not matter, because nothing is dispatched through focus.
- The selection is cleared when the item is pressed, like any left click in the window.

Dispatching Root's `Copy` from the item was the first plan. It copied nothing, because the press on the item had already cleared the selection.

### Two list elements, one per wrap mode

While wrapping is off, the view stays the current `uniform_list`. While it is on, the view renders `gpui::list` from a `ListState` with `ListMeasuringBehavior::Visible`, and rows use `whitespace_normal` at the list's width. Both modes share row rendering, the selection handle, search, and status.

A single `ListState` for both modes was considered, matching the diff view. The fixed row height of `uniform_list` is what makes `keep_place`, the sideways width from `with_width_from_item`, and the cost of 100,000 rows simple and cheap today, and `ListState` gives none of those for free. Keeping it for the default mode leaves the existing behavior and its tests untouched.

Computing wrapped rows ourselves inside `uniform_list` was rejected. It needs exact glyph advances for fallback fonts and wide characters, and a prefix sum over every line that must be rebuilt on each resize.

### The wrapped list follows its own scroll state

In wrapped mode, the `ListState` holds the item count in step with the buffer:

- **Appends**: `apply_status` splices the new lines at the end.
- **Drops**: it splices `0..dropped` away. `ListState` adjusts its logical top to the same item, so the place is kept with no `keep_place` arithmetic.
- **Following**: render scrolls to the end on every frame while following, as the unwrapped view does.
- **Noticing a scroll up**: `notice_scrolling` reads `logical_scroll_top()` and compares `(item_ix, offset_in_item)` with the last render, ignoring changes that come only from drops.
- **Matches**: `go_to_match` calls `scroll_to_reveal_item`.
- **Scrollbar**: the vertical scrollbar is built from the `ListState`, and the horizontal one is not rendered.

The `ListState` exists only while wrapping is on. Turning wrapping on creates it with the retained count and scrolls it to the top line visible in the `uniform_list`. Turning it off reads `logical_scroll_top().item_ix` and sets the `uniform_list` offset to that row. While following, both directions simply pin to the end.

### Selection auto-scroll arrives as wheel events

A participant that does not scroll itself gets drag auto-scroll as synthetic wheel events, sent inside its hitbox's content mask. The selection area therefore inserts its hitbox under a content mask of its own bounds. Otherwise the mask would be the whole tab, and a drag just above the rows would not count as leaving them. The wheel events then scroll whichever list is shown, and scrolling up turns following off through the same path as the user's own wheel.

Copying goes through `Root`, which trims leading and trailing whitespace from the copied text.

## Risks / Trade-offs

- **Risk** → Measuring wrapped rows costs more than fixed rows. Mitigation: `ListMeasuringBehavior::Visible` measures only painted rows, and shown text is still cut at `MAX_SHOWN_BYTES`, so a row wraps to a bounded height. A UI test streams at high volume while wrapped, and the task list includes a manual check at 10,000 lines per second.
- **Risk** → With only visible rows measured, the vertical scrollbar thumb in wrapped mode is an estimate and can change size while scrolling. Mitigation: accepted. The spec requires only that it shows and pauses following when moved up.
- **Risk** → The selection API is new in this toolkit snapshot and could change when the kit is upgraded. Mitigation: the toolkit's diff view uses the same calls, so an upgrade that breaks them breaks there too and will be visible in its changelog, and our UI tests cover selection and copy.
- **Risk** → A content key holds only 16 bits for the offset. Mitigation: shown text is capped well below 65,536 bytes, and a unit test pins that the largest shown text fits.
