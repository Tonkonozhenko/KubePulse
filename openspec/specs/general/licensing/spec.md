# general/licensing Specification

## Purpose
Defines the license terms users receive and the rules that keep every distribution compliant with them.

## Requirements

### Requirement: Apache License terms
Every release SHALL be licensed under the Apache License, Version 2.0.

#### Scenario: Repository license
- **WHEN** someone inspects the repository
- **THEN** the root license file states the Apache License, Version 2.0

#### Scenario: Distributed application
- **WHEN** a user installs a release
- **THEN** the license text is included with the application and available from its About screen

### Requirement: Accurate license description
Project materials SHALL describe the product as open source under the Apache License, Version 2.0.

#### Scenario: Release materials
- **WHEN** the README, website, or release notes describe the license
- **THEN** they name the Apache License, Version 2.0

### Requirement: Compatible dependencies only
Distributed builds MUST contain only dependencies whose licenses permit distribution under the project's terms, and GPL-family dependencies MUST NOT be included.

#### Scenario: Incompatible dependency added
- **WHEN** a change introduces a dependency with a disallowed license
- **THEN** the automated license check fails

#### Scenario: Third-party notices
- **WHEN** a release is built
- **THEN** it includes the license notices its dependencies require

### Requirement: Contributions under the project license
The project SHALL accept code contributions under the Apache License, Version 2.0, as its section 5 describes, without a separate contributor agreement.

#### Scenario: First contribution
- **WHEN** a new contributor opens a pull request
- **THEN** the pull request can be merged without the contributor signing an additional agreement

### Requirement: Permissive extension contract
The extension contract SHALL be licensed permissively, so that authors can build and license their own extensions independently of the application's license.

#### Scenario: Third-party extension
- **WHEN** an author builds an extension using only the extension contract
- **THEN** the author can choose that extension's license, including a proprietary one

### Requirement: No license enforcement inside the application
The application MUST run without license keys, activation, or usage reporting.

#### Scenario: Offline use
- **WHEN** the application runs on a machine with no internet access
- **THEN** every feature works, given network access to the target clusters

#### Scenario: Usage data
- **WHEN** the application runs
- **THEN** it sends no license or usage information to the project
