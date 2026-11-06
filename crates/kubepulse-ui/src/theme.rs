//! Bundled and user themes, and applying the user's choice. This replaces the
//! kit's theme registry, which forgets embedded themes on reload and only logs
//! files it cannot load.

use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

use gpui_kit::{
    component::{Theme, ThemeConfig, ThemeMode, ThemeRegistry, ThemeSet},
    *,
};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};
use tokio::sync::watch;

use crate::preferences::{Appearance, DEFAULT_DARK_THEME, DEFAULT_LIGHT_THEME, Preferences};

const BUNDLED: [&str; 3] = [
    include_str!("../themes/high-contrast.json"),
    include_str!("../themes/paper.json"),
    include_str!("../themes/slate.json"),
];
const RELOAD_DEBOUNCE: Duration = Duration::from_millis(200);

fn bundled_files() -> Vec<ThemeConfig> {
    BUNDLED
        .iter()
        .flat_map(|text| {
            let set: ThemeSet = serde_json::from_str(text).expect("bundled themes are valid");
            set.themes
        })
        .collect()
}

fn reserved_names() -> HashSet<SharedString> {
    bundled_files()
        .into_iter()
        .map(|theme| theme.name)
        .chain([DEFAULT_LIGHT_THEME.into(), DEFAULT_DARK_THEME.into()])
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ThemeFileProblem {
    Unreadable(String),
    Invalid(String),
    NameTaken(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeFileError {
    pub file: String,
    pub problem: ThemeFileProblem,
}

#[derive(Debug, Default)]
pub struct LoadedThemes {
    pub version: u64,
    pub themes: Vec<ThemeConfig>,
    pub errors: Vec<ThemeFileError>,
}

/// Files load in name order, so the same file wins a name on every run.
pub fn load_folder(folder: &Path) -> LoadedThemes {
    let mut loaded = LoadedThemes::default();
    let mut taken = reserved_names();
    let entries = match fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return loaded,
        Err(error) => {
            tracing::warn!(path = %folder.display(), %error, "could not read the themes folder");
            return loaded;
        }
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|e| e == "json"))
        .collect();
    paths.sort();
    for path in paths {
        let file = path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        let set = match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str::<ThemeSet>(&text)
                .map_err(|error| ThemeFileProblem::Invalid(error.to_string())),
            Err(error) => Err(ThemeFileProblem::Unreadable(error.to_string())),
        };
        match set {
            Ok(set) => {
                for theme in set.themes {
                    if taken.insert(theme.name.clone()) {
                        loaded.themes.push(theme);
                    } else {
                        loaded.errors.push(ThemeFileError {
                            file: file.clone(),
                            problem: ThemeFileProblem::NameTaken(theme.name.to_string()),
                        });
                    }
                }
            }
            Err(problem) => loaded.errors.push(ThemeFileError { file, problem }),
        }
    }
    loaded
}

/// Loads the themes folder now, and again whenever a file in it changes.
pub struct ThemeWatcher {
    folder: PathBuf,
    themes: watch::Receiver<Arc<LoadedThemes>>,
    _watcher: Option<RecommendedWatcher>,
}

impl ThemeWatcher {
    /// A failure here stops live reload only, so it is logged and themes
    /// already in the folder still load.
    pub fn start(folder: PathBuf) -> Self {
        if let Err(error) = fs::create_dir_all(&folder) {
            tracing::warn!(path = %folder.display(), %error, "could not create the themes folder");
        }
        let (sender, themes) = watch::channel(Arc::new(load_folder(&folder)));
        let (changed, changes) = mpsc::channel();
        let watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
            match event {
                // Reading the themes reports access events, which must not
                // cause another reload.
                Ok(event) if !matches!(event.kind, EventKind::Access(_)) => {
                    // The receiver is gone only while the application exits.
                    let _ = changed.send(());
                }
                Ok(_) => {}
                Err(error) => tracing::warn!(%error, "themes folder watcher error"),
            }
        })
        .and_then(|mut watcher| {
            watcher.watch(&folder, RecursiveMode::NonRecursive)?;
            Ok(watcher)
        })
        .inspect_err(|error| tracing::warn!(%error, "theme files will not reload live"))
        .ok();
        let watched = folder.clone();
        thread::spawn(move || {
            while changes.recv().is_ok() {
                while changes.recv_timeout(RELOAD_DEBOUNCE).is_ok() {}
                let version = sender.borrow().version + 1;
                sender.send_replace(Arc::new(LoadedThemes {
                    version,
                    ..load_folder(&watched)
                }));
            }
        });
        Self {
            folder,
            themes,
            _watcher: watcher,
        }
    }

    pub fn themes(&self) -> watch::Receiver<Arc<LoadedThemes>> {
        self.themes.clone()
    }
}

