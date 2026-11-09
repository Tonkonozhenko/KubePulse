# general/manifest-editing Specification

## Purpose
Lets users change any resource by editing its YAML manifest, with server validation and a diff review before anything reaches the cluster.

## Requirements

### Requirement: Edit any resource manifest
The system SHALL let the user open any object they are allowed to read as YAML, including custom resources.

#### Scenario: Open a manifest
- **WHEN** the user chooses to edit an object
- **THEN** its current YAML opens in the editor

#### Scenario: Server-managed fields
- **WHEN** a manifest opens for editing
- **THEN** status and internal bookkeeping fields are left out of the editable text

#### Scenario: Read-only access
- **WHEN** the user's account can read an object but not update it
- **THEN** the manifest opens read-only and the system says why

### Requirement: YAML editing assistance
The editor SHALL provide YAML syntax highlighting, line numbers, search, undo and redo, and inline reporting of syntax errors.

#### Scenario: Syntax error
- **WHEN** the text is not valid YAML
- **THEN** the error is marked at its location and applying is disabled

### Requirement: Validation before apply
The system SHALL validate every change with a server-side dry run before offering to apply it, so that admission rules and schemas are checked by the cluster.

#### Scenario: Rejected change
- **WHEN** the dry run rejects the change
- **THEN** the server's message is shown and the cluster is unchanged

### Requirement: Diff review before apply
The system SHALL show a diff between the live object and the dry-run result, and MUST NOT apply a change until the user confirms it from that diff.

#### Scenario: Review a change
- **WHEN** the dry run succeeds
- **THEN** the system shows the diff and the target cluster, namespace, and object

#### Scenario: Server-side mutations are visible
- **WHEN** an admission rule changes the submitted object during the dry run
- **THEN** the diff includes that change

#### Scenario: No effective change
- **WHEN** the dry run produces no difference from the live object
- **THEN** the system reports that there is nothing to apply

### Requirement: Conflict detection
The system MUST NOT silently overwrite changes made to an object after the user opened it.

#### Scenario: Object changed during editing
- **WHEN** another actor modifies the object while the user is editing it
- **THEN** applying is refused, and the user can see the newer version and reapply their edits on top of it

### Requirement: Apply result is reported
The system SHALL report whether each apply succeeded and SHALL keep the user's edits if it failed.

#### Scenario: Apply fails
- **WHEN** the apply request fails
- **THEN** the error is shown and the editor keeps the user's text

### Requirement: Unsaved edits are protected
The system SHALL warn before closing an editor with unapplied changes.

#### Scenario: Close with unapplied changes
- **WHEN** the user closes an editor tab with unapplied changes
- **THEN** the system asks for confirmation before discarding them
