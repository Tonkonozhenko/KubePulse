//! Every host operation passes through a [`Capability`] bound to one extension's
//! declared permissions. Built-ins take this same path.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use async_trait::async_trait;
use kubepulse_extension_api::{
    Host, HostError, HostOperation, HostResponse, KindRef, Permission, Verb,
};

/// Carries out permitted operations. The cluster's own authorization still
/// applies, and its denials come back as [`HostError::ClusterDenied`].
#[async_trait]
pub trait HostBackend: Send + Sync + 'static {
    async fn execute(&self, operation: HostOperation) -> Result<HostResponse, HostError>;
}

pub struct Capability {
    extension: String,
    permissions: Vec<Permission>,
    backend: Arc<dyn HostBackend>,
    refused: Arc<AtomicU64>,
}

impl Capability {
    pub(crate) fn new(
        extension: String,
        permissions: Vec<Permission>,
        backend: Arc<dyn HostBackend>,
        refused: Arc<AtomicU64>,
    ) -> Self {
        Self {
            extension,
            permissions,
            backend,
            refused,
        }
    }

    fn allows(&self, kind: &KindRef, verb: Verb) -> bool {
        self.permissions
            .iter()
            .any(|p| &p.kind == kind && p.verbs.contains(&verb))
    }
}

#[async_trait]
impl Host for Capability {
    async fn call(&self, operation: HostOperation) -> Result<HostResponse, HostError> {
        if let Some((kind, verb)) = operation.required_permission()
            && !self.allows(kind, verb)
        {
            self.refused.fetch_add(1, Ordering::Relaxed);
            tracing::warn!(extension = %self.extension, kind = %kind.kind, ?verb, "refused undeclared operation");
            return Err(HostError::NotPermitted {
                kind: Some(kind.clone()),
                verb: Some(verb),
            });
        }
        self.backend.execute(operation).await
    }
}
