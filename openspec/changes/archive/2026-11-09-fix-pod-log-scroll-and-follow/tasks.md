# Tasks

## 1. Follow state

- [x] 1.1 Pin to the end from `Render::render` while following, stop scrolling in `apply_status`, and drop the `settled` check from `notice_scrolling`; verify with a new test in `crates/kubepulse-ui/tests/logs.rs` that shrinks the window on a quiet stream and still finds the newest line, which fails before this change
- [x] 1.2 Add the `log-follow` checkbox after the Timestamps button, and remove `jump_to_newest`, the `log-jump` button, and `log.jump`; add `log.follow` to `locales/en.json`, run `python3 scripts/check-i18n.py --write`, and verify `python3 scripts/check-i18n.py` passes
- [x] 1.3 Update `the_view_follows_new_lines_until_the_user_scrolls_up` to use the checkbox, and add tests for pausing and resuming with the checkbox, a search match turning it off, and a new container starting with it on; verify `cargo test -p kubepulse-ui --test logs` passes

## 2. Horizontal scrolling

- [x] 2.1 Add `unicode-width` to `crates/kubepulse-ui/Cargo.toml` and a `WidestLines` sliding maximum in `log_tab.rs`, with unit tests for a wider line replacing narrower ones, the widest line leaving while a narrower one takes over, and display width ranking over byte length; verify with `cargo test -p kubepulse-ui --lib log_tab` and `cargo deny check licenses`
- [x] 2.2 Feed `WidestLines` from `apply_status`, reset it in `start_stream`, and verify with the scroll tests in 2.4
- [x] 2.3 Switch the list to `ListHorizontalSizingBehavior::Unconstrained` with `with_width_from_item`, move the font and text size to a wrapper, and add the horizontal scrollbar strip; verify `exists("log-line-…")` assertions in `crates/kubepulse-ui/tests/logs.rs` still pass
- [x] 2.4 Add tests that scroll the view sideways: a wide line far from row 0 is reachable, the width shrinks when that line is dropped, following and the sideways offset both survive new lines, and lines that fit do not scroll; expose only what they need, and verify `cargo test -p kubepulse-ui --test logs` passes
- [x] 2.5 Add the vertical scrollbar in a strip beside the list, with the horizontal strip stopping short of it, and verify with a test that moving it up turns the checkbox off
- [x] 2.6 Make the Timestamps toggle a checkbox, and verify `cargo test -p kubepulse-ui --test logs` passes
- [x] 2.7 Update the stale doc comments on `scrolled` and `notice_scrolling` in `log_tab.rs`, and verify `cargo clippy --workspace --all-targets` and `cargo fmt --all --check` are clean

## 3. Previous instance

- [x] 3.1 Make Previous instance a checkbox that is always shown and always enabled, with no restart check, and verify with a test in `crates/kubepulse-ui/tests/logs.rs` and `python3 scripts/check-i18n.py`

## 4. Integration

- [x] 4.1 Run `cargo test --workspace` and `python3 scripts/check-crate-graph.py`, and verify both pass
- [x] 4.2 On the `kubepulse-e2e` minikube profile, open logs in a real window for a pod with a long quiet log and for a busy one, and confirm the view opens at the newest line, the checkbox pauses and resumes, the horizontal scrollbar and shift+wheel reach the end of a long line, the vertical scrollbar moves through the lines, Previous instance shows for each container of a multi-container pod, and scrolling up clears the checkbox, in light, dark, and `en-XA`

## Workflow follow-up

- Archive this change after `define-app-architecture`, because its `pod/log` delta modifies a requirement that exists only once that change is archived.
