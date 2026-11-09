# Spec Delta

## MODIFIED Requirements

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

## ADDED Requirements

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
- **WHEN** the application restarts, including when it restores open tabs
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
