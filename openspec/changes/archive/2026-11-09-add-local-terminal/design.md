# Design

## Context

Container shells already work end to end:

- `kubepulse-cluster/src/exec.rs` defines `exec_channel(size)`, which returns an `ExecFeed` for the producer and an `ExecStream` for the tab. The stream carries output bytes, typed input, size changes, and an `ExecState` (`Connecting`, `Running`, `Exited(code)`, `Dropped`, `Failed(reason)`, `NoShell`). Dropping the stream ends the session.
- `kubepulse-ui/src/terminal_tab.rs` owns the emulator (`terminal::Emulator`, built on the terminal emulation library already in the workspace), painting, keyboard and mouse input, selection, copy, and paste. It is bound to a cluster: it holds a `ClusterKey`, an `ObjectTarget`, the `ClusterModel`, a container list, and the no-shell command prompt, and it reaches the cluster through `ClusterServices::open_exec`.
- `ClusterServices` (`kubepulse-ui/src/services.rs`) is the seam between the UI and background work. `RuntimeServices` enters the Tokio runtime to start work, and UI tests substitute a fake.
- The rail footer in `cluster_rail.rs` holds the port-forwards button, then the Settings gear, each an icon-only ghost `Button` that dispatches an action handled by `Workspace`.
- `IconName::SquareTerminal` is already in the icon catalog and used in container shell tab titles.

See proposal.md for motivation and `specs/shell/local-terminal/spec.md` for the required behavior.

## Goals / Non-Goals

**Goals:**
- Reuse the existing emulator, painting, and input handling unchanged for local shells.
- Keep blocking pseudo-terminal reads and process waits off the UI thread.
- End every local shell and its process group when its tab closes or the application quits.

**Non-Goals:**
- Choosing a shell, its arguments, or its starting directory in Settings. The platform default is used for now.
- Opening a local shell already pointed at the selected cluster, for example with a context flag or a temporary kubeconfig. That would make it easy to run commands against a protected cluster by accident, and needs its own design.
- Splitting `TerminalTab` into a separate reusable view entity. A source enum is enough while there are only two sources.

## Decisions

### The pseudo-terminal comes from `portable-pty`

`portable-pty` (MIT) opens a pseudo-terminal on Linux, macOS, and Windows (through the console pseudo-terminal API), spawns a child in it, resizes it, and kills it. `CommandBuilder::new_default_prog()` picks the user's login shell on Unix, from `$SHELL` or the account database, and `%ComSpec%` on Windows, which matches the spec's "default shell for the platform". The builder sets the working directory to the home directory and adds `TERM=xterm-256color` and `COLORTERM=truecolor`, which the emulator supports. The rest of the environment is inherited, so `KUBECONFIG` and `PATH` match the application's.

The alternatives were the emulator library's own `tty` module, which needs its event loop and default features the workspace turns off, and writing pseudo-terminal code per platform, which the reuse rule rules out.

### The local shell lives in `kubepulse-cluster` and reuses the exec stream

A new `kubepulse-cluster/src/local_shell.rs` exposes `open_local_shell(size) -> ExecStream`. It calls `exec_channel` and spawns:

- a blocking reader that sends master output to `ExecFeed::output`,
- a blocking writer that sends typed input to the master,
- a task that watches `ExecFeed::size` and resizes the pseudo-terminal,
- a blocking waiter that sets `ExecState::Exited(code)` when the child ends.

State goes to `Running` once the child spawns, and to `Failed(error)` with the platform's message when opening the pseudo-terminal or spawning fails.

When the tab drops the stream, the session closes every handle on the pseudo-terminal, as closing a terminal window does. The system then hangs up the terminal, which ends the program in front, and the shell ends the jobs it started. Sending the shell a hangup signal instead is not enough: a shell idle at its prompt can exit on that signal and leave its jobs running while the terminal stays open. On Unix the reader and writer threads each hold their own copy of the master, and each blocking call also waits on a pipe that closes with the session, so no thread keeps the terminal open. On Windows, closing the console pseudo-terminal ends the programs attached to it. The writer from the pseudo-terminal library types end-of-input when it is dropped, which would let the shell exit before the hangup without ending its jobs, so it is kept until the session ends.

