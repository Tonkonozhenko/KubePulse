# Spec Delta

## Purpose

Gives the user a thin, always-visible column for choosing and managing clusters, so that cluster choice takes almost no window width and connection state stays visible at a glance.

## ADDED Requirements

### Requirement: Icon-only cluster rail
The system SHALL show a narrow column at the left edge of the window containing one icon for every context in the cluster list, with no text labels. The column SHALL scroll when the icons do not fit, and connected clusters SHALL appear above the others, each set keeping kubeconfig order.

#### Scenario: Hundreds of contexts
- **WHEN** the kubeconfig has 500 contexts
- **THEN** the rail lists all 500 without slowing startup, and the user can scroll through them

#### Scenario: Cluster connects
- **WHEN** a cluster becomes connected
- **THEN** its icon moves into the connected set at the top, and it stays selected if it was selected

#### Scenario: Degraded cluster
- **WHEN** a connected cluster becomes degraded
- **THEN** its icon stays in the connected set

### Requirement: Rail icon identity
Each rail icon SHALL be drawn from the cluster's identity, using its color and the leading characters of its display name. Hovering an icon SHALL show a tooltip with the full display name, the context name, and the connection state.

#### Scenario: Hover an icon
- **WHEN** the user hovers a rail icon
- **THEN** the tooltip shows the display name, the context name, and the connection state

#### Scenario: Identity changes
- **WHEN** the user changes a cluster's name or color
- **THEN** its rail icon updates immediately

#### Scenario: Protected cluster
- **WHEN** a cluster is protected
- **THEN** its rail icon carries a protection mark that does not depend on color alone

### Requirement: Connection state on rail icons
Each rail icon SHALL show its cluster's connection state, and every state SHALL be distinguishable without relying on color alone.

#### Scenario: Connected and disconnected clusters
- **WHEN** the rail shows a connected cluster and a disconnected cluster
- **THEN** the disconnected icon is visibly muted compared with the connected icon

#### Scenario: Attempt in progress
- **WHEN** a cluster is connecting or authenticating
- **THEN** its icon shows that an attempt is in progress

#### Scenario: Failed or degraded cluster
- **WHEN** a cluster is failed or degraded
- **THEN** its icon shows a marker for that state, and its tooltip includes the reason

### Requirement: Selecting a cluster
A single click on a rail icon SHALL make that cluster the selected cluster and MUST NOT start a connection or change which tab is active.

#### Scenario: Single click on a disconnected cluster
- **WHEN** the user single-clicks a disconnected cluster
- **THEN** it becomes selected, no connection starts, and no credential helper runs

#### Scenario: Selected cluster is marked
- **WHEN** a cluster is selected
- **THEN** its icon shows a selection marker

### Requirement: Double-click connects
A double-click on the rail icon of a disconnected or failed cluster SHALL connect that cluster, and only that cluster. A double-click on a cluster in any other state MUST NOT send a connection command.

#### Scenario: Double-click a disconnected cluster
- **WHEN** the user double-clicks a disconnected cluster
- **THEN** it becomes selected, and a connection to that cluster starts

#### Scenario: Double-click a failed cluster
- **WHEN** the user double-clicks a failed cluster
- **THEN** the connection is retried

#### Scenario: Double-click during an attempt
- **WHEN** the user double-clicks a cluster that is already connecting or authenticating
- **THEN** no second attempt starts

#### Scenario: Double-click a connected cluster
- **WHEN** the user double-clicks a connected cluster
- **THEN** it becomes selected, and the connection is unchanged

### Requirement: Rail context menu
Right-clicking a rail icon SHALL open a menu holding the cluster's actions, offering only those that apply to its state. The rail itself SHALL have no other per-cluster buttons.

#### Scenario: Menu for a disconnected cluster
- **WHEN** the user right-clicks a disconnected cluster
- **THEN** the menu offers Connect and cluster settings

#### Scenario: Menu for a failed cluster
- **WHEN** the user right-clicks a failed cluster
- **THEN** the menu offers Retry and cluster settings

#### Scenario: Menu during an attempt
- **WHEN** the user right-clicks a cluster that is connecting or authenticating
- **THEN** the menu offers Cancel and cluster settings, and choosing Cancel stops the attempt

#### Scenario: Menu for a connected cluster
- **WHEN** the user right-clicks a connected or degraded cluster
- **THEN** the menu offers Disconnect and cluster settings

#### Scenario: Right-click does not select
- **WHEN** the user right-clicks a cluster that is not selected
- **THEN** the selected cluster does not change

#### Scenario: Disconnect with active work
- **WHEN** the user chooses Disconnect on a cluster with active shells or port-forwards
- **THEN** a dialog lists that work and asks for confirmation, and nothing stops until the user confirms

### Requirement: Cluster settings dialog
The cluster settings action SHALL open a dialog where the user can change the cluster's display name and color and turn its protection on or off.

#### Scenario: Change settings
- **WHEN** the user changes a setting in the dialog
- **THEN** the change applies to that cluster's identity everywhere and is saved

### Requirement: Search from the rail
The rail SHALL have a search button that opens a popover listing clusters whose name matches the typed text, ignoring case, with each result showing its connection state.

#### Scenario: Narrow by name
- **WHEN** the user opens search and types part of a name with 500 contexts listed
- **THEN** only the matching clusters remain in the popover

#### Scenario: Choose a result
- **WHEN** the user clicks a search result
- **THEN** the cluster becomes selected, no connection starts, the popover closes, and the rail scrolls to its icon

#### Scenario: Double-click a result
- **WHEN** the user double-clicks a result for a disconnected or failed cluster
- **THEN** the cluster becomes selected and connects

### Requirement: Kubeconfig errors stay visible
When a kubeconfig file cannot be read, the rail SHALL show a warning button that reveals each failing file and its error.

#### Scenario: Invalid kubeconfig
- **WHEN** one kubeconfig file cannot be parsed
- **THEN** the warning button appears, its detail names the file and the error, and contexts from valid files are still listed

#### Scenario: No errors
- **WHEN** every kubeconfig file parses
- **THEN** no warning button appears
