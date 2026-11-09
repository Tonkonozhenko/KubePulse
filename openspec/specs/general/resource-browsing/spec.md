# general/resource-browsing Specification

## Purpose
Lets users see and inspect every resource kind in a cluster through live tables that stay smooth at 10k pods in each cluster.

## Requirements

### Requirement: Every served resource kind is browsable
The system SHALL discover resource kinds from the cluster's API and offer a view for each listable kind, including custom resources, without kind-specific support.

#### Scenario: Built-in resource kinds
- **WHEN** the user connects to a cluster
- **THEN** the navigation lists the workload, network, config, storage, and access-control kinds the cluster serves

#### Scenario: Custom resource kind
- **WHEN** a cluster serves a custom resource kind
- **THEN** the user can list and inspect its objects, and any printer columns it declares appear in the table

#### Scenario: Custom resource definition added
- **WHEN** a custom resource definition is installed while the session is connected
- **THEN** its kind appears in the navigation without reconnecting

### Requirement: Namespace scoping
The system SHALL let the user view namespaced resources across all namespaces or restricted to a selected set of namespaces, chosen from a searchable multiselect on the table of every namespaced kind.

#### Scenario: Select namespaces
- **WHEN** the user selects two namespaces
- **THEN** the table shows only objects from those namespaces

#### Scenario: Default selection
- **WHEN** the user has selected no namespaces on a cluster
- **THEN** the control reads all namespaces, and the table shows objects from every namespace

#### Scenario: Clear the selection
- **WHEN** the user clears the selection
- **THEN** the table shows objects from every namespace

#### Scenario: Search namespaces
- **WHEN** the user types in the multiselect
- **THEN** the list narrows to matching namespaces, and namespaces already selected stay selected

#### Scenario: Custom resources
- **WHEN** the user views a namespaced custom resource kind
- **THEN** the multiselect is available, the same as on built-in kinds

### Requirement: Live resource tables
Resource tables SHALL reflect cluster changes without manual refresh.

#### Scenario: Object added or removed
- **WHEN** an object is created or deleted in the cluster
- **THEN** the table reflects the change within 2 seconds under normal network conditions

#### Scenario: Selection during updates
- **WHEN** rows change while the user has a row selected
- **THEN** the selection stays on the same object

### Requirement: Sorting and filtering
The system SHALL let the user sort by any visible column and filter by text and by labels.

#### Scenario: Filter by text
- **WHEN** the user types a filter
- **THEN** only objects matching the filter remain, and the row count updates

#### Scenario: Filter by label
- **WHEN** the user filters with a label selector
- **THEN** only objects with matching labels remain

### Requirement: Scale at 10k pods per cluster
With 10,000 pods in each connected cluster, the interface SHALL sustain at least 60 frames per second while scrolling on a reference machine.

#### Scenario: Scrolling a large table
- **WHEN** the user scrolls a table of 10,000 pods
- **THEN** rendering stays at 60 frames per second or higher, and row content appears without placeholders

#### Scenario: Sorting and filtering a large table
- **WHEN** the user sorts or filters a table of 10,000 pods
- **THEN** the result appears within 100 milliseconds, and the interface never stops responding

### Requirement: Stability under update bursts
Bursts of cluster changes MUST NOT reduce interface responsiveness or cause the visible state to fall behind the cluster permanently.

#### Scenario: Large rollout
- **WHEN** a burst of 1,000 object changes per second arrives for 30 seconds
- **THEN** the interface stays at 60 frames per second or higher, and the table matches the cluster within 2 seconds after the burst ends

### Requirement: Bounded memory use
The application SHALL use no more than 500 MiB of memory with one connected cluster showing 10,000 pods, and each additional such cluster SHALL add no more than 200 MiB.

#### Scenario: Memory at scale
- **WHEN** one cluster with 10,000 pods is connected and its pod table has been open for 10 minutes
- **THEN** application memory stays at or below 500 MiB

#### Scenario: Closing a view releases data
- **WHEN** no open tab has used a resource kind for 5 minutes
- **THEN** the system stops watching that kind and releases its cached data

