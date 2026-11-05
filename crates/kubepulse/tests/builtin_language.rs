//! The only crate that sees the built-in views, the host, and the navigation.

use std::sync::Arc;

use async_trait::async_trait;
use kubepulse_extension_api::{HostError, HostOperation, HostResponse};
use kubepulse_extension_host::{ExtensionHost, HostBackend, HostOptions};
use kubepulse_store::synthetic::pod_kind;
use kubepulse_ui::{columns::navigation, language};

struct NoBackend;

#[async_trait]
impl HostBackend for NoBackend {
    async fn execute(&self, _: HostOperation) -> Result<HostResponse, HostError> {
        unreachable!("nothing runs in this test")
    }
}

#[test]
fn the_sidebar_shows_built_in_titles_from_their_catalog() {
    let host = ExtensionHost::new(
        Arc::new(NoBackend),
        HostOptions {
            language: language::PSEUDO.to_owned(),
            ..HostOptions::default()
        },
    );
    host.load(kubepulse_builtin::extensions());
    let contributions = host.contributions().borrow().clone();
    let groups = navigation(&[pod_kind()], &contributions);
    assert_eq!(groups[0].name, "Workloads", "groups by the declared name");
    assert!(groups[0].title.starts_with('['), "{}", groups[0].title);
    assert!(groups[0].kinds[0].title.starts_with('['));
}
