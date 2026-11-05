# Spec Delta

## Purpose

Defines the operating systems and display servers on which the application runs as a native application.

## ADDED Requirements

### Requirement: Supported platforms
The system SHALL run as a native application on Linux, macOS, and Windows, with the same features on each.

#### Scenario: Feature parity
- **WHEN** a release is published
- **THEN** every first-release capability is available on Linux, macOS, and Windows

#### Scenario: Linux display servers
- **WHEN** the application starts on Linux
- **THEN** it runs under both Wayland and X11
