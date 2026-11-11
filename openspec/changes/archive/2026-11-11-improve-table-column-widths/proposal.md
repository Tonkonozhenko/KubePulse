# Proposal

## Why

The first pass at this change set every data column to a fixed 40 px width (90 px for status) and kept Name at 300 px. In the table toolkit, that call sets the actual width rather than a minimum. As a result, Namespace, Node, IP, Restarts, Created, Type, Storage class, and every printer column are clipped to a few letters, with nothing showing that the text continues. Headers such as "Storage class" and "Last run" are clipped too. Users can't read most of a table without dragging every column.

## What Changes

- Replace the fixed widths with **content-fitted widths**. When a table first fills with rows, it sizes every text column to the widest of its header and the loaded values. Each column stays within a readable minimum and an automatic cap, so one very long value can't take over the table.
- Give columns **initial widths by role** (name, namespace, status, timestamp, general text), so a table that is still loading, or has no rows, already looks right.
- Fit **computed columns** once their first values arrive.
- Leave alone any column the user has resized; automatic fitting never overrides it.
- A **double-click on a column's resize handle** fits that column to its content, past the automatic cap, up to the column maximum.
- Show an **ellipsis** on any cell or header text that is still cut off, so clipped text never looks complete.
- Size status tags as drawn, including the tag's padding and border, so tags such as `CrashLoopBackOff` aren't clipped.
- Count headers at the width they render in the active language, including the sort icon.
- Remove the unused width constants, the `declared_columns_min_width` helper, and its tests from the first pass.

## Capabilities

### New Capabilities
- `general/table-columns`: How resource tables size their columns: initial widths, fitting to content, limits, user resizing, and marking text that is cut off.

### Modified Capabilities
<!-- None: general/resource-browsing keeps its column set and order; only sizing is new. -->

## Impact

- `crates/kubepulse-ui/src/resource_browser.rs`: column construction in `ResourceBrowser::new()`, `RowsDelegate::fit_column`, the fit after the first snapshot and after the first computed values, and ellipsis in `render_th` and `render_td`.
- `crates/kubepulse-ui/src/status.rs`: the tag's measured width comes from the same sizes `status_tag` draws with.
- `crates/kubepulse-ui/tests/browsing.rs`: new table-width tests.
- No contract, store, or dependency changes. Persisting widths across restarts is out of scope.
