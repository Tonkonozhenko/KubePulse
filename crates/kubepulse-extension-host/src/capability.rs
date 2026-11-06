//! Every host operation passes through a [`Capability`] bound to one extension's
//! declared permissions. Built-ins take this same path.

use std::sync::{
    Arc,
    atomic::{AtomicU64, AtomicUsize, Ordering},
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

/// Tracks the time an extension spends waiting for the user to review a change,
/// which does not count against its call limit.
#[derive(Debug, Default)]
pub(crate) struct ReviewClock {
    open: AtomicUsize,
    finished: AtomicU64,
}

impl ReviewClock {
    /// Changes whenever a review starts or ends, and while one is open.
    pub(crate) fn progress(&self) -> Option<u64> {
        (self.open.load(Ordering::SeqCst) == 0).then(|| self.finished.load(Ordering::SeqCst))
    }
}

struct OpenReview<'a>(&'a ReviewClock);

impl<'a> OpenReview<'a> {
    fn start(clock: &'a ReviewClock) -> Self {
        clock.open.fetch_add(1, Ordering::SeqCst);
        Self(clock)
    }
}

impl Drop for OpenReview<'_> {
    fn drop(&mut self) {
        self.0.finished.fetch_add(1, Ordering::SeqCst);
        self.0.open.fetch_sub(1, Ordering::SeqCst);
    }
}

pub struct Capability {
    extension: String,
    permissions: Vec<Permission>,
    backend: Arc<dyn HostBackend>,
    refused: Arc<AtomicU64>,
    language: String,
    reviews: Arc<ReviewClock>,
}

impl Capability {
    pub(crate) fn new(
        extension: String,
        permissions: Vec<Permission>,
        backend: Arc<dyn HostBackend>,
        refused: Arc<AtomicU64>,
        language: String,
        reviews: Arc<ReviewClock>,
    ) -> Self {
        Self {
            extension,
            permissions,
            backend,
            refused,
            language,
            reviews,
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
        if matches!(operation, HostOperation::GetLanguage) {
            return Ok(HostResponse::Language {
                tag: self.language.clone(),
            });
        }
        let _review = matches!(operation, HostOperation::RequestChange { .. })
            .then(|| OpenReview::start(&self.reviews));
        self.backend.execute(operation).await
    }
}
