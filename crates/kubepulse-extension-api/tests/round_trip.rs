//! Every contract type must survive serialization unchanged, because a sandboxed
//! loader will carry these values as messages.

use std::{collections::BTreeMap, fmt::Debug};

use kubepulse_extension_api::*;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::json;

fn round_trip<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: &T) {
    let encoded = serde_json::to_string(value).unwrap();
    let decoded: T = serde_json::from_str(&encoded).unwrap();
    assert_eq!(&decoded, value, "{encoded}");
}

fn all<T: Serialize + DeserializeOwned + PartialEq + Debug>(values: &[T]) {
    values.iter().for_each(round_trip);
}

fn kind() -> KindRef {
    KindRef::new("apps", "Deployment")
}

fn cluster() -> ClusterRef {
    ClusterRef("prod".into())
}

fn target() -> ObjectRef {
    ObjectRef {
        cluster: cluster(),
        kind: kind(),
        namespace: Some("web".into()),
        name: "api".into(),
    }
}

fn manifest() -> Manifest {
    Manifest {
        id: "example.all".into(),
        name: "Everything".into(),
        version: "1.2.3".into(),
        contract: CONTRACT_VERSION,
        translations: Default::default(),
        activation: vec![
            ActivationTrigger::Startup,
            ActivationTrigger::Kind { kind: kind() },
            ActivationTrigger::Command {
                command: "c".into(),
            },
        ],
        contributions: Contributions {
            resource_views: vec![ResourceViewContribution {
                id: "v".into(),
                title: "V".into(),
                kind: kind(),
                category: "C".into(),
                icon: ViewIcon::Box,
            }],
            columns: vec![
                ColumnContribution {
                    id: "c".into(),
                    title: "C".into(),
                    kind: kind(),
                    source: ColumnSource::Path {
                        path: ".status.phase".into(),
                    },
                },
                ColumnContribution {
                    id: "s".into(),
                    title: "S".into(),
                    kind: kind(),
                    source: ColumnSource::Status {
                        paths: vec![".status.reason".into(), ".status.phase".into()],
                    },
                },
                ColumnContribution {
                    id: "d".into(),
                    title: "D".into(),
                    kind: kind(),
                    source: ColumnSource::Computed,
                },
            ],
            detail_tabs: vec![DetailTabContribution {
                id: "t".into(),
                title: "T".into(),
                kind: kind(),
            }],
            actions: vec![ActionContribution {
                id: "a".into(),
                title: "A".into(),
                kind: kind(),
            }],
            commands: vec![CommandContribution {
                id: "c".into(),
                title: "C".into(),
            }],
            navigation: vec![NavigationContribution {
                id: "n".into(),
                title: "N".into(),
                view: "v".into(),
            }],
            settings: vec![
                SettingContribution {
                    id: "a".into(),
                    title: "A".into(),
                    kind: SettingKind::Toggle { default: true },
                },
                SettingContribution {
                    id: "b".into(),
                    title: "B".into(),
                    kind: SettingKind::Text {
                        default: "x".into(),
                    },
                },
                SettingContribution {
                    id: "c".into(),
                    title: "C".into(),
                    kind: SettingKind::Choice {
                        options: vec!["x".into()],
                        default: "x".into(),
                    },
                },
            ],
        },
        permissions: vec![Permission {
            kind: kind(),
            verbs: vec![
                Verb::Get,
                Verb::List,
                Verb::Create,
                Verb::Update,
                Verb::Delete,
                Verb::Logs,
                Verb::Exec,
                Verb::PortForward,
            ],
        }],
    }
}

