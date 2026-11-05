# Spec Delta

## ADDED Requirements

### Requirement: Localized contributions
A manifest SHALL be able to carry translations by language. The host SHALL use them for contribution titles and navigation categories in the interface language, and SHALL show the declared text for any text with no translation.

#### Scenario: Translated view title
- **WHEN** an extension declares a view title and a translation of it for the interface language
- **THEN** the navigation shows the translation

#### Scenario: No translation
- **WHEN** an extension has no translation of a title for the interface language
- **THEN** the title appears as declared, and the extension's other titles are unaffected

#### Scenario: Regional variant
- **WHEN** the interface language is `de-AT` and a manifest has translations for `de`
- **THEN** the host uses the `de` translations

#### Scenario: Manifest without translations
- **WHEN** an extension built for an earlier contract 1.x version has no translations
- **THEN** it loads and shows its declared text

### Requirement: Built-in features localize through the manifest
Built-in titles and categories SHALL be translated through manifest translations, with no private path into the host's catalogs.

#### Scenario: Built-in catalog is incomplete
- **WHEN** a built-in title has no entry in a shipped built-in catalog
- **THEN** the build's tests fail and name the title

### Requirement: Categories group by declared name
Views SHALL be grouped by their declared category, whatever language is showing, so views from different extensions stay together when only one translates the category.

#### Scenario: Two extensions share a category
- **WHEN** two extensions declare the category `Workloads` and only one translates it
- **THEN** the navigation shows one group with the translated name, holding both extensions' views

### Requirement: Extensions can read the interface language
The contract SHALL let an extension ask the host for the interface language with no permission declared. The language MUST NOT change during an extension's activation.

#### Scenario: Extension text in a view
- **WHEN** an extension asks for the language before rendering a view
- **THEN** the host returns the language tag, and the extension can write its text in that language

### Requirement: Extension interface text is not rewritten
The host MUST NOT change the text inside an extension's declarative interface, because that text can hold cluster data. Extensions SHALL localize that text themselves.

#### Scenario: Cell matching a translation
- **WHEN** a table in an extension's view has a cell whose text equals a key in the extension's translations
- **THEN** the cell shows its original text
