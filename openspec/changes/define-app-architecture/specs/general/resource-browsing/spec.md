# Spec Delta

## Purpose

Lets users see and inspect every resource kind in a cluster through live tables that stay smooth at 10k pods in each cluster.

## ADDED Requirements

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
The system SHALL let the user view namespaced resources across all namespaces or restricted to a selected set of namespaces.

#### Scenario: Select namespaces
- **WHEN** the user selects two namespaces
- **THEN** the table shows only objects from those namespaces

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