fn ui() -> UiNode {
    let surface = |kind| Surface {
        kind,
        reference: SurfaceRef("s".into()),
    };
    UiNode::Stack {
        direction: Direction::Vertical,
        children: vec![
            UiNode::Text {
                text: "hello".into(),
            },
            UiNode::Stack {
                direction: Direction::Horizontal,
                children: vec![],
            },
            UiNode::Table {
                table: Table {
                    columns: vec!["A".into()],
                    rows: vec![vec!["1".into()]],
                },
            },
            UiNode::DescriptionList {
                items: vec![Description {
                    label: "L".into(),
                    value: "V".into(),
                }],
            },
            UiNode::Form {
                id: "f".into(),
                submit_label: "Save".into(),
                fields: vec![
                    FormField {
                        id: "a".into(),
                        label: "A".into(),
                        kind: FormFieldKind::Text { value: "x".into() },
                    },
                    FormField {
                        id: "b".into(),
                        label: "B".into(),
                        kind: FormFieldKind::Toggle { value: true },
                    },
                    FormField {
                        id: "c".into(),
                        label: "C".into(),
                        kind: FormFieldKind::Choice {
                            options: vec!["x".into()],
                            value: "x".into(),
                        },
                    },
                ],
            },
            UiNode::Chart {
                chart: Chart {
                    title: "T".into(),
                    series: vec![ChartSeries {
                        name: "s".into(),
                        points: vec![(0.0, 1.5)],
                    }],
                },
            },
            UiNode::Surface {
                surface: surface(SurfaceKind::Log),
            },
            UiNode::Surface {
                surface: surface(SurfaceKind::Terminal),
            },
            UiNode::Surface {
                surface: surface(SurfaceKind::Editor),
            },
            UiNode::Unknown,
        ],
    }
}

#[test]
fn manifest_types() {
    round_trip(&manifest());
    round_trip(&CONTRACT_VERSION);
    round_trip(&kind());
}

#[test]
fn ui_types() {
    round_trip(&ui());
}

#[test]
fn extension_messages() {
    all(&[
        ExtensionRequest::RenderView { view: "v".into() },
        ExtensionRequest::RenderDetailTab {
            tab: "t".into(),
            target: target(),
        },
        ExtensionRequest::RunAction {
            action: "a".into(),
            target: target(),
        },
        ExtensionRequest::RunCommand {
            command: "c".into(),
        },
        ExtensionRequest::ComputeColumn {
            column: "c".into(),
            targets: vec![target()],
        },
        ExtensionRequest::SubmitForm {
            form: "f".into(),
            values: BTreeMap::from([("a".into(), "b".into())]),
        },
    ]);
    all(&[
        ExtensionResponse::Ui { root: ui() },
        ExtensionResponse::Cells {
            cells: vec!["x".into()],
        },
        ExtensionResponse::Done,
    ]);
    round_trip(&ExtensionError::new("failed"));
}

#[test]
fn host_messages() {
    let manifest_value = json!({"kind": "Deployment"});
    all(&[
        HostOperation::GetObject { target: target() },
        HostOperation::ListObjects {
            cluster: cluster(),
            kind: kind(),
            namespace: None,
            labels: BTreeMap::from([("app".into(), "web".into())]),
        },
        HostOperation::RequestChange {
            request: ChangeRequest::Create {
                cluster: cluster(),
                kind: kind(),
                manifest: manifest_value.clone(),
            },
        },
        HostOperation::RequestChange {
            request: ChangeRequest::Update {
                target: target(),
                manifest: manifest_value.clone(),
            },
        },
        HostOperation::RequestChange {
            request: ChangeRequest::Delete { target: target() },
        },
        HostOperation::OpenSurface {
            request: SurfaceRequest::Logs {
                target: target(),
                container: Some("app".into()),
            },
        },
        HostOperation::OpenSurface {
            request: SurfaceRequest::Terminal {
                target: target(),
                container: None,
            },
        },
        HostOperation::OpenSurface {
            request: SurfaceRequest::Editor { target: target() },
        },
        HostOperation::Notify {
            message: "hi".into(),
        },
    ]);
    let object = ResourceObject {
        reference: target(),
        object: manifest_value,
    };
    all(&[
        HostResponse::Object {
            object: object.clone(),
        },
        HostResponse::Objects {
            objects: vec![object],
        },
        HostResponse::Change {
            outcome: ChangeOutcome::Applied,
        },
        HostResponse::Change {
            outcome: ChangeOutcome::Declined,
        },
        HostResponse::Change {
            outcome: ChangeOutcome::Failed { reason: "r".into() },
        },
        HostResponse::Surface {
            reference: SurfaceRef("s".into()),
        },
        HostResponse::Done,
    ]);
    all(&[
        HostError::NotPermitted {
            kind: Some(kind()),
            verb: Some(Verb::Delete),
        },
        HostError::NotPermitted {
            kind: None,
            verb: None,
        },
        HostError::ClusterDenied {
            message: "m".into(),
        },
        HostError::NotFound {
            message: "m".into(),
        },
        HostError::Failed {
            message: "m".into(),
        },
    ]);
}

