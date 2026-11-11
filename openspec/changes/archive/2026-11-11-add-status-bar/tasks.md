# Tasks

## 1. Actions column width (design D3)

- [x] 1.1 Replace `ACTIONS_PADDING` and `ACTION_WIDTH` with `actions_width(n, rem)`, built from `ACTION_BUTTON_REMS`, `ACTION_GAP_REMS`, and `CELL_PADDING`. Verify by updating the pinned-column assertion in `the_first_rows_fit_each_column_up_to_its_cap` to the new pod width, and with a `tests/browsing.rs` test that the rendered actions row of the first pod row fits inside the actions column (its right edge is within the column, with at most the cell padding to spare).

## 2. Status bar (design D1)

- [x] 2.1 Make `state_mark` and `state_label` public. Add `crates/kubepulse-ui/src/status_bar.rs` with the `StatusBar` view, its observers, and the five items, and add its text keys (`status.work`, `status.forwards`) to `locales/en.json`. Regenerate `en-XA` with `python3 scripts/check-i18n.py --write`. Verify that the crate builds and that `check-i18n.py` passes.
- [x] 2.2 Lay `StatusBar` out under the rail, sidebar, and dock in `Workspace::render`. Verify with a `tests/shell.rs` test: with a connected cluster selected, `status-cluster` and `status-state` exist and the state reads Connected; switching the selection to a second cluster shows that cluster's name; with nothing selected, `status-cluster` is absent.
- [x] 2.3 Verify the broken-connection items with tests: a Failed cluster shows `status-reason` with the reason text, a Degraded cluster also shows the stale note, and a Connected cluster shows no `status-reason`.
- [x] 2.4 Verify the work and forwards items with tests: a cluster with active work labels shows `status-work` with the count, and none without work. A cluster with one forward in the registry shows `status-forwards`, and clicking it opens the forwards list, as the rail button's existing test checks.

## 3. List view top (design D2)

- [x] 3.1 Add the `Notes::All` and `Notes::Actions` choice to `connection_notes`. Pass `Actions` from `ClusterTab` and `All` from the logs, shell, and editor tabs. Stop rendering the name in the list view. Verify with `tests/browsing.rs` tests: a connected list view has no `tab-state` row, a disconnected one still has `tab-connect`, and the existing log, shell, and editor tests that check `tab-state` still pass.

## 4. Integration checks

- [x] 4.1 Run `cargo fmt --all --check`, `cargo clippy --workspace --all-targets`, `cargo test --workspace`, `python3 scripts/check-i18n.py`, and `python3 scripts/check-crate-graph.py`. All must pass.
- [x] 4.2 Run the app against the `kubepulse-e2e` profile only. Confirm that the status bar follows the active tab, shows a failure reason when a context fails, counts a shell and a port-forward, and opens the forwards list. Confirm that the list view starts right under its color stripe and that the actions column hugs its buttons, in `en` and `en-XA`.
- [x] 4.3 Run `openspec validate add-status-bar --strict`, which must pass.

## Workflow follow-up

- Archive the change with `/opsx:archive` after review.
