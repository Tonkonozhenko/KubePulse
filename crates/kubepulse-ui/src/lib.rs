//! Workspace, tabs, table, editor, terminal, and the declarative UI renderer.
//!
//! This is the only crate that uses the UI toolkit.

pub mod app;
pub mod cluster_model;
pub mod cluster_panel;
pub mod cluster_tab;
pub mod columns;
pub mod declarative;
pub mod detail;
pub mod identity;
pub mod persistence;
pub mod resource_browser;
pub mod runtime;
pub mod services;
pub mod workspace;

pub use app::{RunOptions, run, run_with};
