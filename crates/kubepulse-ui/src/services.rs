//! What views need from the background. Replies are futures, so the UI thread
//! awaits them without blocking, and they run on the background runtime.

use std::{future::Future, pin::Pin, sync::Arc};

use gpui_kit::Global;
use kubepulse_cluster::{EventRecord, ObjectError, ObjectTarget, ResourceKind};
use kubepulse_extension_api::{ExtensionRequest, ExtensionResponse, KindRef};
use kubepulse_extension_host::{CallError, ContributionSet};
use kubepulse_store::{View, ViewQuery};
use serde_json::Value;

pub type Reply<T, E = ObjectError> = Pin<Box<dyn Future<Output = Result<T, E>> + Send>>;

pub trait ClusterServices: Send + Sync + 'static {
    /// Opens a live view, or `None` if the cluster is not connected.
    fn open_view(&self, context: &str, kind: &ResourceKind, query: ViewQuery) -> Option<View>;
    fn get_object(&self, context: &str, target: ObjectTarget) -> Reply<Value>;
    fn events(
        &self,
        context: &str,
        namespace: Option<String>,
        uid: String,
    ) -> Reply<Vec<EventRecord>>;
    fn delete_object(&self, context: &str, target: ObjectTarget) -> Reply<()>;
    fn contributions(&self) -> Arc<ContributionSet>;
    fn kind_opened(&self, kind: KindRef);
    fn extension(&self, id: &str, request: ExtensionRequest)
    -> Reply<ExtensionResponse, CallError>;
}

/// Lets views find the services.
pub struct SharedServices(pub Arc<dyn ClusterServices>);

impl Global for SharedServices {}
