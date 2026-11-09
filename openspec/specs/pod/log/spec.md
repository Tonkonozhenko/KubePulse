# pod/log Specification

## Purpose
Lets users read, follow, and search container logs without leaving the application or letting log volume degrade it.

## Requirements

### Requirement: View container logs
The system SHALL show logs for any container of a pod, including init containers and the previous instance of a container, and SHALL offer the previous instance for every container without checking whether it restarted.

#### Scenario: Single-container pod
- **WHEN** the user opens logs for a pod with one container
- **THEN** that container's logs appear

#### Scenario: Multi-container pod
- **WHEN** the user opens logs for a pod with several containers
- **THEN** the user can choose which container to view

#### Scenario: Restarted container
- **WHEN** a container has restarted
- **THEN** the user can view the logs of its previous instance

#### Scenario: Container with no previous instance
- **WHEN** the user asks for the previous instance of a container that has none
- **THEN** the view shows the cluster's error, and the previous-instance control stays available so the user can turn it off

### Requirement: Follow logs live
The system SHALL stream new log lines as they are produced, SHALL keep the newest line in view while following, SHALL start every new stream at the end and following, and SHALL offer a follow control that shows and changes whether the view is following. Pausing MUST NOT stop the stream.

#### Scenario: Opening logs
- **WHEN** the user opens logs for a container that already has lines
- **THEN** the view shows the newest line and the follow control is on

#### Scenario: New lines while following
- **WHEN** new lines arrive while the follow control is on
- **THEN** the view moves so the newest line stays in view

#### Scenario: View changes size while following
- **WHEN** the view gets shorter while the follow control is on and no new lines arrive
- **THEN** the newest line stays in view

#### Scenario: Pausing with the follow control
- **WHEN** the user turns the follow control off while logs are streaming
- **THEN** the view stays in place and the stream continues

#### Scenario: Resuming with the follow control
- **WHEN** the user turns the follow control on after pausing
- **THEN** the view moves to the newest line and follows again

#### Scenario: Reading earlier lines
- **WHEN** the user scrolls up while logs are streaming
- **THEN** the view stays in place, the stream continues, the follow control turns off, and the user can return to the newest line by turning it on

#### Scenario: Moving to a search match
- **WHEN** the user moves to a search match while following
- **THEN** the view shows the match and the follow control turns off

#### Scenario: Switching streams while paused
- **WHEN** the user chooses another container or instance while the follow control is off
- **THEN** the new stream starts at its newest line with the follow control on

#### Scenario: Stream interrupted
- **WHEN** a log stream ends because the connection dropped
- **THEN** the system shows the interruption and resumes without duplicating or silently losing the lines it can recover

### Requirement: Search logs
The system SHALL let the user search the retained log lines and move between matches.

#### Scenario: Search retained lines
- **WHEN** the user searches for text
- **THEN** matches are highlighted and the user can step through them

### Requirement: Log timestamps
The system SHALL let the user show or hide line timestamps.

#### Scenario: Show timestamps
- **WHEN** the user turns on timestamps
- **THEN** each line shows the time reported by the cluster

### Requirement: Bounded log retention
The system SHALL keep a bounded number of log lines in memory, and high log volume MUST NOT degrade interface responsiveness.

#### Scenario: High-volume stream
- **WHEN** a container writes 10,000 lines per second
- **THEN** the interface stays responsive, memory stays within the retention limit, and the oldest lines are dropped with a visible notice

### Requirement: Scroll long log lines sideways
The system SHALL let the user scroll the log view sideways by wheel, trackpad, and a horizontal scrollbar, far enough to reach the end of the widest line currently retained, while the wrap control is off.

#### Scenario: Line wider than the view
- **WHEN** a retained line is wider than the view and the wrap control is off
- **THEN** the user can scroll sideways to the end of that line

#### Scenario: Widest line is not the first line
- **WHEN** the widest retained line is far from the first line and out of view
- **THEN** the scrollable width still covers it

#### Scenario: Widest line is dropped
- **WHEN** the widest retained line is dropped by retention
- **THEN** the scrollable width shrinks to the widest line still retained

#### Scenario: Scrolling sideways while following
- **WHEN** the user scrolls sideways while the follow control is on
- **THEN** the view keeps following and keeps its sideways position as new lines arrive

#### Scenario: All lines fit
- **WHEN** every retained line fits in the view
- **THEN** the view does not scroll sideways

### Requirement: Log vertical scrollbar
The system SHALL show a vertical scrollbar on the log view whenever retained lines do not all fit, and moving it up SHALL pause following as scrolling up does.

#### Scenario: Moving the scrollbar up while following
- **WHEN** the user moves the vertical scrollbar up while the follow control is on
- **THEN** the view moves to the earlier lines, the stream continues, and the follow control turns off

### Requirement: Select log text
The system SHALL let the user select log text with the mouse. Dragging selects characters, a double click selects a word, a triple click selects a line, and a shift click extends the selection. A selection SHALL be able to span lines that are out of view, and SHALL be highlighted with the theme's selection color.

#### Scenario: Dragging across lines
- **WHEN** the user drags from the middle of one log line to the middle of a later line
- **THEN** the text between those two points is highlighted