### Requirement: Resource detail view
The system SHALL show an object's metadata, status, conditions, related events, and owner references when the user opens it.

#### Scenario: Open an object
- **WHEN** the user opens an object from a table
- **THEN** its details appear, including its recent events

#### Scenario: Owner navigation
- **WHEN** an object has an owner reference
- **THEN** the user can navigate to the owning object

### Requirement: Secret values hidden by default
The system MUST keep Secret values hidden until the user explicitly reveals them.

#### Scenario: Open a Secret
- **WHEN** the user opens a Secret
- **THEN** its keys are listed and its values stay hidden until revealed

### Requirement: Resource deletion requires confirmation
The system SHALL require explicit confirmation naming the cluster, namespace, and object before deleting a resource.

#### Scenario: Delete one object
- **WHEN** the user chooses to delete an object
- **THEN** a confirmation shows the cluster, namespace, and object name, and nothing is deleted until the user confirms

### Requirement: Namespace selection is per cluster
Each cluster SHALL have one namespace selection, shared by every namespaced kind and every tab on that cluster. It SHALL last until the application exits and SHALL NOT be saved.

#### Scenario: Open another kind
- **WHEN** the user selects a namespace on Pods and then opens Deployments on the same cluster
- **THEN** Deployments opens restricted to that namespace

#### Scenario: Two tabs on one cluster
- **WHEN** two tabs show namespaced kinds on the same cluster and the user changes the selection in one
- **THEN** both tables and both controls update

#### Scenario: Another cluster
- **WHEN** the user changes the selection on one cluster
- **THEN** tables on other clusters do not change

#### Scenario: Reconnect
- **WHEN** the user disconnects from a cluster and connects again without restarting
- **THEN** the selection still applies

#### Scenario: Restart
- **WHEN** the application restarts
- **THEN** every cluster starts on all namespaces

### Requirement: Cluster-scoped kinds ignore the selection
Tables of cluster-scoped kinds SHALL NOT show the namespace control and SHALL show all their objects whatever the cluster's selection is.

#### Scenario: View a cluster-scoped kind
- **WHEN** a namespace is selected and the user opens Nodes
- **THEN** the table shows every node, and no namespace control appears

#### Scenario: Return to a namespaced kind
- **WHEN** the user goes from a cluster-scoped kind back to a namespaced kind
- **THEN** the cluster's selection still applies

### Requirement: Namespace choices stay current
The multiselect SHALL offer the cluster's namespaces and any namespace present in the table's objects, and SHALL update while the cluster changes, without reconnecting.

#### Scenario: Namespace with no objects of this kind
- **WHEN** a namespace holds no objects of the kind on screen
- **THEN** it is still offered

#### Scenario: Namespace created
- **WHEN** a namespace is created while the cluster is connected
- **THEN** it is offered within 2 seconds under normal network conditions, and the current selection does not change

#### Scenario: Namespaces cannot be listed
- **WHEN** the user is not allowed to list namespaces
- **THEN** the multiselect offers the namespaces present in the table and says that the list may be incomplete

### Requirement: Selected namespaces stay selected
A selected namespace SHALL stay selected and offered after it is deleted, until the user removes it. The system MUST NOT widen a selection on its own.

#### Scenario: Selected namespace deleted
- **WHEN** a selected namespace is deleted from the cluster
- **THEN** it stays selected, and the table keeps showing only the selected namespaces, even if that leaves it empty

#### Scenario: Last selected namespace removed by the user
- **WHEN** the user removes a deleted namespace from the selection
- **THEN** it is no longer offered

### Requirement: Statuses show their health
The system SHALL show a recognized status as a tag colored by health, in tables and in the detail panel. The status text MUST stay visible and exactly as received, so health never depends on color alone. An unrecognized status SHALL appear in the neutral style.

#### Scenario: Healthy status
- **WHEN** an object's status is `Running`, `Bound`, or `Succeeded`
- **THEN** the status shows that text in a success-colored tag

#### Scenario: Status needing attention
- **WHEN** an object's status is `Pending` or `ContainerCreating`
- **THEN** the status shows that text in a warning-colored tag

