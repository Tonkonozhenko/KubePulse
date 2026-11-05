//! Workspace, tabs, table, editor, terminal, and the declarative UI renderer.
//!
//! This is the only crate that uses the UI toolkit.

pub mod app;
pub mod cluster_model;
pub mod cluster_panel;
pub mod cluster_tab;
pub mod identity;
pub mod persistence;
pub mod runtime;
pub mod workspace;

pub use app::{RunOptions, run, run_with};