#### Scenario: Selecting a word or a line
- **WHEN** the user double-clicks a word in a log line, or triple-clicks the line
- **THEN** that word, or that whole line, is selected

#### Scenario: Selecting past the edge of the view
- **WHEN** the user drags past the top or bottom edge of the view
- **THEN** the view scrolls and the selection grows with it

#### Scenario: Dragging up while following
- **WHEN** the user drags past the top edge of the view while the follow control is on
- **THEN** the follow control turns off, as when scrolling up

#### Scenario: New lines while text is selected
- **WHEN** new lines arrive while text is selected
- **THEN** the same text stays selected

#### Scenario: Selected lines are dropped
- **WHEN** retention drops lines at the start of the selection
- **THEN** the selection keeps only the lines still retained

#### Scenario: Selection cleared on a new stream
- **WHEN** the user chooses another container or instance, or turns timestamps on or off
- **THEN** the selection is cleared

### Requirement: Copy log text
The system SHALL copy the selected log text as plain text when the user presses the platform copy shortcut in the log view, or chooses Copy from the menu that right-clicking the log lines opens. The Copy item SHALL show the copy shortcut and SHALL be disabled while no log text is selected. Each log line SHALL be one line of the copied text, timestamps SHALL be included when shown, and a line cut for display SHALL be copied whole when the selection reaches its cut end.

#### Scenario: Copying several lines
- **WHEN** the user selects text spanning three log lines and presses the copy shortcut
- **THEN** the clipboard holds the selected text with a line break between each log line

#### Scenario: Copying from the right-click menu
- **WHEN** the user selects text spanning three log lines, right-clicks the selection, and chooses Copy
- **THEN** the selection stays highlighted while the menu is open, and the clipboard then holds the same text the copy shortcut would copy

#### Scenario: Right-clicking with nothing selected
- **WHEN** no log text is selected and the user right-clicks the log lines
- **THEN** the menu opens with Copy disabled, and choosing it leaves the clipboard unchanged

#### Scenario: Right-clicking while the search field has focus
- **WHEN** the search field has focus, log text is selected, and the user chooses Copy from the log lines' menu
- **THEN** the selected log text is copied, not the search field's text

#### Scenario: Copying with timestamps shown
- **WHEN** timestamps are shown and the user copies whole lines
- **THEN** each copied line starts with its timestamp as shown

#### Scenario: Copying a line cut for display
- **WHEN** the user selects through the end of a line that was cut for display and copies
- **THEN** the clipboard holds the whole line as received, without the cut marker

#### Scenario: Copying wrapped lines
- **WHEN** lines are wrapped and the user copies a selection spanning wrapped lines
- **THEN** the copied text has line breaks only between log lines, not where lines wrapped

#### Scenario: Copying from the search field
- **WHEN** the search field has focus and the user presses the copy shortcut
- **THEN** the search field's own selection is copied and the log selection is left alone

### Requirement: Wrap long log lines
The system SHALL offer a wrap control in the log toolbar that is off when a tab opens. While it is on, lines wider than the view SHALL wrap at the view width and the view SHALL NOT scroll sideways. The setting SHALL stay as set when the user chooses another container or instance in the same tab.

#### Scenario: Turning wrapping on
- **WHEN** the user turns the wrap control on while a line is wider than the view
- **THEN** that line continues on the following rows, and the horizontal scrollbar is gone

#### Scenario: View changes width while wrapping
- **WHEN** the view gets narrower or wider while the wrap control is on
- **THEN** lines wrap again at the new width

#### Scenario: Turning wrapping off
- **WHEN** the user turns the wrap control off
- **THEN** each line takes one row again and the view can scroll sideways

#### Scenario: Toggling keeps the place
- **WHEN** the user turns the wrap control on or off while paused
- **THEN** the line at the top of the view stays at the top

#### Scenario: Toggling while following
- **WHEN** the user turns the wrap control on or off while the follow control is on
- **THEN** the newest line stays in view and following continues

#### Scenario: Switching streams keeps wrapping
- **WHEN** the user chooses another container or instance while the wrap control is on
- **THEN** the new stream's lines are wrapped

### Requirement: Wrapped lines follow, scroll, and search as unwrapped ones
While the wrap control is on, following, pausing, keeping place as old lines are dropped, moving to search matches, and the vertical scrollbar SHALL behave as they do while it is off, and high log volume MUST NOT degrade interface responsiveness.

#### Scenario: Following wrapped lines
- **WHEN** new wrapped lines arrive while the follow control is on
- **THEN** the last row of the newest line stays in view

#### Scenario: Reading earlier wrapped lines
- **WHEN** the user scrolls up or moves the vertical scrollbar up while wrapping
- **THEN** the view stays in place, the stream continues, and the follow control turns off

#### Scenario: Old lines dropped while paused and wrapping
- **WHEN** retention drops the oldest lines while paused and wrapping
- **THEN** the lines in view stay in view

#### Scenario: Moving to a match in a wrapped line
- **WHEN** the user moves to a search match while wrapping
- **THEN** the view shows the matching line and the follow control turns off

#### Scenario: High-volume stream while wrapping
- **WHEN** a container writes 10,000 lines per second while the wrap control is on
- **THEN** the interface stays responsive and memory stays within the retention limit
