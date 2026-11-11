# Spec Delta

## Purpose

Defines how resource tables size their columns so content fits without unnecessary horizontal scrolling while remaining readable at any width.

## ADDED Requirements

### Requirement: Name column uses remaining space

The Name column SHALL fill the available horizontal space beyond pinned columns and data columns, with a minimum width that matches typical short names. Its effective min-width SHALL be 120px when the table is wider than 480px, and shall fall back to half the table width minus pinned columns when narrower.

#### Scenario: Typical wide table

- **WHEN** the resource table renders in a window wider than 480px
- **THEN** the Name column fills remaining space and does not exceed its content by more than 80px when every object name fits within that space

#### Scenario: Narrow table

- **WHEN** the resource table is narrower than 480px wide
- **THEN** the Name column uses at most half the table width minus pinned columns, preventing overflow

### Requirement: Data columns respect content

Each data column SHALL have a minimum width sufficient for its shortest plausible values, so labels and status tags are never clipped to unreadability. A numeric column's min-width SHALL accommodate at least the largest label of its kind plus two characters.

#### Scenario: Short value visible

- **WHEN** a column displays a value shorter than its min-width
- **THEN** the column is wide enough for the min-width and wraps only values exceeding it

#### Scenario: Status column not clipped

- **WHEN** a status column renders a tag such as "CrashLoopBackOff"
- **THEN** the column width accommodates the full text in a single line when space allows

### Requirement: Pinned columns stay fixed

The checkbox and row-actions columns SHALL remain pinned at their current fixed widths (40px and ~24+28×action_count px). Their widths are independent of table width and content, and SHALL NOT change as part of this capability.

#### Scenario: Wide table pinning

- **WHEN** the user scrolls right in a wide table
- **THEN** selected and action columns remain visible on the left edge

### Requirement: No horizontal scroll when content fits

The resource table SHALL not display a horizontal scrollbar when all column contents fit within the available width. The only exceptions are the pinned columns whose minimum layout may exceed the table's visual bounds, in which case a vertical-only scrollbar is acceptable.

#### Scenario: Compact cluster fits

- **WHEN** every object label fits its column's min-width and no name exceeds 60 characters
- **THEN** the table renders without a horizontal scrollbar

#### Scenario: Long name triggers scroll only if it must

- **WHEN** the Name column reaches its base allocation and at least one name exceeds that width
- **THEN** a horizontal scrollbar appears, but shorter names in other columns still render fully

### Requirement: Column order and count are stable

The number and order of data columns SHALL not affect the policy above. New or removed columns SHALL only change how much horizontal space remains for the Name column; they SHALL NOT alter min-widths of existing data columns.

#### Scenario: Adding a kind-specific column

- **WHEN** a kind declares an additional printer column via its CRD
- **THEN** the Name column shrinks by exactly that column's width, and other data column widths stay unchanged

#### Scenario: Removing a column by filter

- **WHEN** a filter reduces visible rows but no columns are removed
- **THEN** column widths remain unchanged
