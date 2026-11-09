# Tasks

## 1. Preferences and Settings tab

- [x] 1.1 Add `preferences.rs` from D1 and extend `ConfigPaths` with `preferences` and `themes`, and verify with unit tests for defaults, a partial file, and a damaged file
- [x] 1.2 Add the `SharedPreferences` entity from D11, read it in `run_with`, and register it in `app.rs` and the test harnesses, and verify with a UI test that a change is saved once and restored by a new application
- [x] 1.3 Add `SettingsTab` as a dock panel with an empty page, `Workspace::open_settings`, the `OpenSettings` action on `cmd-,` or `ctrl-,`, and the rail's gear button, and verify with UI tests that the button and shortcut open it, a second open activates the same tab, and a layout restart brings it back
- [x] 1.4 Verify with UI tests that activating the Settings tab leaves the selected cluster unchanged, and that selecting a kind does not replace it when it sits in the preview tab's pane

## 2. Themes

- [x] 2.1 Add `ThemeCatalog` from D2 with bundled themes, synchronous folder loading, and per-file errors, and verify with unit tests for a valid file, an invalid file, a name taken by a bundled theme, and a name taken by another file
- [x] 2.2 Write the High Contrast Light, High Contrast Dark, Paper Light, and Slate Dark theme files, and verify with a unit test that measures foreground against background contrast for all six bundled themes against the spec's limits
- [x] 2.3 Apply the selection through `Theme::update` and `Theme::change`, falling back to the mode's default for a missing theme while keeping the saved name, and verify with UI tests that a swap changes `cx.theme()`, the inactive mode's choice does not, and restoring the file brings the theme back
- [x] 2.4 Apply preferences before `open_window`, and verify with a UI test that the first frame uses the saved theme with user themes already loaded
- [x] 2.5 Add System mode from D4 with `observe_window_appearance`, and verify with UI tests that an appearance change is followed in System mode and ignored in Dark mode
- [x] 2.6 Add the folder watcher from D3, and verify with a test that adding, changing, and removing a file updates the catalog, and that a burst of writes reloads once
- [x] 2.7 Add the Appearance group to the Settings tab, with the mode, the two theme pickers filtered by mode, the open-folder action, and the list of file errors and missing themes, and verify with UI tests that each control changes its preference, the pickers list only their mode, and an error appears and clears
- [x] 2.8 Verify with a UI test that an open declarative extension view and the table change with the theme, and that a cluster's identity color does not

## 3. Language infrastructure

- [x] 3.1 Add `rust-i18n` and `sys-locale`, the `i18n!` call, and `locales/en.json` with one key used by one view, and verify the crate builds and that `cargo deny check licenses` and `scripts/check-crate-graph.py` pass
- [x] 3.2 Add `language.rs` with `resolve` from D6, and verify with unit tests for an exact tag, a regional variant, an unshipped language, several system languages in order, an unknown saved language, and no system language
- [x] 3.3 Set the locale in `run_with` before any view is built, and verify in a separate test binary that a string resolves in `en-XA`, and that `gpui_kit::component::locale()` reports the same locale
- [x] 3.4 Add `scripts/check-i18n.py` from D10 with `--write`, add it to CI, and verify with its checks failing on a missing key, an unused key, a placeholder mismatch, and a literal `.label("Text")`, in checked-in fixtures under `scripts/`
- [x] 3.5 Add the Language group to the Settings tab, listing System default and each shipped language in its own name with `en-XA` hidden, plus the restart notice and button, and verify with UI tests that the choice is saved, the interface language does not change, and no restart happens without the button

## 4. Move strings to catalogs

- [x] 4.1 Move the strings in `app.rs`, `workspace.rs`, `cluster_tab.rs`, and the rail, sidebar, and dialog modules from `add-three-column-layout`, and verify that `tests/shell.rs` passes
- [x] 4.2 Move the strings in `resource_browser.rs`, `detail.rs`, `declarative.rs`, and `columns.rs`, including the count phrasing from D6 and the "Other resources" and "Created" labels, leaving declared printer columns verbatim, and verify that `tests/browsing.rs` passes and a unit test asserts that a printer column keeps its title
- [x] 4.3 Apply the summary and detail rule from D8 to every shown error, and verify with a UI test in `en-XA` that a failed connection shows a pseudo-translated summary and an unchanged server message
- [x] 4.4 Enable the literal check for every UI source file and generate `en-XA.json`, and verify that `scripts/check-i18n.py` passes and a UI test in `en-XA` finds no untranslated text in the rail, sidebar, table, detail view, and Settings tab, and that a pod status stays verbatim
- [ ] 4.5 Run the application in `en-XA` on a real window, and verify that the rail, sidebar, tab titles, dialogs, and Settings tab do not clip or overflow

## 5. Extension contract

- [x] 5.1 Add `Manifest::translations`, `HostOperation::GetLanguage`, and `HostResponse::Language`, and set `CONTRACT_VERSION` to 1.1, and verify with tests that a 1.0 manifest without the field parses and loads, and that a 1.1 extension is refused by a 1.0 host with the existing message
- [x] 5.2 Add `locale` to `HostOptions` and resolve titles when building `ContributionSet`, using the matching rules in `resolve`, and verify with host tests for an exact tag, a regional variant, a partial translation, and rebuilding after an extension is disabled
- [x] 5.3 Add `category_titles` and group by declared category in `navigation()`, with a display `title` on `NavigationGroup`, and verify with a test for two extensions sharing a category when only one translates it
- [x] 5.4 Answer `GetLanguage` with no permission, and verify with a host test that an extension with no permissions receives the tag, and that a table cell equal to a translation key renders unchanged
- [x] 5.5 Add the `CATALOGS` list, `en.json`, and generated `en-XA.json` to `kubepulse-builtin` from D9, and verify with a test that fails when a title or category is missing from a catalog, and with a UI test that the sidebar shows the `en-XA` titles

## 6. Standards and integration

- [x] 6.1 Add the theme and text rules to the coding standards in `CLAUDE.md`, along with `scripts/check-i18n.py` under Commands, and verify the command runs as written
- [x] 6.2 Add theme colors and catalog strings to tasks 9.2 and 11.2 of `define-app-architecture`, so the editor and terminal verify the `shell/theme` requirements when they are built, and verify that `openspec validate define-app-architecture --strict` still passes
- [x] 6.3 Verify startup still meets the 1-second limit, with 50 user theme files in the folder
- [ ] 6.4 Run the application on Linux under Wayland and X11, and on macOS and Windows, and verify on each that System mode follows the operating system, the settings shortcut works, live theme reload works, and the restart button brings the application back

## Workflow follow-up

- Apply after `add-three-column-layout`.
- Archive after `define-app-architecture`, because the `arch/extension-system` delta adds to a spec that change creates.
