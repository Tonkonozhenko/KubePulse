## ADDED Requirements

### Requirement: Table timestamps show as ages
A table cell in a timestamp column SHALL show the time elapsed since its value, in at most two units, from seconds up to years. The age SHALL stay current as time passes, and the exact value as received SHALL show on hover, in a copied cell, and in the detail panel.

#### Scenario: Recent object
- **WHEN** a pod was created 5 minutes and 30 seconds ago
- **THEN** its Created cell shows `5m30s`

#### Scenario: Older object
- **WHEN** a pod was created 3 days and 4 hours ago
- **THEN** its Created cell shows `3d4h`

#### Scenario: Age keeps up
- **WHEN** a table stays open while a minute passes
- **THEN** the ages shown grow by that minute without any change from the cluster

#### Scenario: Exact value on hover
- **WHEN** the user hovers over an age
- **THEN** a tooltip shows the timestamp exactly as received

#### Scenario: Not a time
- **WHEN** a timestamp column holds a value that is not a valid time, or a time in the future
- **THEN** the cell shows the value exactly as received

### Requirement: Timestamp columns sort by age
Sorting a timestamp column ascending SHALL put the youngest object first, so that the ages shown increase down the table.

#### Scenario: Ascending sort
- **WHEN** the user sorts the Created column ascending
- **THEN** the most recently created object is first

## MODIFIED Requirements

### Requirement: Conditions show their health
The detail panel SHALL show each condition as a compact tag reading its type and status, colored by whether that status is healthy for that type, and wrapping like labels. For types that describe a problem, True SHALL be unhealthy and False healthy. Each condition's reason and message SHALL be shown in one compact line under the tags, only for conditions that have them, and in the tag's tooltip.

#### Scenario: Healthy condition
- **WHEN** a condition of type `Ready` is True
- **THEN** it shows in the success color

#### Scenario: Unmet condition
- **WHEN** a condition of type `Ready` is False
- **THEN** it shows in the warning color, with its reason and message

#### Scenario: Problem condition inactive
- **WHEN** a condition of type `DiskPressure` is False
- **THEN** it shows in the success color

#### Scenario: Problem condition active
- **WHEN** a condition of type `DiskPressure` is True
- **THEN** it shows in the danger color

#### Scenario: Unknown condition
- **WHEN** a condition's status is Unknown
- **THEN** it shows in the warning color

#### Scenario: Many conditions stay compact
- **WHEN** a pod has five conditions and none has a reason or message
- **THEN** they show as five tags that wrap like labels, with no line per condition
