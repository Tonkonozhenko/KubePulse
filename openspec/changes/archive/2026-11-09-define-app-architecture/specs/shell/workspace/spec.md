# Spec Delta

## Purpose

Provides the tabs and panes that hold every view, keeps each tab tied to one cluster, brings the layout back after a restart, and opens quickly regardless of cluster availability.

## ADDED Requirements

### Requirement: Multiple tabs
The system SHALL let the user open multiple tabs, including several tabs on the same cluster and tabs on different clusters at the same time.

#### Scenario: Tabs on two clusters
- **WHEN** the user has tabs open on two clusters
- **THEN** both stay live, and switching between them needs no reloading

#### Scenario: Two tabs on one cluster
- **WHEN** the user opens the same resource kind in two tabs
- **THEN** each tab keeps its own filter, sort, and scroll position

### Requirement: Splittable workspace
The system SHALL let the user split the workspace into resizable panes and move tabs between them.

#### Scenario: Side-by-side views
- **WHEN** the user drags a tab to the edge of a pane
- **THEN** the pane splits and both tabs stay visible

### Requirement: Each tab is bound to one cluster
Every cluster-related tab SHALL belong to exactly one cluster for its whole lifetime, and the system SHALL NOT offer views that merge data from several clusters.

#### Scenario: Changing the active cluster
- **WHEN** the user switches to a tab on another cluster
- **THEN** the other tabs keep showing their own cluster's data

### Requirement: Tabs survive disconnection
Disconnecting a cluster SHALL leave its tabs open in a disconnected state.

#### Scenario: Disconnect a cluster with open tabs
- **WHEN** the user disconnects a cluster that has open tabs
- **THEN** those tabs stay open and offer a connect action, and tabs on other clusters are unaffected

### Requirement: Workspace restore
The system SHALL restore the workspace layout on startup, and MUST NOT connect to any cluster until the user chooses to connect it.

#### Scenario: Restart
- **WHEN** the application restarts
- **THEN** tabs and panes return to their previous layout, in a disconnected state until the user connects their cluster

#### Scenario: Activating a restored tab
- **WHEN** the user activates a restored tab whose cluster is disconnected
- **THEN** no connection starts, and the tab offers a connect action

### Requirement: Fast startup
The system SHALL show a usable window within 1 second of launch on a reference machine, independent of cluster availability.

#### Scenario: Unreachable clusters at startup
- **WHEN** the application launches while restored clusters are unreachable
- **THEN** the window appears within 1 second
