//! The extension contract: manifests, contribution types, declarative UI, and
//! host operations.
//!
//! Built-in features and plugins use this same contract. Every value crossing
//! it is owned and serializable, and it names no UI toolkit or Kubernetes client
//! type, so a sandboxed loader can carry these messages without changing them.
//! See `docs/extensions.md` for the authoring guide.

pub mod extension;
pub mod host;
pub mod manifest;
pub mod ui;

pub use extension::{Extension, ExtensionError, ExtensionRequest, ExtensionResponse};
pub use host::{
    ChangeOutcome, ChangeRequest, ClusterRef, Host, HostError, HostOperation, HostResponse,
    ObjectRef, ResourceObject, SurfaceRequest,
};
pub use manifest::{
    ActionContribution, ActivationTrigger, ColumnContribution, ColumnSource, CommandContribution,
    ContractVersion, Contributions, DetailTabContribution, KindRef, Manifest,
    NavigationContribution, Permission, ResourceViewContribution, SettingContribution, SettingKind,
    Verb, ViewIcon, best_language,
};
pub use ui::{
    Chart, ChartSeries, Description, Direction, FormField, FormFieldKind, Surface, SurfaceKind,
    SurfaceRef, Table, UiNode,
};

/// The contract version this build provides.
pub const CONTRACT_VERSION: ContractVersion = ContractVersion { major: 1, minor: 2 };

// Compiles the authoring guide's examples as doctests.
#[cfg(doctest)]
#[doc = include_str!("../../../docs/extensions.md")]
struct AuthoringGuide;
