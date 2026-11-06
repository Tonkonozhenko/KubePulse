# Tasks

## 1. Status columns in the contract and store

- [x] 1.1 Add `ColumnSpec::first_of` with a list of paths and a public `evaluate`, and have `Row::project` keep the first non-empty cell. Verify with unit tests in `row.rs` for first path, later path, no path, and the waiting-reason filter path.
- [x] 1.2 Add `ColumnSource::Status { paths }`, move `CONTRACT_VERSION` to 1.3, and handle the variant in `columns_for`, with invalid paths skipped and logged. Verify with tests in `columns.rs`, and that a 1.2 extension still loads in `crates/kubepulse-extension-host/tests/host.rs`.
- [x] 1.3 Declare the Pod, PersistentVolumeClaim, and PersistentVolume Status columns with the new source, and treat printer columns named Status, Phase, or State, in any case, as status columns. Verify with tests in `kubepulse-builtin` and `columns.rs`.
- [x] 1.4 Document status columns in `docs/extensions.md`. Verify with `cargo test -p kubepulse-extension-api --doc`.

## 2. Status tones

- [x] 2.1 Add `status.rs` with `Tone`, `status_tone`, `condition_tone`, and the tag helper from the design. Verify with unit tests for each table row, letter case, an unknown value, problem-type polarity, and Unknown.
- [x] 2.2 Render status columns as toned tags in `RowsDelegate::render_td`. Verify in `crates/kubepulse-ui/tests/browsing.rs` that the text is unchanged, including a crash-looping pod, and that `the_table_shows_declared_columns_at_ten_thousand_rows` passes.

## 3. Row actions

- [x] 3.1 Add the pinned actions column with kind-dependent width and shift `leading_columns`, `sort_key`, and `cell_text`. Verify that sorting tests pass with updated column indices and that a Pods row has Logs while a Deployments row does not.
- [x] 3.2 Add the `BrowserEvent` action variants and route them in `ClusterTab`. Verify with tests that clicking each button opens the right tab or dialog and does not open the detail panel.
- [x] 3.3 Add the right-click menu with `context_menu`, capturing the row. Verify with a test that a row moving after the menu opens still targets the same object.

## 4. Multi-selection

- [x] 4.1 Add `checked` and `anchor` to `RowsDelegate`, and the checkbox column with its click wrapper. Verify with tests that a click toggles one row and does not open the detail panel.
- [x] 4.2 Implement Shift+click ranges. Verify with tests for downward, upward, after sorting, no anchor, an anchor that left the table, and that already checked rows outside the range stay checked.
- [x] 4.3 Add the header checkbox. Verify with tests for selecting all shown and clearing.
- [x] 4.4 Retain only present keys in `apply_snapshot`. Verify with tests for rows arriving above a checked row, a filter hiding a checked row, and an object deleted elsewhere.
- [x] 4.5 Add the selection bar with the count, delete, and clear. Add a test that selecting all of 10,000 rows completes within 100 milliseconds.

## 5. Delete dialog

- [x] 5.1 Move the dialog from `detail.rs` to `delete_dialog.rs` as `DeleteDialog`, keeping its element ids and enabling `futures` in `crates/kubepulse-ui/Cargo.toml`. Verify that the existing delete tests pass unchanged.
- [x] 5.2 Support several targets: the virtualized object list, the typed cluster name on protected clusters, and Enter not confirming. Verify with tests for each spec scenario.
- [x] 5.3 Run deletions with a limit of 8, with progress, cancel, per-object failures, retry of failures only, and `NotFound` counted as deleted. Extend `FakeServices` so a delete can fail, and verify with tests for partial failure, retry, already gone, and cancel.
- [x] 5.4 Wire the row action and the selection bar to the dialog, and handle `Deleted` in `ClusterTab`. Verify with tests that deleted rows are unchecked and that deleting the open object closes the detail panel.

## 6. Detail panel

- [x] 6.1 Add the close button and `DetailEvent::Close`, `ClusterTab::close_detail`, and `ResourceBrowser::clear_open`, and close on Escape. Verify with tests for closing, reopening the same row, Escape, and checked rows surviving.
- [x] 6.2 Split the panel into a fixed header and a scrolling body, with the status from the kind's status column. Verify with tests that the table and the panel agree for a crash-looping pod and that the existing button ids still work.
- [x] 6.3 Render labels as wrapping tags and conditions as rows with tone, reason, and message. Verify with tests on a pod and on a `DiskPressure` condition.
- [x] 6.4 Render events with their severity. Verify with a test on a Warning event and that its message appears as received.
- [x] 6.5 Add the collapsed annotations section, with keys only on Secrets. Verify with tests that a Deployment shows values, and that a Secret's `last-applied-configuration` value never appears.

## 7. Zoom

- [x] 7.1 Override `zoomable` to return false on `ClusterTab`, `LogTab`, `ManifestTab`, and `SettingsTab`. Verify with a test in `crates/kubepulse-ui/tests/shell.rs` that a zoom request leaves every kind of tab unzoomed, with one pane and when split.

## 8. Integration

- [x] 8.1 Add the new strings to `locales/en.json` with no plurals, and run `python3 scripts/check-i18n.py --write`. Verify that it passes with `en-XA` regenerated.
- [x] 8.2 Record in `TODO.md` that extension actions are not rendered in rows or the detail panel. Verify that the entry is under Decide.
- [x] 8.3 Run `cargo fmt --all --check`, `cargo clippy --workspace --all-targets`, `cargo test --workspace`, `python3 scripts/check-crate-graph.py`, and `cargo deny check licenses`. Verify that all pass.
- [ ] 8.4 Run the app with `/run` against the `kubepulse-e2e` minikube profile, and check tags, shift-selection, multi-deletion, the right-click menu, and closing the panel in light, dark, and `en-XA`. Verify by recording what was checked in the change's final report.
