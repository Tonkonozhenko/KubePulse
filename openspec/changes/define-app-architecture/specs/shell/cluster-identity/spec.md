# Spec Delta

## Purpose

Makes the target cluster unmistakable everywhere in the interface, so that users do not act on the wrong cluster by accident.

## ADDED Requirements

### Requirement: Distinct cluster identity
Each cluster SHALL have a user-configurable name and color, shown on every tab, editor, shell, and destructive confirmation for that cluster.

#### Scenario: Default identity
- **WHEN** a cluster is connected for the first time
- **THEN** it receives a stable default color, and its context name is used as its display name

### Requirement: Protected clusters
The system SHALL let the user mark a cluster as protected, which raises the confirmation required for destructive actions on it.

#### Scenario: Protected cluster
- **WHEN** the user marks a cluster as protected
- **THEN** its identity is emphasized everywhere, and destructive actions require typing the object's name to confirm
