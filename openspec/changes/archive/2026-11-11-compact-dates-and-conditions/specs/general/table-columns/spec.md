## MODIFIED Requirements

### Requirement: Initial widths by column role

Before any rows arrive, every data column SHALL start at a width suited to its role: name, namespace, status, timestamp, or general text. No data column SHALL start narrower than its header text in the active language.

#### Scenario: Empty table is readable

- **WHEN** a kind's table opens and no rows have arrived yet
- **THEN** every column header is shown in full, and each column is at least its role's initial width

#### Scenario: Timestamp column fits a full timestamp

- **WHEN** a table shows the Created column
- **THEN** its initial width shows a two-unit age such as `23h59m` without cutting it off, and the full timestamp as received shows on hover
