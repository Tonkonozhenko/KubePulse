//! Extension registry, permission enforcement, activation, and failure
//! isolation.

pub mod capability;
pub mod host;

pub use capability::{Capability, HostBackend};
pub use host::{
    CallError, ContributionSet, ExtensionHost, ExtensionInfo, ExtensionStatus, HostOptions,
};
