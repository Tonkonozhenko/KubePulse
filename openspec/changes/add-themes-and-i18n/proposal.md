# Proposal

## Why

KubePulse has no way to change how it looks or what language it speaks. The window uses the kit's default theme, ignores the system's light or dark preference, and about 150 English strings are written directly in the code. Those strings grow with every feature group, so moving them to catalogs now is far cheaper than after logs, shell, and editing land.

## What Changes

- Add a Settings tab, opened from a gear button at the bottom of the cluster rail or with the platform's settings shortcut. It is the first settings surface in the application.
- Add appearance modes: System, Light, and Dark. System follows the operating system while the application runs.
- Let the user choose one theme for light mode and one for dark mode, from the bundled themes and their own theme files.
- Bundle High Contrast Light, High Contrast Dark, Paper Light, and Slate Dark beside the kit's default pair.
- Load user theme files from a `themes` folder in the configuration directory, reload them live, and show the user any file that could not be loaded.
- Move every user-visible string in `kubepulse-ui` to translation catalogs, and add a language setting that defaults to the system language. Changing language takes effect after a restart, which the Settings tab offers.
- Ship English only. Adding a language later means adding catalog files, with no code change beyond registering the built-in catalog.
- Add a pseudo-locale for testing, and a CI check that catalogs are complete and that no user-visible literal stays in the UI code.
- Add translation catalogs to the extension manifest, so built-in and third-party contributions are localized the same way, and let extensions ask for the current language. This is contract version 1.1, which only adds to 1.0.
- Show cluster data, logs, and server or extension error details exactly as they arrive. Nothing from a cluster is ever translated.
- Add theme and text rules to the coding standards in `CLAUDE.md`, so the remaining feature groups follow them.

## Out of Scope

- Languages other than English, and right-to-left layouts.
- Plural rules and locale-aware number or date formats. Timestamps keep their current form, and text with counts is phrased so it needs no plural.
- Themes contributed by extensions.
- Showing the settings that extensions contribute. The Settings tab is where they will go, in a later change.

## Capabilities

### New Capabilities

- `shell/settings`: the Settings tab, how it opens, and how preferences are saved and restored.
- `shell/theme`: appearance modes, theme selection, bundled themes, user theme files, and what theming covers.
- `shell/language`: language selection, catalogs, fallback, and which text is never translated.

### Modified Capabilities

- `arch/extension-system`: adds localized contributions and a language query to the contract. Existing requirements stay as written. This capability is defined by the in-flight change `define-app-architecture` and is not yet in `openspec/specs/`.

## Impact

- **Code:** `kubepulse-ui` gains preferences, theme, language, and Settings tab modules, and every view file changes as its strings move to catalogs. `kubepulse-extension-api` gains the manifest field and host operation. `kubepulse-extension-host` resolves translations, and `kubepulse-builtin` gains catalogs.
- **Contract:** the minor version goes to 1.1. Extensions built for 1.0 keep working unchanged.
- **Dependencies:** `kubepulse-ui` adds `rust-i18n` and `sys-locale`, both already in the dependency tree through the kit, plus `notify`, which the workspace already uses. `kubepulse-builtin` adds `serde_json`. All are MIT or Apache-2.0 licensed.
- **Files:** `settings.json` and a `themes` folder join the configuration directory. Existing layouts and cluster settings are unaffected.
- **Tooling:** adds `scripts/check-i18n.py` and a CI step.
- **Dependencies between changes:** apply this after `add-three-column-layout`, because that change replaces the cluster panel and tab navigation, whose strings this change would otherwise extract twice, and it provides the rail that holds the gear button. Like that change, this one adds to a spec from `define-app-architecture`, which must be archived first.
