# Spec Delta

## MODIFIED Requirements

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

## ADDED Requirements

### Requirement: Scroll long log lines sideways
The system SHALL let the user scroll the log view sideways by wheel, trackpad, and a horizontal scrollbar, far enough to reach the end of the widest line currently retained, without wrapping lines.

#### Scenario: Line wider than the view
- **WHEN** a retained line is wider than the view
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
