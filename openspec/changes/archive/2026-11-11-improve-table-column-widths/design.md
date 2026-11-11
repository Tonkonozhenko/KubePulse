# Design: Improve Table Column Widths

## Context

See proposal.md for why. Every kind's table is a `TableState<RowsDelegate>` built in `ResourceBrowser::new()`. In gpui-kit 0.9.1:

- `Column::width` sets the drawn width. The default is 100 px. `min_width` (default 20 px) and `max_width` (default 1200 px) only clamp resizing and fitting. The first pass called `.width(px(40.))`, which caused the clipping.
- Cells and headers are `overflow_hidden` and `whitespace_nowrap`, with no ellipsis, so clipped text looks complete.
- The table has no flexible column that fills the remaining width. It does provide `TableDelegate::fit_column`, which returns content width and is called on a double-click of the resize handle, and `TableState::fit_col(ix)`, which adds cell padding, rounds to device pixels, clamps to `min_width`/`max_width`, and emits `ColumnWidthsChanged`.
- Rows arrive as snapshots through `apply_snapshot`. Tables reach 10k rows, and snapshots publish at most once per frame.
- Status cells render `status_tag` (a small `Tag`: 0.625 rem text, `px_1p5` padding, 1 px border).

## Goals / Non-Goals

**Goals:**
- Use the kit's own fitting path (`fit_column` / `fit_col`) for both automatic fitting and double-click fitting, with one measuring routine.
- Keep fitting cheap enough at 10k rows to run on the UI thread within one frame.

**Non-Goals:**
- Making the Name column fill the remaining width. The kit has no flexible column, and emulating one would mean tracking viewport resizes. Any space left over stays at the right of the table.
- Saving widths across restarts or between tables of the same kind.
- Wrapping cell text onto several lines.
- Showing Created as an age. Values stay as received.

## Decisions

### D1: Roles and their widths

A `ColumnRole` (Name, Namespace, Status, Timestamp, Text) is derived when columns are built. Name and Namespace come from position, Status from `ColumnDef.status`, Timestamp from the `created` column id and printer columns whose path ends in `Timestamp` or `Time`, and Text from everything else, including computed columns. Each role has one table of widths:

| Role | Initial | Min | Auto cap |
|---|---|---|---|
| Name | 240 | 120 | 480 |
| Namespace | 140 | 80 | 280 |
| Status | 130 | 80 | 240 |
| Timestamp | 170 | 120 | 240 |
| Text | 110 | 60 | 320 |

The initial width is raised to at least the measured header width at construction, so translated and pseudo-locale headers are never clipped in an empty table. The column `max_width` keeps the kit's 1200 px, which bounds both dragging and double-click fitting.

*Alternative:* derive everything from measurement and skip initial widths. Rejected: a table with no rows would then be built entirely from header widths, and Name would be tiny.

### D2: One measuring routine, implemented in `RowsDelegate::fit_column`

`fit_column(col_ix)` returns `max(header, widest value)`, where:

- the header width is the shaped title plus, for sortable columns, the kit's sort icon (0.75 rem plus 2 px on each side), as the kit's doc comment requires;
- a value's width is the shaped text in the table's text style, or for status cells the text shaped at the tag's 0.625 rem size plus twice `px_1p5` plus 2 px of border. These sizes are taken from `status.rs`, next to `status_tag`, so the measurement and the drawing share their numbers.

Shaping 10k values per column on every fit would take too long, so the routine first scans the rows' strings and keeps the **8 longest by character count** (O(rows), no allocation beyond the 8). It shapes only those 8. Proportional fonts make character count an approximation, and the 8 candidates absorb that error. Pinned columns return `None`, and they are already `resizable(false)`.

`RowsDelegate` has an `auto_fit: bool` field that is set only while the browser runs an automatic fit. While it is set, `fit_column` limits its answer to the role's cap minus the cell padding. A double-click reaches `fit_column` with the field unset, so it is limited only by `max_width`, as the spec's on-request requirement says.

### D3: Automatic fitting goes through `fit_col` with the cap on

The automatic fit, once per table, runs in `ResourceBrowser::apply_snapshot` on the first snapshot with at least one row. Inside one `table.update`, it sets `auto_fit`, calls `fit_col(ix)` for each data column that is not user-sized, and clears `auto_fit`. This uses the kit's padding, device-pixel rounding, and `min_width` clamp unchanged, with no `refresh`. A `fitted: bool` makes sure the fit runs only once.

Computed columns are fitted the same way, the first time `computed` gains a value for that column. A `HashSet<usize>` records which computed columns have been fitted.

*Alternative:* fit on every snapshot, growing only. Rejected because columns would jump while the user reads, and it adds per-frame work at 10k rows.

### D4: Detecting user resizes

`ResourceBrowser::on_table_event` already receives `TableEvent`s. The browser keeps `applied: Vec<Pixels>`, the widths it last set itself, and records them by reading `table.col_width` right after each automatic fit. When a `ColumnWidthsChanged` arrives, every column whose new width differs from `applied` joins `user_sized: HashSet<usize>`. The browser's own fits always match `applied`, so they never count, however late their events are delivered. No reentrancy guard is needed. D3 skips user-sized columns. A double-click fit changes a width without going through `applied`, so it also marks the column user-sized, which matches the spec.

### D5: Ellipsis on overflow

`render_th` and the plain-text branch of `render_td` add `.text_ellipsis()` and `.overflow_hidden()` (with `w_full`) to the text `div`. Status tags are already fitted by D2, and a tag in a column the user narrowed is clipped by the cell, as now. The full value stays reachable through the detail panel and `cell_text`, which copying uses.

### D6: Remove the first pass's leftovers

The leftovers are `STATUS_COLUMN_MIN`, `TIMESTAMP_COLUMN_MIN`, `NUMERIC_COLUMN_MIN`, `GENERIC_COLUMN_MIN`, `declared_columns_min_width`, its `#[allow(dead_code)]`s, and its unit tests. The role table in D1 replaces them.

## Risks / Trade-offs

- **Character count picks the wrong top 8 for wide scripts or mixed-width text** → The error is bounded: a missed value is at most slightly wider than the fitted column, it shows an ellipsis (D5), and a double-click refits.
- **The first snapshot holds only part of a large list, so later rows may be wider** → The spec fits "loaded rows" on purpose: the ellipsis marks overflow, and a double-click fits again.
- **Text shaping in `TestAppContext` reports widths unlike real fonts** → Tests compare a column's width with the same text system's measurement of its widest value, never with absolute pixels.
- **Space left at the right of a wide window stays empty instead of going to Name** → Accepted: every value is readable, and no layout depends on the viewport.
