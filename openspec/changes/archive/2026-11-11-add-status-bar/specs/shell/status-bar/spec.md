## Purpose

Gives the main window one fixed place that states which cluster the user is working in, whether it is connected, and what is running on it.

## ADDED Requirements

### Requirement: Status bar shows the selected cluster
The main window SHALL show a status bar along its bottom edge, under the rail, sidebar, and tabs. It SHALL show the selected cluster's color, name, and connection state, and SHALL follow the selection as the active tab changes. A protected cluster SHALL be emphasized in the bar the same way it is elsewhere.

#### Scenario: Switching tabs
- **WHEN** the user switches from a tab on one cluster to a tab on another
- **THEN** the status bar shows the second cluster's color, name, and state

#### Scenario: No cluster selected
- **WHEN** no cluster is selected
- **THEN** the status bar shows no cluster details

### Requirement: Status bar explains a broken connection
When the selected cluster's connection failed or is being retried, the status bar SHALL show the reason and, while retrying, that the data shown is the last known. A reason too long for the bar SHALL end in an ellipsis, with the full reason exactly as received on hover.

#### Scenario: Failed connection
- **WHEN** the selected cluster's connection attempt fails because a credential helper exits with an error
- **THEN** the status bar shows the failure reason next to the Failed state

#### Scenario: Lost connection
- **WHEN** the selected cluster's connection is lost and being retried
- **THEN** the status bar shows Degraded, the reason, and that the last known data is shown

### Requirement: Status bar shows running work
The status bar SHALL show how much work is running on the selected cluster (its shells and port-forwards, the same work a disconnect warns about), and SHALL list that work on hover. It SHALL show nothing for running work when there is none.

#### Scenario: Open shell and port-forward
- **WHEN** the selected cluster has one open shell and one active port-forward
- **THEN** the status bar shows a count of two, and hovering lists both

### Requirement: Status bar opens port-forwards
The status bar SHALL show how many port-forwards are active on the selected cluster. Clicking that count SHALL open the port-forwards list.

#### Scenario: Open the forwards list
- **WHEN** the selected cluster has one active port-forward and the user clicks its count in the status bar
- **THEN** the port-forwards list opens

### Requirement: List view leaves cluster details to the status bar
A cluster's list view SHALL show the cluster's color at its top, and SHALL NOT repeat the cluster's name or connection state there. A list view whose cluster is not connected SHALL still offer a connect action in its body.

#### Scenario: Connected list view
- **WHEN** a list view's cluster is connected
- **THEN** the table starts directly below the cluster color stripe

#### Scenario: Disconnected list view
- **WHEN** a list view's cluster is disconnected
- **THEN** the list view offers a connect action, and the status bar shows the Disconnected state
