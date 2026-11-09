# Proposal

## Why

When a credential helper fails, the cluster shows only "authentication failed". The user cannot tell an expired login from a missing program or a broken kubeconfig entry, because the helper's own message is thrown away. The application also has no quit shortcut: Command+Q does nothing on macOS, and no platform has a keyboard way to quit.

## What Changes

- A credential helper failure names the helper command and its exit status, followed by what the helper wrote to its error output. Its standard output, where credentials go, is never shown.
- Other authentication failures, such as a missing helper program or an unreadable token file, name the specific cause instead of the generic text.
- Helper error output appears in the failure reason only. It is never written to logs.
- Credential helpers run non-interactively, so their error output can be captured. A desktop application has no terminal for a helper to prompt in.
- Quitting by keyboard uses each platform's standard shortcut: Command+Q on macOS, Ctrl+Q on Linux, and Alt+F4 on Windows. On Linux, a focused terminal tab keeps receiving Ctrl+Q.
- Closing the main window quits the application on every platform, including macOS, where an application without windows would otherwise keep running with no way to reopen one.

## Capabilities

### New Capabilities
- `shell/lifecycle`: How the application quits, through the platform's quit shortcut or by closing its window.

### Modified Capabilities
- `general/cluster-sessions`: The connection failure reason for authentication failures carries the helper's error output and the specific cause, and the credential handling requirement says which helper output may be shown and that none of it is logged.

## Impact

- `kubepulse-cluster`: `session.rs` builds the failure reason from each authentication error, and forces helpers to run non-interactively. The test credential helper and `tests/credential_logs.rs` change, because the helper's error output is now meant to appear in the reason.
- `kubepulse-ui`: a quit action with per-platform bindings, and quitting when the last window closes. No new user-visible text: the shortcut has no menu item.
- No new dependencies.
