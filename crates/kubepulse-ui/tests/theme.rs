mod support;

use std::{fs, sync::Arc, time::Duration};

use gpui_kit::{
    AppContext as _, Entity, Hsla, Rgba, SharedString, TestAppContext, VisualTestContext,
    WindowAppearance, WindowOptions,
    component::{ActiveTheme as _, ThemeMode},
    test::TestWindowExt as _,
};
use kubepulse_ui::{
    cluster_model::ClusterModel,
    identity::{ClusterKey, IdentityStore},
    language,
    persistence::ConfigPaths,
    preferences::{Appearance, Preferences, PreferencesModel, SharedPreferences},
    runtime::ClusterState,
    services::SharedServices,
    settings_tab,
    shell_state::{SharedShellState, ShellState},
    theme::{self, SharedThemes, ThemeCatalog, ThemeWatcher},
    workspace::Workspace,
};
use support::FakeServices;
use tokio::sync::{mpsc, watch};

const MY_DARK: &str = r##"{ "name": "Mine", "themes": [{
    "name": "My Dark", "mode": "dark", "colors": { "background": "#112233" }
}] }"##;

fn setup(
    cx: &mut TestAppContext,
    preferences: Preferences,
    themes: Option<ThemeWatcher>,
) -> Entity<ClusterModel> {
    let (_state, receiver) = watch::channel(Arc::new(ClusterState {
        version: 1,
        clusters: vec![],
        errors: vec![],
    }));
    let (sender, _commands) = mpsc::unbounded_channel();
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_reduce_motion(true);
        cx.set_global(SharedServices(Arc::new(FakeServices::idle())));
        let model = cx.new(|cx| ClusterModel::new(receiver, sender, IdentityStore::default(), cx));
        let shell = cx.new(|_| ShellState::default());
        cx.set_global(SharedShellState(shell));
        settings_tab::init(preferences, themes, cx);
        model
    })
}

fn open_workspace(cx: &mut TestAppContext, model: Entity<ClusterModel>) -> &mut VisualTestContext {
    let (handle, _) = cx
        .update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| Workspace::new(model, None, window, cx))
            })
        })
        .unwrap();
    let cx = VisualTestContext::from_window(handle, cx).into_mut();
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1400.), gpui_kit::px(900.)));
    cx.run_until_parked();
    cx
}

fn preferences(cx: &mut VisualTestContext) -> Entity<PreferencesModel> {
    cx.update(|_, cx| cx.global::<SharedPreferences>().0.clone())
}

fn change(cx: &mut VisualTestContext, edit: impl FnOnce(&mut Preferences)) {
    let model = preferences(cx);
    cx.update(|_, cx| model.update(cx, |model, cx| model.change(edit, cx)));
    cx.run_until_parked();
}

fn catalog(cx: &mut VisualTestContext) -> Entity<ThemeCatalog> {
    cx.update(|_, cx| cx.global::<SharedThemes>().0.clone())
}

fn background(cx: &mut VisualTestContext) -> Hsla {
    cx.update(|_, cx| cx.theme().background)
}

fn is_dark(cx: &mut VisualTestContext) -> bool {
    cx.update(|_, cx| cx.theme().is_dark())
}

fn theme_name(cx: &mut VisualTestContext) -> SharedString {
    cx.update(|_, cx| cx.theme().theme_name().clone())
}

fn click(id: &str, cx: &mut VisualTestContext) {
    let id = SharedString::from(id.to_owned());
    cx.update(|window, cx| window.click(id, cx));
    cx.run_until_parked();
}

fn exists(id: &str, cx: &mut VisualTestContext) -> bool {
    let id = SharedString::from(id.to_owned());
    cx.update(|window, cx| {
        window.render_frame(cx);
        window.try_find(id).is_some()
    })
}

fn pick(button: &str, index: usize, cx: &mut VisualTestContext) {
    click(button, cx);
    cx.update(|window, cx| {
        window.render_frame(cx);
        window
            .within("popup-menu")
            .click(gpui_kit::ElementId::Integer(index as u64), cx);
    });
    cx.run_until_parked();
}

fn menu_count(button: &str, cx: &mut VisualTestContext) -> usize {
    click(button, cx);
    let count = (0..)
        .take_while(|index| {
            cx.update(|window, cx| {
                window.render_frame(cx);
                window
                    .within("popup-menu")
                    .try_find(gpui_kit::ElementId::Integer(*index as u64))
                    .is_some()
            })
        })
        .count();
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    count
}

fn luminance(color: Hsla) -> f32 {
    let color = Rgba::from(color);
    let channel = |value: f32| {
        if value <= 0.03928 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color.r) + 0.7152 * channel(color.g) + 0.0722 * channel(color.b)
}

fn contrast(a: Hsla, b: Hsla) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

