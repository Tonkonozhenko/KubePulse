# Design

## Context

See `proposal.md` for motivation and `specs/` for required behavior.

Theming is close to done at the view level. Every view already reads `cx.theme()`, and the only literal colors are cluster identity colors, which the spec keeps fixed. What is missing is choosing, loading, and persisting themes. The opposite is true of text: strings are literals in every view, and nothing selects a language.

Four facts about the kit shape the design:

- `Theme` has only `Light` and `Dark` modes, with public `light_theme` and `dark_theme` fields. `Theme::change` reloads the config for the mode, including one just swapped in, and refreshes every window.
- `ThemeRegistry::reload` clears the registry back to the two defaults plus the watched folder, so themes added with `load_themes_from_str` disappear on the first file change. It also only logs invalid files, and `load_themes_from_str` cannot replace or remove a theme.
- The kit translates its own strings with `rust-i18n`, covering en, pt-BR, ru, zh-CN, zh-HK, and zh-TW, and `rust-i18n` keeps one process-wide locale that all crates share.
- `gpui_kit::component::setting` provides pages, groups, and dropdown fields for the Settings tab.

`run_with` already reads local settings synchronously before the application starts, so preferences fit there.

## Goals / Non-Goals

**Goals:**

- No theme flash: the first frame uses the final theme.
- Adding a language needs catalog files and one registration line, and no view changes.
- Keep crates below `kubepulse-ui` free of any translation dependency.

**Non-Goals:**

- Translating diagnostic text produced by crates below the UI. See D8.
- A theme editor, or per-cluster themes.
- Matching search by both translated and English text. The first translated language can decide this for the command palette and kind search, since English is the only language until then.

## Decisions

### D1. Preferences in one file

`preferences.rs` defines a serde struct with defaults for missing fields, and `ConfigPaths` gains `preferences` (`settings.json`) and `themes` (a folder).

```json
{ "appearance": "system", "light_theme": "Default Light", "dark_theme": "Default Dark", "language": null }
```

`run_with` reads it with `read_or_default`, beside the cluster identities. Saving uses the existing `write_atomic` path on a background executor. `null` language means System default.

*Alternative:* adding preferences to `workspace.json`. That file is the dock's own serialized state, and it is discarded when `LAYOUT_VERSION` changes.

### D2. Our own theme catalog instead of the kit's registry

A `ThemeCatalog` in `theme.rs` holds the bundled themes, the user themes, and per-file errors. It parses files as the kit's public `ThemeSet`, and applying a selection means swapping `light_theme` and `dark_theme` in `Theme::update`, then calling `Theme::change`. The kit's registry is left holding only its defaults, so its observer never acts.

The registry cannot meet the specs: bundled themes would vanish on the first reload, invalid files would stay invisible, and user theme names colliding with existing ones would be dropped silently.

Bundled themes are JSON files in `crates/kubepulse-ui/themes/`, embedded with `include_str!`. The kit's default pair comes from `ThemeRegistry::default_themes`.

*Alternatives:* restoring bundled themes from an observer after each kit reload still leaves errors invisible and reloads twice. Copying bundled themes into the user's folder makes them editable, and then they go stale across upgrades.

### D3. Reloading user themes

A `notify` watcher on the `themes` folder ignores access events, and a small thread waits for the folder to go quiet, then re-reads it and publishes the parsed result on a `watch` channel. The UI replaces the catalog's user themes and errors, then reapplies the selection. It uses `notify` directly because on Linux reading a file reports an open event, and `notify-debouncer-mini` drops the event kind, so a debounced reload would trigger itself forever. The first load happens synchronously in `run_with`, before the window opens, so the first frame has the final theme. The folder is created when the watcher starts.

### D4. System mode

`ThemeMode` has no System value, so `Appearance` in `preferences.rs` is our own enum with three values. In System mode, `Workspace` passes `window.observe_window_appearance` changes to `Theme::change`, using `window.appearance()` because the kit documents the application-level value as unreliable on Linux. Before the first window exists, the startup theme comes from `cx.window_appearance()`.

### D5. Language is fixed for the life of the process

The locale is chosen once at startup, and changing it needs a restart through `cx.restart()`.

Live switching would need every cached string to refresh: input placeholders set at construction, table columns built from `Arc<str>` titles, and resolved contributions, plus re-rendering open extension views and telling extensions to redo their text. With one shipped language, that machinery would have nothing to do, and language changes are rare. A fixed locale also lets the host resolve titles once (D7), and keeps the spec's language query simple.

*Alternative:* live switching later stays possible, and the only spec change would be to the restart requirement.

### D6. `rust-i18n` with JSON catalogs

`kubepulse-ui` calls `rust_i18n::i18n!("locales", fallback = "en")`, and views use `t!("cluster.connect")`. Catalogs are one JSON file per language in `crates/kubepulse-ui/locales/`, so a translator adds one file, and the check script reads them with the standard library. Keys are `area.name`. Placeholders use `%{name}`.

`rust-i18n` is already in the tree, and sharing it means one `set_locale` call covers both our strings and the kit's. A test asserts that `gpui_kit::component::locale()` sees the locale we set, so a kit upgrade that splits the crate into two versions fails loudly.

`language.rs` holds `resolve(saved, system_tags, available)`, a pure function for the spec's matching rules. `sys-locale` provides the system tags. The result is set before any view is built and passed to `HostOptions`.

