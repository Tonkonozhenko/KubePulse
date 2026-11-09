# Spec Delta

## Purpose

Lets users restart the pods of a Deployment, StatefulSet, or DaemonSet through the workload's own rollout, for one object or several, after a confirmation naming what will restart.

## ADDED Requirements

### Requirement: Restartable kinds
The system SHALL offer Restart on Deployments, StatefulSets, and DaemonSets from the `apps` group, in the row actions column, the row's right-click menu, and the detail panel. Other kinds MUST NOT offer it.

#### Scenario: Deployment row
- **WHEN** the user views a Deployments table
- **THEN** each row offers Restart, and the row's right-click menu offers it too

#### Scenario: Detail panel
- **WHEN** the user opens a StatefulSet or a DaemonSet
- **THEN** the detail panel's actions include Restart

#### Scenario: Other kind
- **WHEN** the user views Pods, ReplicaSets, Jobs, or a custom resource kind
- **THEN** no Restart action is offered

### Requirement: Restart uses the workload's rollout
Restarting SHALL record the current time on the workload's pod template, so its controller replaces the pods using the workload's own update strategy. The system MUST NOT delete pods directly, and MUST NOT change anything else in the object.

#### Scenario: Restart a Deployment
- **WHEN** the user confirms restarting a Deployment
- **THEN** its pod template carries the restart time, and the Deployment rolls out new pods according to its strategy

#### Scenario: Restart twice
- **WHEN** the user restarts the same object again later
- **THEN** the recorded time is replaced with the new time, and another rollout starts

### Requirement: Restart requires confirmation
The system SHALL ask for confirmation naming the cluster and every object with its namespace, and nothing SHALL restart until the user confirms. Pressing Enter MUST NOT confirm. The confirmation shows no diff.

#### Scenario: Restart one object
- **WHEN** the user chooses Restart on a Deployment
- **THEN** a confirmation shows the cluster, namespace, and Deployment name, and nothing restarts until the user confirms

#### Scenario: Enter does not confirm
- **WHEN** the confirmation is open and the user presses Enter
- **THEN** nothing restarts

#### Scenario: Protected cluster, one object
- **WHEN** the user restarts one object on a protected cluster
- **THEN** the confirm button appears only after the user types the object's name

#### Scenario: Protected cluster, several objects
- **WHEN** the user restarts several objects on a protected cluster
- **THEN** the confirm button appears only after the user types the cluster's name

### Requirement: Restart never overwrites other changes
The restart write MUST carry the `resourceVersion` the system read. If the object changed in between, the system SHALL read it again and record the restart on the newer version, so no one else's change is lost.

#### Scenario: Object changes during the restart
- **WHEN** another actor updates a Deployment between the system reading it and writing the restart
- **THEN** the other actor's change is kept, and the Deployment also carries the restart time

#### Scenario: Object keeps changing
- **WHEN** the object changes on every attempt and the system gives up
- **THEN** the restart is reported as failed with the cluster's conflict error

### Requirement: Restarting several objects
When checked rows belong to a restartable kind, the table SHALL offer to restart them next to deleting them. The confirmation SHALL list every checked object.

#### Scenario: Restart selected
- **WHEN** the user checks three Deployments and chooses Restart selected
- **THEN** a confirmation shows the cluster and those three Deployments with their namespaces

#### Scenario: Kind that cannot restart
- **WHEN** the user checks rows in a Pods table
- **THEN** no Restart selected button appears

### Requirement: Restart reports each result
The system SHALL report progress while restarting and SHALL name every object that failed with the cluster's error. A failure MUST NOT stop the remaining restarts. An object that no longer exists SHALL be reported as failed.

#### Scenario: Some objects fail
- **WHEN** one of three restarts is denied
- **THEN** the other two restart, and the confirmation stays open naming the denied object and the error

#### Scenario: Retry
- **WHEN** the confirmation shows failures and the user confirms again
- **THEN** only the failed objects are restarted again

#### Scenario: Object gone
- **WHEN** an object in the list was deleted before its restart ran
- **THEN** it is reported as failed with the cluster's error

#### Scenario: Cancel during restart
- **WHEN** the user cancels while restarts are running
- **THEN** no further restarts start, and the objects already restarted stay restarted

#### Scenario: All objects restarted
- **WHEN** every restart succeeds
- **THEN** the confirmation closes, and the checked rows stay checked

### Requirement: Restart permission comes from the cluster
The system SHALL offer Restart whatever the user's permissions are, and SHALL report the cluster's denial when the user may not update the object.

#### Scenario: Denied
- **WHEN** the user's account may read a Deployment but not update it, and the user confirms a restart
- **THEN** the confirmation shows the cluster's denial for that Deployment, and nothing changes
