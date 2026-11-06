# Spec Delta

## ADDED Requirements

### Requirement: Columns can declare a status
An extension SHALL be able to declare a column as a status, with paths the host tries in order. The host SHALL use the first path that has a value, without calling the extension, and SHALL choose the color itself. Extensions MUST NOT supply colors.

#### Scenario: First path has a value
- **WHEN** a status column declares two paths and the first has a value on an object
- **THEN** the cell shows that value

#### Scenario: Later path used
- **WHEN** the first path has no value on an object and the second has one
- **THEN** the cell shows the second path's value

#### Scenario: No path has a value
- **WHEN** no declared path has a value on an object
- **THEN** the cell is empty

#### Scenario: Invalid path
- **WHEN** any declared path in a status column is invalid
- **THEN** the column is skipped and the problem is logged, the same as for any other invalid column path

#### Scenario: Extension built for an earlier contract
- **WHEN** an extension built for contract version 1.2 loads
- **THEN** it loads, and its existing columns behave as before
