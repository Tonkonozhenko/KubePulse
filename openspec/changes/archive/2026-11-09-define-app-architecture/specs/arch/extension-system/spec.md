# Spec Delta

## Purpose

Defines the single contract through which all features extend the application, so that built-in features and future third-party plugins use the same extension points.

## ADDED Requirements

### Requirement: Built-in features use the extension contract
Built-in resource views, detail tabs, and actions SHALL be delivered as extensions through the same contract available to plugins, with no private access to core state.

#### Scenario: Built-in feature boundary
- **WHEN** a built-in feature needs cluster data
- **THEN** it obtains that data through the contract, and the build fails if it depends on core internals directly

### Requirement: Contribution points
The contract SHALL let an extension contribute resource views, table columns, detail tabs, actions, commands, navigation entries, and settings.

#### Scenario: Detail tab for a custom resource
- **WHEN** an extension declares a detail tab for a resource kind
- **THEN** the tab appears when the user opens an object of that kind

#### Scenario: Action in the command palette
- **WHEN** an extension declares a command
- **THEN** it appears in the command palette

#### Scenario: Removing an extension
- **WHEN** an extension is disabled
- **THEN** all of its contributions disappear without a restart

### Requirement: Declarative extension UI
Extensions SHALL describe their interface using host-provided elements, including tables, description lists, forms, charts, text, and log, terminal, and editor surfaces. The host SHALL render them natively.

#### Scenario: Consistent appearance
- **WHEN** an extension renders a table
- **THEN** it has the same appearance, theme, and keyboard behavior as built-in tables

#### Scenario: Unknown element
- **WHEN** an extension uses an element the host does not support
- **THEN** that region shows a placeholder error and the rest of the interface works normally

### Requirement: Extensions cannot affect table scale
Extension-contributed columns on resource tables MUST NOT reduce scrolling, sorting, or filtering below the performance required for 10k pods.

#### Scenario: Declarative column
- **WHEN** a column is declared as a path into the object
- **THEN** the host evaluates it without calling the extension

#### Scenario: Computed column
- **WHEN** a column needs extension logic to compute its value
- **THEN** only visible rows are computed, and an unfinished cell shows a placeholder without blocking scrolling

### Requirement: Declared permissions
Every extension SHALL declare the permissions it needs, per resource kind and per verb. The host MUST reject any operation the extension did not declare.

#### Scenario: Undeclared operation
- **WHEN** an extension attempts an operation outside its declared permissions
- **THEN** the host refuses the operation and reports it

#### Scenario: Cluster authorization still applies
- **WHEN** an extension has a permission but the user's account does not
- **THEN** the cluster's denial is returned to the extension

### Requirement: Host-mediated cluster access
Extensions MUST reach clusters only through the host, and MUST NOT receive credentials, tokens, or kubeconfig contents.

#### Scenario: Extension reads resources
- **WHEN** an extension requests resources
- **THEN** the host performs the request, and the extension receives only the results

### Requirement: Mutations from extensions use the safe flow
Any cluster change requested by an extension SHALL go through the same review and confirmation that apply to a user's own changes.

#### Scenario: Extension applies a change
- **WHEN** an extension requests a change to a resource
- **THEN** the user sees the target and the diff, and nothing changes until the user confirms

### Requirement: Lazy activation
An extension SHALL activate only when one of its declared activation triggers occurs, and inactive extensions SHALL add no measurable cost beyond their manifest.

#### Scenario: Activation by resource kind
- **WHEN** an extension declares a trigger on a custom resource kind
- **THEN** it stays inactive until the user opens that kind

### Requirement: Extension failure isolation
A failing extension MUST NOT crash the application or disrupt other extensions.

#### Scenario: Extension fails during a request
- **WHEN** an extension fails while handling a request
- **THEN** its contributions show an error, it is disabled after repeated failures, and the rest of the application keeps working

### Requirement: Versioned contract
The contract SHALL carry a version. An extension built for an incompatible contract version MUST be refused with a clear message.

#### Scenario: Incompatible extension
- **WHEN** an extension declares an unsupported contract version
- **THEN** it does not load, and the extension list says why

### Requirement: Contract is independent of the loading mechanism
All data crossing the contract SHALL be serializable and independent of the UI toolkit, so that a new extension loader needs no contract changes.

#### Scenario: Adding a sandboxed loader
- **WHEN** a sandboxed extension loader is added
- **THEN** existing contribution points work through it without changing the contract
