# Spec Delta

## ADDED Requirements

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

## MODIFIED Requirements

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
