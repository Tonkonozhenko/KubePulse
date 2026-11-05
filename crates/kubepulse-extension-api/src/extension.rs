//! What an extension implements.

use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    host::{Host, HostError, ObjectRef},
    manifest::Manifest,
    ui::UiNode,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "request", rename_all = "kebab-case")]
pub enum ExtensionRequest {
    RenderView {
        view: String,
    },
    RenderDetailTab {
        tab: String,
        target: ObjectRef,
    },
    RunAction {
        action: String,
        target: ObjectRef,
    },
    RunCommand {
        command: String,
    },
    /// Asks for a computed column's cells. The host sends only visible rows, in
    /// batches, and caches the results.
    ComputeColumn {
        column: String,
        targets: Vec<ObjectRef>,
    },
    SubmitForm {
        form: String,
        values: BTreeMap<String, String>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "response", rename_all = "kebab-case")]
pub enum ExtensionResponse {
    Ui {
        root: UiNode,
    },
    /// One cell per requested target, in order.
    Cells {
        cells: Vec<String>,
    },
    Done,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionError {
    pub message: String,
}

impl ExtensionError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ExtensionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ExtensionError {}

impl From<HostError> for ExtensionError {
    fn from(error: HostError) -> Self {
        Self::new(error.to_string())
    }
}

/// An extension. The manifest is read without activating it, and `handle` runs
/// only after activation.
#[async_trait]
pub trait Extension: Send + Sync + 'static {
    fn manifest(&self) -> Manifest;

    /// Called once, when an activation trigger first fires.
    async fn activate(&self, _host: Arc<dyn Host>) -> Result<(), ExtensionError> {
        Ok(())
    }

    async fn handle(
        &self,
        request: ExtensionRequest,
        host: Arc<dyn Host>,
    ) -> Result<ExtensionResponse, ExtensionError>;

    /// Called when the extension is disabled.
    async fn deactivate(&self) {}
}
