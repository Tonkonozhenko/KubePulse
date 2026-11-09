# shell/command-palette Specification

## Purpose
Gives users a keyboard-driven way to reach every action and view, including those contributed by extensions.

## Requirements

### Requirement: Command palette
The system SHALL provide a keyboard-driven command palette for actions and navigation, including those contributed by extensions.

#### Scenario: Open a resource kind by name
- **WHEN** the user opens the palette and types a kind name
- **THEN** the user can open that kind on a connected cluster using only the keyboard
