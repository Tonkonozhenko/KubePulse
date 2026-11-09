# shell/language Specification

## Purpose
Lets the application present its interface in the user's language, keeps cluster data exactly as the cluster reports it, and makes adding a language a matter of adding translations.

## Requirements

### Requirement: Language setting
The system SHALL offer a language setting with a System default choice and one choice for each shipped language, each shown in its own language. The first release ships English.

#### Scenario: Choices in the first release
- **WHEN** the user opens the language setting
- **THEN** it offers System default and English

#### Scenario: Testing locale stays hidden
- **WHEN** the user opens the language setting
- **THEN** the pseudo-locale is not offered

### Requirement: System language detection
With System default selected, the system SHALL use the first system language that matches a shipped language, first by exact tag and then by primary language. If none matches, it SHALL use English.

#### Scenario: Exact match
- **WHEN** the system language is `pt-BR` and `pt-BR` ships
- **THEN** the application uses `pt-BR`

#### Scenario: Regional variant
- **WHEN** the system language is `de-AT` and only `de` ships
- **THEN** the application uses `de`

#### Scenario: Language not shipped
- **WHEN** the system language is not shipped
- **THEN** the application uses English

### Requirement: Language change applies after restart
A language change SHALL take effect on the next launch. The Settings tab SHALL say so and offer to restart, SHALL warn that restarting disconnects every cluster and closes every tab, and MUST NOT restart without the user choosing to.

#### Scenario: Change language
- **WHEN** the user changes the language
- **THEN** the interface stays in its current language, and the Settings tab offers a restart with that warning

#### Scenario: Restart now
- **WHEN** the user accepts the restart
- **THEN** the application restarts in the new language with no open tabs, and connects no cluster

### Requirement: Unknown saved language
A saved language that is not shipped SHALL behave as System default, and the problem SHALL be logged.

#### Scenario: Language removed from a later version
- **WHEN** the saved language is not shipped by this version
- **THEN** the application uses the system language and logs the saved value

### Requirement: All interface text is translatable
All text the application itself shows SHALL come from translation catalogs, and CI MUST fail if a user-visible literal remains in the UI code. Text from the toolkit's components SHALL use the same language.

#### Scenario: Pseudo-locale
- **WHEN** the application runs in the pseudo-locale
- **THEN** every string from the application's catalogs appears in its pseudo-translated form

#### Scenario: Literal added to a view
- **WHEN** a change adds a user-visible literal to the UI code
- **THEN** CI fails and names the location

#### Scenario: Toolkit text
- **WHEN** the language is one the toolkit also translates
- **THEN** toolkit text such as menu and dialog buttons uses that language

### Requirement: Catalogs stay complete
CI MUST fail if a shipped catalog lacks a key, has a key English lacks, or has different placeholders. At run time, a text with no translation SHALL fall back to English.

#### Scenario: Missing translation
- **WHEN** a catalog lacks a key that English has
- **THEN** CI fails and names the catalog and key

#### Scenario: Placeholder mismatch
- **WHEN** a translation drops a placeholder from the English text
- **THEN** CI fails and names the catalog and key

### Requirement: Cluster data is never translated
The system MUST show data from clusters, including names, statuses, field values, events, logs, and manifests, exactly as received. Column titles a cluster declares for a resource kind SHALL also stay as declared.

#### Scenario: Pod status in a translated interface
- **WHEN** the interface language is not English and a pod's status is `Running`
- **THEN** the table shows `Running`

#### Scenario: Custom resource column
- **WHEN** a cluster declares a printer column for a custom resource
- **THEN** the column title stays as the cluster declared it

### Requirement: Error details stay verbatim
An error shown to the user SHALL have a translated summary, with the underlying detail from the cluster, a credential helper, the kubeconfig, or an extension kept as received.

#### Scenario: Connection failure in a translated interface
- **WHEN** a connection fails with a server message
- **THEN** the summary is in the interface language, and the server message follows unchanged
