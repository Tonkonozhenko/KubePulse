# Spec Delta

## MODIFIED Requirements

### Requirement: Icons render
The application SHALL make its icon catalog available before opening a window, so every icon it or a kit component draws is visible on every supported platform.

#### Scenario: Rail buttons
- **WHEN** the application opens
- **THEN** the rail's search, local terminal, and settings buttons show their icons

#### Scenario: Protected cluster mark
- **WHEN** a protected cluster is on the rail
- **THEN** its rail icon shows a visible shield
