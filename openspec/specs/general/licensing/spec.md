# general/licensing Specification

## Purpose
Defines the license terms users receive and the rules that keep every distribution compliant with them, so that commercial use is paid for and the source stays public.

## Requirements

### Requirement: Business Source License terms
Every release SHALL be licensed under Business Source License 1.1, with non-commercial use granted free of charge and commercial use requiring a paid license.

#### Scenario: Repository license
- **WHEN** someone inspects the repository
- **THEN** the root license file states the Business Source License 1.1 terms and all of its parameters

#### Scenario: Distributed application
- **WHEN** a user installs a release
- **THEN** the license text is included with the application and available from its About screen

### Requirement: Scheduled conversion
Each released version SHALL convert to AGPL-3.0 on the fourth anniversary of its first public release.

#### Scenario: Version reaches its change date
- **WHEN** four years have passed since a version's first public release
- **THEN** that version is available under AGPL-3.0, while newer versions keep their own dates

### Requirement: Accurate license description
Project materials SHALL describe unconverted releases as source-available, and MUST NOT describe them as open source.

#### Scenario: Release materials
- **WHEN** the README, website, or release notes describe the license
- **THEN** they say source-available, name the conversion to AGPL-3.0, and explain how to buy a commercial license

### Requirement: Compatible dependencies only
Distributed builds MUST contain only dependencies whose licenses permit distribution under the project's terms, and GPL-family dependencies MUST NOT be included.

#### Scenario: Incompatible dependency added
- **WHEN** a change introduces a dependency with a disallowed license
- **THEN** the automated license check fails

#### Scenario: Third-party notices
- **WHEN** a release is built
- **THEN** it includes the license notices its dependencies require

### Requirement: Contributions can be commercially licensed
The project SHALL accept code contributions only under an agreement that allows them to be distributed under the commercial license and the Business Source License.

#### Scenario: First contribution
- **WHEN** a new contributor opens a pull request
- **THEN** the pull request cannot be merged until the contributor has accepted the agreement

### Requirement: Permissive extension contract
The extension contract SHALL be licensed permissively, so that authors can build and license their own extensions independently of the application's license.

#### Scenario: Third-party extension
- **WHEN** an author builds an extension using only the extension contract
- **THEN** the author can choose that extension's license, including a proprietary one

### Requirement: No license enforcement inside the application
The application MUST run without license keys, activation, or usage reporting. Compliance relies on the license terms.

#### Scenario: Offline use
- **WHEN** the application runs on a machine with no internet access
- **THEN** every feature works, given network access to the target clusters

#### Scenario: Usage data
- **WHEN** the application runs
- **THEN** it sends no license or usage information to the project
