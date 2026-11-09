# shell/workspace Specification

## Purpose
Provides the tabs and panes that hold every view, keeps each tab tied to one cluster, starts every launch with an empty workspace, and opens quickly regardless of cluster availability.

## Requirements

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

### Requirement: Fast startup
The system SHALL show a usable window within 1 second of launch on a reference machine, independent of cluster availability.

#### Scenario: Unreachable clusters at startup
- **WHEN** the application launches while clusters in the kubeconfig are unreachable
- **THEN** the window appears within 1 second

### Requirement: Three-column layout
The main window SHALL be arranged in three columns from left to right: the cluster rail, the kind sidebar, and the tabs and panes. The rail and sidebar SHALL stay in place as tabs open, close, split, and move, and tabs MUST NOT contain their own kind navigation.

#### Scenario: Default arrangement
- **WHEN** the application starts
- **THEN** the rail, the sidebar, and the tab area appear in that order, with the tab area taking the remaining width

#### Scenario: No open tabs
- **WHEN** every tab is closed
- **THEN** the rail and sidebar stay in place, and the tab area shows a hint to choose a kind

### Requirement: Selected cluster follows the active tab
Activating a tab SHALL select that tab's cluster. Selecting a cluster MUST NOT activate, open, or close any tab.

#### Scenario: Switch to a tab on another cluster
- **WHEN** the user activates a tab on a different cluster
- **THEN** that cluster becomes selected, and the sidebar shows it

#### Scenario: Context points at another server
- **WHEN** a tab's context has been removed or now points at a different server
- **THEN** activating that tab does not select the context of the same name

### Requirement: Preview tab
A single click on a kind in the sidebar SHALL show that kind in the preview tab, opening one if none exists. If the preview tab holds other content, the new kind SHALL replace it and keep its position and pane, and the tab SHALL become active.

#### Scenario: Browse several kinds
- **WHEN** the user single-clicks Pods, then Services, then Deployments on one cluster
- **THEN** exactly one tab was used, and it now shows Deployments

#### Scenario: Pane of the preview tab
- **WHEN** the preview tab is in one pane, another pane is active, and the user single-clicks a kind
- **THEN** the content appears in the preview tab's pane

#### Scenario: Replacing clears view state
- **WHEN** a preview tab with a filter is replaced by another kind
- **THEN** the new content starts with no filter, and any open object detail closes

### Requirement: Preview across clusters
A preview tab SHALL keep its binding to one cluster. Selecting a kind on another cluster SHALL close the preview tab and open a new preview tab bound to that cluster in the same position.

#### Scenario: Kind on another cluster
- **WHEN** the preview tab shows a kind on cluster A and the user single-clicks a kind on cluster B
- **THEN** the tab for cluster A is gone, a preview tab for cluster B takes its position, and the number of tabs is unchanged

### Requirement: At most one preview tab
The workspace SHALL hold at most one preview tab across all panes and clusters.

#### Scenario: Pinned tabs and a preview
- **WHEN** several pinned tabs are open and the user single-clicks a kind that no tab shows
- **THEN** the pinned tabs are unchanged, and one preview tab holds the new kind

#### Scenario: Close the preview tab
- **WHEN** the user closes the preview tab and then single-clicks a kind
- **THEN** a new preview tab opens

### Requirement: Pinning a tab
A double-click on a kind in the sidebar, or on the preview tab's title, SHALL pin that content to its tab. A pinned tab MUST NOT be replaced, and the next single click on a different kind SHALL use a new preview tab.

#### Scenario: Double-click a kind
- **WHEN** the user double-clicks Pods in the sidebar
- **THEN** a tab shows Pods and is pinned

#### Scenario: Next selection after pinning
- **WHEN** the user pins Pods and then single-clicks Services
- **THEN** Pods stays open in its tab, and Services opens in a new preview tab

#### Scenario: Double-click the tab title
- **WHEN** the user double-clicks the preview tab's title
- **THEN** the tab becomes pinned, and its content does not change

#### Scenario: Interacting inside a preview tab
- **WHEN** the user filters, sorts, or opens an object in the preview tab
- **THEN** the tab stays a preview tab

### Requirement: Existing tabs are reused
Choosing a kind that an open tab already shows on that cluster SHALL activate that tab instead of changing the preview tab.

#### Scenario: Kind already pinned
- **WHEN** Pods is pinned on a cluster and the user single-clicks Pods for that cluster
- **THEN** the pinned tab becomes active, and the preview tab is unchanged

#### Scenario: Kind already in the preview tab
- **WHEN** the preview tab shows Pods and the user double-clicks Pods
- **THEN** that tab becomes active and pinned, and no new tab opens

#### Scenario: Same kind name on another cluster
- **WHEN** a tab shows Pods on cluster A and the user single-clicks Pods on cluster B
- **THEN** the tab on cluster A is not reused

### Requirement: Open a kind in a new tab
Right-clicking a kind in the sidebar SHALL offer an action that opens the kind in a new pinned tab, even when a tab already shows it.

#### Scenario: Second tab of one kind
- **WHEN** Pods is open and the user chooses to open Pods in a new tab
- **THEN** two tabs show Pods, each with its own filter, sort, and scroll position

### Requirement: Tab titles
Each cluster tab's title SHALL show its kind and its cluster identity, and the preview tab's title SHALL look visibly different from a pinned tab's, in a way that does not depend on color alone.

#### Scenario: Tab title
- **WHEN** a tab shows Pods on a cluster
- **THEN** its title shows Pods with the cluster's name and color

#### Scenario: Preview tab title
- **WHEN** a tab is a preview tab
- **THEN** its title is shown in italics

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

### Requirement: Tabs cannot be zoomed
No tab SHALL be zoomable, whatever kind of tab it is and however many panes are open. Where the UI kit still draws a zoom entry in a tab's menu, that entry MUST be disabled.

#### Scenario: Single pane
- **WHEN** one pane is open and the user opens the tab's menu
- **THEN** the zoom entry is disabled, and choosing it changes nothing

#### Scenario: Split panes
- **WHEN** several panes are open and the user opens a tab's menu
- **THEN** the zoom entry is disabled, and every pane stays visible
