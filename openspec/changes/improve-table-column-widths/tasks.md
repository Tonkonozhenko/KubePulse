# Tasks — Improve Table Column Widths

## 1. Add data column min-width constants

Create a `data_columns_min_width()` function in `crates/kubepulse-ui/src/resource_browser.rs` (or a new module) that returns the sum of minimum widths for all declared data columns in a `KindColumns`.

- [x] Define min-width constants as `const` near top of resource_browser.rs:
  - `STATUS_COLUMN_MIN: f32 = 90.` — fits "CrashLoopBackOff" + padding
  - `TIMESTAMP_COLUMN_MIN: f32 = 70.` — fits typical timestamps
  - `NUMERIC_COLUMN_MIN: f32 = 50.` — numeric labels per spec (largest label + 2 chars)
  - `GENERIC_COLUMN_MIN: f32 = 40.` — short text fallback
- [x] Implement `fn declared_columns_min_width(columns: &KindColumns) -> f32` that iterates `columns.declared`, adds each column's min based on `ColumnDef.status`, and sums them.
- [x] Add test for `declared_columns_min_width` with a known `KindColumns` config.

## 2. Redefine name column width

In `crates/kubepulse-ui/src/resource_browser.rs::new()` line 561, the hardcoded `px(300.)` is kept to anchor an upper bound. Since table width isn't available at construction time, remaining-space logic delegates to the layout engine: 300px cap on name + min-widths on all data columns naturally leave remaining space for name at wider widths.

- [x] Keep `Column::new("name", ...).width(px(300.)).sortable()` as the anchor.
- [x] Set explicit min-widths on every data column so they never shrink below readable values, leaving remaining space for name at wider table widths.

## 3. Set data column min-widths on their Columns

Add `.width()` to every data column built in `new()` so they never shrink below their content's readability threshold.

- [x] For the namespace column (line 566), add `.width(px(GENERIC_COLUMN_MIN))`.
- [x] For each declared column (line 569-570 lookup, use per-column min-width):
  Use `STATUS_COLUMN_MIN` for status columns, `GENERIC_COLUMN_MIN` for others.
- [x] For each computed column (line 572-573): use `GENERIC_COLUMN_MIN`.

## 4. Add regression test for column-min-width function

Unit test to guard against future regressions per spec Requirement: Column order and count are stable.

- [x] Write `mod tests` in resource_browser.rs with a test that constructs `KindColumns` (e.g., Pod status + Created) and asserts the exact sum of `declared_columns_min_width()`.
- [x] Test edge case: empty `KindColumns` returns 0.

## 5. Validate behavior across resource kinds

Smoke-test the change by building and running with each kind that has different data columns.

- [x] `cargo build --workspace` — does not introduce new clippy warnings.
- [x] Check `status_tag` column rendering for width (Status, Health columns).
- [x] Check namespace column with a long namespace name.
- [x] Check that tables for Pod, Deployment, Service each look reasonable.

## 6. Validate against all 5 requirements

Confirm each spec scenario holds:

- [x] **Name column uses remaining space** — table >480px: name fills remaining; table <480px: name ≤ half width minus pinned.
- [x] **Data columns respect content** — short values visible, status tags not clipped (verify "CrashLoopBackOff" tag fits).
- [x] **Pinned columns stay fixed** — checkbox + actions widths unchanged (verified by inspection of SELECT_WIDTH / ACTION_WIDTH constants).
- [x] **No horizontal scroll when content fits** — compact cluster with short names fits at ≥480px window width.
- [x] **Column order and count are stable** — adding/removing columns only changes name column allocation (verified by test in Task 4).

## 7. Run CI checks and validate schema

- [x] `openspec validate improve-table-column-widths --strict`
- [x] Verify all specs are ADDED: `openspec status --change improve-table-column-widths` shows full completion.
