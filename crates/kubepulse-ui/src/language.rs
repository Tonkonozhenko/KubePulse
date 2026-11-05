//! Which language the interface uses. It is chosen once at startup, so nothing
//! holding text needs to refresh.

use kubepulse_extension_api::best_language;
use rust_i18n::t;

use crate::preferences::Preferences;

pub const ENGLISH: &str = "en";
/// A testing locale, shown nowhere unless typed into the preferences file.
pub const PSEUDO: &str = "en-XA";

/// Every language with a catalog, including the testing locale.
fn catalogs() -> Vec<String> {
    let mut tags: Vec<String> = rust_i18n::available_locales!()
        .into_iter()
        .map(|tag| tag.into_owned())
        .collect();
    tags.sort();
    tags
}

/// The languages a user can choose.
pub fn shipped() -> Vec<String> {
    catalogs().into_iter().filter(|tag| tag != PSEUDO).collect()
}

/// The saved language if it has a catalog, otherwise the first system language
/// that matches one, otherwise English.
pub fn resolve(saved: Option<&str>, system: &[String], available: &[&str]) -> String {
    if let Some(saved) = saved {
        if available.contains(&saved) {
            return saved.to_owned();
        }
        tracing::warn!(saved, "the saved language is not shipped");
    }
    let selectable = || available.iter().copied().filter(|tag| *tag != PSEUDO);
    system
        .iter()
        .find_map(|tag| best_language(tag, selectable()))
        .unwrap_or(ENGLISH)
        .to_owned()
}

/// The language these preferences call for on this system.
pub fn select(preferences: &Preferences) -> String {
    let system: Vec<String> = sys_locale::get_locales().collect();
    let catalogs = catalogs();
    let available: Vec<&str> = catalogs.iter().map(String::as_str).collect();
    resolve(preferences.language.as_deref(), &system, &available)
}

/// Sets the language of our catalogs and the kit's, which share one locale.
pub fn set_current(tag: &str) {
    rust_i18n::set_locale(tag);
}

pub fn current() -> String {
    rust_i18n::locale().to_string()
}

/// The language's name, in that language.
pub fn name(tag: &str) -> String {
    t!("language.name", locale = tag).to_string()
}

#[cfg(test)]
mod tests {
    use super::{PSEUDO, resolve};

    const SHIPPED: [&str; 3] = ["de", "en", PSEUDO];

    fn system(tags: &[&str]) -> Vec<String> {
        tags.iter().map(|tag| (*tag).to_owned()).collect()
    }

    #[test]
    fn a_saved_language_wins() {
        assert_eq!(resolve(Some("de"), &system(&["en-US"]), &SHIPPED), "de");
        assert_eq!(resolve(Some(PSEUDO), &[], &SHIPPED), PSEUDO);
    }

    #[test]
    fn an_exact_or_regional_system_language_matches() {
        assert_eq!(resolve(None, &system(&["de"]), &SHIPPED), "de");
        assert_eq!(resolve(None, &system(&["de-AT"]), &SHIPPED), "de");
    }

    #[test]
    fn the_first_matching_system_language_wins() {
        let languages = system(&["fr-FR", "de-DE", "en-US"]);
        assert_eq!(resolve(None, &languages, &SHIPPED), "de");
    }

    #[test]
    fn an_unshipped_or_unknown_language_means_english() {
        assert_eq!(resolve(None, &system(&["fr"]), &SHIPPED), "en");
        assert_eq!(resolve(None, &[], &SHIPPED), "en");
    }

    #[test]
    fn an_unknown_saved_language_follows_the_system() {
        assert_eq!(resolve(Some("xx"), &system(&["de"]), &SHIPPED), "de");
    }

    #[test]
    fn the_testing_locale_never_matches_a_system_language() {
        assert_eq!(
            resolve(None, &system(&["en-XA"]), &["de", "en", PSEUDO][..]),
            "en"
        );
        assert_eq!(
            resolve(None, &system(&["en-GB"]), &["de", PSEUDO][..]),
            "en"
        );
    }
}
