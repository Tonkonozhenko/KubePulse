# general/table-columns Specification

## Purpose

Defines how resource tables size their columns so that headers and values are readable without manual resizing, and so that text cut off by a narrow column is visibly marked as cut off.

## Requirements

### Requirement: Initial widths by column role

Before any rows arrive, every data column SHALL start at a width suited to its role: name, namespace, status, timestamp, or general text. No data column SHALL start narrower than its header text in the active language.

#### Scenario: Empty table is readable

- **WHEN** a kind's table opens and no rows have arrived yet
- **THEN** every column header is shown in full, and each column is at least its role's initial width

#### Scenario: Timestamp column fits a full timestamp

- **WHEN** a table shows the Created column
- **THEN** its initial width shows a two-unit age such as `23h59m` without cutting it off, and the full timestamp as received shows on hover

### Requirement: Columns fit their content when the table first fills

When a table first receives rows, the system SHALL widen or narrow each data column to fit the widest of its header and its values in the loaded rows. The width SHALL stay between the column's minimum and its automatic cap. A fit SHALL happen once per table, not on every update.

#### Scenario: Short values give compact columns

- **WHEN** the first rows of a Pod table arrive and every IP is an IPv4 address
- **THEN** the IP column fits the longest address and its header, with no extra blank space

#### Scenario: Values longer than the old fixed widths

- **WHEN** the first rows arrive with namespaces, node names, and timestamps wider than 40 px
- **THEN** those columns widen to show them in full, up to the automatic cap

#### Scenario: One very long value

- **WHEN** a single value in a column is wider than the automatic cap
- **THEN** the column stops at the cap, and that value is marked as cut off

#### Scenario: Later updates do not move columns

- **WHEN** rows are added, changed, or filtered after the first fit
- **THEN** no column width changes on its own

### Requirement: Computed columns fit when their values arrive

A column whose values an extension computes SHALL fit its content once, when its first values arrive, under the same limits as other data columns.

#### Scenario: Extension answers after the table fills

- **WHEN** an extension returns the first values of its computed column
- **THEN** that column fits the widest of its header and those values, and no other column changes width

### Requirement: Status tags fit as drawn

A status column SHALL be measured as its tags are drawn, including the tag's padding and border, so that a fitted status column never clips a tag.

#### Scenario: Long status

- **WHEN** a Pod in the first rows has status `CrashLoopBackOff`
- **THEN** the Status column shows the whole tag on one line

### Requirement: The user's widths win

Once the user resizes a column, automatic fitting SHALL NOT change that column's width again for the life of the table.

#### Scenario: Resize before computed values arrive

- **WHEN** the user resizes a computed column and its first values then arrive
- **THEN** the column keeps the width the user chose

### Requirement: Fit a column on request

A double-click on a column's resize handle SHALL fit that column to the widest of its header and the values in the loaded rows. This fit is not limited by the automatic cap, only by the column's maximum width.

#### Scenario: Long name beyond the cap

- **WHEN** a Name value is wider than the automatic cap and the user double-clicks the Name column's resize handle
- **THEN** the Name column widens to show that value in full

### Requirement: Cut-off text is marked

Any header or text cell whose content is wider than its column SHALL end in an ellipsis instead of being clipped at the edge. The full value SHALL remain available in the detail panel and when copied from the cell.

#### Scenario: User narrows a column

- **WHEN** the user drags a column narrower than its values
- **THEN** the cut-off values end in an ellipsis

### Requirement: Pinned columns keep their widths

The checkbox and row actions columns SHALL keep their fixed widths and SHALL NOT be fitted or resized.

#### Scenario: Fitting leaves pinned columns alone

- **WHEN** a table fits its columns after its first rows arrive
- **THEN** the checkbox and row actions columns keep their widths

### Requirement: Actions column fits its buttons

The row actions column SHALL be exactly as wide as its buttons, the gaps between them, and its cell padding, so that it holds no empty space beside the last button.

#### Scenario: Pod actions

- **WHEN** a Pod table shows its five row actions
- **THEN** the actions column ends just after the fifth button's cell padding