#### Scenario: Failed status
- **WHEN** an object's status is `Failed` or `CrashLoopBackOff`
- **THEN** the status shows that text in a danger-colored tag

#### Scenario: Unrecognized status
- **WHEN** a custom resource reports a status the system does not recognize
- **THEN** the status shows that text in the neutral style

#### Scenario: Custom resource status column
- **WHEN** a custom resource declares a printer column named Status, Phase, or State, in any letter case
- **THEN** its table shows that column as a status

#### Scenario: Theme change
- **WHEN** the user changes theme
- **THEN** status colors come from the new theme

### Requirement: Pod status shows why a container is waiting
A pod's status SHALL be the first waiting container's reason when one exists, and its phase otherwise. The table and the detail panel MUST show the same status for the same pod.

#### Scenario: Crash-looping pod
- **WHEN** a pod's phase is `Running` and one of its containers is waiting with reason `CrashLoopBackOff`
- **THEN** the table and the detail panel both show `CrashLoopBackOff` in a danger-colored tag

#### Scenario: Pod with no waiting container
- **WHEN** no container in a pod is waiting
- **THEN** its status is its phase

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

### Requirement: Selecting rows with checkboxes
Every table SHALL show a pinned checkbox column. Checking rows MUST NOT open the detail panel, and it SHALL be independent of the row whose details are open.

#### Scenario: Toggle one row
- **WHEN** the user clicks a row's checkbox
- **THEN** only that row's checked state changes

#### Scenario: Open details while rows are checked
- **WHEN** rows are checked and the user clicks another row outside its checkbox
- **THEN** that row's details open, and the checked rows do not change

#### Scenario: Select every row shown
- **WHEN** the user clicks the header checkbox and any shown row is unchecked
- **THEN** every row shown becomes checked

#### Scenario: Clear from the header
- **WHEN** the user clicks the header checkbox and every shown row is checked
- **THEN** no row stays checked

### Requirement: Shift-selecting a range
Shift+click on a checkbox SHALL check every row between the last row whose checkbox was clicked and this row, inclusive, in the order shown. It SHALL add to the rows already checked, and MUST NOT uncheck any row.

#### Scenario: Range downward
- **WHEN** the user clicks row 3's checkbox and then Shift+clicks row 7's
- **THEN** rows 3 through 7 are checked

#### Scenario: Range upward
- **WHEN** the user clicks row 7's checkbox and then Shift+clicks row 3's
- **THEN** rows 3 through 7 are checked

#### Scenario: Range after sorting
- **WHEN** the user clicks a row's checkbox, sorts the table, and then Shift+clicks another row's
- **THEN** the rows between those two objects in the new order are checked

#### Scenario: No earlier click
- **WHEN** the user Shift+clicks a checkbox with no earlier checkbox click in this table, or that row has left the table
- **THEN** only the clicked row becomes checked

### Requirement: Checked rows stay on their objects
Checked rows SHALL follow their objects through sorting and live updates. A row that leaves the table, including through a filter or namespace change, SHALL stop being checked, so hidden objects can never be deleted.

#### Scenario: Rows arrive above a checked row
- **WHEN** rows are added above a checked row
- **THEN** the same object stays checked

#### Scenario: Filter hides a checked row
- **WHEN** the user filters a table so that a checked row is hidden, and then clears the filter
- **THEN** that row is not checked

#### Scenario: Checked object deleted elsewhere
- **WHEN** a checked object is deleted from the cluster
- **THEN** the selected count drops by one

#### Scenario: Selecting at scale
- **WHEN** the user selects every row of a table of 10,000 pods
- **THEN** the table updates within 100 milliseconds and keeps scrolling at 60 frames per second or higher

### Requirement: Deleting several objects
When rows are checked, the table SHALL show how many and offer to delete them or clear the selection. The confirmation SHALL name the cluster and list every object with its namespace, and nothing SHALL be deleted until the user confirms.

#### Scenario: Confirm a multi-deletion
- **WHEN** the user chooses to delete three checked objects
- **THEN** a confirmation shows the cluster and those three objects with their namespaces, and nothing is deleted until the user confirms

