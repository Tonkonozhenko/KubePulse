# Proposal

## Why

Users cannot copy anything from the log view. Each row is plain painted text, so an error message, a stack trace, or a request ID has to be retyped. Long lines can only be read by scrolling sideways one line at a time, which makes multi-line messages and long JSON lines hard to read.

## What Changes

- The user can select log text with the mouse: drag, double-click for a word, triple-click for a line, and shift-click to extend. A selection can span lines that have scrolled out of view.
- The platform copy shortcut copies the selection as plain text, with one line per log line and timestamps included when they are shown. A line that was cut for display is copied whole.
- A new **Wrap lines** checkbox in the log toolbar wraps long lines at the view width. Wrapping is off by default and stays as set when the user switches container or instance in the same tab.
- While lines wrap, the horizontal scrollbar is hidden and the view does not scroll sideways. Following, pausing, keeping place as old lines are dropped, moving to search matches, and the vertical scrollbar behave the same as without wrapping.
- The sideways-scrolling requirement now applies only while wrapping is off.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `pod/log`: adds requirements for selecting and copying log text and for wrapping lines. Changes "Scroll long log lines sideways" so it applies only while wrapping is off.

## Impact

- `crates/kubepulse-ui/src/log_tab.rs`: log rows join the toolkit's window text selection, a copy action and key bindings are added, a wrap checkbox is added, and a variable-height list is used while wrapping.
- `crates/kubepulse-ui/locales/en.json` and the generated `en-XA` pseudo-locale: one new string for the wrap checkbox.
- `crates/kubepulse-ui/tests`: UI tests for selection, copy, and wrapping.
- No new dependencies. The toolkit already provides window text selection and the variable-height list.
