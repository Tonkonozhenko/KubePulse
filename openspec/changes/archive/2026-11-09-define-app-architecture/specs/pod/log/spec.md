# Spec Delta

## Purpose

Lets users read, follow, and search container logs without leaving the application or letting log volume degrade it.

## ADDED Requirements

### Requirement: View container logs
The system SHALL show logs for any container of a pod, including init containers and the previous instance of a restarted container.

#### Scenario: Single-container pod
- **WHEN** the user opens logs for a pod with one container
- **THEN** that container's logs appear

#### Scenario: Multi-container pod
- **WHEN** the user opens logs for a pod with several containers
- **THEN** the user can choose which container to view

#### Scenario: Restarted container
- **WHEN** a container has restarted
- **THEN** the user can view the logs of its previous instance

### Requirement: Follow logs live
The system SHALL stream new log lines as they are produced, and SHALL pause automatic scrolling while the user reads earlier lines.

#### Scenario: Reading earlier lines
- **WHEN** the user scrolls up while logs are streaming
- **THEN** the view stays in place, the stream continues, and the user can jump back to the newest line

#### Scenario: Stream interrupted
- **WHEN** a log stream ends because the connection dropped
- **THEN** the system shows the interruption and resumes without duplicating or silently losing the lines it can recover

### Requirement: Search logs
The system SHALL let the user search the retained log lines and move between matches.

#### Scenario: Search retained lines
- **WHEN** the user searches for text
- **THEN** matches are highlighted and the user can step through them

### Requirement: Log timestamps
The system SHALL let the user show or hide line timestamps.

#### Scenario: Show timestamps
- **WHEN** the user turns on timestamps
- **THEN** each line shows the time reported by the cluster

### Requirement: Bounded log retention
The system SHALL keep a bounded number of log lines in memory, and high log volume MUST NOT degrade interface responsiveness.

#### Scenario: High-volume stream
- **WHEN** a container writes 10,000 lines per second
- **THEN** the interface stays responsive, memory stays within the retention limit, and the oldest lines are dropped with a visible notice
