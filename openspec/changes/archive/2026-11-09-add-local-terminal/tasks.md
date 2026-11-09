# Tasks

## 1. Local shell process

- [x] 1.1 Add `portable-pty` to `[workspace.dependencies]` and `crates/kubepulse-cluster/Cargo.toml`, regenerate `assets/third-party-notices.txt` with `cargo about generate --locked --fail -o assets/third-party-notices.txt about.hbs`, and verify `cargo deny check licenses` and `python3 scripts/check-crate-graph.py` pass
- [x] 1.2 Add `kubepulse-cluster/src/local_shell.rs` with `open_local_shell(size) -> ExecStream`: spawn `CommandBuilder::new_default_prog()` in the home directory with `TERM=xterm-256color` and `COLORTERM=truecolor`, pump output on a blocking reader, write typed input, apply size changes, set `Running`, `Exited(code)`, or `Failed(error)`, and, when the stream's drop guard fires, close every handle on the pseudo-terminal so the system hangs it up; export it from `lib.rs` and verify `cargo build -p kubepulse-cluster`
- [x] 1.3 Add Unix-only unit tests in `local_shell.rs` that run a local shell, type `echo` and `exit 3`, and see the echoed text and `Exited(3)`; that a resize reaches the shell (`stty size` reports it); and that dropping the stream ends a foreground `sleep` and a background `sleep` job in interactive bash and zsh (CI installs zsh on Linux); verify `cargo test -p kubepulse-cluster --lib local_shell` passes

## 2. Terminal tab with a local source

- [x] 2.1 Add `ClusterServices::open_local_shell(size)`, implement it in `RuntimeServices` by entering the runtime and calling the cluster crate, and make `FakeServices` in `crates/kubepulse-ui/tests/support` return a scripted `exec_channel`; verify `cargo build --workspace --all-targets`
- [x] 2.2 Move the cluster fields of `TerminalTab` into `Source::Container`, add `Source::Local` with a `TerminalTab::local` constructor, make `key()` return `Option<&ClusterKey>`, and skip container loading, connection tracking, and the command prompt for local tabs; verify the existing `cargo test -p kubepulse-ui --test terminal` and `--test shell` tests still pass
- [x] 2.3 Give local tabs the `SquareTerminal` icon with `local_terminal.title` ("Local terminal") and no cluster identity, and an ended bar with the exit code and a `local_terminal.start_again` ("Start again") button that restarts the shell below kept scrollback; add strings to `crates/kubepulse-ui/locales/en.json`, run `python3 scripts/check-i18n.py --write`, and verify `python3 scripts/check-i18n.py` passes
- [x] 2.4 Add tests in `crates/kubepulse-ui/tests/terminal.rs` that a local tab shows scripted output, sends typed keys and resizes to the feed, shows the exit code and keeps scrollback on exit, starts a new shell on Start again, shows a translated summary followed by the raw error on failure, keeps running when a cluster disconnects, and drops its stream when closed; verify `cargo test -p kubepulse-ui --test terminal` passes

## 3. Opening local terminals

- [x] 3.1 Add the `OpenLocalTerminal` action and `Workspace::open_local_terminal`, which adds a new local tab to the dock and focuses its grid, and have the active-tab follower treat a tab with no cluster key like Settings; verify with a workspace test that two opens create two active-in-turn local tabs and the selected cluster stays the same
- [x] 3.2 Add the `rail-local-terminal` ghost button with `IconName::SquareTerminal` and a `local_terminal.open` ("New local terminal") tooltip between the port-forwards button and the Settings gear in `cluster_rail.rs`; verify with a test that clicking it opens a local tab and that it is the element directly above `rail-settings`
- [x] 3.3 Add `PaletteAction::OpenLocalTerminal` with a "New local terminal" entry beside Settings and handle it in `Workspace`; verify with a command palette test that choosing it opens a local tab, then run `python3 scripts/check-i18n.py` and `cargo test -p kubepulse-ui`

## 4. Integration

- [x] 4.1 Run `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --all --check`, `python3 scripts/check-crate-graph.py`, and `python3 scripts/check-i18n.py`, and verify all pass
- [x] 4.2 Run the app on Linux and open two local terminals from the rail and one from the palette; confirm the button sits above the gear with its tooltip, a full-screen program renders and resizes, copy and paste work, `kubectl config current-context` shows the unchanged default context, `exit` shows the exit code and Start again works, and closing a tab with `sleep 1000` running leaves no `sleep` process, in light, dark, and `en-XA`
