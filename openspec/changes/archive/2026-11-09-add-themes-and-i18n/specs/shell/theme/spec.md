# Spec Delta

## Purpose

Lets users control the application's appearance through light and dark modes, bundled themes, and their own theme files, on every supported platform.

## ADDED Requirements

### Requirement: Appearance mode
The system SHALL offer System, Light, and Dark appearance modes, defaulting to System. System mode SHALL follow the operating system's appearance while the application runs.

#### Scenario: Default on first launch
- **WHEN** the application starts with no saved preferences on a system set to dark
- **THEN** the first frame uses the dark theme

#### Scenario: System appearance changes
- **WHEN** the mode is System and the operating system switches between light and dark
- **THEN** the application switches with it, without a restart

#### Scenario: Fixed mode ignores the system
- **WHEN** the mode is Dark and the operating system switches to light
- **THEN** the application stays dark

### Requirement: Theme selection
The system SHALL let the user choose one theme for light mode and one for dark mode, each from the themes of that mode. A change SHALL apply to every view immediately, without a restart.

#### Scenario: Choose a dark theme
- **WHEN** the user chooses Slate Dark while the application is dark
- **THEN** every open view shows the new theme

#### Scenario: Choose the theme for the inactive mode
- **WHEN** the user chooses a light theme while the application is dark
- **THEN** the appearance does not change until the application is light

#### Scenario: Pickers show only matching themes
- **WHEN** the user opens the dark theme picker
- **THEN** it lists only dark themes

### Requirement: Bundled themes
The system SHALL bundle Default Light, Default Dark, High Contrast Light, High Contrast Dark, Paper Light, and Slate Dark. Body text SHALL reach a contrast ratio of at least 7:1 in the High Contrast themes and 4.5:1 in the others.

#### Scenario: No theme files present
- **WHEN** the user has no theme files
- **THEN** all six bundled themes are available

#### Scenario: Contrast of a bundled theme
- **WHEN** a bundled theme's foreground is measured against its background
- **THEN** the ratio meets the limit for that theme

### Requirement: User theme files
The system SHALL load theme files from a `themes` folder in the configuration directory. Adding, changing, or removing a file SHALL take effect without a restart.

#### Scenario: Add a theme file
- **WHEN** the user saves a valid theme file in the folder
- **THEN** its themes appear in the pickers without a restart

#### Scenario: Edit the active theme
- **WHEN** the user changes the file of the active theme
- **THEN** every view shows the change without a restart

#### Scenario: Open the folder
- **WHEN** the user selects the theme folder action in the Settings tab
- **THEN** the folder opens in the platform's file manager, created first if it did not exist

### Requirement: Theme file errors are shown
A theme file that cannot be loaded SHALL be skipped, and the Settings tab SHALL name the file and the reason until the problem is fixed. Other themes MUST keep working.

#### Scenario: Invalid file
- **WHEN** a file in the themes folder is not valid
- **THEN** the Settings tab names the file and the error, and every other theme stays available

#### Scenario: Name already taken
- **WHEN** a theme file uses the name of a bundled theme or of a theme in another file
- **THEN** that theme is skipped, and the Settings tab names the file and the conflict

#### Scenario: Problem fixed
- **WHEN** the user corrects an invalid file
- **THEN** the error disappears and the file's themes become available, without a restart

### Requirement: Missing selected theme
If the selected theme is unavailable, the system SHALL use the default theme of that mode, tell the user in the Settings tab, and keep the selection, so the theme returns when it is available again.

#### Scenario: Selected theme file removed
- **WHEN** the file holding the active theme is removed
- **THEN** the application uses the default theme of that mode, and the Settings tab says the chosen theme is missing

#### Scenario: Theme file restored
- **WHEN** that file returns
- **THEN** the chosen theme applies again without any action from the user

### Requirement: Themes cover the whole application
Every host-drawn view SHALL take its colors from the active theme, including extension interfaces and the editor, log, and terminal surfaces. Colors in the terminal output and syntax highlighting SHALL also come from the theme.

#### Scenario: Declarative extension view
- **WHEN** the theme changes while an extension view is open
- **THEN** the view changes with it, and the extension takes no part

#### Scenario: Terminal colors
- **WHEN** a program in a terminal prints colored text
- **THEN** the named colors come from the active theme

### Requirement: Cluster colors do not follow the theme
A cluster's identity color SHALL stay the same in every theme and mode.

#### Scenario: Switch theme
- **WHEN** the user changes the theme
- **THEN** each cluster keeps its color
