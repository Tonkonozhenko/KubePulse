# Proposal

## Why

Tables show times exactly as received, such as `2026-11-10T12:34:56Z`. That is wide, hard to scan, and doesn't answer the usual question: how long ago? In the detail panel, each condition takes two or three lines (a tag, its status and reason, then its message), so a pod's five conditions fill most of the panel. Labels in the same panel are compact tags that wrap.

## What Changes

- Table cells in timestamp columns (Created, and any column whose path is a time, such as CronJob "Last run") show the **age**: time since, in at most two units, such as `45s`, `5m30s`, `3h4m`, `3d4h`, or `41d`. Ages update as time passes.
- Hovering an age shows the **exact timestamp as received**. Copying the cell and the detail panel also give the exact value.
- Sorting a timestamp column **ascending puts the youngest object first**, matching the numbers shown.
- A value that is not a valid time, or is in the future, shows exactly as received.
- Timestamp columns get **narrower role widths** to suit ages.
- The detail panel shows conditions as **compact tags that wrap, like labels**: one tag per condition reading `Type=Status`, colored by health. Reasons and messages appear in one compact list under the tags, only for conditions that have them, and in each tag's tooltip.
- `AGENTS.md` gets the exception to its "show data exactly as received" rule: table timestamps show as ages, and the exact value stays one hover away.

## Capabilities

### New Capabilities
<!-- None -->

### Modified Capabilities
- `general/resource-browsing`: Adds a requirement that table timestamps show as ages. The "Conditions show their health" requirement changes to compact tags.
- `general/table-columns`: The timestamp role's initial width now suits an age instead of a full timestamp.

## Impact

- `crates/kubepulse-ui/src/resource_browser.rs`: displayed text for timestamp cells, a tooltip, a once-a-second refresh while any timestamp column exists, reversed sort for timestamp columns, timestamp role widths, and fit measurement of the displayed text.
- A new `crates/kubepulse-ui/src/age.rs`: age formatting from RFC 3339 text.
- `crates/kubepulse-ui/src/detail.rs`: `render_conditions`.
- `locales/en.json`, regenerated `en-XA`: age units.
- `crates/kubepulse-ui/Cargo.toml`: a direct dependency on `chrono`, which is already in the lockfile and licensed under MIT or Apache-2.0.
- `AGENTS.md`: the timestamp exception.
- Tests: `tests/browsing.rs` and unit tests for age formatting.
