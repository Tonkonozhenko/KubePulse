# Spec Delta

## Purpose

Defines how the application is built into installers for each supported platform, and what an installed copy provides on the user's system.

## ADDED Requirements

### Requirement: Installer for each platform
The system SHALL provide an installer for each supported platform, so the application can be installed without a Rust toolchain.

#### Scenario: Linux package
- **WHEN** packaging runs on Linux
- **THEN** it produces a `.deb` package that installs with the system package manager

#### Scenario: macOS package
- **WHEN** packaging runs on macOS
- **THEN** it produces a `.app` bundle and a `.dmg` disk image

#### Scenario: Windows package
- **WHEN** packaging runs on Windows
- **THEN** it produces an installer executable

### Requirement: Packages use an optimized build
Every package SHALL contain a release build made from the locked dependency versions, at the version declared by the workspace.

#### Scenario: Package version
- **WHEN** a package is built from a workspace at version 0.1.0
- **THEN** the installed application reports version 0.1.0 to the platform's package manager

### Requirement: Desktop integration on Linux
An installed Linux package SHALL add the application to the desktop launcher with its logo, and the running window SHALL be matched to that launcher entry.

#### Scenario: Launching from the launcher
- **WHEN** the package is installed and the user opens the launcher
- **THEN** KubePulse is listed with its logo and starts when selected

#### Scenario: Window matches the launcher entry
- **WHEN** the application is running after being started from the launcher
- **THEN** the desktop shows its window under the KubePulse entry and logo, not as a separate unknown application

#### Scenario: Starting from a terminal
- **WHEN** the package is installed and the user runs `kubepulse` in a terminal
- **THEN** the application starts

### Requirement: Runtime libraries are declared on Linux
A Linux package SHALL declare every system library the application needs, including libraries opened only at runtime, on both Wayland and X11.

#### Scenario: Missing libraries
- **WHEN** the package is installed on a system without one of those libraries
- **THEN** the package manager installs the library with the package

### Requirement: License ships with the package
Every package SHALL include the product license, and package metadata MUST NOT describe the product as open source.

#### Scenario: Reading the license
- **WHEN** the application is installed
- **THEN** the Business Source License text is available on the system or shown by the installer

### Requirement: Removal keeps user data
Removing the application SHALL remove everything the package installed, and MUST NOT remove the user's settings, themes, or kubeconfig files.

#### Scenario: Removing on Linux
- **WHEN** the user removes the package with the system package manager
- **THEN** the binary, launcher entry, and icon are gone, and files in the user's home directory are unchanged

### Requirement: Packages contact nothing
Installing, upgrading, or removing a package MUST NOT contact any network service run for the product, and the package MUST NOT add license activation or telemetry.

#### Scenario: Offline install
- **WHEN** the package and its dependencies are already available and the system is offline
- **THEN** installation completes and the application starts
