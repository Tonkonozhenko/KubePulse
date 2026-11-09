# arch/website Specification

## Purpose

Defines the public project page: what it tells visitors about the application, what it is allowed to load, and how it is published from the repository.

## Requirements

### Requirement: Page explains the product
The project page SHALL state that the application is a lightweight, native Kubernetes IDE for Linux, macOS, and Windows, and SHALL list its main features: offline listing of kubeconfig contexts with explicit connect and disconnect, resource browsing at scale, pod logs, container shells, port forwarding, reviewed manifest editing, several clusters side by side, and the extension system.

#### Scenario: Visitor opens the page
- **WHEN** a visitor opens the project page
- **THEN** the first screen names the application, says it is a Kubernetes IDE, and names Linux, macOS, and Windows
- **AND** the page lists each main feature

### Requirement: Page states the license and privacy terms
The project page SHALL describe the application as open source under the Apache License, Version 2.0, and SHALL state that it needs no account or license key and sends no telemetry.

#### Scenario: Visitor checks the terms
- **WHEN** a visitor reads the project page
- **THEN** it names the Apache License, Version 2.0, and says there are no accounts, license keys, or telemetry

### Requirement: Page links to downloads and source
The project page SHALL link to the latest release for downloads and to the source repository, and SHALL say that the project is in early development.

#### Scenario: Visitor wants to install
- **WHEN** a visitor follows the download link
- **THEN** they reach the latest release of the repository, which lists the installers for each platform

#### Scenario: Visitor wants the source
- **WHEN** a visitor follows the source link
- **THEN** they reach the source repository

### Requirement: Page loads nothing from third parties
The project page MUST load every resource it uses from its own site, and MUST NOT include analytics, tracking, or resources such as fonts, scripts, or stylesheets from other hosts.

#### Scenario: Loading the page
- **WHEN** a browser loads the project page
- **THEN** every request it makes goes to the page's own host

### Requirement: Page adapts to screen and color preference
The project page SHALL be readable without horizontal scrolling at a viewport width of 360 pixels, and SHALL use a light or dark color scheme matching the visitor's system preference.

#### Scenario: Phone-width screen
- **WHEN** the page is shown in a viewport 360 pixels wide
- **THEN** all text and links are readable and the page does not scroll sideways

#### Scenario: Dark system preference
- **WHEN** the visitor's system prefers a dark color scheme
- **THEN** the page uses a dark background with light text

### Requirement: Page is published from the main branch
The project page SHALL be published to the repository's GitHub Pages site from the main branch whenever its source changes there, and a maintainer SHALL be able to start publishing by hand. Pushes that do not change the page source MUST NOT republish it.

#### Scenario: Changing the page on main
- **WHEN** a commit that changes the page source is pushed to the main branch
- **THEN** the published page is updated to that commit's content

#### Scenario: Changing only application code
- **WHEN** a commit that changes only application code is pushed to the main branch
- **THEN** the page is not republished

#### Scenario: Publishing by hand
- **WHEN** a maintainer starts the publishing workflow manually
- **THEN** the page is published from the main branch
