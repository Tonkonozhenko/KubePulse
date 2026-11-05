//! What an extension can ask the host to do. There is no other route to a
//! cluster, and nothing here carries credentials.

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    manifest::{KindRef, Verb},
    ui::{SurfaceKind, SurfaceRef},
};

/// A cluster, by kubeconfig context name.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClusterRef(pub String);

/// One object in one cluster.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectRef {
    pub cluster: ClusterRef,
    pub kind: KindRef,
    pub namespace: Option<String>,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResourceObject {
    pub reference: ObjectRef,
    pub object: Value,
}

/// A cluster change, which always goes through the user's review.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "change", rename_all = "kebab-case")]
pub enum ChangeRequest {
    Create {
        cluster: ClusterRef,
        kind: KindRef,
        manifest: Value,
    },
    Update {
        target: ObjectRef,
        manifest: Value,
    },
    Delete {
        target: ObjectRef,
    },
}

impl ChangeRequest {
    fn required(&self) -> (&KindRef, Verb) {
        match self {
            Self::Create { kind, .. } => (kind, Verb::Create),
            Self::Update { target, .. } => (&target.kind, Verb::Update),
            Self::Delete { target } => (&target.kind, Verb::Delete),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "surface", rename_all = "kebab-case")]
pub enum SurfaceRequest {
    Logs {
        target: ObjectRef,
        container: Option<String>,
    },
    Terminal {
        target: ObjectRef,
        container: Option<String>,
    },
    Editor {
        target: ObjectRef,
    },
}

impl SurfaceRequest {
    pub fn surface_kind(&self) -> SurfaceKind {
        match self {
            Self::Logs { .. } => SurfaceKind::Log,
            Self::Terminal { .. } => SurfaceKind::Terminal,
            Self::Editor { .. } => SurfaceKind::Editor,
        }
    }

    fn required(&self) -> (&KindRef, Verb) {
        match self {
            Self::Logs { target, .. } => (&target.kind, Verb::Logs),
            Self::Terminal { target, .. } => (&target.kind, Verb::Exec),
            Self::Editor { target } => (&target.kind, Verb::Get),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "kebab-case")]
pub enum HostOperation {
    GetObject {
        target: ObjectRef,
    },
    ListObjects {
        cluster: ClusterRef,
        kind: KindRef,
        namespace: Option<String>,
        #[serde(default)]
        labels: BTreeMap<String, String>,
    },
    /// Shows the user the target and diff. Nothing changes unless they confirm.
    RequestChange {
        request: ChangeRequest,
    },
    OpenSurface {
        request: SurfaceRequest,
    },
    /// Asks for the interface language, which stays the same until the
    /// application restarts. Needs no permission.
    GetLanguage,
    /// Shows a message to the user. Needs no permission.
    Notify {
        message: String,
    },
}

impl HostOperation {
    /// The permission this operation needs, if any.
    pub fn required_permission(&self) -> Option<(&KindRef, Verb)> {
        match self {
            Self::GetObject { target } => Some((&target.kind, Verb::Get)),
            Self::ListObjects { kind, .. } => Some((kind, Verb::List)),
            Self::RequestChange { request } => Some(request.required()),
            Self::OpenSurface { request } => Some(request.required()),
            Self::GetLanguage | Self::Notify { .. } => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum ChangeOutcome {
    Applied,
    /// The user declined. The cluster is unchanged.
    Declined,
    Failed {
        reason: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "kebab-case")]
pub enum HostResponse {
    Object { object: ResourceObject },
    Objects { objects: Vec<ResourceObject> },
    Change { outcome: ChangeOutcome },
    Surface { reference: SurfaceRef },
    Language { tag: String },
    Done,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "error", rename_all = "kebab-case")]
pub enum HostError {
    /// The extension did not declare this permission.
    NotPermitted {
        kind: Option<KindRef>,
        verb: Option<Verb>,
    },
    /// The cluster refused, because the user's account lacks access.
    ClusterDenied {
        message: String,
    },
    NotFound {
        message: String,
    },
    Failed {
        message: String,
    },
}

impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotPermitted {
                kind: Some(kind),
                verb: Some(verb),
            } => {
                write!(f, "extension did not declare {verb:?} on {}", kind.kind)
            }
            Self::NotPermitted { .. } => write!(f, "extension did not declare this permission"),
            Self::ClusterDenied { message }
            | Self::NotFound { message }
            | Self::Failed { message } => {
                write!(f, "{message}")
            }
        }
    }
}

impl std::error::Error for HostError {}

/// The host, as an extension sees it. Each call goes through a handle bound to
/// the extension's declared permissions.
#[async_trait]
pub trait Host: Send + Sync {
    async fn call(&self, operation: HostOperation) -> Result<HostResponse, HostError>;
}