#[gpui_kit::test]
fn every_bundled_theme_meets_its_contrast_limit(cx: &mut TestAppContext) {
    let model = setup(cx, Preferences::default(), None);
    let cx = open_workspace(cx, model);
    let catalog = catalog(cx);
    for (mode, appearance) in [
        (ThemeMode::Light, Appearance::Light),
        (ThemeMode::Dark, Appearance::Dark),
    ] {
        let names = cx.update(|_, cx| catalog.read(cx).names(mode));
        assert_eq!(names.len(), 3, "{mode:?}");
        for name in names {
            let chosen = name.to_string();
            change(cx, |p| {
                p.appearance = appearance;
                p.light_theme.clone_from(&chosen);
                p.dark_theme.clone_from(&chosen);
            });
            assert_eq!(theme_name(cx), name);
            let (foreground, background) =
                cx.update(|_, cx| (cx.theme().foreground, cx.theme().background));
            let limit = if name.contains("High Contrast") {
                7.
            } else {
                4.5
            };
            let ratio = contrast(foreground, background);
            assert!(ratio >= limit, "{name}: {ratio}");
        }
    }
}

#[gpui_kit::test]
fn a_theme_change_shows_at_once_but_the_inactive_mode_waits(cx: &mut TestAppContext) {
    let dark = Preferences {
        appearance: Appearance::Dark,
        ..Preferences::default()
    };
    let model = setup(cx, dark, None);
    let cx = open_workspace(cx, model);
    let before = background(cx);

    change(cx, |p| p.dark_theme = "Slate Dark".to_owned());
    assert_ne!(background(cx), before);

    let slate = background(cx);
    change(cx, |p| p.light_theme = "Paper Light".to_owned());
    assert_eq!(background(cx), slate);

    change(cx, |p| p.appearance = Appearance::Light);
    assert_eq!(theme_name(cx), "Paper Light");
}

#[gpui_kit::test]
fn a_missing_theme_uses_the_default_and_keeps_the_choice(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().unwrap();
    let folder = directory.path().join("themes");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("mine.json"), MY_DARK).unwrap();
    let chosen = Preferences {
        appearance: Appearance::Dark,
        dark_theme: "My Dark".to_owned(),
        ..Preferences::default()
    };
    let model = setup(cx, chosen, Some(ThemeWatcher::start(folder.clone())));
    let cx = open_workspace(cx, model);
    assert_eq!(theme_name(cx), "My Dark");
    click("rail-settings", cx);

    fs::remove_file(folder.join("mine.json")).unwrap();
    wait_for(cx, |cx| theme_name(cx) == "Default Dark");
    assert!(exists("theme-problems", cx));
    let model = preferences(cx);
    assert_eq!(
        cx.update(|_, cx| model.read(cx).get().dark_theme.clone()),
        "My Dark"
    );

    fs::write(folder.join("mine.json"), MY_DARK).unwrap();
    wait_for(cx, |cx| theme_name(cx) == "My Dark");
    assert!(!exists("theme-problems", cx));
}

fn wait_for(cx: &mut VisualTestContext, mut done: impl FnMut(&mut VisualTestContext) -> bool) {
    for _ in 0..500 {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out");
}

#[gpui_kit::test]
fn the_first_frame_uses_saved_user_themes(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().unwrap();
    let folder = directory.path().join("themes");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("mine.json"), MY_DARK).unwrap();
    let chosen = Preferences {
        appearance: Appearance::Dark,
        dark_theme: "My Dark".to_owned(),
        ..Preferences::default()
    };
    setup(cx, chosen, Some(ThemeWatcher::start(folder)));
    let name = cx.update(|cx| cx.theme().theme_name().clone());
    assert_eq!(name, "My Dark", "applied before any window opened");
}

#[gpui_kit::test]
fn system_mode_follows_the_window_and_fixed_modes_do_not(cx: &mut TestAppContext) {
    let model = setup(cx, Preferences::default(), None);
    let cx = open_workspace(cx, model);
    assert!(!is_dark(cx), "the test platform starts light");

    // The test platform cannot report an appearance change to a window, so
    // this applies what the window's observer would.
    let catalog = catalog(cx);
    let follow = |preferences: Preferences, system, cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            catalog.update(cx, |catalog, cx| {
                theme::apply(&preferences, catalog, system, cx);
            });
        });
    };
    follow(Preferences::default(), WindowAppearance::Dark, cx);
    assert!(is_dark(cx));
    follow(Preferences::default(), WindowAppearance::Light, cx);
    assert!(!is_dark(cx));

    let fixed = Preferences {
        appearance: Appearance::Light,
        ..Preferences::default()
    };
    follow(fixed, WindowAppearance::Dark, cx);
    assert!(!is_dark(cx), "a fixed mode ignores the system");
}

