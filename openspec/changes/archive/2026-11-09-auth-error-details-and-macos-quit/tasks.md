# Tasks

## 1. Authentication failure details

- [x] 1.1 In `kubepulse-cluster/src/session.rs`, set `interactive_mode` to `Never` on every auth info with `exec` in the parsed `Kubeconfig` before `Config::from_custom_kubeconfig`; verify the existing helper tests in `tests/sessions.rs` still pass
- [x] 1.2 Rewrite the authentication arms of `describe` to map each `AuthError` variant as listed in design.md: `AuthExecRun` gives the command, exit status, and trimmed stderr and never reads stdout; `AuthExecStart` names the command taken from the kubeconfig; parse, serialize, and free-form variants use fixed text; verify with `cargo clippy -p kubepulse-cluster --all-targets`
- [x] 1.3 Change `examples/fake_credential_helper.rs` to take an optional stderr message, so that on failure it prints the token only to stdout and the message only to stderr, and update its doc comment and the `Auth::Helper` support in `tests/support/mod.rs`; verify `cargo test -p kubepulse-cluster` builds
- [x] 1.4 Add a test in `tests/sessions.rs` where a helper exits with status 3 after writing a message to stderr, and assert that the failure reason names the command, the status, and the message; verify it passes
- [x] 1.5 Add a test in `tests/sessions.rs` where the helper command does not exist, and assert that the reason names the command; verify it passes
- [x] 1.6 Update `tests/credential_logs.rs` so the failing helper writes the token to stdout and a marker to stderr, then assert that the reason contains the marker but not the token, and that the logs contain neither; verify `cargo test -p kubepulse-cluster --test credential_logs` passes

## 2. Quitting

- [x] 2.1 In `kubepulse-ui`, add a `Quit` action handled by `cx.quit()`, with a `QUIT_SHORTCUT` bound globally: `cmd-q` on macOS, `ctrl-q` on Linux, and `alt-f4` on Windows, following `SETTINGS_SHORTCUT`; call its setup from `app.rs`; verify with a UI test that pressing `QUIT_SHORTCUT` dispatches `Quit`
- [x] 2.2 In `terminal_tab.rs` on Linux, bind `ctrl-q` in the terminal context to a new `SendXon` action that types `0x11`; verify with a test in `tests/terminal.rs` that Ctrl+Q in a focused terminal sends `0x11` to the shell and does not dispatch `Quit`
- [x] 2.3 In `app.rs`, register `cx.on_window_closed` to call `cx.quit()` when no windows remain; verify with `cargo run` that closing the window ends the process

## 3. Integration checks

- [x] 3.1 Run `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --all --check`, and `python3 scripts/check-i18n.py`; verify all pass
- [x] 3.2 Start the app with a kubeconfig context whose helper fails with a message, connect it, and verify that the rail tooltip and sidebar show the command, exit status, and message
- [x] 3.3 Verify on each platform available that its quit shortcut quits the app, and on Linux that Ctrl+Q in a terminal tab reaches the shell

## Workflow follow-up

- Archive the change after implementation, so `shell/lifecycle` becomes a main spec and the `general/cluster-sessions` requirement is added.
