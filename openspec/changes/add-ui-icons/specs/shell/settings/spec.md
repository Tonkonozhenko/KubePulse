# Spec Delta

## MODIFIED Requirements

### Requirement: Settings tab
The system SHALL show application preferences in a Settings tab that belongs to no cluster. At most one Settings tab SHALL exist in the workspace.

#### Scenario: Open settings
- **WHEN** the user selects the gear button at the bottom of the cluster rail
- **THEN** a Settings tab opens and becomes active

#### Scenario: Open settings from the keyboard
- **WHEN** the user presses the platform's settings shortcut, Command+Comma on macOS and Ctrl+Comma on Linux and Windows
- **THEN** the Settings tab opens and becomes active

#### Scenario: Settings already open
- **WHEN** the user opens settings while a Settings tab exists
- **THEN** the existing tab becomes active and no second tab opens

#### Scenario: Selected cluster stays the same
- **WHEN** the user activates the Settings tab
- **THEN** the selected cluster does not change

#### Scenario: Restart with settings open
- **WHEN** the application restarts while a Settings tab is open
- **THEN** the application starts with no tabs, and the Settings tab does not return
