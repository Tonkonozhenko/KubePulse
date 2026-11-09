# arch/release Specification

## Purpose
Defines how a tagged version of the application becomes a published release, what each release contains, and how users verify what they download.

## Requirements

### Requirement: Releases start from a version tag
The system SHALL publish a release only when a tag of the form `v<version>` is pushed to the repository, and pushes to branches MUST NOT publish one.

#### Scenario: Pushing a version tag
- **WHEN** a maintainer pushes the tag `v0.1.0`
- **THEN** a release named `v0.1.0` is built and published

#### Scenario: Pushing to the main branch
- **WHEN** a commit is pushed to the main branch without a version tag
- **THEN** no release is created or changed

### Requirement: Tag matches the workspace version
The release process SHALL refuse to publish when the version in the tag differs from the workspace version of the tagged commit, and SHALL report both versions.

#### Scenario: Mismatched tag
- **WHEN** the tag `v0.2.0` is pushed on a commit whose workspace version is 0.1.0
- **THEN** no release is published and the failure names both 0.2.0 and 0.1.0

### Requirement: Release contains an installer for each platform
Each release SHALL attach the Linux, macOS, and Windows installers defined by packaging, built from the tagged commit, and MUST NOT be published while any of them is missing.

#### Scenario: Complete release
- **WHEN** a release for version 0.1.0 is published
- **THEN** it has a `.deb` package, a `.dmg` disk image, and a Windows installer executable, each for version 0.1.0

#### Scenario: One platform fails to build
- **WHEN** the Windows installer fails to build for a tag
- **THEN** no release is published for that tag

### Requirement: Downloads can be verified
Each release SHALL attach a checksum file listing the SHA-256 digest of every other file attached to it.

#### Scenario: Verifying a download
- **WHEN** a user downloads an installer and the checksum file from a release
- **THEN** the installer's SHA-256 digest matches its line in the checksum file

### Requirement: Release notes list the changes
Each release SHALL include notes listing the changes since the previous release.

#### Scenario: Second release
- **WHEN** version 0.2.0 is released after version 0.1.0
- **THEN** its notes list the changes merged between `v0.1.0` and `v0.2.0`

### Requirement: Pre-release versions are marked
A release whose version has a pre-release suffix SHALL be marked as a pre-release, and a release without one SHALL be marked as the latest release.

#### Scenario: Beta version
- **WHEN** the tag `v0.2.0-beta.1` is released
- **THEN** the release is marked as a pre-release and the latest release is unchanged

#### Scenario: Stable version
- **WHEN** the tag `v0.2.0` is released
- **THEN** the release becomes the latest release

### Requirement: Published releases are not replaced
The release process MUST NOT overwrite or delete an existing release or its files.

#### Scenario: Pushing a tag that was already released
- **WHEN** the release process runs for a tag that already has a published release
- **THEN** it fails and the existing release and its files are unchanged