`kubepulse-cluster` already owns every terminal stream type, and the crate graph lets `kubepulse-ui` use it, so no new crate or graph entry is needed. The alternative was a module in `kubepulse-ui`, which would put blocking process work in the toolkit crate and duplicate the stream plumbing.

### `ClusterServices::open_local_shell` starts it

`ClusterServices` gains `fn open_local_shell(&self, size: ExecSize) -> ExecStream`. `RuntimeServices` enters the runtime and calls the cluster crate, the same way `open_exec` does, and the UI test fake returns an `exec_channel` it can feed. This keeps UI tests free of real processes. The local shell is not attached to any cluster session, so disconnecting or cancelling a cluster never touches it.

### `TerminalTab` gets a source enum

`TerminalTab` replaces its cluster fields with:

```rust
enum Source {
    Container { key, target, requested, model, containers, selected, command, .. },
    Local,
}
```

Emulator, painting, input, selection, copy, and paste stay shared. For `Local`:

- `start_session` calls `open_local_shell` instead of loading containers and calling `open_exec`.
- Connection tracking (`sync_with_connection`, `can_reconnect`) is skipped, so a cluster disconnecting never affects it.
- The title shows the terminal icon and `t!("local_terminal.title")`, with no cluster identity.
- The ended-state bar shows a start-again button that calls `start_session` again, after `restore_modes`, below the kept scrollback. `NoShell` and the command prompt do not apply.
- `key()` becomes `Option<&ClusterKey>`, and `Workspace` treats a `None` key like the Settings tab when it follows the active tab, so the selected cluster stays the same.

The alternative was a separate `LocalTerminalTab` that shares a new terminal-view entity with `TerminalTab`. It would be cleaner if more sources appear, but today it adds a layer for one extra variant.

### `OpenLocalTerminal` action, rail button, and palette entry

A new `OpenLocalTerminal` action is handled by `Workspace::open_local_terminal`, which mirrors `open_terminal`: create the tab, add it to the dock, and focus the grid. The rail footer adds a ghost `Button` with `IconName::SquareTerminal` between the port-forwards button and the Settings gear, so it sits directly above the gear. `PaletteAction::OpenLocalTerminal` adds a palette entry in the same group as Settings. There is no default keyboard shortcut, because common terminal shortcuts collide with keys shells need.

### Quitting

Quitting already drops every tab. Dropping the `ExecStream` runs the kill guard, so no new quit hook is needed. Local terminal tabs are not persisted, like every other tab.

## Risks / Trade-offs

**Risk** → A blocking reader or writer thread could keep the pseudo-terminal open after the tab closes, so the hangup never comes. → On Unix each blocking call also waits on a pipe that closes with the session. Unit tests check that closing ends a foreground program and a background job in two common shells.

**Risk** → A program that ignores the hangup, such as one started through a no-hangup wrapper, keeps running after the tab closes. → This matches closing a terminal window, and the user asked for that program to survive.

**Risk** → On Windows the default program is `cmd.exe`, which many users would not pick. → It follows the platform default the spec names. A shell preference in Settings is the planned follow-up and does not change these requirements.

**Risk** → The console pseudo-terminal API needs Windows 10 version 1809 or later. → Check the toolkit's minimum Windows version during implementation. If it is lower, an older system gets the `Failed` state with the platform's error, as the spec requires.

**Risk** → A shell started from a desktop launcher on macOS can miss the user's `PATH` additions. → Login shells read the user's profile, and `new_default_prog` starts the shell as a login shell on macOS.

**Risk** → Killing a shell with a running job loses that job's work without a prompt. → The spec accepts this to match container shells and quitting, and closing a tab is an explicit action.
