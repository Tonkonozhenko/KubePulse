# Tasks

## 1. Remove the first pass

- [x] 1.1 In `crates/kubepulse-ui/src/resource_browser.rs`, delete `STATUS_COLUMN_MIN`, `TIMESTAMP_COLUMN_MIN`, `NUMERIC_COLUMN_MIN`, `GENERIC_COLUMN_MIN`, `declared_columns_min_width`, the fixed `.width(...)` calls on the name, namespace, declared, and computed columns, and the `mod tests` that covered them. Verify with `cargo build -p kubepulse-ui` and `cargo clippy --workspace --all-targets`, with no `dead_code` allowances left.

## 2. Roles and initial widths (design D1)

- [x] 2.1 Add `ColumnRole` (Name, Namespace, Status, Timestamp, Text) with the initial, min, and auto-cap widths from the D1 table. Assign the role while building `table_columns`: Timestamp for the `created` column and for declared columns whose path ends in `Timestamp` or `Time`, such as CronJob "Last run". Keep the roles on `RowsDelegate` by column index. Verify by unit-testing the role assignment for Pod, CronJob, and a custom resource with printer columns, in `columns.rs` or `resource_browser.rs` tests.
- [x] 2.2 Build every data column with `.width(initial).min_width(min)`, with the initial width raised to at least the header's measured width (title plus sort icon, as in 3.1). Verify with a `tests/browsing.rs` test: before any rows arrive, every data column's `col_width` is at least its role's initial width and at least its header's measured width.

## 3. Measuring content (design D2)

- [x] 3.1 Implement `RowsDelegate::fit_column`: pinned columns return `None`. Otherwise, pick the 8 longest values by character count from `cell_text`, shape them and the header with the window's text system, and return the maximum. Headers count the sort icon (0.75 rem plus 4 px), and status cells use the tag's text size, padding, and border. When `auto_fit` is set, limit the result to the role's cap minus cell padding. Verify by building the crate and with the tests in 4.x.
- [x] 3.2 Move the tag's text size, horizontal padding, and border into named constants in `crates/kubepulse-ui/src/status.rs`, used by both `status_tag` and the measurement. Verify that `status_tag` still renders by running `cargo test -p kubepulse-ui` (the existing status and browsing tests pass).

## 4. Automatic fitting (design D3 and D4)

- [x] 4.1 On the first snapshot with rows, in `apply_snapshot`, set `auto_fit`, call `fit_col` for each data column that is not user-sized, clear `auto_fit`, record `applied` from `col_width`, and set `fitted`. Verify with `tests/browsing.rs` tests: after the first rows arrive, the Created, Namespace, Node, and IP columns are each at least the measured width of their widest loaded value, and a column holding one value wider than its cap ends at that cap.
- [x] 4.2 Leave widths alone after the first fit. Verify with a test: rows that arrive later with a longer value don't change any `col_width`.
- [x] 4.3 Fit each computed column the first time it gains a value, using the same path. Verify by extending `computed_cells_fill_in_when_the_extension_answers` (or adding a test beside it): the computed column fits its values, and no other column changes width.
- [x] 4.4 Track user resizes from `TableEvent::ColumnWidthsChanged` against `applied`, and skip user-sized columns in 4.1 and 4.3. Verify with a test: resizing a computed column through the table before its values arrive keeps that width afterward.
- [x] 4.5 Verify that a double-click fit ignores the cap. In a test, call `fit_col` on Name directly (with `auto_fit` unset) for a row whose name is wider than 480 px, and check that the width reaches the name's measured width.
- [x] 4.6 Verify the pinned columns: after the automatic fit, the checkbox column is still 40 px and the actions column is still `ACTIONS_PADDING + ACTION_WIDTH * n`.
- [x] 4.7 Verify the cost at scale: in `the_table_shows_declared_columns_at_ten_thousand_rows` (or a sibling test), the automatic fit over 10k rows finishes within the frame budget the existing scale tests use.

## 5. Ellipsis (design D5)

- [x] 5.1 Make the header text in `render_th` and plain cell text in `render_td` end in an ellipsis when they overflow (`text_ellipsis`, `overflow_hidden`, `w_full`). Verify by running the app and dragging a column narrower than its values: the values end in "…". Also check that copying a cell still gives the full value.

## 6. Integration checks

- [x] 6.1 Run `cargo fmt --all --check`, `cargo clippy --workspace --all-targets`, `cargo test --workspace`, `python3 scripts/check-i18n.py`, and `python3 scripts/check-crate-graph.py`. All must pass.
- [x] 6.2 Run the app against the `kubepulse-e2e` minikube profile only (`KUBECONFIG=$PWD/target/e2e.kubeconfig cargo run`). Open Pods, Deployments, CronJobs, Services, Secrets, PersistentVolumeClaims, and a custom resource. Confirm that every header and value is readable without resizing, that status tags are whole, that Created shows a full timestamp, and that no column has a large blank margin.
- [x] 6.3 Repeat 6.2 with the `en-XA` pseudo-locale selected, and confirm that the longer headers are not cut off.
- [x] 6.4 Run `openspec validate improve-table-column-widths --strict`, which must pass.

## Workflow follow-up

- Archive the change with `/opsx:archive` after review.
