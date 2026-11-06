//! Application preferences, in a tab that belongs to no cluster.

use std::{fs, path::PathBuf};

use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme as _, Icon, Sizable as _, ThemeMode,
        button::Button,
        dock::{self, PanelEvent},
        menu::{DropdownMenu as _, PopupMenuItem},
        setting::{SettingField, SettingGroup, SettingItem, SettingPage, Settings},
    },
    *,
};

use rust_i18n::t;

use crate::{
    language,
    preferences::{Appearance, Preferences, PreferencesModel, SharedPreferences},
    shell_state::{SharedShellState, ShellState, close_button, close_on_middle_click},
    theme::{SharedThemes, ThemeCatalog, ThemeFileError, ThemeFileProblem, ThemeWatcher, apply},
};

pub const PANEL_NAME: &str = "SettingsTab";
pub const SETTINGS_SHORTCUT: &str = if cfg!(target_os = "macos") {
    "cmd-,"
} else {
    "ctrl-,"
};

gpui_kit::actions!(kubepulse, [OpenSettings]);

/// Must run before any window opens, because the first frame must use the chosen
/// theme.
pub fn init(preferences: Preferences, themes: Option<ThemeWatcher>, cx: &mut App) {
    cx.bind_keys([KeyBinding::new(SETTINGS_SHORTCUT, OpenSettings, None)]);
    let catalog = cx.new(|cx| ThemeCatalog::new(themes, cx));
    let system = cx.window_appearance();
    catalog.update(cx, |catalog, cx| apply(&preferences, catalog, system, cx));
    cx.set_global(SharedThemes(catalog));
    let model = cx.new(|_| PreferencesModel::new(preferences));
    cx.set_global(SharedPreferences(model));
}

/// Stores one preference from a picker.
fn set_preference(cx: &mut App, edit: impl FnOnce(&mut Preferences)) {
    let preferences = cx.global::<SharedPreferences>().0.clone();
    preferences.update(cx, |preferences, cx| preferences.change(edit, cx));
}

/// A button that opens a menu of values. Its id stays stable, unlike the kit's
/// dropdown field, so tests can reach it.
fn picker<T: Clone + PartialEq + 'static>(
    id: &'static str,
    current: T,
    options: Vec<(T, SharedString)>,
    set: impl Fn(T, &mut App) + Clone + 'static,
) -> impl IntoElement {
    let label = options
        .iter()
        .find(|(value, _)| *value == current)
        .map(|(_, label)| label.clone())
        .unwrap_or_default();
    Button::new(id)
        .label(label)
        .dropdown_caret(true)
        .outline()
        .small()
        .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
            options.iter().fold(menu, |menu, (value, label)| {
                let (chosen, set) = (value.clone(), set.clone());
                menu.item(
                    PopupMenuItem::new(label.clone())
                        .checked(*value == current)
                        .on_click(move |_, _, cx| set(chosen.clone(), cx)),
                )
            })
        })
}

fn theme_item(
    id: &'static str,
    title: String,
    mode: ThemeMode,
    chosen: String,
    names: Vec<SharedString>,
    set: fn(&mut Preferences, String),
) -> SettingItem {
    SettingItem::new(
        title,
        SettingField::render(move |_, _, _| {
            let options = names
                .iter()
                .map(|name| (name.to_string(), name.clone()))
                .collect();
            picker(id, chosen.clone(), options, move |name, cx| {
                set_preference(cx, |preferences| set(preferences, name));
            })
        }),
    )
    .description(match mode {
        ThemeMode::Light => t!("settings.light_theme_help").to_string(),
        ThemeMode::Dark => t!("settings.dark_theme_help").to_string(),
    })
}

fn problem_text(error: &ThemeFileError) -> String {
    match &error.problem {
        ThemeFileProblem::Unreadable(reason) | ThemeFileProblem::Invalid(reason) => {
            format!("{}: {reason}", error.file)
        }
        ThemeFileProblem::NameTaken(name) => {
            t!("settings.theme_name_taken", file = error.file, name = name).to_string()
        }
    }
}

fn open_folder(folder: PathBuf, cx: &mut App) {
    let create = {
        let folder = folder.clone();
        cx.background_spawn(async move { fs::create_dir_all(&folder) })
    };
    cx.spawn(async move |cx| match create.await {
        Ok(()) => cx.update(|cx| cx.open_with_system(&folder)),
        Err(error) => {
            tracing::warn!(path = %folder.display(), %error, "could not create the themes folder");
        }
    })
    .detach();
}

