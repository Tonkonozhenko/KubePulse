//! Extension registry, loaders, permission enforcement, activation, and failure
//! isolation.

pub mod capability;
pub mod host;
pub mod loader;

pub use capability::{Capability, HostBackend};
pub use host::{
    CallError, ContributionSet, ExtensionHost, ExtensionInfo, ExtensionStatus, HostOptions,
};
pub use loader::{BuiltinLoader, Loader};
