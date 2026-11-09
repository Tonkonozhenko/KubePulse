# Spec Delta

## Purpose

Lets users run interactive shells on their own machine inside the application, next to their cluster views, without switching to another program.

## ADDED Requirements

### Requirement: Open a local terminal
The system SHALL open a new local terminal tab, and make it active with keyboard focus in the terminal, each time the user asks for one. Several local terminal tabs SHALL be able to run at the same time.

#### Scenario: Open from the rail
- **WHEN** the user selects the terminal button at the bottom of the cluster rail
- **THEN** a new local terminal tab opens, becomes active, and receives keyboard input

#### Scenario: Open from the command palette
- **WHEN** the user opens the command palette and chooses New local terminal
- **THEN** a new local terminal tab opens and becomes active

#### Scenario: Open a second terminal
- **WHEN** a local terminal tab is open and the user selects the terminal button again
- **THEN** a second local terminal tab opens with its own shell, and the first keeps running

#### Scenario: No cluster connected
- **WHEN** the user opens a local terminal while no cluster is connected
- **THEN** the terminal opens and runs normally

### Requirement: Rail terminal button
The bottom of the cluster rail SHALL show an icon-only terminal button directly above the Settings gear, with a translated tooltip naming its action.

#### Scenario: Button placement
- **WHEN** the application opens
- **THEN** the terminal button is directly above the Settings gear at the bottom of the rail

#### Scenario: Button tooltip
- **WHEN** the user hovers the terminal button
- **THEN** a tooltip names the action of opening a local terminal

### Requirement: Local shell
A local terminal SHALL run the user's default shell for the platform, started in the user's home directory with the application's environment. Opening a local terminal MUST NOT change the kubeconfig, the current context, or any cluster connection.

#### Scenario: Default shell
- **WHEN** a local terminal opens
- **THEN** it runs the user's default shell, and its working directory is the user's home directory

#### Scenario: Command-line tools see the same kubeconfig
- **WHEN** the user runs a command-line Kubernetes client in a local terminal
- **THEN** it reads the same default kubeconfig the application lists clusters from

#### Scenario: Selected cluster is not applied
- **WHEN** the user opens a local terminal while a cluster is selected
- **THEN** the kubeconfig's current context is unchanged

### Requirement: Local terminal belongs to no cluster
A local terminal tab SHALL belong to no cluster. Its title SHALL show a terminal icon and a translated local terminal label with no cluster identity.

#### Scenario: Tab title
- **WHEN** a local terminal tab is open
- **THEN** its title shows the terminal icon and the local terminal label, and no cluster name or color

#### Scenario: Selected cluster stays the same
- **WHEN** the user activates a local terminal tab
- **THEN** the selected cluster does not change

#### Scenario: Cluster disconnects
- **WHEN** a cluster disconnects while a local terminal is open
- **THEN** the local terminal keeps running

### Requirement: Local terminal behavior
A local terminal SHALL behave like a container shell: colors, full-screen programs, window resizing, scrollback, selection, and copy and paste.

#### Scenario: Full-screen program
- **WHEN** the user runs a full-screen terminal program in a local terminal
- **THEN** it renders and responds to input correctly

#### Scenario: Resize
- **WHEN** the local terminal pane changes size
- **THEN** the shell receives the new dimensions

### Requirement: Local shell ends
When the shell of a local terminal exits, the tab SHALL show that it ended with its exit code, keep its scrollback, and offer to start a new shell in the same tab.

#### Scenario: Shell exits
- **WHEN** the user types exit in a local terminal
- **THEN** the tab shows the exit code and keeps the output above

#### Scenario: Start again
- **WHEN** the user chooses to start a new shell in an ended local terminal
- **THEN** a new shell starts in the same tab below the kept output

#### Scenario: Shell cannot start
- **WHEN** the shell cannot be started
- **THEN** the tab shows a translated summary followed by the error exactly as the platform reported it

### Requirement: Closing ends the local shell
Closing a local terminal tab or quitting the application SHALL end its shell and the programs running in the terminal, and MUST NOT ask for confirmation. Local terminals SHALL NOT be restored after a restart.

#### Scenario: Close the tab
- **WHEN** the user closes a local terminal tab while a program is running in it
- **THEN** the tab closes without a prompt, and the shell and that program stop

#### Scenario: Quit with a local terminal open
- **WHEN** the user quits while a local terminal is open
- **THEN** the application quits without a prompt, and the shell stops

#### Scenario: Restart with a local terminal open
- **WHEN** the application restarts while a local terminal tab is open
- **THEN** the application starts with no tabs
