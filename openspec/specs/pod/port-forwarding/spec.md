# pod/port-forwarding Specification

## Purpose
Lets users reach ports on pods and services from their own machine, and keep track of every active forward.

## Requirements

### Requirement: Create a port-forward
The system SHALL forward a local port to a port on a pod, or on a service through one of its ready pods.

#### Scenario: Forward to a pod
- **WHEN** the user forwards a local port to a pod port
- **THEN** connections to the local port reach that pod

#### Scenario: Forward to a service
- **WHEN** the user forwards a local port to a service
- **THEN** the system selects a ready pod behind the service and forwards to it

#### Scenario: Automatic local port
- **WHEN** the user does not choose a local port
- **THEN** the system picks a free one and shows it

#### Scenario: Local port in use
- **WHEN** the chosen local port is already in use
- **THEN** the system reports the conflict and starts nothing

### Requirement: Local-only binding
Port-forwards MUST listen only on the loopback interface.

#### Scenario: Remote machine connects
- **WHEN** another machine tries to connect to a forwarded port
- **THEN** the connection is not accepted

### Requirement: Port-forward management
The system SHALL list all active port-forwards across all clusters, showing each one's cluster, target, ports, and status, and SHALL let the user stop any of them.

#### Scenario: Forwards outlive their tab
- **WHEN** the user closes the tab that started a port-forward
- **THEN** the forward keeps running and stays listed

#### Scenario: Stop a forward
- **WHEN** the user stops a port-forward
- **THEN** its local port is released

### Requirement: Port-forward failure reporting
The system SHALL show when a port-forward stops working, and MUST NOT report it as active.

#### Scenario: Target pod deleted
- **WHEN** the target pod of a pod forward is deleted
- **THEN** the forward is marked as failed and the user can restart it against another target

#### Scenario: Service backend replaced
- **WHEN** the selected pod behind a service forward goes away and another ready pod exists
- **THEN** the system reconnects to the ready pod
