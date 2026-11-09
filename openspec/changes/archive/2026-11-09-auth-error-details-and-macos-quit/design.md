# Design

## Context

`describe` in `kubepulse-cluster/src/session.rs` turns every `kube::Error::Auth` and every `AuthError` wrapped in a service error into the fixed text "authentication failed". The same function produces the reason for a failed connection (`establish` → `probe`) and for a degraded one (`monitor` → `checked_probe`), so one change covers both.

The client library runs exec helpers in two modes. If the kubeconfig's `interactiveMode` is anything but `Never`, which includes the default, it inherits the parent's stdin and stderr. A desktop launch has no terminal behind them, so the helper's message is lost and `AuthExecRun` carries an empty stderr. With `Never`, stderr is captured into the error.

The test helper `examples/fake_credential_helper.rs` prints its token to both stdout and stderr on failure, and `tests/credential_logs.rs` asserts that the token appears in neither the reason nor the logs. That asserted the old rule that no helper output may be shown.

On the UI side, `gpui-kit` deliberately leaves `cmd-q`, `cmd-w`, and `alt-f4` unbound for the application to own. Nothing in KubePulse binds them or quits when the window closes. The terminal tab already overrides kit bindings inside its key context, such as `ctrl-c` → `SendInterrupt`.

## Goals / Non-Goals

**Goals:*
- Show the helper's own message for an authentication failure, and the specific cause for other authentication errors.
- Keep anything a helper writes to stdout out of the UI, and any helper output out of logs.
- One standard quit path per platform.

**Non-Goals:**
- Translating the cause text. Reasons from the cluster crate are English today, and this change keeps that.
- A macOS application menu. The shortcut is enough for this change, and a menu needs its own translated items.
- A confirmation before quitting with active shells or port-forwards.
- Details for an unusable kubeconfig entry, which keeps its current message.

## Decisions

**Map each `AuthError` variant to its own text in `describe`.** The rules for each variant:
- `AuthExecRun { out, .. }`: `the credential helper "<command>" failed with <exit status>` and then the trimmed stderr on the next line, or the first part alone when stderr is empty. stdout is never read. The command comes from the kubeconfig's `exec.command`, because the variant's `cmd` field is the debug form of the whole process command, which includes the environment variables passed to the helper.
- `AuthExecStart(io)`: the helper command could not be started, with the I/O error. The command again comes from the kubeconfig.
- `AuthExecParse` and `ExecPluginFailed`: a fixed "the response of the credential helper could not be read". A parse error can quote the response, which holds the token.
- `ReadTokenFile(io, path)`: names the file and the I/O error.
- `InvalidBearerToken` and `InvalidBasicAuth`: fixed text for each. Their `InvalidHeaderValue` does not print the value.
- `AuthExec(String)` and any other variant: fixed text, because the strings are free-form and some embed the provider's output.

Alternative: show `Display` of the whole error. Rejected because `AuthExecRun`'s `Display` prints the full `Output`, stdout included.

**Force `interactiveMode: Never` on every exec user before building the config.** `establish` already holds the parsed `Kubeconfig`, so it sets the field on each auth info that has `exec` before calling `Config::from_custom_kubeconfig`. This makes the library capture stderr, and it tells the helper through `KUBERNETES_EXEC_INFO` that it cannot prompt, which is true of a desktop application. Alternative: run helpers ourselves. Rejected because it duplicates the library's exec handling, including token caching and refresh.

**Two texts for one error.** `describe` stays the text for logs and never includes helper output. A new `explain` adds the helper's error output and is used only where the text becomes a session or port-forward failure reason. No `tracing` call takes a reason. The `credential_logs` test checks this: the helper writes the token only to stdout and a marker message only to stderr. The reason must contain the marker and not the token, and the logs must contain neither.

**One `Quit` action bound per platform.** In `kubepulse-ui`, an `app::init` (or the existing setup in `app.rs`) defines `actions!(kubepulse, [Quit])`, binds it with no key context, and handles it with `cx.on_action(|_: &Quit, cx| cx.quit())`. The bindings follow the settings shortcut's `cfg!(target_os)` pattern: `cmd-q` on macOS, `ctrl-q` on Linux, and `alt-f4` on Windows. Binding `alt-f4` explicitly on Windows does not rely on how the toolkit forwards the system key. Ctrl+Q on Windows is not bound, because Windows applications do not use it to quit.

**The Linux terminal tab keeps Ctrl+Q.** The terminal adds `ctrl-q` → `SendXon` (byte `0x11`) in its own key context, next to `ctrl-c` → `SendInterrupt`. A binding in a deeper context takes precedence over a global one, so the shell receives the key. This follows desktop terminal conventions, where Ctrl+Q belongs to the shell.

**Quit when the last window closes.** `cx.on_window_closed` calls `cx.quit()` once no windows remain. KubePulse has one window, so this behaves the same on every platform and stops macOS keeping a windowless process alive. Quitting drops the runtime, which cancels every session's task tree. That already ends shells, logs, and port-forwards.

## Risks / Trade-offs

**Risk** Helper stderr could contain a secret. → Helpers write credentials to stdout by protocol, and stdout is never shown. The reason is shown only in the UI for this cluster and is never logged.

**Risk** A helper that needs to prompt in a terminal stops working when it was started from one. → Started from a desktop, those helpers already fail, and the reason now shows the helper's own error. Browser-based logins do not read stdin and keep working.

**Risk** Long stderr overflows the rail tooltip and the sidebar. → The text is trimmed. If real helpers prove too verbose, limit the shown lines later without changing the rule.

**Risk** A helper that prints a login code to stderr and waits, such as a device-code flow, hides the code until it times out. → Those helpers already show nothing when started from a desktop, and the user still sees that the helper timed out.
