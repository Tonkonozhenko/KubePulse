# shell/lifecycle Specification

## Purpose

Defines how the application quits, so that users can leave it with each platform's standard shortcut or by closing its window.

## Requirements

### Requirement: Quit shortcut
The system SHALL quit when the user presses the platform's quit shortcut: Command+Q on macOS, Ctrl+Q on Linux, and Alt+F4 on Windows. The shortcut SHALL work whichever tab or control has focus, except that on Linux a focused terminal tab SHALL send Ctrl+Q to its shell.

#### Scenario: Quit on macOS
- **WHEN** the user presses Command+Q on macOS
- **THEN** the application quits

#### Scenario: Quit on Linux
- **WHEN** the user presses Ctrl+Q on Linux while no terminal tab has focus
- **THEN** the application quits

#### Scenario: Quit on Windows
- **WHEN** the user presses Alt+F4 on Windows
- **THEN** the application quits

#### Scenario: Quit from a terminal tab on macOS
- **WHEN** a terminal tab has focus and the user presses Command+Q on macOS
- **THEN** the application quits

#### Scenario: Ctrl+Q in a terminal tab on Linux
- **WHEN** a terminal tab has focus and the user presses Ctrl+Q on Linux
- **THEN** the shell receives Ctrl+Q and the application keeps running

### Requirement: Closing the window quits
Closing the main window SHALL quit the application on every platform, including macOS.

#### Scenario: Close the window on macOS
- **WHEN** the user closes the main window on macOS
- **THEN** the application quits and does not keep running without a window

#### Scenario: Close the window on Linux or Windows
- **WHEN** the user closes the main window on Linux or Windows
- **THEN** the application quits

### Requirement: Quitting ends cluster work
Quitting SHALL end every cluster session along with its shells, log streams, and port-forwards, and MUST NOT ask for confirmation.

#### Scenario: Quit with active work
- **WHEN** the user quits while a shell and a port-forward are open
- **THEN** the application quits without a prompt, and the shell and port-forward stop
