# Proposal

## Why

Table columns use fixed widths (e.g., name column at 300px) that create excessive empty space on clusters with short object names while still cutting off longer values behind horizontal scroll. This makes tables feel cluttered, especially when many kinds have concise labels.

## What Changes

- Remove the hardcoded 300px minimum on the Name column; use a tighter base width that fills remaining space so tables show more meaningful columns without horizontal scroll.
- Add min-widths to data columns that prevent useful values from shrinking below readability.
- Keep pinned columns (checkbox, actions) at their current fixed widths since they contain icons only.

## Capabilities

### New Capabilities
- `general/table-columns`: Column width policy for resource tables — base width, minimum width, and how remaining space is distributed.

### Modified Capabilities
<!-- None -->

## Impact

- `crates/kubepulse-ui/src/resource_browser.rs` — column width setup in `ResourceBrowser::new()` (lines 557-574).
- `crates/kubepulse-store/src/column.rs` (ColumnSpec) and `columns.rs` — column titles and specs feed the table but not widths.
- Existing tables across all resource kinds benefit automatically; no kind-specific changes needed.
