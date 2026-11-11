# Tasks

## 1. Age formatting (design D1)

- [x] 1.1 Add `chrono` as a direct dependency of `kubepulse-ui` at the lockfile's version, with only the features needed for RFC 3339 parsing. Verify that `cargo deny check licenses` passes and that `cargo about generate --locked --fail -o assets/third-party-notices.txt about.hbs` leaves the notices current.
- [x] 1.2 Add `crates/kubepulse-ui/src/age.rs` with `age(text, now) -> Option<String>` following the D1 table, and add the unit keys to `locales/en.json`. Run `python3 scripts/check-i18n.py --write` to regenerate `en-XA`. Verify with unit tests at each boundary in the table (119 s, 2 min, 9 min 59 s, 3 h, 8 h, 2 d, 8 d, 2 y), plus a zero second part, invalid text, and a future time.

## 2. Ages in tables (design D2–D5)

- [x] 2.1 Add `display_text` to `RowsDelegate`, and use it in `render_td` and `longest_values`, leaving `cell_text` as received. Timestamp cells get an id and a tooltip with the raw value. Verify with a `tests/browsing.rs` test: a pod created a known time ago shows its age in the rendered cell (find the element by its id), while `dump_range` still gives the RFC 3339 value. Also verify that a non-time value in a timestamp column shows as received.
- [x] 2.2 Refresh tables that have a timestamp column once a second (D3). Verify with a test that advances the test clock by 60 s and checks that the table redrew at least 60 times, with no event from the cluster. Ages read the wall clock, so the test checks the redraws that pick up the new time; the test fails with no redraws when the refresh is slowed.
- [x] 2.3 Reverse the sort direction for timestamp columns in `perform_sort` (D4). Verify with a test: sorting Created ascending puts the most recently created pod first.
- [x] 2.4 Change the `Timestamp` role widths to 90 / 60 / 160 (D5). Verify by updating the expected initial width in `an_empty_table_starts_every_column_wide_enough_for_its_role_and_header`, and by checking that `the_first_rows_fit_each_column_up_to_its_cap` passes with Created fitted to its age.

## 3. Compact conditions (design D6)

- [x] 3.1 Rewrite `render_conditions` as wrapping `Type=Status` tags with tooltips, plus one muted line for each condition that has a reason or message. Add the `detail.condition_detail` key and regenerate `en-XA`. Verify with a `tests/browsing.rs` test: a pod with a `Ready=False` condition that has a reason and message shows a `condition-0` tag reading `Ready=False` and a detail line with the reason and message. A condition with neither adds no line. Check that the existing condition tests still pass.

## 4. Project rule (design D7)

- [x] 4.1 Add the timestamp exception to the **Text** standard in `AGENTS.md`. Verify by reading the updated line.

## 5. Integration checks

- [x] 5.1 Run `cargo fmt --all --check`, `cargo clippy --workspace --all-targets`, `cargo test --workspace`, `python3 scripts/check-i18n.py`, `python3 scripts/check-crate-graph.py`, and `cargo deny check licenses`. All must pass.
- [x] 5.2 Run the app against the `kubepulse-e2e` profile only (`KUBECONFIG=$PWD/target/e2e.kubeconfig cargo run`). Confirm that Created and CronJob "Last run" show ages that tick up, that hovering shows the exact time, and that a pod's conditions read as compact tags with any reasons below them, in both `en` and `en-XA`.
- [x] 5.3 Run `openspec validate compact-dates-and-conditions --strict`, which must pass.

## Workflow follow-up

- Archive the change with `/opsx:archive` after review.
