# Spec Delta

## Purpose

Lets users open interactive terminal sessions inside running containers from within the application.

## ADDED Requirements

### Requirement: Open a shell in a container
The system SHALL open an interactive terminal session in a selected container of a running pod.

#### Scenario: Open a shell
- **WHEN** the user opens a shell on a running container
- **THEN** an interactive terminal connected to that container appears in a tab

#### Scenario: Choose a container
- **WHEN** the pod has several containers
- **THEN** the user chooses the container before the session starts

#### Scenario: No shell available
- **WHEN** the container has no usable shell
- **THEN** the system reports that no shell was found and lets the user enter a command to run instead

#### Scenario: Access denied
- **WHEN** the user's account is not allowed to exec into the pod
- **THEN** the system reports the denial from the cluster

### Requirement: Terminal behavior
The terminal SHALL support standard terminal emulation, including colors, full-screen programs, window resizing, scrollback, and copy and paste.

#### Scenario: Full-screen program
- **WHEN** the user runs a full-screen terminal program in the session
- **THEN** it renders and responds to input correctly

#### Scenario: Resize
- **WHEN** the terminal pane changes size
- **THEN** the container's program receives the new dimensions

### Requirement: Shell session lifetime
The system SHALL show when a shell session has ended and SHALL keep its output readable.

#### Scenario: Session ends
- **WHEN** the shell exits or its connection drops
- **THEN** the tab shows that the session ended, keeps its scrollback, and offers to reconnect

### Requirement: Shell sessions show their target
Each shell session SHALL display its cluster, namespace, pod, and container.

#### Scenario: Identify a session
- **WHEN** a shell session is open
- **THEN** its tab shows the cluster identity and the full target