#[gpui_kit::test]
fn the_pickers_change_preferences_and_list_only_their_mode(cx: &mut TestAppContext) {
    let model = setup(cx, Preferences::default(), None);
    let cx = open_workspace(cx, model);
    click("rail-settings", cx);
    assert_eq!(menu_count("light-theme", cx), 3);
    assert_eq!(menu_count("dark-theme", cx), 3);

    pick("appearance-mode", 2, cx);
    assert!(is_dark(cx));

    pick("dark-theme", 2, cx);
    assert_eq!(theme_name(cx), "Slate Dark");

    pick("light-theme", 2, cx);
    let model = preferences(cx);
    assert_eq!(
        cx.update(|_, cx| model.read(cx).get().light_theme.clone()),
        "Paper Light"
    );
}

#[gpui_kit::test]
fn a_file_error_appears_and_clears(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().unwrap();
    let folder = directory.path().join("themes");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("broken.json"), "not json").unwrap();
    let model = setup(
        cx,
        Preferences::default(),
        Some(ThemeWatcher::start(folder.clone())),
    );
    let cx = open_workspace(cx, model);
    click("rail-settings", cx);
    assert!(exists("theme-problems", cx));

    fs::write(folder.join("broken.json"), MY_DARK).unwrap();
    let catalog = catalog(cx);
    wait_for(cx, |cx| {
        cx.update(|_, cx| catalog.read(cx).errors().is_empty())
    });
    assert!(!exists("theme-problems", cx));
    let names = cx.update(|_, cx| catalog.read(cx).names(ThemeMode::Dark));
    assert!(names.iter().any(|name| name == "My Dark"));
}

#[gpui_kit::test]
fn the_folder_is_created_at_startup_and_offered_in_settings(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().unwrap();
    let config = ConfigPaths::in_directory(directory.path());
    let watcher = ThemeWatcher::start(config.themes.clone());
    assert!(config.themes.is_dir(), "created when the watcher starts");
    let model = setup(cx, Preferences::default(), Some(watcher));
    let cx = open_workspace(cx, model);
    click("rail-settings", cx);
    assert!(exists("open-themes-folder", cx));
}

#[tokio::test]
async fn adding_changing_and_removing_a_file_reloads() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("mine.json");
    let watcher = ThemeWatcher::start(directory.path().to_path_buf());
    let mut themes = watcher.themes();
    assert!(themes.borrow().themes.is_empty());

    let wait = async |themes: &mut watch::Receiver<_>| {
        tokio::time::timeout(Duration::from_secs(5), themes.changed())
            .await
            .expect("the folder reloads")
            .unwrap();
    };
    fs::write(&path, MY_DARK).unwrap();
    wait(&mut themes).await;
    assert_eq!(themes.borrow_and_update().themes.len(), 1);

    fs::write(&path, MY_DARK.replace("My Dark", "Other Dark")).unwrap();
    wait(&mut themes).await;
    assert_eq!(themes.borrow_and_update().themes[0].name, "Other Dark");

    fs::remove_file(&path).unwrap();
    wait(&mut themes).await;
    assert!(themes.borrow_and_update().themes.is_empty());
}

#[tokio::test]
async fn a_burst_of_writes_reloads_once() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("mine.json");
    let watcher = ThemeWatcher::start(directory.path().to_path_buf());
    for _ in 0..20 {
        fs::write(&path, MY_DARK).unwrap();
    }
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(watcher.themes().borrow().version, 1);
}

#[gpui_kit::test]
fn a_cluster_keeps_its_color_in_every_theme(cx: &mut TestAppContext) {
    let model = setup(cx, Preferences::default(), None);
    let key = ClusterKey::new("a", None);
    let color = cx.update(|cx| model.read(cx).identity(&key).color);
    let cx = open_workspace(cx, model.clone());
    change(cx, |p| {
        p.appearance = Appearance::Dark;
        p.dark_theme = "High Contrast Dark".to_owned();
    });
    assert_eq!(
        cx.update(|_, cx| model.read(cx).identity(&key).color),
        color
    );
}

#[gpui_kit::test]
fn the_language_setting_hides_the_testing_locale(cx: &mut TestAppContext) {
    let model = setup(cx, Preferences::default(), None);
    let cx = open_workspace(cx, model);
    click("rail-settings", cx);
    assert_eq!(menu_count("language", cx), 2, "System default and English");

    pick("language", 1, cx);
    let model = preferences(cx);
    let chosen = cx.update(|_, cx| model.read(cx).get().language.clone());
    assert_eq!(chosen.as_deref(), Some("en"));
    assert!(
        !exists("language-restart", cx),
        "English is already showing"
    );
}

#[gpui_kit::test]
fn a_new_language_waits_for_a_restart_the_user_chooses(cx: &mut TestAppContext) {
    let model = setup(cx, Preferences::default(), None);
    let cx = open_workspace(cx, model);
    click("rail-settings", cx);
    let before = language::current();
    change(cx, |p| p.language = Some(language::PSEUDO.to_owned()));
    assert!(exists("language-restart", cx));
    assert_eq!(language::current(), before, "the interface stays as it was");
}
