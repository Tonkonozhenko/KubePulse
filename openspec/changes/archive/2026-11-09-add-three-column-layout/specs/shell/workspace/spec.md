# Spec Delta

## ADDED Requirements

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

#### Scenario: Restart
- **WHEN** the application restores a layout
- **THEN** the restored active tab's cluster is selected, and no connection starts

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

### Requirement: Preview state survives restart
Workspace restore SHALL keep each tab's pinned or preview state, and SHALL leave at most one preview tab.

#### Scenario: Restart with a preview tab
- **WHEN** the application restarts with one preview tab and pinned tabs open
- **THEN** the same tabs are restored, and the preview tab is still the only one replaceable by a single click

#### Scenario: Layout saved by an older version
- **WHEN** a saved layout has no pinned information
- **THEN** all its tabs load as pinned

#### Scenario: Several preview tabs in a saved layout
- **WHEN** a saved layout marks several tabs as previews
- **THEN** all but one load as pinned