pub struct ThemeCatalog {
    bundled: Vec<Rc<ThemeConfig>>,
    user: Vec<Rc<ThemeConfig>>,
    errors: Vec<ThemeFileError>,
    folder: Option<PathBuf>,
    _watcher: Option<ThemeWatcher>,
}

/// Lets views find the catalog.
pub struct SharedThemes(pub Entity<ThemeCatalog>);

impl Global for SharedThemes {}

/// `apply_config` keeps the previous syntax colors for a theme with none, which
/// would make them depend on the theme chosen before.
fn shared(mut theme: ThemeConfig, cx: &App) -> Rc<ThemeConfig> {
    if theme.highlight.is_none() {
        let default = &ThemeRegistry::global(cx).default_themes()[&theme.mode];
        theme.highlight = default.highlight.clone();
    }
    Rc::new(theme)
}

/// The kit's default themes pair status backgrounds with text too faint to
/// read at badge size.
fn readable_status_text(theme: &ThemeConfig) -> Rc<ThemeConfig> {
    let mut theme = theme.clone();
    let colors = &mut theme.colors;
    for foreground in [
        &mut colors.danger_foreground,
        &mut colors.info_foreground,
        &mut colors.success_foreground,
        &mut colors.warning_foreground,
    ] {
        *foreground = Some("neutral-950".into());
    }
    Rc::new(theme)
}

impl ThemeCatalog {
    pub fn new(watcher: Option<ThemeWatcher>, cx: &mut Context<Self>) -> Self {
        let registry = ThemeRegistry::global(cx);
        let defaults = [ThemeMode::Light, ThemeMode::Dark]
            .map(|mode| readable_status_text(&registry.default_themes()[&mode]));
        let bundled = defaults
            .into_iter()
            .chain(bundled_files().into_iter().map(|theme| shared(theme, cx)))
            .collect();
        let mut catalog = Self {
            bundled,
            user: Vec::new(),
            errors: Vec::new(),
            folder: watcher.as_ref().map(|watcher| watcher.folder.clone()),
            _watcher: None,
        };
        if let Some(watcher) = watcher {
            let mut themes = watcher.themes();
            catalog.set_user(&themes.borrow_and_update().clone(), cx);
            cx.spawn(async move |this, cx| {
                while themes.changed().await.is_ok() {
                    let latest = themes.borrow_and_update().clone();
                    let updated = this.update(cx, |this, cx| {
                        this.set_user(&latest, cx);
                        cx.notify();
                    });
                    if updated.is_err() {
                        return;
                    }
                }
            })
            .detach();
            catalog._watcher = Some(watcher);
        }
        catalog
    }

    fn set_user(&mut self, loaded: &LoadedThemes, cx: &App) {
        self.user = loaded
            .themes
            .iter()
            .map(|theme| shared(theme.clone(), cx))
            .collect();
        self.errors = loaded.errors.clone();
    }

    pub fn errors(&self) -> &[ThemeFileError] {
        &self.errors
    }

    pub fn folder(&self) -> Option<&Path> {
        self.folder.as_deref()
    }

    fn all(&self) -> impl Iterator<Item = &Rc<ThemeConfig>> {
        self.bundled.iter().chain(&self.user)
    }

    fn find(&self, name: &str, mode: ThemeMode) -> Option<&Rc<ThemeConfig>> {
        self.all()
            .find(|theme| theme.name == name && theme.mode == mode)
    }