Text with counts avoids plurals, because `rust-i18n` has no plural rules: `{} objects` becomes `Objects: %{count}`. This also fixes today's "1 objects".

*Alternative:* a catalog format with plural rules, such as ICU messages. It adds a second translation system beside the kit's, for a problem that careful phrasing avoids.

### D7. Manifest translations keyed by source text

`Manifest` gains one field:

```rust
#[serde(default)]
pub translations: BTreeMap<String, BTreeMap<String, String>>, // language -> declared text -> translation
```

Titles stay as they are, and the declared text is the key, so a manifest adds no per-field ids and an extension without translations needs no change. Two uses of one English word with different meanings cannot differ, which is acceptable for short titles.

The host resolves translations when it builds `ContributionSet`, replacing `title` on all seven contribution types with the same matching rules as D6. `category` stays as declared so grouping holds, and `ContributionSet` gains `category_titles`, mapping declared names to the first translation found in load order. `navigation()` groups by declared name, and `NavigationGroup` gains a display `title`. Saved state and collapsed groups continue to use the declared name.

`HostOperation::GetLanguage` returns `HostResponse::Language { tag }` and needs no permission. It is a message rather than a field on each request, so existing request types stay as they are and a sandboxed loader carries it unchanged.

*Alternative:* the host rewriting strings in `UiNode` trees. Those trees mix labels with cluster data, so a pod named like a translation key would be changed.

### D8. Translate summaries in the UI, keep details verbatim

Crates below the UI keep their English `Display` text. The UI translates the summary it chooses, such as "Connection failed", and appends the detail as received. Most of those details come from the server or a credential helper anyway, and translating the small remainder would spread catalogs into `kubepulse-cluster` and `kubepulse-store`.

### D9. Built-in catalogs

`crates/kubepulse-builtin/locales/` holds JSON maps from declared text to translation, embedded in a `CATALOGS` list of `(tag, include_str!(...))` that feeds the manifest. `en.json` maps each title to itself: it is the source list translators copy, and a test asserts that every catalog covers every title and category in the manifest. The test is what the "Built-in catalog is incomplete" scenario relies on.

### D10. `en-XA` pseudo-locale and `check-i18n.py`

`scripts/check-i18n.py` uses only the standard library and runs in the CI job beside the crate graph check. It fails when:

- a `t!` key is not in `en.json`, or a key in `en.json` is unused,
- a catalog has missing or extra keys, or different placeholders,
- a string literal containing letters is passed to `.label(`, `.placeholder(`, `.tooltip(`, `.title(`, `.child(`, or `.item(` outside test modules, or built with `format!` in those calls.

`--write` regenerates `en-XA.json` in both crates from `en.json`: accented letters, brackets, and 30% extra length, with placeholders untouched. Text outside the catalogs stands out when running in this locale, and overflow shows up early. `en-XA` is selectable only by editing `settings.json`, because the picker hides it.

The locale is process-wide, so UI tests that change it live in their own test binary.

### D11. Settings tab

`SettingsTab` is a dock panel registered with `dock::register_panel` under its own name, with no saved state. It does not register with `ShellState`, so it never changes the selected cluster and preview tab replacement never considers it. `Workspace::open_settings` activates an existing one by id, and otherwise adds it. An `OpenSettings` action bound to `cmd-,` on macOS and `ctrl-,` elsewhere and the gear button at the bottom of the rail both call it.

`Workspace` already owns background saves, so it also owns a new `SharedPreferences` entity. Its change events drive saving and theme application, in the same way `ClusterModelEvent` drives identity saves.

### D12. Terminal colors

The kit's theme has `base.*` colors for the named palette. The terminal from D13 of `define-app-architecture` maps ANSI colors to these, so a theme file has one set of colors for all surfaces. Syntax highlighting comes from the theme's `highlight` section, which the kit's editor already applies.

## Risks / Trade-offs

- **The catalog bypasses the kit's registry, so a kit upgrade could change how `Theme::change` treats swapped themes** → The behavior is documented on `Theme::change`, and the theme tests fail if it moves.
- **The kit replaces an unparsable color in a theme file with the default, silently** → Reported file errors cover only structure and names. Accepted, because a wrong color is visible immediately in a live-reloading file.
- **A user theme can make cluster identity colors hard to read, because those colors are fixed** → Identity also shows as a name, and the bundled themes are chosen to work with the palette. Accepted for user themes.
- **Restarting to change language drops connected clusters** → The tab says so, and the restart is the user's choice, per the spec.
- **The literal check is a pattern match, so a string can reach a view through a variable** → The pseudo-locale shows those in review. The check covers the common cases, not all of them.
- **Later feature groups add untranslated strings and colors** → `CLAUDE.md` states the rules, and CI fails on the common cases.
- **Using source text as keys means a title change silently orphans its translations** → The built-in coverage test catches this for built-ins. Other extensions fall back to the declared text, which is readable.
- **Longer languages or non-Latin scripts may need layout and font work** → The pseudo-locale covers length now. Font coverage is checked when the first such language is added.

## Migration Plan

No migration. `settings.json` is created on the first change, and `themes` when the watcher starts. Older files are unaffected. Contract 1.1 only adds, so 1.0 extensions load unchanged, and rolling back leaves two unused files.
