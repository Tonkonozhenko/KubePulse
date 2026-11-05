# Spec Delta

## Purpose

Gives the user one place to choose which resource kind to view on the selected cluster, in groups that keep long lists of kinds easy to scan.

## ADDED Requirements

### Requirement: Sidebar follows the selected cluster
The system SHALL show a sidebar between the cluster rail and the tabs. The sidebar SHALL always describe the selected cluster, showing its identity and connection state at the top.

#### Scenario: Select another cluster
- **WHEN** the user selects a different cluster
- **THEN** the sidebar switches to that cluster, and the open tabs are unaffected

#### Scenario: Protected cluster
- **WHEN** the selected cluster is protected
- **THEN** the sidebar's cluster header shows that it is protected

### Requirement: Kinds are grouped
The sidebar SHALL list the kinds a connected cluster serves in named, collapsible groups, using the category each view declares. Groups SHALL appear in contribution order, with undeclared kinds last, and all declared groups SHALL start expanded.

#### Scenario: Built-in groups
- **WHEN** the user selects a connected cluster
- **THEN** the sidebar shows Workloads, Network, Config, Storage, and Access control in that order, each listing only the kinds the cluster serves

#### Scenario: Empty group
- **WHEN** a cluster serves none of a group's kinds
- **THEN** that group is not shown

#### Scenario: Collapse a group
- **WHEN** the user collapses a group
- **THEN** its kinds are hidden, and they stay hidden while the user switches clusters during the session

#### Scenario: Extension category
- **WHEN** an extension declares a view in a new category
- **THEN** that category appears as its own group after the groups contributed before it

### Requirement: Undeclared kinds are grouped by API group
Listable kinds that no extension declares SHALL appear under a final Other resources group, nested by API group and sorted by name. That group SHALL start collapsed.

#### Scenario: Custom resources
- **WHEN** a cluster serves custom kinds from two API groups
- **THEN** Other resources contains one nested entry for each API group, each listing its kinds

#### Scenario: Custom resource definition added
- **WHEN** a custom resource definition is installed while the session is connected
- **THEN** its kind appears in the sidebar without reconnecting, and the sidebar's scroll position and collapsed groups do not change

### Requirement: Current kind is marked
The sidebar SHALL mark the kind shown by the active tab when that tab is on the selected cluster.

#### Scenario: Active tab on the selected cluster
- **WHEN** the active tab shows Pods on the selected cluster
- **THEN** Pods is marked in the sidebar

#### Scenario: Active tab on another cluster
- **WHEN** the selected cluster is not the active tab's cluster
- **THEN** no kind is marked

### Requirement: Sidebar without a browsable cluster
When no kinds can be listed, the sidebar SHALL explain why instead of showing an empty column, and SHALL offer a connect action for a disconnected cluster and a retry action for a failed one.

#### Scenario: No cluster selected
- **WHEN** no cluster is selected
- **THEN** the sidebar asks the user to select a cluster

#### Scenario: Disconnected cluster
- **WHEN** the selected cluster is disconnected
- **THEN** the sidebar shows that state and a connect action, and no connection starts until the user uses it

#### Scenario: Attempt in progress
- **WHEN** the selected cluster is connecting or authenticating
- **THEN** the sidebar shows that state

#### Scenario: Failed cluster
- **WHEN** the selected cluster failed to connect
- **THEN** the sidebar shows the reason and a retry action

#### Scenario: Degraded cluster
- **WHEN** the selected cluster is degraded
- **THEN** the sidebar keeps listing its last known kinds, marked as stale
