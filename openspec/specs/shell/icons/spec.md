# shell/icons Specification

## Purpose
Gives kinds, actions, and states one consistent icon each, drawn in theme colors and always paired with a text label.

## Requirements

### Requirement: Icons render
The application SHALL make its icon catalog available before opening a window, so every icon it or a kit component draws is visible on every supported platform.

#### Scenario: Rail buttons
- **WHEN** the application opens
- **THEN** the rail's search, local terminal, and settings buttons show their icons

#### Scenario: Protected cluster mark
- **WHEN** a protected cluster is on the rail
- **THEN** its rail icon shows a visible shield

### Requirement: Kinds have icons
The sidebar and tab titles SHALL show an icon for each kind, using the icon its view declares. A kind with no declared icon SHALL show a generic icon.

#### Scenario: Built-in kind
- **WHEN** a connected cluster serves Pods
- **THEN** the Pods sidebar entry and any Pods tab title show the same icon

#### Scenario: Undeclared kind
- **WHEN** a custom kind appears under Other resources
- **THEN** it shows the generic icon

### Requirement: Icons accompany labels
Icons SHALL be added beside visible text and SHALL NOT replace it, so meaning never depends on an icon alone. An icon-only control SHALL have a translated tooltip.

#### Scenario: Action button
- **WHEN** a Connect, Disconnect, Retry, Delete, Reveal, or Hide action is shown
- **THEN** it shows an icon and its translated label

#### Scenario: Icon-only control
- **WHEN** the user hovers an icon-only rail button
- **THEN** a tooltip names its action

#### Scenario: Filter inputs
- **WHEN** the resource browser shows its filter and label selector inputs
- **THEN** each shows an icon and keeps its placeholder text

### Requirement: States use one icon each
Each connection state SHALL use the same indicator wherever it is shown, and a state's text SHALL stay visible beside it outside the rail. A connection attempt SHALL show a spinner.

#### Scenario: Failed cluster
- **WHEN** a selected cluster has failed
- **THEN** the rail mark and the sidebar header use the same failed icon

### Requirement: Icons follow the theme
Icons SHALL take their color from the current theme, so they stay legible when the theme or mode changes.

#### Scenario: Change mode
- **WHEN** the user switches between light and dark
- **THEN** icons change color with the surrounding interface
