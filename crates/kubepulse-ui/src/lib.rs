//! Workspace, tabs, table, editor, terminal, and the declarative UI renderer.
//!
//! This is the only crate that uses the UI toolkit.

rust_i18n::i18n!("locales", fallback = "en");

pub mod app;
pub mod cluster_model;
pub mod cluster_rail;
pub mod cluster_settings;
pub mod cluster_tab;
pub mod columns;
pub mod command_palette;
pub mod declarative;
pub mod detail;
pub mod icons;
pub mod identity;
pub mod kind_sidebar;
pub mod language;
pub mod log_tab;
pub mod manifest_tab;
pub mod persistence;
pub mod preferences;
pub mod resource_browser;
pub mod runtime;
pub mod services;
pub mod settings_tab;
pub mod shell_state;
pub mod tab_connection;
pub mod theme;
pub mod workspace;

pub use app::{RunOptions, run, run_with};
