# Tasks

## 1. Selecting log text

- [x] 1.1 Add the content key encoding `(seq, offset)` as `seq << 16 | offset` to `log_tab.rs`, with unit tests that it round-trips and that the largest shown text (a cut line with a timestamp prefix) fits in 16 bits; verify with `cargo test -p kubepulse-ui --lib log_tab`
- [x] 1.2 Give `LogTab` a `TextSelectionHandle` and replace the row's `StyledText` with a small element that registers with `document_order = seq` and one `TextSelectionRun`, painting the projected range in `cx.theme().selection` beneath search highlights; wire `resolve_content_key_with` to painted rows, and verify with a test in `crates/kubepulse-ui/tests/logs.rs` that dragging from one line to a later one selects across them
- [x] 1.3 Clear the selection in `start_stream` and when timestamps are toggled, and verify with tests for choosing another container, the previous instance, and toggling timestamps
- [x] 1.4 Give the selection hitbox a content mask of the rows' own area, so drag auto-scroll sends wheel events to the list and a drag past the top turns following off through `notice_scrolling`; verify with a test that drags above the view while following and finds the follow checkbox off
- [x] 1.5 Add tests that a double click selects a word, a triple click selects a line, new lines arriving keep the selection on the same text, and dropped lines shrink the selection to retained ones; verify `cargo test -p kubepulse-ui --test logs` passes

## 2. Copying log text

- [x] 2.1 Implement `copy_with` to read the selected lines from the buffer, rebuild shown text, join lines with `\n`, copy a cut line whole when the range reaches the cut marker, and start at the oldest retained line when the start was dropped; cover the text building with unit tests for partial lines, timestamps, and a cut line, and verify with `cargo test -p kubepulse-ui --lib log_tab`
- [x] 2.2 Track a focus handle on the line area so a click there takes focus from the search field, and verify with a test that selects three lines, dispatches the platform copy shortcut, and reads the expected text from the test clipboard
- [x] 2.3 Add a test that the copy shortcut in a focused search field copies the field's own selection and leaves the clipboard without log text, and verify `cargo test -p kubepulse-ui --test logs` passes

## 3. Wrapping lines

- [x] 3.1 Add the `log-wrap` checkbox after Timestamps with `log.wrap` ("Wrap lines") in `crates/kubepulse-ui/locales/en.json`, keep its value across `start_stream`, run `python3 scripts/check-i18n.py --write`, and verify `python3 scripts/check-i18n.py` passes
- [x] 3.2 While wrapping, render `gpui::list` from a `ListState` with `ListMeasuringBehavior::Visible`, rows with normal whitespace, the vertical scrollbar from the `ListState`, and no horizontal scrollbar; verify with a test that a wide line spans more than one row height, the horizontal scrollbar is gone, and turning wrapping off restores one row per line
- [x] 3.3 Splice appends and drops into the `ListState` in `apply_status`, pin to the end on render while following, notice a scroll up from `logical_scroll_top`, and use `scroll_to_reveal_item` in `go_to_match`; verify with wrapped versions of the follow, scroll-up, scrollbar, dropped-lines-while-paused, and search-match tests
- [x] 3.4 Carry the top visible line across turning wrapping on and off while paused, and pin to the end while following; verify with tests for both cases in each direction
- [x] 3.5 Add tests that resizing the window rewraps lines, that a new container keeps wrapping on, and that selection and copy across wrapped lines put line breaks only between log lines; verify `cargo test -p kubepulse-ui --test logs` passes

## 4. Integration

- [x] 4.1 Run `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --all --check`, and `python3 scripts/check-crate-graph.py`, and verify all pass
- [ ] 4.2 On the `kubepulse-e2e` minikube profile, open logs in a real window for a quiet pod and for a pod writing at high volume, and confirm selection by drag, word, line, and shift-click, copying with the platform shortcut, drag auto-scroll, the wrap checkbox on and off with following and paused, and a responsive view while wrapped at high volume, in light, dark, and `en-XA`
