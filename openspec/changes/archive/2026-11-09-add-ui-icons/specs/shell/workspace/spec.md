# Spec Delta

## ADDED Requirements

### Requirement: Closing tabs
Every tab SHALL show a close button with a translated tooltip, including preview tabs and the Settings tab. The user SHALL be able to close any tab this way, including the last one in the workspace. Closing a tab MUST NOT disconnect its cluster.

#### Scenario: Close the active tab
- **WHEN** the user closes the active tab and other tabs remain in its pane
- **THEN** the tab is gone, another tab in that pane becomes active, and the selected cluster follows it

#### Scenario: Close an inactive tab
- **WHEN** the user closes a tab that is not active
- **THEN** that tab is gone, the active tab stays active, and the closed tab was not activated or pinned first

#### Scenario: Close the last tab
- **WHEN** the user closes the only open tab
- **THEN** the tab area shows the hint to choose a kind, and the rail and sidebar stay in place

#### Scenario: Close the last tab of a pane
- **WHEN** the user closes the only tab in one of two panes
- **THEN** that pane is removed, and the other pane takes its space

#### Scenario: Close a cluster's last tab
- **WHEN** the user closes the last tab of a connected cluster
- **THEN** the cluster stays connected and selected

#### Scenario: Close button tooltip
- **WHEN** the user hovers a tab's close button
- **THEN** a tooltip names the action

### Requirement: Workspace starts empty
The system SHALL start with no open tabs, one empty tab area, and no selected cluster. It MUST NOT save or restore tabs, panes, the active tab, tab contents, filters, label selectors, sort order, namespace selections, or the selected cluster between launches.

#### Scenario: Restart with tabs open
- **WHEN** the user quits with tabs and split panes open and starts the application again
- **THEN** no tabs are open, the tab area shows the hint to choose a kind, no cluster is selected, and no connection starts

#### Scenario: Restart after filtering
- **WHEN** the user sets a filter, label selector, sort order, and namespace selection, restarts, connects the cluster, and opens the same kind
- **THEN** the table shows every namespace with an empty filter, an empty label selector, and the default sort order

#### Scenario: Layout saved by an earlier version
- **WHEN** the configuration directory holds a layout file from an earlier version
- **THEN** the application ignores it and starts empty

### Requirement: Settings outlive the session
The system SHALL keep cluster display settings, application preferences, and theme files between launches, and SHALL keep nothing else.

#### Scenario: Restart keeps cluster settings
- **WHEN** the user renames a cluster, gives it a color, marks it protected, and restarts
- **THEN** the cluster keeps its name, color, and protected mark

## MODIFIED Requirements

### Requirement: Selected cluster follows the active tab
Activating a tab SHALL select that tab's cluster. Selecting a cluster MUST NOT activate, open, or close any tab.

#### Scenario: Switch to a tab on another cluster
- **WHEN** the user activates a tab on a different cluster
- **THEN** that cluster becomes selected, and the sidebar shows it

#### Scenario: Context points at another server
- **WHEN** a tab's context has been removed or now points at a different server
- **THEN** activating that tab does not select the context of the same name

### Requirement: Fast startup
The system SHALL show a usable window within 1 second of launch on a reference machine, independent of cluster availability.

#### Scenario: Unreachable clusters at startup
- **WHEN** the application launches while clusters in the kubeconfig are unreachable
- **THEN** the window appears within 1 second

## REMOVED Requirements

### Requirement: Workspace restore
**Reason**: Each launch now starts clean, so tabs and panes are no longer saved. See "Workspace starts empty".
**Migration**: None. An existing layout file is ignored and can be deleted.
