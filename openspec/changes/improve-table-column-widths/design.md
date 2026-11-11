# Design — Improve Table Column Widths

## Context

Every resource kind renders through `ResourceBrowser`, which builds a `DataTable` at init time. Column widths are declared in `new()` (lines 555-574 of `resource_browser.rs`):

| Column          | Kind                |
|-----------------|---------------------|
| Select (pinned) | `px(40.0)`          |
| Actions (pinned)| `ACTIONS_PADDING + ACTION_WIDTH * n` (~24+28×count px) |
| Name            | `px(300.)`          |
| Namespace       | *none* (default)    |
| Declared        | *none* (default)    |
| Computed        | *none* (default)    |

The name column is the only column with an explicit `.width()` call. Data columns (namespace, declared from CRD or extensions, computed by extensions) all inherit whatever default width `gpui_kit` provides. On narrow windows this forces horizontal scroll; on wide windows it wastes space — especially on clusters where almost every object name fits in far less than 300px.

## Goals

1. Name column fills remaining table width instead of holding a fixed 300px.
2. Data columns get sensible min-widths so short labels survive at any width.
3. No horizontal scroll when all content fits within those widths and the window is ≥480px.
4. Pinned column widths stay exactly as-is — this change does not touch them.

## Decisions

### D1: Name column fills remaining space, guarded by a 60px minimum

The name column width becomes `remaining - min_data_widths` with a floor of 60px to prevent the name column from collapsing entirely when many data columns are declared (e.g., custom resource definitions with many printer columns). This matches **Requirement: Name column uses remaining space** — the spec sets its effective minimum at 120px for windows ≥480px, but 60px is the implementation floor; the 120px result follows automatically when table width ≥480px and data widths are small.

- **Wording clarification in D2**: I'll add `min(300., remaining)` rather than dropping it entirely. At full window width, `remaining` will be larger than 300, so the 300 still anchors as a sensible upper bound in practice and avoids regressions where names that previously wrapped now don't.
- **Revised D1**: Name column uses `px(min(300., remaining_space))`. The effective min-width derives from remaining space at narrow widths (≥120px at 480px table width with current pinned data widths) and the remaining-space calculation shrinks gracefully as more columns are added.

### D2: Data columns get per-kind min-widths via a new config function

Each declared column type (status, numeric, timestamp, generic text) gets a minimum width constant in `columns.rs` or a nearby module. A helper function computes total data-column width as `sum(min_width per column)`. This is computed once at table creation and cached.

- Constants: status tag ~90px ("CrashLoopBackOff" + padding), numeric label ~50px, timestamp ~70px, generic text ~40px (allows short label before wrapping).
- The namespace column uses the generic-text min-width because it displays namespace names which are typically short.

### D3: Column width policy lives in `resource_browser.rs`, not in `columns.rs`

Column widths are a rendering concern tied to the table, not a metadata concern about columns themselves. The spec (column order, kind associations) stays in `columns.rs`; the width policy (how much space each gets) stays in `resource_browser.rs`. The `new()` function calls a new `data_columns_min_width()` method or free function that iterates `columns.declared` and sums the min-widths derived from each column's `ColumnDef.status` flag.

### D4: Computed columns also get a min-width, but no per-column differentiation

Computed columns' data is extension-provided and unknown to the host at compile time. They share the generic-text min-width (40px), consistent with how their titles are already rendered as plain text without style overrides.

### D5: Column order stability verified by regression test

The spec's "Column order and count are stable" requirement is structural — adding or removing columns shouldn't change other column widths except for the name column's remaining-space allocation. A test in `resource_browser.rs` validates that `data_columns_min_width()` returns expected sums for known `KindColumns` configs, serving as a regression guard.

## Risks and Trade-offs

| Risk | Mitigation |
|---|---|
| Very wide tables still waste space if the name cap is too low (300px) | The spec allows the name column to shrink below 300; at typical viewport widths it will be 120-250px. Revisit 300 as a hard max in follow-up iteration after user feedback. |
| Long object names (>60 chars) still wrap or scroll | This is by design — no column width policy eliminates both wrapping *and* horizontal scroll for arbitrarily long values. The spec scenario "No horizontal scroll when content fits" uses 60-char names as the upper bound for "fits." |
| New data columns from extensions shift remaining space unpredictably | The name column shrinks proportionally; this is the intended behavior per Requirement: Column order and count are stable (Scenario: Adding a kind-specific column). |
| `gpui_kit` default width behavior unknown | We now define explicit min-widths on every data column. If `gpui_kit` ignores them, the feature degrades gracefully to the old fixed-300 name approach plus whatever defaults remain. |
