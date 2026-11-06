//! Discovery repeats for as long as a cluster is connected, and definition
//! schemas can reach many megabytes, so reading them must not build them.

mod support;

use std::{alloc::System, time::Duration};

use kubepulse_cluster::SessionManager;
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};
use support::*;
use tokio::time;
use wiremock::{
    Mock,
    matchers::{header_regex, method, path},
};

#[global_allocator]
static ALLOCATOR: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const DEFINITIONS: usize = 80;
const PROPERTIES: usize = 1_000;
/// Total allocation allowed for one discovery, as a multiple of the response.
/// Transport buffers and the mock server's own copies account for most of it.
const LIMIT_RATIO: usize = 6;

fn definition(index: usize) -> String {
    let properties = (0..PROPERTIES)
        .map(|property| {
            format!(r#""field{property}":{{"type":"string","description":"A described field of this resource."}}"#)
        })
        .collect::<Vec<_>>()
        .join(",");
    let plural = if index == 0 {
        "widgets".to_owned()
    } else {
        format!("widgets{index}")
    };
    format!(
        r#"{{"metadata":{{"name":"{plural}.example.com","resourceVersion":"{index}"}},"spec":{{"group":"example.com","scope":"Namespaced",
        "names":{{"kind":"Widget","plural":"{plural}"}},
        "versions":[{{"name":"v1","served":true,"storage":true,
          "schema":{{"openAPIV3Schema":{{"type":"object","properties":{{{properties}}}}}}},
          "additionalPrinterColumns":[{{"name":"Size","type":"string","jsonPath":".spec.size"}}]}}]}}}}"#
    )
}

fn metadata_list() -> String {
    let items = (0..DEFINITIONS)
        .map(|index| {
            format!(r#"{{"metadata":{{"name":"widgets{index}.example.com","resourceVersion":"{index}"}}}}"#)
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadataList","metadata":{{}},"items":[{items}]}}"#
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn large_definition_schemas_are_not_built_in_memory() {
    let server = api_server().await;
    get(
        "/apis",
        json(
            r#"{"kind":"APIGroupList","apiVersion":"v1","groups":[{"name":"example.com",
        "versions":[{"groupVersion":"example.com/v1","version":"v1"}],
        "preferredVersion":{"groupVersion":"example.com/v1","version":"v1"}}]}"#,
        ),
    )
    .mount(&server)
    .await;
    get("/apis/example.com/v1", json(r#"{"kind":"APIResourceList","apiVersion":"v1","groupVersion":"example.com/v1","resources":[
        {"name":"widgets","singularName":"widget","namespaced":true,"kind":"Widget","verbs":["get","list","watch"]}]}"#)).mount(&server).await;
    let items = (0..DEFINITIONS)
        .map(definition)
        .collect::<Vec<_>>()
        .join(",");
    let body = format!(
        r#"{{"apiVersion":"apiextensions.k8s.io/v1","kind":"CustomResourceDefinitionList","metadata":{{}},"items":[{items}]}}"#
    );
    Mock::given(method("GET"))
        .and(path(
            "/apis/apiextensions.k8s.io/v1/customresourcedefinitions",
        ))
        .and(header_regex("accept", "PartialObjectMetadataList"))
        .respond_with(json(&metadata_list()))
        .with_priority(1)
        .mount(&server)
        .await;
    get(
        "/apis/apiextensions.k8s.io/v1/customresourcedefinitions",
        json(&body),
    )
    .mount(&server)
    .await;

    let dir = temp();
    let list = kubeconfig(dir.path(), &[("a", &server.uri(), Auth::None)]);
    let options = kubepulse_cluster::SessionOptions {
        discovery_interval: Duration::from_secs(3600),
        discovery_timeout: WAIT,
        ..fast_options()
    };
    let mut sessions = SessionManager::new(list_channel(list), options);

    let region = Region::new(ALLOCATOR);
    sessions.connect("a").unwrap();
    let mut resources = sessions.session("a").unwrap().subscribe_resources();
    time::timeout(
        WAIT,
        resources.wait_for(|kinds| {
            kinds
                .iter()
                .any(|kind| kind.kind == "Widget" && !kind.printer_columns.is_empty())
        }),
    )
    .await
    .expect("the columns should appear")
    .unwrap();
    let allocated = region.change().bytes_allocated;

    println!(
        "{} MiB allocated for a {} MiB response",
        allocated / (1024 * 1024),
        body.len() / (1024 * 1024)
    );
    assert!(
        allocated <= body.len() * LIMIT_RATIO,
        "{allocated} bytes allocated for a {} byte response",
        body.len()
    );
}
