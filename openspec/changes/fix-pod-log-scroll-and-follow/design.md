# Design

## Context

`LogTab` shows lines in a `uniform_list` with its default horizontal sizing, which fixes content width to the viewport and leaves horizontal overflow off. Rows clip at that width, so nothing wider can be reached. There is no scrollbar on the list.

Unconstrained sizing turns on horizontal overflow, but takes content width from a single measured row, by default row 0. That measurement also runs outside the list's own text style, so the font and size set on the list do not apply to it.

The view moves to the end only in `apply_status`, when a stream update arrives. A deferred scroll happens once, against that frame's layout. If the view then gets shorter, for example when a status note, connection note, or wrapped toolbar row appears, the offset stays where it was and the newest line sits below the view. Nothing corrects it until another line arrives, which on a quiet log may be never. This cause comes from reading the code, and the task list confirms it in a real window.

See proposal.md for motivation.

## Goals / Non-Goals

**Goals:**
- Reach the end of any displayed line, with a scrollable width that tracks retained lines at bounded cost under high volume.
- Make following a state the view holds, not a reaction to arriving lines.

**Non-Goals:**
- A virtualized width per visible range. The width covers all retained lines, so it does not jump while scrolling vertically.
- Changing the stream, the buffer, or retention.

## Decisions

### Pin to the end on every render while following

`Render::render` calls `scroll_to_bottom` when `following` is true, after `notice_scrolling`, and `apply_status` stops scrolling. A deferred scroll is one field write, so this costs nothing per frame. It covers every reason the end can move: new lines, a resize, and notes appearing.

It also closes a race. Today a status update can queue a scroll between a wheel event and the render that would notice it, which overrides the user's scroll. With one call site after `notice_scrolling`, that render stops following first, so no pin is queued.

`notice_scrolling` drops its `settled` check, because a queued scroll is consumed in the same frame and only `go_to_match` queues one otherwise, which has already stopped following.

The alternative was to pin only on status updates and on resize events. That needs a case for each layout change and would miss the next one.

### Measure the widest retained line with a sliding maximum

`with_width_from_item` picks the measured row, so `LogTab` tracks the index of the widest retained line. A small `WidestLines` type holds a `VecDeque<(u64, usize)>` of sequence number and width, with widths strictly decreasing. A new line removes narrower entries from the back, and lines that left the buffer are removed from the front. Each line enters and leaves once, so the cost is amortized constant per line, and the front is always the widest retained line.

`apply_status` feeds it the lines since the last update, as `search_new_lines` does, regardless of whether a search is active. It resets with the stream in `start_stream`.

Alternatives:
- Rescanning all retained lines costs up to 100,000 lines each time the widest one drops.
- Keeping the widest width ever seen, and forcing rows to that width, avoids the structure but leaves the scrollbar too wide after the line is gone, which the spec rules out.
- Using row 0, the default, leaves every wider line unreachable.

### Width is display columns of the shown text

Ranking uses `unicode-width` on the text after the `MAX_SHOWN_BYTES` cut. Byte length misranks: two-byte Latin or Cyrillic text ranks above wider ASCII. Character count misranks wide CJK text and emoji. The ranking picks only which row to measure, and the real width still comes from shaping that row.

The timestamp prefix is left out, since it is nearly the same width on every line and showing it does not need a rescan.

### Text style moves to a wrapper around the list

The mono font and `text_sm` move from the list to a wrapping `div`, which also holds the scrollbar. Measuring runs under the wrapper's text style, so measured width and row height match what is painted. Row height shrinks to its real size as a result, which is the intended look.

The wrapper needs `flex_1` and `min_h_0`, and the list stays `flex_1` inside it.

### Horizontal scrollbar in its own strip

`Scrollbar::horizontal(&self.list).viewport_from_layout()` sits in a strip of height `Scrollbar::width()` below the list, in `ScrollbarMode::Always`. An overlay would cover the newest line, which is the line this view most needs to show. List padding is not an option, because the list and its scroll clamp account for padding differently.

The strip is always present, so the viewport height does not change when the first wide line arrives.

### Checkbox replaces the Newest line button

A `Checkbox` with id `log-follow` sits after the Timestamps button, bound to `following`. Its handler sets `following`, and the next render pins to the end when it is on. `jump_to_newest` and the `log-jump` button go away, along with `log.jump`, because the checkbox covers the same action in a position that does not move.

`start_stream` keeps setting `following` to true, so a new container, instance, retry, or reconnect starts at the end. `keep_place` still holds the reader's lines while paused.

## Risks / Trade-offs

- **Risk** The widest row is shaped on every frame even when out of view, up to 4 KiB of text. → The line layout cache reuses the shaped result across frames, and visible rows already carry the same cost.
- **Risk** `unicode-width` can disagree with shaped width for some scripts or fallback fonts, so the row measured may not be the very widest. → The difference is small, and the 10k-lines-per-second check covers its cost.
- **Risk** The always-present strip takes one row of height when no line needs it. → Accepted, since a stable viewport is worth more than the height.
- **Risk** A search match past the right edge stays out of view after moving to it. → Listed as out of scope. The reader can now scroll to it, which was impossible before.
