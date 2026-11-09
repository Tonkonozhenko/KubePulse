# Spec Delta

## ADDED Requirements

### Requirement: Views can declare an icon
A resource view contribution SHALL be able to name an icon from a host-defined set. Built-in views SHALL declare their icons through this same field. Extensions MUST NOT supply image data or toolkit types.

#### Scenario: Declared icon
- **WHEN** an extension declares a view with an icon from the set
- **THEN** the host shows that icon for the view's kind

#### Scenario: Manifest without an icon
- **WHEN** an extension built for an earlier contract 1.x version declares no icon
- **THEN** it loads and its view shows the generic icon

#### Scenario: Icon from a newer contract
- **WHEN** a manifest names an icon this host does not know
- **THEN** the manifest loads and the view shows the generic icon
