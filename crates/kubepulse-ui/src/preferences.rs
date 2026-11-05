//! What the user has chosen for the whole application.

use gpui_kit::*;
use serde::{Deserialize, Serialize};

pub const DEFAULT_LIGHT_THEME: &str = "Default Light";
pub const DEFAULT_DARK_THEME: &str = "Default Dark";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub appearance: Appearance,
    pub light_theme: String,
    pub dark_theme: String,
    /// `None` follows the system language.
    pub language: Option<String>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            appearance: Appearance::default(),
            light_theme: DEFAULT_LIGHT_THEME.to_owned(),
            dark_theme: DEFAULT_DARK_THEME.to_owned(),
            language: None,
        }
    }
}

impl Preferences {
    pub fn from_json(text: &str) -> serde_json::Result<Self> {
        serde_json::from_str(text)
    }

    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }
}

pub struct PreferencesModel {
    preferences: Preferences,
}

/// Lets panels rebuilt from a saved layout find the preferences.
pub struct SharedPreferences(pub Entity<PreferencesModel>);

impl Global for SharedPreferences {}

pub enum PreferencesEvent {
    Changed,
}

impl EventEmitter<PreferencesEvent> for PreferencesModel {}

impl PreferencesModel {
    pub fn new(preferences: Preferences) -> Self {
        Self { preferences }
    }

    pub fn get(&self) -> &Preferences {
        &self.preferences
    }

    pub fn change(&mut self, edit: impl FnOnce(&mut Preferences), cx: &mut Context<Self>) {
        let before = self.preferences.clone();
        edit(&mut self.preferences);
        if self.preferences != before {
            cx.emit(PreferencesEvent::Changed);
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Appearance, DEFAULT_LIGHT_THEME, Preferences};

    #[test]
    fn an_empty_file_means_defaults() {
        assert_eq!(
            Preferences::from_json("{}").unwrap(),
            Preferences::default()
        );
    }

    #[test]
    fn a_partial_file_keeps_defaults_for_the_rest() {
        let preferences = Preferences::from_json(r#"{ "appearance": "dark" }"#).unwrap();
        assert_eq!(preferences.appearance, Appearance::Dark);
        assert_eq!(preferences.light_theme, DEFAULT_LIGHT_THEME);
        assert_eq!(preferences.language, None);
    }

    #[test]
    fn a_damaged_file_is_an_error() {
        assert!(Preferences::from_json("not json").is_err());
        assert!(Preferences::from_json(r#"{ "appearance": "purple" }"#).is_err());
    }

    #[test]
    fn preferences_round_trip() {
        let preferences = Preferences {
            appearance: Appearance::Light,
            light_theme: "Paper Light".to_owned(),
            dark_theme: "Slate Dark".to_owned(),
            language: Some("en".to_owned()),
        };
        let text = preferences.to_json().unwrap();
        assert_eq!(Preferences::from_json(&text).unwrap(), preferences);
    }
}
