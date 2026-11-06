# Proposal

## Why

Lines wider than the log view are cut off at its right edge, and nothing can bring the rest into view. Opening logs can also stop short of the newest line, because the view moves to the end only when lines arrive, so a layout change on a quiet stream leaves it behind. Following can be paused only by scrolling up, and resumed only by a button that appears afterwards.

## What Changes

- Let the log view scroll sideways by wheel, trackpad, and a horizontal scrollbar, across the width of the widest retained line.
- Add a vertical scrollbar to the log view, so the user can move through retained lines by dragging.
- Make the Timestamps and Previous instance toggles checkboxes, matching Follow.
- Always show Previous instance and keep it enabled for every container, without checking whether it restarted, so a pod with several containers does not hide it. A container with no previous instance shows the cluster's error.
- Keep the newest line in view for as long as the view is following, including after opening and after the view changes size, and not only when lines arrive.
- Add a Follow checkbox to the log toolbar. Turning it off pauses following while the stream continues. Turning it on returns to the newest line and follows again.
- Keep scrolling up and moving to a search match as ways to pause, and show that in the checkbox.
- **BREAKING** Remove the Newest line button, because the checkbox now does its job.

## Out of Scope

- Wrapping long lines.
- Scrolling sideways to bring a search match into view.
- Showing lines past the 4 KiB display cut. That remains an open decision.
- Remembering the checkbox across streams. Every new stream starts at the end and following.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `pod/log`: following gains an explicit control and keeps the newest line in view at all times, long lines become reachable by scrolling sideways, the view gains a vertical scrollbar, and viewing a previous instance is always offered. This capability is still defined in the in-flight `define-app-architecture` change.

## Impact

- `crates/kubepulse-ui/src/log_tab.rs`: list sizing, scrollbar, follow state, and toolbar.
- `crates/kubepulse-ui/tests/logs.rs`: follow and scrolling tests.
- `crates/kubepulse-ui/locales/`: a new follow string replaces the jump string.
- `crates/kubepulse-ui/Cargo.toml`: adds `unicode-width` as a direct dependency. It is already in `Cargo.lock`.
