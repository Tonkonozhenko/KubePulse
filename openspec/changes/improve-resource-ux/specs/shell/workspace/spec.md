# Spec Delta

## ADDED Requirements

### Requirement: Tabs cannot be zoomed
No tab SHALL be zoomable, whatever kind of tab it is and however many panes are open. Where the UI kit still draws a zoom entry in a tab's menu, that entry MUST be disabled.

#### Scenario: Single pane
- **WHEN** one pane is open and the user opens the tab's menu
- **THEN** the zoom entry is disabled, and choosing it changes nothing

#### Scenario: Split panes
- **WHEN** several panes are open and the user opens a tab's menu
- **THEN** the zoom entry is disabled, and every pane stays visible