#### Scenario: Enter does not confirm
- **WHEN** the confirmation is open and the user presses Enter
- **THEN** nothing is deleted

#### Scenario: Protected cluster
- **WHEN** the user deletes several objects on a protected cluster
- **THEN** the confirm button appears only after the user types the cluster's name

#### Scenario: Row action on a protected cluster
- **WHEN** the user deletes one object from its row on a protected cluster
- **THEN** the confirmation asks for the object's name, the same as deleting from the detail panel

### Requirement: Multi-deletion reports each result
The system SHALL report progress while deleting and SHALL name every object that failed with the cluster's error. A failure MUST NOT stop the remaining deletions, and an object already gone SHALL count as deleted.

#### Scenario: Some objects fail
- **WHEN** one of three deletions is denied
- **THEN** the other two are deleted, the confirmation stays open naming the denied object and the error, and only that object stays checked

#### Scenario: Retry
- **WHEN** the confirmation shows failures and the user confirms again
- **THEN** only the failed objects are retried

#### Scenario: Object already gone
- **WHEN** an object in the list no longer exists
- **THEN** it counts as deleted, and no failure is shown for it

#### Scenario: Cancel during deletion
- **WHEN** the user cancels while deletions are running
- **THEN** no further deletions start, and the objects already deleted stay deleted

#### Scenario: All objects deleted
- **WHEN** every deletion succeeds
- **THEN** the confirmation closes, and the deleted objects are no longer checked

#### Scenario: Open object is deleted
- **WHEN** a deletion removes the object open in the detail panel
- **THEN** the detail panel closes

### Requirement: Closing the detail panel
The detail panel SHALL have a close button. Closing it SHALL give its width back to the table and clear the highlighted row, so the same row can be opened again.

#### Scenario: Close the panel
- **WHEN** the user clicks the panel's close button
- **THEN** the panel closes, the table takes the full width, and no row is highlighted

#### Scenario: Reopen the same object
- **WHEN** the user closes the panel and clicks the same row
- **THEN** that object's details open

#### Scenario: Escape in the table
- **WHEN** the table has focus and the user presses Escape
- **THEN** the panel closes

#### Scenario: Checked rows survive closing
- **WHEN** rows are checked and the user closes the panel
- **THEN** the same rows stay checked

### Requirement: Detail panel keeps its identity and actions in view
The detail panel SHALL show the object's kind, name, status, and actions at its top, and they SHALL stay in place while its content scrolls.

#### Scenario: Scroll a long object
- **WHEN** the user scrolls down a long detail panel
- **THEN** the title, status, close button, and actions stay visible

#### Scenario: Too little room
- **WHEN** the panel's content is taller than the panel
- **THEN** a scrollbar shows, every section keeps its full height, and the user can scroll to the last one

### Requirement: Labels are shown as tags
The detail panel SHALL show each label as its own wrapping tag holding the full key and value.

#### Scenario: Many labels
- **WHEN** an object has more labels than fit on one line
- **THEN** the tags wrap onto further lines, and none is cut off

### Requirement: Conditions show their health
The detail panel SHALL show each condition's type, status, reason, and message, colored by whether that status is healthy for that type. For types that describe a problem, True SHALL be unhealthy and False healthy.

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

### Requirement: Events show their severity
The detail panel SHALL show each event's type, reason, count, and message, with Warning events in the warning color.

#### Scenario: Warning event
- **WHEN** an object has a Warning event
- **THEN** its type shows in a warning-colored tag, and its message shows exactly as received

#### Scenario: No events
- **WHEN** the cluster holds no events for an object, for example because they have expired
- **THEN** the section says there are no recent events, rather than staying blank

### Requirement: Annotations are available
The detail panel SHALL list an object's annotations in a section that starts collapsed, and showing it SHALL reveal each full key and value. On a Secret, the panel MUST show annotation keys only, because an annotation can hold a copy of the Secret's data.

#### Scenario: Expand annotations
- **WHEN** the user expands the annotations of a Deployment
- **THEN** every annotation's key and full value appear

#### Scenario: Annotations on a Secret
- **WHEN** the user expands the annotations of a Secret
- **THEN** only the keys appear, and no value is shown
