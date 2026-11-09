# Proposal

## Why

Users often need a shell on their own machine next to the cluster views, for example to run a command-line client against the same kubeconfig, inspect a file, or script a quick check. Today they have to switch to a separate terminal application. The application already has a full terminal emulator for container shells, so it can offer a local shell with little new code.

## What Changes

- A new terminal button sits at the bottom of the cluster rail, directly above the Settings gear. Selecting it opens a new local terminal tab running the user's default shell on this machine, and focuses it.
- Each press opens another tab, so several local shells can run at once.
- The command palette gets a **New local terminal** entry that does the same.
- A local terminal tab belongs to no cluster. Activating it does not change the selected cluster, and its title shows a terminal icon and a translated "Local terminal" label instead of a cluster identity.
- The shell starts in the user's home directory with the application's environment, so command-line tools see the same default kubeconfig. It does not switch to the selected cluster's context.
- The tab uses the same terminal behavior as container shells: colors, full-screen programs, resizing, scrollback, selection, copy, and paste.
- When the shell exits, the tab shows the exit code, keeps its scrollback, and offers to start a new shell in the same tab.
- Closing the tab or quitting the application ends the shell and the programs it started, without a prompt. Local terminals do not return after a restart.

## Capabilities

### New Capabilities

- `shell/local-terminal`: opening, running, and ending interactive shells on the local machine from the rail and the command palette.

### Modified Capabilities

- `shell/icons`: the "Rail buttons" scenario now includes the local terminal button.

## Impact

- `crates/kubepulse-cluster`: a new `local_shell` module that spawns the default shell in a pseudo-terminal and feeds it through the existing `exec_channel` stream types.
- `crates/kubepulse-ui/src/terminal_tab.rs`: the tab gains a local source alongside the container source, skipping container choice and cluster connection tracking for local shells.
- `crates/kubepulse-ui/src/cluster_rail.rs`, `workspace.rs`, `command_palette.rs`: the rail button, an `OpenLocalTerminal` action, and the palette entry.
- `crates/kubepulse-ui/locales/en.json` and the generated `en-XA` pseudo-locale: new strings for the button tooltip, tab title, palette entry, and restart action.
- New dependency: `portable-pty` (MIT), which provides pseudo-terminals on Linux, macOS, and Windows. The third-party notices are regenerated.