    /// The theme names of one mode, sorted for a picker.
    pub fn names(&self, mode: ThemeMode) -> Vec<SharedString> {
        let mut names: Vec<SharedString> = self
            .all()
            .filter(|theme| theme.mode == mode)
            .map(|theme| theme.name.clone())
            .collect();
        names.sort();
        names
    }

    /// The chosen themes that are not available.
    pub fn missing(&self, preferences: &Preferences) -> Vec<String> {
        [
            (&preferences.light_theme, ThemeMode::Light),
            (&preferences.dark_theme, ThemeMode::Dark),
        ]
        .into_iter()
        .filter(|(name, mode)| self.find(name, *mode).is_none())
        .map(|(name, _)| name.clone())
        .collect()
    }

    fn chosen(&self, name: &str, mode: ThemeMode, default: &str) -> Rc<ThemeConfig> {
        self.find(name, mode)
            .or_else(|| self.find(default, mode))
            .expect("the kit's default themes are always in the catalog")
            .clone()
    }
}

/// Shows the chosen themes in every window. `system` is ignored unless the
/// appearance follows it.
pub fn apply(
    preferences: &Preferences,
    catalog: &ThemeCatalog,
    system: WindowAppearance,
    cx: &mut App,
) {
    let light = catalog.chosen(
        &preferences.light_theme,
        ThemeMode::Light,
        DEFAULT_LIGHT_THEME,
    );
    let dark = catalog.chosen(&preferences.dark_theme, ThemeMode::Dark, DEFAULT_DARK_THEME);
    let mode = match preferences.appearance {
        Appearance::System => system.into(),
        Appearance::Light => ThemeMode::Light,
        Appearance::Dark => ThemeMode::Dark,
    };
    let theme = Theme::global_mut(cx);
    theme.light_theme = light;
    theme.dark_theme = dark;
    Theme::change(mode, None, cx);
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{ThemeFileProblem, load_folder};

    const VALID: &str = r#"{ "name": "Mine", "themes": [{ "name": "My Dark", "mode": "dark" }] }"#;

    #[test]
    fn a_missing_folder_has_no_themes() {
        let directory = tempfile::tempdir().unwrap();
        let loaded = load_folder(&directory.path().join("absent"));
        assert!(loaded.themes.is_empty() && loaded.errors.is_empty());
    }

    #[test]
    fn a_valid_file_loads_and_other_files_are_ignored() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("mine.json"), VALID).unwrap();
        fs::write(directory.path().join("notes.txt"), "not a theme").unwrap();
        let loaded = load_folder(directory.path());
        assert_eq!(loaded.themes.len(), 1);
        assert_eq!(loaded.themes[0].name, "My Dark");
        assert!(loaded.errors.is_empty());
    }

    #[test]
    fn an_invalid_file_is_named_and_the_others_still_load() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("broken.json"), "not json").unwrap();
        fs::write(directory.path().join("mine.json"), VALID).unwrap();
        let loaded = load_folder(directory.path());
        assert_eq!(loaded.themes.len(), 1);
        assert_eq!(loaded.errors[0].file, "broken.json");
        assert!(matches!(
            loaded.errors[0].problem,
            ThemeFileProblem::Invalid(_)
        ));
    }

    #[test]
    fn a_bundled_name_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        let text = VALID.replace("My Dark", "Slate Dark");
        fs::write(directory.path().join("copy.json"), text).unwrap();
        let loaded = load_folder(directory.path());
        assert!(loaded.themes.is_empty());
        assert_eq!(
            loaded.errors[0].problem,
            ThemeFileProblem::NameTaken("Slate Dark".to_owned())
        );
    }

    #[test]
    fn a_name_in_two_files_goes_to_the_first_file() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("a.json"), VALID).unwrap();
        fs::write(directory.path().join("b.json"), VALID).unwrap();
        let loaded = load_folder(directory.path());
        assert_eq!(loaded.themes.len(), 1);
        assert_eq!(loaded.errors[0].file, "b.json");
    }
}
