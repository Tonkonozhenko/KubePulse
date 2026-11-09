# general/cluster-sessions Specification

## Purpose
Lets users connect to any number of Kubernetes clusters at once, with each cluster isolated so that a slow or failing cluster never affects the others.

## Requirements

### Requirement: Cluster discovery from kubeconfig
The system SHALL add every context from the user's default kubeconfig to the cluster list automatically, honoring the `KUBECONFIG` environment variable and merging multiple files in the standard Kubernetes order.

#### Scenario: Default kubeconfig
- **WHEN** `KUBECONFIG` is unset and the default kubeconfig file exists
- **THEN** the system lists every context from that file

#### Scenario: Multiple kubeconfig files
- **WHEN** `KUBECONFIG` names several files
- **THEN** the system lists the merged set of contexts, and the first file to define a context wins

#### Scenario: Many contexts
- **WHEN** the kubeconfig has hundreds of contexts
- **THEN** the full list appears without slowing startup, and the user can search it by name

#### Scenario: Kubeconfig changes on disk
- **WHEN** a kubeconfig file changes while the application is running
- **THEN** the cluster list updates without a restart

#### Scenario: Invalid kubeconfig
- **WHEN** a kubeconfig file cannot be parsed
- **THEN** the system reports the file and the error, and still lists contexts from any valid files

### Requirement: Explicit cluster connection
The system SHALL connect to a cluster only when the user chooses to connect it. Listing clusters, opening a cluster, and activating a tab MUST NOT contact any cluster or run any credential helper.

#### Scenario: Startup with many contexts
- **WHEN** the application starts with hundreds of contexts in the kubeconfig
- **THEN** all of them are listed as disconnected, with no network requests and no credential helpers run

#### Scenario: User connects a cluster
- **WHEN** the user chooses to connect a disconnected cluster
- **THEN** the system connects to that cluster only, and the others stay disconnected

#### Scenario: User opens a disconnected cluster
- **WHEN** the user opens a cluster or a tab whose cluster is disconnected
- **THEN** no connection starts, and the view offers a connect action

### Requirement: Explicit cluster disconnection
A connected cluster SHALL stay connected until the user disconnects it or the application exits. The system MUST NOT disconnect a cluster because it is idle or has no open tabs.

#### Scenario: Last tab closed
- **WHEN** the user closes the last tab on a connected cluster
- **THEN** the cluster stays connected

#### Scenario: Cluster idle
- **WHEN** a connected cluster goes unused for any length of time
- **THEN** the cluster stays connected

### Requirement: Concurrent cluster sessions
The system SHALL allow the user to connect to multiple clusters at the same time, with no fixed limit on the number of connected clusters.

#### Scenario: Connect a second cluster
- **WHEN** the user connects to a cluster while another is already connected
- **THEN** both clusters are usable at the same time

### Requirement: Cluster failure isolation
A failure, timeout, or slow response in one cluster session MUST NOT block or degrade the user interface or any other cluster session.

#### Scenario: Unreachable cluster during connect
- **WHEN** the user connects to an unreachable cluster
- **THEN** other clusters and the user interface stay responsive while the connection is attempted

#### Scenario: Slow authentication
- **WHEN** a credential helper takes a long time to respond
- **THEN** the session shows that it is authenticating, and the user can cancel the attempt

### Requirement: Visible connection state
The system SHALL show each cluster's connection state as one of disconnected, connecting, authenticating, connected, degraded, or failed.

#### Scenario: Cluster list shows state
- **WHEN** the user views the cluster list
- **THEN** every entry shows its connection state, and connected clusters are clearly distinguishable from disconnected ones

#### Scenario: Connection lost
- **WHEN** a connected cluster stops responding
- **THEN** its state changes to degraded and its last known data stays visible, marked as stale

#### Scenario: Connection failure reason
- **WHEN** a connection attempt fails
- **THEN** the system shows the reason and offers a retry

### Requirement: Automatic reconnection
The system SHALL retry a lost cluster connection with increasing delays, and SHALL bring all data back up to date once the connection is restored.

#### Scenario: Connection restored
- **WHEN** a degraded cluster becomes reachable again
- **THEN** its state returns to connected and stale data is refreshed without user action

### Requirement: Credential handling
The system SHALL authenticate using the mechanisms defined in the kubeconfig, including credential helpers, and MUST NOT persist credentials to its own storage or write them to logs.

#### Scenario: Credential helper login
- **WHEN** a context uses a credential helper
- **THEN** the system runs that helper to obtain credentials

#### Scenario: Application logs
- **WHEN** the application writes diagnostic logs
- **THEN** tokens, certificates, and other credentials are absent from them

### Requirement: Cluster permissions are respected
The system SHALL treat authorization failures as normal states, and SHALL show what the user lacks access to instead of failing the session.

#### Scenario: Forbidden resource kind
- **WHEN** the user's account cannot list a resource kind
- **THEN** that view reports that access is denied, and the rest of the session keeps working

### Requirement: Cluster-scoped work ends with disconnection
Disconnecting a cluster SHALL stop all of its data streams, shells, log streams, and port-forwards, and SHALL release its cached data.

#### Scenario: User disconnects a cluster with active work
- **WHEN** the user disconnects a cluster that has active shells or port-forwards
- **THEN** the system warns the user first, then stops that work and releases the session's memory
