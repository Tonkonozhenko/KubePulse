# Writing extensions

Every feature in KubePulse is an extension, including the built-in resource views. Built-ins use the contract in `kubepulse-extension-api`, the same one described here, and have no private access to the application.

For now, extensions are Rust crates compiled into the application. A sandboxed loader for third-party plugins will come later and carry the same messages, so the contract will not change. The contract crate is Apache-2.0, so you can choose your extension's license.

## A complete extension

This extension adds a column, a detail tab, and a palette command for pods. Its examples are compiled with the test suite, so they always match the contract.

```rust
use std::sync::Arc;

use async_trait::async_trait;
use kubepulse_extension_api::{
    ActivationTrigger, CONTRACT_VERSION, ColumnContribution, ColumnSource, CommandContribution,
    Contributions, DetailTabContribution, Description, Extension, ExtensionError, ExtensionRequest,
    ExtensionResponse, Host, HostOperation, HostResponse, KindRef, Manifest, Permission, UiNode, Verb,
};

struct PodInfo;

fn pods() -> KindRef {
    KindRef::new("", "Pod")
}

#[async_trait]
impl Extension for PodInfo {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: "example.pod-info".into(),
            name: "Pod info".into(),
            version: "1.0.0".into(),
            contract: CONTRACT_VERSION,
            activation: vec![ActivationTrigger::Kind { kind: pods() }],
            contributions: Contributions {
                columns: vec![ColumnContribution {
                    id: "qos".into(),
                    title: "QoS".into(),
                    kind: pods(),
                    source: ColumnSource::Path { path: ".status.qosClass".into() },
                }],
                detail_tabs: vec![DetailTabContribution {
                    id: "summary".into(),
                    title: "Summary".into(),
                    kind: pods(),
                }],
                commands: vec![CommandContribution {
                    id: "greet".into(),
                    title: "Say hello".into(),
                }],
                ..Default::default()
            },
            permissions: vec![Permission { kind: pods(), verbs: vec![Verb::Get] }],
            translations: Default::default(),
        }
    }

    async fn handle(
        &self,
        request: ExtensionRequest,
        host: Arc<dyn Host>,
    ) -> Result<ExtensionResponse, ExtensionError> {
        match request {
            ExtensionRequest::RenderDetailTab { target, .. } => {
                let HostResponse::Object { object } =
                    host.call(HostOperation::GetObject { target }).await?
                else {
                    return Err(ExtensionError::new("unexpected response"));
                };
                let node = object.object["spec"]["nodeName"].as_str().unwrap_or("none");
                let items = vec![Description { label: "Node".into(), value: node.into() }];
                Ok(ExtensionResponse::Ui { root: UiNode::DescriptionList { items } })
            }
            ExtensionRequest::RunCommand { .. } => {
                host.call(HostOperation::Notify { message: "Hello".into() }).await?;
                Ok(ExtensionResponse::Done)
            }
            _ => Err(ExtensionError::new("unsupported request")),
        }
    }
}
```

## The manifest

The host reads the manifest without running your code, so an inactive extension costs nothing beyond it.

| Field | Purpose |
|---|---|
| `id` | Unique across extensions. A duplicate is refused. |
| `contract` | Use `CONTRACT_VERSION`. A host with a different major version, or an older minor version, refuses the extension and the extension list says why. |
| `activation` | What wakes the extension. Without a trigger, it activates on its first request. |
| `contributions` | Everything it adds. |
| `permissions` | The verbs it needs, per resource kind. |
| `translations` | Optional. Translations of your titles and categories, by language. See [Translations](#translations). |

## Contribution points

| Contribution | Where it appears |
|---|---|
| `resource_views` | A table for a resource kind, and an entry in the kind sidebar under its `category` |
| `columns` | A column in that kind's tables |
| `detail_tabs` | A tab when the user opens an object of that kind |
| `actions` | An action on an object |
| `commands` | The command palette |
| `navigation` | The kind sidebar |
| `settings` | The settings screen |

Categories appear as groups in the order extensions load. Served kinds that no view declares go under Other resources, by API group.

A resource view can set `icon` to a `ViewIcon`, which the sidebar and tab titles show. The host defines the set, and extensions cannot supply images. A view with no icon shows the generic one, and so does an icon name this host does not know, so a manifest that names a newer icon still loads. The field needs contract 1.2.

Disabling an extension removes all of its contributions without a restart.

### Columns

Prefer `ColumnSource::Path`. The host evaluates the path, so your code never runs per row and the column cannot slow a table. `.status.phase`, `.spec.containers[0].image`, and `.metadata.labels['app.kubernetes.io/name']` are all valid paths.

Use `ColumnSource::Computed` only when a value needs your logic. The host requests visible rows in batches with `ExtensionRequest::ComputeColumn`, and shows a placeholder until your answer arrives. Return one cell per target, in order.

## Declarative UI

You return a tree of `UiNode` elements, and the host renders it with its own components, so your interface matches the application. You never touch the UI toolkit.

Logs, terminals, and editors are host-owned surfaces. Call `HostOperation::OpenSurface`, then place the returned reference in a `UiNode::Surface`. Their data stays in the host.

If a host does not know an element, that region shows a placeholder error and the rest of the interface still works.

## Translations

`translations` maps a language tag to a map from your declared text to its translation:

```rust
use std::collections::BTreeMap;

let german = BTreeMap::from([("Say hello".to_owned(), "Hallo sagen".to_owned())]);
let translations = BTreeMap::from([("de".to_owned(), german)]);
# let _: BTreeMap<String, BTreeMap<String, String>> = translations;
```

The host uses these for contribution titles and categories, and shows the declared text for anything without a translation. It matches the interface language by exact tag, then primary language, so `de` also serves `de-AT`. Views stay grouped by their declared category, so two extensions that share a category stay in one group even if only one translates it.

The host never changes the text inside your declarative interfaces, because that text can hold cluster data. To localize it, ask for the language with `HostOperation::GetLanguage`, which needs no permission and returns `HostResponse::Language`, then write your text in that language. The language stays the same until the application restarts.

## Reaching clusters

The `Host` handle is your only route to a cluster, and you never receive credentials or kubeconfig contents.

- **Permissions:** an operation you did not declare fails with `HostError::NotPermitted`. Built-ins follow the same rule.
- **Cluster authorization:** declaring a permission does not grant it. If the user's account lacks access, you get `HostError::ClusterDenied`, and you should handle that as a normal result.
- **Changes:** `HostOperation::RequestChange` shows the user the target and the diff, and nothing changes until they confirm. `ChangeOutcome::Declined` is a normal result.

## Failures

A panic, an error, or a call over its time limit counts as a failure. The default limits are 5 seconds for a call and 10 seconds for activation.

- The failed contribution shows an error, and the rest of the application keeps working.
- After 3 consecutive failures the extension is disabled, and the user can enable it again. A success resets the count.

Calls are asynchronous, so never block the thread. The host stops waiting at the time limit, but it cannot stop code stuck in a blocking call. Extensions run in the application's process, so a time limit is not memory isolation.