#[test]
fn an_unknown_element_decodes_without_losing_its_siblings() {
    let encoded = r#"{"element":"stack","direction":"vertical","children":[
        {"element":"hologram","depth":3},{"element":"text","text":"still here"}]}"#;
    let decoded: UiNode = serde_json::from_str(encoded).unwrap();
    assert_eq!(
        decoded,
        UiNode::Stack {
            direction: Direction::Vertical,
            children: vec![
                UiNode::Unknown,
                UiNode::Text {
                    text: "still here".into()
                }
            ],
        }
    );
}

#[test]
fn a_manifest_needs_only_its_identity() {
    let manifest: Manifest = serde_json::from_str(
        r#"{"id":"a","name":"A","version":"1","contract":{"major":1,"minor":0}}"#,
    )
    .unwrap();
    assert_eq!(manifest.contributions, Contributions::default());
    assert!(manifest.activation.is_empty());
}

#[test]
fn contract_versions_match_on_major_and_accept_older_minors() {
    let host = ContractVersion { major: 1, minor: 2 };
    assert!(host.supports(ContractVersion { major: 1, minor: 0 }));
    assert!(host.supports(ContractVersion { major: 1, minor: 2 }));
    assert!(!host.supports(ContractVersion { major: 1, minor: 3 }));
    assert!(!host.supports(ContractVersion { major: 2, minor: 0 }));
}

#[test]
fn every_cluster_operation_names_its_permission() {
    let cases = [
        (HostOperation::GetObject { target: target() }, Verb::Get),
        (
            HostOperation::ListObjects {
                cluster: cluster(),
                kind: kind(),
                namespace: None,
                labels: BTreeMap::new(),
            },
            Verb::List,
        ),
        (
            HostOperation::RequestChange {
                request: ChangeRequest::Create {
                    cluster: cluster(),
                    kind: kind(),
                    manifest: json!({}),
                },
            },
            Verb::Create,
        ),
        (
            HostOperation::RequestChange {
                request: ChangeRequest::Update {
                    target: target(),
                    manifest: json!({}),
                },
            },
            Verb::Update,
        ),
        (
            HostOperation::RequestChange {
                request: ChangeRequest::Delete { target: target() },
            },
            Verb::Delete,
        ),
        (
            HostOperation::OpenSurface {
                request: SurfaceRequest::Logs {
                    target: target(),
                    container: None,
                },
            },
            Verb::Logs,
        ),
        (
            HostOperation::OpenSurface {
                request: SurfaceRequest::Terminal {
                    target: target(),
                    container: None,
                },
            },
            Verb::Exec,
        ),
        (
            HostOperation::OpenSurface {
                request: SurfaceRequest::Editor { target: target() },
            },
            Verb::Get,
        ),
    ];
    for (operation, verb) in cases {
        assert_eq!(operation.required_permission(), Some((&kind(), verb)));
    }
    assert_eq!(
        HostOperation::Notify {
            message: "m".into()
        }
        .required_permission(),
        None
    );
}

#[test]
fn a_manifest_without_translations_still_parses() {
    let mut value = serde_json::to_value(manifest()).unwrap();
    value.as_object_mut().unwrap().remove("translations");
    value["contract"] = serde_json::json!({ "major": 1, "minor": 0 });
    let parsed: Manifest = serde_json::from_value(value).unwrap();
    assert!(parsed.translations.is_empty());
    assert!(CONTRACT_VERSION.supports(parsed.contract));
}

#[test]
fn the_language_messages_round_trip() {
    round_trip(&HostOperation::GetLanguage);
    round_trip(&HostResponse::Language { tag: "de".into() });
}

#[test]
fn a_view_without_an_icon_is_generic() {
    let view: ResourceViewContribution = serde_json::from_value(json!({
        "id": "v", "title": "V", "kind": { "group": "", "kind": "Pod" },
    }))
    .unwrap();
    assert_eq!(view.icon, ViewIcon::Generic);
}

#[test]
fn an_unknown_icon_name_is_generic() {
    let icon: ViewIcon = serde_json::from_value(json!("from-a-newer-contract")).unwrap();
    assert_eq!(icon, ViewIcon::Generic);
}

#[test]
fn icon_names_are_kebab_case_and_survive_a_round_trip() {
    assert_eq!(
        serde_json::to_value(ViewIcon::SettingsFile).unwrap(),
        json!("settings-file")
    );
    all(&[ViewIcon::Box, ViewIcon::StorageClass, ViewIcon::Generic]);
}