pub struct SettingsTab {
    preferences: Entity<PreferencesModel>,
    themes: Entity<ThemeCatalog>,
    shell: Entity<ShellState>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl SettingsTab {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let preferences = cx.global::<SharedPreferences>().0.clone();
        let themes = cx.global::<SharedThemes>().0.clone();
        let subscriptions = vec![
            cx.observe(&preferences, |_, _, cx| cx.notify()),
            cx.observe(&themes, |_, _, cx| cx.notify()),
        ];
        Self {
            preferences,
            themes,
            shell: cx.global::<SharedShellState>().0.clone(),
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    fn appearance_group(&self, cx: &App) -> SettingGroup {
        let preferences = self.preferences.read(cx).get().clone();
        let themes = self.themes.read(cx);
        let appearance = preferences.appearance;
        let mut problems: Vec<String> = themes.errors().iter().map(problem_text).collect();
        problems.extend(
            themes
                .missing(&preferences)
                .iter()
                .map(|name| t!("settings.theme_missing", name = name).to_string()),
        );
        let folder = themes.folder().map(PathBuf::from);

        let mut group = SettingGroup::new()
            .title(t!("settings.appearance").to_string())
            .item(SettingItem::new(
                t!("settings.mode").to_string(),
                SettingField::render(move |_, _, _| {
                    let options = vec![
                        (
                            Appearance::System,
                            t!("settings.mode_system").to_string().into(),
                        ),
                        (
                            Appearance::Light,
                            t!("settings.mode_light").to_string().into(),
                        ),
                        (
                            Appearance::Dark,
                            t!("settings.mode_dark").to_string().into(),
                        ),
                    ];
                    picker("appearance-mode", appearance, options, |appearance, cx| {
                        set_preference(cx, |preferences| preferences.appearance = appearance);
                    })
                }),
            ))
            .item(theme_item(
                "light-theme",
                t!("settings.light_theme").to_string(),
                ThemeMode::Light,
                preferences.light_theme.clone(),
                themes.names(ThemeMode::Light),
                |preferences, name| preferences.light_theme = name,
            ))
            .item(theme_item(
                "dark-theme",
                t!("settings.dark_theme").to_string(),
                ThemeMode::Dark,
                preferences.dark_theme.clone(),
                themes.names(ThemeMode::Dark),
                |preferences, name| preferences.dark_theme = name,
            ));
        if let Some(folder) = folder {
            group = group.item(SettingItem::new(
                t!("settings.theme_files").to_string(),
                SettingField::render(move |_, _, _| {
                    let folder = folder.clone();
                    Button::new("open-themes-folder")
                        .icon(IconName::FolderOpen)
                        .label(t!("settings.open_folder").to_string())
                        .outline()
                        .small()
                        .on_click(move |_, _, cx| open_folder(folder.clone(), cx))
                }),
            ));
        }
        if !problems.is_empty() {
            group = group.item(SettingItem::render(move |_, _, cx| {
                div()
                    .id("theme-problems")
                    .flex()
                    .flex_col()
                    .gap_1()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .children(problems.iter().map(|problem| div().child(problem.clone())))
                    .test_support()
            }));
        }
        group
    }

    fn language_group(&self, cx: &App) -> SettingGroup {
        let preferences = self.preferences.read(cx).get();
        let chosen = preferences.language.clone();
        let needs_restart = language::select(preferences) != language::current();
        let mut group = SettingGroup::new()
            .title(t!("settings.language").to_string())
            .item(SettingItem::new(
                t!("settings.language").to_string(),
                SettingField::render(move |_, _, _| {
                    let options =
                        std::iter::once((None, t!("settings.language_system").to_string().into()))
                            .chain(
                                language::shipped()
                                    .into_iter()
                                    .map(|tag| (Some(tag.clone()), language::name(&tag).into())),
                            )
                            .collect();
                    picker("language", chosen.clone(), options, |tag, cx| {
                        set_preference(cx, |preferences| preferences.language = tag);
                    })
                }),
            ));
        if needs_restart {
            group = group.item(
                SettingItem::new(
                    t!("settings.restart_to_apply").to_string(),
                    SettingField::render(|_, _, _| {
                        Button::new("language-restart")
                            .icon(IconName::RotateCw)
                            .label(t!("settings.restart_now").to_string())
                            .outline()
                            .small()
                            .on_click(|_, _, cx| cx.restart())
                    }),
                )
                .description(t!("settings.restart_warning").to_string()),
            );
        }
        group
    }
}

impl gpui_kit::base::dock::Panel for SettingsTab {
    fn panel_name(&self) -> &'static str {
        PANEL_NAME
    }

    fn closable(&self, _: &App) -> bool {
        false
    }

    fn on_added_to(
        &mut self,
        _: WeakEntity<gpui_kit::base::dock::TabGroup>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tab = cx.weak_entity();
        self.shell
            .update(cx, |shell, _| shell.set_settings_tab(Some(tab)));
    }

    fn on_removed(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.shell
            .update(cx, |shell, _| shell.set_settings_tab(None));
    }
}

impl dock::Panel for SettingsTab {
    fn title(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("settings-tab-title")
            .flex()
            .items_center()
            .gap_2()
            .child(Icon::new(IconName::Settings).xsmall())
            .child(t!("settings.title").to_string())
            .on_aux_click(close_on_middle_click(cx.entity_id(), self.shell.clone()))
            .child(close_button(cx.entity_id(), self.shell.clone()))
            .test_support()
    }
}

impl EventEmitter<PanelEvent> for SettingsTab {}

impl Focusable for SettingsTab {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SettingsTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("settings")
            .size_full()
            .track_focus(&self.focus)
            .child(
                Settings::new("settings-pages").page(
                    SettingPage::new(t!("settings.title").to_string())
                        .group(self.appearance_group(cx))
                        .group(self.language_group(cx)),
                ),
            )
            .test_support()
    }
}
