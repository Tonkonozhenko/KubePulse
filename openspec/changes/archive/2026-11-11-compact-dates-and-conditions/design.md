# Design: Compact Dates and Conditions

## Context

See proposal.md for why.

- `RowsDelegate::cell_text` returns a cell's value as received. It feeds rendering, `dump_range` (used for copying and by tests), and since the last change, column fitting.
- The `Timestamp` role already exists: `ColumnDef.timestamp` is set for the `created` column and for paths ending in `Timestamp` or `Time`. Its widths are 170 / 120 / 240 (initial / min / cap).
- Sorting goes to the store as `SortKey::Column(ix)` with a direction. RFC 3339 text in UTC sorts chronologically as text, so the store needs no change.
- `render_conditions` in `detail.rs` draws a column of blocks, each with a tag, status and reason row, then a muted message line. `render_labels` draws `Tag::secondary().small()` tags in a `flex_wrap().gap_1()` row.
- `chrono` 0.4.45 is already in the lockfile, through the Kubernetes client.

## Goals / Non-Goals

**Goals:**
- Display ages without changing any value the store holds or what a copy gives.
- Keep refresh cost bounded at 10k rows: only visible cells are redrawn.

**Non-Goals:**
- Ages in the detail panel. Created there stays exact, as the place where the exact value lives.
- Ages for event times, or a user setting to choose the date format.
- Local-time formatting.

## Decisions

### D1: Age formatting in `age.rs`

`pub fn age(text: &str, now: DateTime<Utc>) -> Option<String>` parses the text with `DateTime::parse_from_rfc3339`. It returns `None` when the text fails to parse or the time is in the future, and the caller then shows the text as received. Otherwise the result has at most two units, chosen by the elapsed time:

| Elapsed | Shown |
|---|---|
| < 2 min | `45s` |
| < 10 min | `5m30s` (the seconds part is omitted when 0) |
| < 3 h | `45m` |
| < 8 h | `3h4m` |
| < 2 d | `36h` |
| < 8 d | `3d4h` |
| < 2 y | `41d` |
| ≥ 2 y | `2y41d` (a year is 365 days) |

Unit suffixes come from `locales/en.json` (`age.seconds` = `%{n}s` and so on), so a language can change them. There are no plurals. `now` is a parameter, so the unit tests don't depend on the clock.

*Alternative:* `humantime` or a similar crate. Rejected: they produce long forms such as "3days 4h 5m", with no two-unit compact style. Parsing still uses `chrono`.

### D2: Displayed text and value text are separate

`RowsDelegate` gets `display_text(row_ix, col_ix, now)`. For a column with the `Timestamp` role it returns `age(...)` and falls back to the value; for any other column it returns the value. `render_td` and `fit_column` (through `longest_values`) use the displayed text. `cell_text`, and therefore `dump_range` and copying, keeps returning the value as received. `now` is read once per render or fit, from `Utc::now()`.

The age cell is a `div` with an id `("age", row_ix, col_ix)`. Its tooltip shows the raw value with the kit's `Tooltip`.

### D3: Refresh once a second, only while needed

When a browser has at least one `Timestamp` column, it spawns a task that waits on a 1-second `Timer` in a loop and calls `cx.notify()` on the table. Only the visible rows render, so each tick costs about a screenful of cells. The task belongs to the browser and ends when the browser closes. A 1-second tick keeps second-level ages (`45s`) correct. Coarser ticks would make young objects look frozen.

### D4: Reversed sort for timestamp columns

In `RowsDelegate::perform_sort`, a `Timestamp` column flips the direction it passes to the store. An ascending sort by age is then a descending sort by time, so the youngest object comes first. The header's sort icon still shows what the user clicked.

### D5: Narrower timestamp widths

The `Timestamp` role becomes 90 / 60 / 160. Ninety pixels fits `23h59m` and the sortable header "Created" at the table's text size. The cap leaves room for a value shown as received when it is not a time.

### D6: Conditions as tags

`render_conditions` becomes a `flex_wrap().gap_1()` row of `tone_tag(tone)` tags. Each tag reads `Type=Status`, the same `key=value` form as labels, which keeps the status readable without color. Each tag has an id `("condition", index)` and a tooltip of its reason and message, when it has either. Under the row, a `text_xs`, muted line for each condition with a reason or message reads `Type: reason — message`. The separator comes from the catalog (`detail.condition_detail`). Conditions that have neither take no extra line.

### D7: The project rule

`AGENTS.md`'s text standard gains: "Table timestamps show as ages, with the exact value on hover, when copied, and in the detail panel." This records the exception the user chose. The rule otherwise stands.

## Risks / Trade-offs

- **A 1-second notify keeps the table repainting while idle** → Each repaint covers only visible cells, and the table is already built for 10k-row updates once per frame. The timer runs only for kinds with a timestamp column, which in practice is every kind because of Created. That is accepted as the cost of live ages.
- **Clock skew makes recent objects look slightly in the future** → Those cells show the value as received, which is honest and rare. Showing `0s` would hide the skew.
- **Copying gives a different text than the cell shows** → That is intended: the copy is the exact value, as the spec requires, and the tooltip shows the same value.
