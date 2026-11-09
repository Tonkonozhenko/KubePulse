# Spec Delta

## MODIFIED Requirements

### Requirement: Row actions
Every table SHALL show a pinned actions column, and the same actions SHALL be available from a right-click menu on the row. Using one MUST NOT open the detail panel or change the checked rows.

#### Scenario: Actions on a pod
- **WHEN** the user views a Pods table
- **THEN** each row offers Logs, Shell, Edit, and Delete

#### Scenario: Actions on a restartable workload
- **WHEN** the user views a Deployments, StatefulSets, or DaemonSets table
- **THEN** each row offers Restart, Edit, and Delete

#### Scenario: Actions on a CronJob
- **WHEN** the user views a CronJobs table
- **THEN** each row offers Run now, Edit, and Delete

#### Scenario: Actions on another kind
- **WHEN** the user views a table of any other kind
- **THEN** each row offers Edit and Delete

#### Scenario: Wide table
- **WHEN** the user scrolls a table sideways
- **THEN** the actions column stays in view

#### Scenario: Row moves while the menu is open
- **WHEN** rows change after the user opens a row's right-click menu
- **THEN** the chosen action applies to the object the user right-clicked
