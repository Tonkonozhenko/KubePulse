# Spec Delta

## Purpose

Gives users one place to change application preferences, and keeps those preferences across restarts on the same machine without any network access.

## ADDED Requirements

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
- **WHEN** the application restarts with a Settings tab in the layout
- **THEN** the tab returns with the rest of the layout

### Requirement: Preferences persist
The system SHALL save preferences in the platform's configuration directory when they change, and SHALL apply them before the first frame on the next launch.

#### Scenario: Restart keeps a preference
- **WHEN** the user chooses Dark mode and restarts the application
- **THEN** the first frame uses Dark mode

#### Scenario: Damaged preferences file
- **WHEN** the preferences file cannot be parsed at startup
- **THEN** the application starts with default preferences and logs the problem

#### Scenario: Preferences cannot be saved
- **WHEN** saving preferences fails
- **THEN** the change still applies for this session, and the failure is logged

### Requirement: Preferences stay local
Preferences MUST NOT contain credentials, and changing a preference MUST NOT make any network request.

#### Scenario: Changing a preference while offline
- **WHEN** the user changes a preference with no network available
- **THEN** the change applies normally
