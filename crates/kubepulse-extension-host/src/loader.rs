//! Where extensions come from. A sandboxed loader would be another
//! implementation of [`Loader`], producing the same [`Extension`] interface.

use std::sync::Arc;

use kubepulse_extension_api::Extension;

pub trait Loader {
    fn load(&self) -> Vec<Arc<dyn Extension>>;
}

/// Extensions compiled into the application.
#[derive(Default)]
pub struct BuiltinLoader(Vec<Arc<dyn Extension>>);

impl BuiltinLoader {
    pub fn new(extensions: Vec<Arc<dyn Extension>>) -> Self {
        Self(extensions)
    }
}

impl Loader for BuiltinLoader {
    fn load(&self) -> Vec<Arc<dyn Extension>> {
        self.0.clone()
    }
}
