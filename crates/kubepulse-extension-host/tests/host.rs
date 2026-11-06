use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use kubepulse_extension_api::*;
use kubepulse_extension_host::*;
use serde_json::json;

/// How a test extension handles requests.
#[derive(Clone, Copy)]
enum Behavior {
    Succeed,
    Fail,
    Panic,
    Hang,
    Block,
}

struct TestExtension {
    manifest: Manifest,
    behavior: Mutex<Behavior>,
    activation_behavior: Behavior,
    activations: AtomicUsize,
    deactivations: AtomicUsize,
    handled: AtomicUsize,
    operation: Mutex<Option<HostOperation>>,
    last_host_result: Mutex<Option<Result<HostResponse, HostError>>>,
}

fn pods() -> KindRef {
    KindRef::new("", "Pod")
}

fn manifest(id: &str) -> Manifest {
    Manifest {
        id: id.to_owned(),
        name: id.to_uppercase(),
        version: "1.0.0".into(),
        contract: CONTRACT_VERSION,
        translations: Default::default(),
        activation: vec![],
        contributions: Contributions {
            commands: vec![CommandContribution {
                id: format!("{id}.run"),
                title: "Run".into(),
            }],
            ..Default::default()
        },
        permissions: vec![],
    }
}

impl TestExtension {
    fn new(manifest: Manifest) -> Arc<Self> {
        Self::with(manifest, Behavior::Succeed, Behavior::Succeed)
    }

    fn with(manifest: Manifest, behavior: Behavior, activation_behavior: Behavior) -> Arc<Self> {
        Arc::new(Self {
            manifest,
            behavior: Mutex::new(behavior),
            activation_behavior,
            activations: AtomicUsize::new(0),
            deactivations: AtomicUsize::new(0),
            handled: AtomicUsize::new(0),
            operation: Mutex::new(None),
            last_host_result: Mutex::new(None),
        })
    }
}

async fn behave(behavior: Behavior) -> Result<(), ExtensionError> {
    match behavior {
        Behavior::Succeed => Ok(()),
        Behavior::Fail => Err(ExtensionError::new("it failed")),
        Behavior::Panic => panic!("deliberate test panic"),
        Behavior::Hang => std::future::pending().await,
        Behavior::Block => {
            std::thread::sleep(Duration::from_millis(400));
            Ok(())
        }
    }
}

#[async_trait]
impl Extension for TestExtension {
    fn manifest(&self) -> Manifest {
        self.manifest.clone()
    }

    async fn activate(&self, _: Arc<dyn Host>) -> Result<(), ExtensionError> {
        self.activations.fetch_add(1, Ordering::SeqCst);
        behave(self.activation_behavior).await
    }

    async fn handle(
        &self,
        _: ExtensionRequest,
        host: Arc<dyn Host>,
    ) -> Result<ExtensionResponse, ExtensionError> {
        self.handled.fetch_add(1, Ordering::SeqCst);
        let operation = self.operation.lock().unwrap().clone();
        if let Some(operation) = operation {
            let result = host.call(operation).await;
            *self.last_host_result.lock().unwrap() = Some(result);
        }
        let behavior = *self.behavior.lock().unwrap();
        behave(behavior).await?;
        Ok(ExtensionResponse::Done)
    }

    async fn deactivate(&self) {
        self.deactivations.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Default)]
struct FakeBackend {
    executed: Mutex<Vec<HostOperation>>,
    deny: bool,
    /// How long each operation takes, as a user's review would.
    delay: Duration,
}

#[async_trait]
impl HostBackend for FakeBackend {
    async fn execute(&self, operation: HostOperation) -> Result<HostResponse, HostError> {
        self.executed.lock().unwrap().push(operation);
        tokio::time::sleep(self.delay).await;
        if self.deny {
            return Err(HostError::ClusterDenied {
                message: "pods is forbidden".into(),
            });
        }
        Ok(HostResponse::Done)
    }
}

fn options() -> HostOptions {
    HostOptions {
        call_timeout: Duration::from_millis(100),
        activation_timeout: Duration::from_millis(100),
        ..Default::default()
    }
}

fn host_with(extensions: Vec<Arc<dyn Extension>>) -> (ExtensionHost, Arc<FakeBackend>) {
    let backend = Arc::new(FakeBackend::default());
    let host = ExtensionHost::new(backend.clone(), options());
    host.load(extensions);
    (host, backend)
}

fn command() -> ExtensionRequest {
    ExtensionRequest::RunCommand {
        command: "run".into(),
    }
}

fn status(host: &ExtensionHost, id: &str) -> ExtensionStatus {
    host.extensions()
        .into_iter()
        .find(|e| e.id == id)
        .unwrap()
        .status
}

fn contributed(host: &ExtensionHost) -> Vec<String> {
    let set = host.contributions().borrow().clone();
    set.extensions.iter().map(|(id, _)| id.clone()).collect()
}

fn target() -> ObjectRef {
    ObjectRef {
        cluster: ClusterRef("prod".into()),
        kind: pods(),
        namespace: Some("web".into()),
        name: "api".into(),
    }
}

// Registry

#[tokio::test]
async fn contributions_appear_and_disappear_without_a_restart() {
    let (host, _) = host_with(vec![
        TestExtension::new(manifest("a")),
        TestExtension::new(manifest("b")),
    ]);
    let mut changes = host.contributions();
    assert_eq!(contributed(&host), ["a", "b"]);
    changes.borrow_and_update();

    host.set_enabled("a", false).await.unwrap();
    assert!(changes.has_changed().unwrap());
    assert_eq!(contributed(&host), ["b"]);
    assert_eq!(status(&host, "a"), ExtensionStatus::Disabled);

    host.set_enabled("a", true).await.unwrap();
    assert_eq!(contributed(&host), ["a", "b"]);
    assert_eq!(status(&host, "a"), ExtensionStatus::Inactive);
}

#[tokio::test]
async fn disabling_an_active_extension_deactivates_it() {
    let extension = TestExtension::new(manifest("a"));
    let (host, _) = host_with(vec![extension.clone()]);
    host.request("a", command()).await.unwrap();
    host.set_enabled("a", false).await.unwrap();
    assert_eq!(extension.deactivations.load(Ordering::SeqCst), 1);
    assert_eq!(
        host.request("a", command()).await,
        Err(CallError::Unavailable)
    );
}

#[tokio::test]
async fn a_duplicate_id_is_refused_and_the_first_keeps_working() {
    let first = TestExtension::new(manifest("a"));
    let (host, _) = host_with(vec![first.clone(), TestExtension::new(manifest("a"))]);
    let statuses: Vec<_> = host.extensions().into_iter().map(|e| e.status).collect();
    assert_eq!(statuses[0], ExtensionStatus::Inactive);
    assert!(matches!(statuses[1], ExtensionStatus::Refused { .. }));

    host.request("a", command()).await.unwrap();
    assert_eq!(first.handled.load(Ordering::SeqCst), 1);
}

// Permissions

fn get_operation() -> HostOperation {
    HostOperation::GetObject { target: target() }
}

fn permitted(verbs: Vec<Verb>) -> Manifest {
    Manifest {
        permissions: vec![Permission {
            kind: pods(),
            verbs,
        }],
        ..manifest("a")
    }
}

#[tokio::test]
async fn undeclared_operations_are_refused_and_reported() {
    let extension = TestExtension::new(permitted(vec![Verb::List]));
    *extension.operation.lock().unwrap() = Some(get_operation());
    let (host, backend) = host_with(vec![extension.clone()]);

    host.request("a", command()).await.unwrap();

    let result = extension.last_host_result.lock().unwrap().clone().unwrap();
    assert_eq!(
        result,
        Err(HostError::NotPermitted {
            kind: Some(pods()),
            verb: Some(Verb::Get)
        })
    );
    assert!(backend.executed.lock().unwrap().is_empty());
    assert_eq!(host.extensions()[0].refused_operations, 1);
}

#[tokio::test]
async fn a_permission_on_another_kind_does_not_count() {
    let manifest = Manifest {
        permissions: vec![Permission {
            kind: KindRef::new("apps", "Pod"),
            verbs: vec![Verb::Get],
        }],
        ..manifest("a")
    };
    let extension = TestExtension::new(manifest);
    *extension.operation.lock().unwrap() = Some(get_operation());
    let (host, backend) = host_with(vec![extension.clone()]);
    host.request("a", command()).await.unwrap();
    assert!(backend.executed.lock().unwrap().is_empty());
}

#[tokio::test]
async fn declared_operations_reach_the_backend() {
    let extension = TestExtension::new(permitted(vec![Verb::Get]));
    *extension.operation.lock().unwrap() = Some(get_operation());
    let (host, backend) = host_with(vec![extension.clone()]);

    host.request("a", command()).await.unwrap();
    assert_eq!(
        *extension.last_host_result.lock().unwrap(),
        Some(Ok(HostResponse::Done))
    );
    assert_eq!(backend.executed.lock().unwrap().len(), 1);
    assert_eq!(host.extensions()[0].refused_operations, 0);
}

#[tokio::test]
async fn cluster_denials_are_returned_to_the_extension() {
    let extension = TestExtension::new(permitted(vec![Verb::Get]));
    *extension.operation.lock().unwrap() = Some(get_operation());
    let backend = Arc::new(FakeBackend {
        deny: true,
        ..Default::default()
    });
    let host = ExtensionHost::new(backend, options());
    host.load(vec![extension.clone()]);

    host.request("a", command()).await.unwrap();
    let result = extension.last_host_result.lock().unwrap().clone().unwrap();
    assert_eq!(
        result,
        Err(HostError::ClusterDenied {
            message: "pods is forbidden".into()
        })
    );
}

#[tokio::test]
async fn cluster_changes_go_to_the_backend_for_review_only_with_permission() {
    let change = HostOperation::RequestChange {
        request: ChangeRequest::Update {
            target: target(),
            manifest: json!({}),
        },
    };
    let extension = TestExtension::new(permitted(vec![Verb::Get]));
    *extension.operation.lock().unwrap() = Some(change.clone());
    let (host, backend) = host_with(vec![extension]);
    host.request("a", command()).await.unwrap();
    assert!(backend.executed.lock().unwrap().is_empty());

    let extension = TestExtension::new(permitted(vec![Verb::Update]));
    *extension.operation.lock().unwrap() = Some(change.clone());
    let (host, backend) = host_with(vec![extension]);
    host.request("a", command()).await.unwrap();
    assert_eq!(*backend.executed.lock().unwrap(), [change]);
}

#[tokio::test]
async fn notifying_needs_no_permission() {
    let extension = TestExtension::new(manifest("a"));
    *extension.operation.lock().unwrap() = Some(HostOperation::Notify {
        message: "hi".into(),
    });
    let (host, backend) = host_with(vec![extension]);
    host.request("a", command()).await.unwrap();
    assert_eq!(backend.executed.lock().unwrap().len(), 1);
}

// Activation

fn triggered_by(id: &str, trigger: ActivationTrigger) -> Manifest {
    Manifest {
        activation: vec![trigger],
        ..manifest(id)
    }
}

#[tokio::test]
async fn stays_inactive_until_its_trigger_fires() {
    let widgets = KindRef::new("example.com", "Widget");
    let trigger = ActivationTrigger::Kind { kind: widgets };
    let extension = TestExtension::new(triggered_by("a", trigger.clone()));
    let (host, _) = host_with(vec![extension.clone()]);

    assert_eq!(status(&host, "a"), ExtensionStatus::Inactive);
    host.trigger(&ActivationTrigger::Startup).await;
    host.trigger(&ActivationTrigger::Kind { kind: pods() })
        .await;
    assert_eq!(status(&host, "a"), ExtensionStatus::Inactive);
    assert_eq!(extension.activations.load(Ordering::SeqCst), 0);

    host.trigger(&trigger).await;
    assert_eq!(status(&host, "a"), ExtensionStatus::Active);
    assert_eq!(extension.activations.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn an_inactive_extension_still_contributes_from_its_manifest() {
    let extension = TestExtension::new(manifest("a"));
    let (host, _) = host_with(vec![extension.clone()]);
    assert_eq!(contributed(&host), ["a"]);
    assert_eq!(extension.activations.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn concurrent_requests_activate_once() {
    let extension = TestExtension::new(manifest("a"));
    let (host, _) = host_with(vec![extension.clone()]);
    let calls = (0..8).map(|_| {
        let host = host.clone();
        tokio::spawn(async move { host.request("a", command()).await })
    });
    for call in calls.collect::<Vec<_>>() {
        call.await.unwrap().unwrap();
    }
    assert_eq!(extension.activations.load(Ordering::SeqCst), 1);
    assert_eq!(extension.handled.load(Ordering::SeqCst), 8);
}

#[tokio::test]
async fn a_slow_activation_does_not_delay_another_extension() {
    let trigger = ActivationTrigger::Startup;
    let slow = TestExtension::with(
        triggered_by("slow", trigger.clone()),
        Behavior::Succeed,
        Behavior::Hang,
    );
    let fast = TestExtension::new(triggered_by("fast", trigger.clone()));
    let (host, _) = host_with(vec![slow, fast]);

    host.trigger(&trigger).await;
    assert_eq!(status(&host, "fast"), ExtensionStatus::Active);
    assert_eq!(status(&host, "slow"), ExtensionStatus::Inactive);
}

// Failure handling

#[tokio::test(flavor = "multi_thread")]
async fn a_panic_leaves_the_host_and_other_extensions_running() {
    let panicking = TestExtension::with(manifest("bad"), Behavior::Panic, Behavior::Succeed);
    let healthy = TestExtension::new(manifest("good"));
    let (host, _) = host_with(vec![panicking, healthy.clone()]);

    let failure = host.request("bad", command()).await;
    assert_eq!(
        failure,
        Err(CallError::Failed {
            message: "the extension panicked".into()
        })
    );

    host.request("good", command()).await.unwrap();
    assert_eq!(healthy.handled.load(Ordering::SeqCst), 1);
    assert_eq!(status(&host, "bad"), ExtensionStatus::Active);
}

#[tokio::test]
async fn a_hung_call_hits_its_time_limit() {
    let hanging = TestExtension::with(manifest("a"), Behavior::Hang, Behavior::Succeed);
    let (host, _) = host_with(vec![hanging]);
    let failure = host.request("a", command()).await.unwrap_err();
    assert!(failure.to_string().contains("did not respond"), "{failure}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_call_blocking_its_thread_does_not_delay_the_host() {
    let blocking = TestExtension::with(manifest("bad"), Behavior::Block, Behavior::Succeed);
    let healthy = TestExtension::new(manifest("good"));
    let (host, _) = host_with(vec![blocking, healthy]);

    let started = std::time::Instant::now();
    let failure = host.request("bad", command()).await;
    assert!(failure.is_err());
    assert!(
        started.elapsed() < Duration::from_millis(300),
        "{:?}",
        started.elapsed()
    );
    host.request("good", command()).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn repeated_failures_disable_the_extension_and_remove_its_contributions() {
    let failing = TestExtension::with(manifest("bad"), Behavior::Panic, Behavior::Succeed);
    let (host, _) = host_with(vec![failing.clone(), TestExtension::new(manifest("good"))]);

    for _ in 0..3 {
        assert!(matches!(
            host.request("bad", command()).await,
            Err(CallError::Failed { .. })
        ));
    }
    assert_eq!(
        status(&host, "bad"),
        ExtensionStatus::Failed {
            reason: "the extension panicked".into()
        }
    );
    assert_eq!(contributed(&host), ["good"]);

    assert_eq!(
        host.request("bad", command()).await,
        Err(CallError::Unavailable)
    );
    assert_eq!(failing.handled.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn a_success_resets_the_failure_count() {
    let extension = TestExtension::new(manifest("a"));
    let (host, _) = host_with(vec![extension.clone()]);
    for _ in 0..5 {
        *extension.behavior.lock().unwrap() = Behavior::Fail;
        let _ = host.request("a", command()).await;
        let _ = host.request("a", command()).await;
        *extension.behavior.lock().unwrap() = Behavior::Succeed;
        host.request("a", command()).await.unwrap();
    }
    assert_eq!(status(&host, "a"), ExtensionStatus::Active);
}

#[tokio::test]
async fn repeated_activation_failures_also_disable_it() {
    let extension = TestExtension::with(manifest("a"), Behavior::Succeed, Behavior::Fail);
    let (host, _) = host_with(vec![extension.clone()]);
    for _ in 0..3 {
        assert!(host.request("a", command()).await.is_err());
    }
    assert!(matches!(status(&host, "a"), ExtensionStatus::Failed { .. }));
    assert_eq!(extension.handled.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn enabling_a_failed_extension_gives_it_another_chance() {
    let extension = TestExtension::with(manifest("a"), Behavior::Fail, Behavior::Succeed);
    let (host, _) = host_with(vec![extension.clone()]);
    for _ in 0..3 {
        let _ = host.request("a", command()).await;
    }
    *extension.behavior.lock().unwrap() = Behavior::Succeed;
    host.set_enabled("a", true).await.unwrap();
    host.request("a", command()).await.unwrap();
    assert_eq!(status(&host, "a"), ExtensionStatus::Active);
    assert_eq!(contributed(&host), ["a"]);
}

// Contract versions

#[tokio::test]
async fn an_incompatible_manifest_is_refused_with_a_clear_message() {
    let future = ContractVersion {
        major: CONTRACT_VERSION.major + 1,
        minor: 0,
    };
    let extension = TestExtension::new(Manifest {
        contract: future,
        ..manifest("a")
    });
    let (host, _) = host_with(vec![extension.clone()]);

    let ExtensionStatus::Refused { reason } = status(&host, "a") else {
        panic!("should be refused")
    };
    assert_eq!(
        reason,
        "built for contract 2.0, but this application provides 1.2"
    );
    assert!(contributed(&host).is_empty());
    assert_eq!(
        host.request("a", command()).await,
        Err(CallError::UnknownExtension)
    );
    assert_eq!(extension.activations.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_refused_extension_cannot_be_enabled() {
    let future = ContractVersion {
        major: CONTRACT_VERSION.major,
        minor: CONTRACT_VERSION.minor + 1,
    };
    let (host, _) = host_with(vec![TestExtension::new(Manifest {
        contract: future,
        ..manifest("a")
    })]);
    assert!(host.set_enabled("a", true).await.is_err());
    assert!(matches!(
        status(&host, "a"),
        ExtensionStatus::Refused { .. }
    ));
}

fn translated_manifest(id: &str, category: &str, translated: &[(&str, &str, &str)]) -> Manifest {
    let mut translations =
        std::collections::BTreeMap::<String, std::collections::BTreeMap<_, _>>::new();
    for (tag, text, translation) in translated {
        translations
            .entry((*tag).to_owned())
            .or_default()
            .insert((*text).to_owned(), (*translation).to_owned());
    }
    Manifest {
        translations,
        contributions: Contributions {
            resource_views: vec![
                ResourceViewContribution {
                    id: "pods".into(),
                    title: "Pods".into(),
                    kind: pods(),
                    category: category.into(),
                    icon: ViewIcon::default(),
                },
                ResourceViewContribution {
                    id: "jobs".into(),
                    title: "Jobs".into(),
                    kind: KindRef::new("batch", "Job"),
                    category: category.into(),
                    icon: ViewIcon::default(),
                },
            ],
            commands: vec![CommandContribution {
                id: "run".into(),
                title: "Run".into(),
            }],
            ..Contributions::default()
        },
        ..manifest(id)
    }
}

fn host_in(language: &str, extensions: Vec<Arc<dyn Extension>>) -> ExtensionHost {
    let host = ExtensionHost::new(
        Arc::new(FakeBackend::default()),
        HostOptions {
            language: language.to_owned(),
            ..options()
        },
    );
    host.load(extensions);
    host
}

fn view_titles(host: &ExtensionHost) -> Vec<String> {
    host.contributions().borrow().extensions[0]
        .1
        .resource_views
        .iter()
        .map(|view| view.title.clone())
        .collect()
}

#[tokio::test]
async fn titles_use_the_translation_for_the_interface_language() {
    let manifest = translated_manifest("a", "Workloads", &[("de", "Pods", "Pods-de")]);
    let host = host_in("de", vec![TestExtension::new(manifest)]);
    assert_eq!(view_titles(&host)[0], "Pods-de");
}

#[tokio::test]
async fn a_regional_language_uses_the_primary_translation() {
    let manifest = translated_manifest("a", "Workloads", &[("de", "Pods", "Pods-de")]);
    let host = host_in("de-AT", vec![TestExtension::new(manifest)]);
    assert_eq!(view_titles(&host)[0], "Pods-de");
}

#[tokio::test]
async fn text_with_no_translation_stays_as_declared() {
    let manifest = translated_manifest("a", "Workloads", &[("de", "Pods", "Pods-de")]);
    let host = host_in("de", vec![TestExtension::new(manifest)]);
    assert_eq!(view_titles(&host), ["Pods-de", "Jobs"]);

    let manifest = translated_manifest("b", "Workloads", &[("de", "Pods", "Pods-de")]);
    let host = host_in("fr", vec![TestExtension::new(manifest)]);
    assert_eq!(view_titles(&host), ["Pods", "Jobs"], "no catalog for fr");
}

#[tokio::test]
async fn every_contribution_title_is_translated() {
    let manifest = translated_manifest("a", "Workloads", &[("de", "Run", "Run-de")]);
    let host = host_in("de", vec![TestExtension::new(manifest)]);
    let set = host.contributions().borrow().clone();
    assert_eq!(set.extensions[0].1.commands[0].title, "Run-de");
}

#[tokio::test]
async fn titles_stay_translated_after_disabling_an_extension() {
    let a = translated_manifest("a", "Workloads", &[("de", "Pods", "Pods-de")]);
    let b = translated_manifest("b", "Other", &[("de", "Pods", "Pods-b")]);
    let extensions: Vec<Arc<dyn Extension>> = vec![TestExtension::new(a), TestExtension::new(b)];
    let host = host_in("de", extensions);
    host.set_enabled("a", false).await.unwrap();
    assert_eq!(view_titles(&host)[0], "Pods-b");
}

#[tokio::test]
async fn a_shared_category_keeps_its_declared_name_and_the_first_translation() {
    let a = translated_manifest("a", "Workloads", &[]);
    let b = translated_manifest("b", "Workloads", &[("de", "Workloads", "Arbeitslasten")]);
    let extensions: Vec<Arc<dyn Extension>> = vec![TestExtension::new(a), TestExtension::new(b)];
    let host = host_in("de", extensions);
    let set = host.contributions().borrow().clone();
    assert_eq!(set.extensions[0].1.resource_views[0].category, "Workloads");
    assert_eq!(set.extensions[1].1.resource_views[0].category, "Workloads");
    assert_eq!(set.category_titles["Workloads"], "Arbeitslasten");
}

#[tokio::test]
async fn an_extension_with_no_permissions_can_read_the_language() {
    let extension = TestExtension::new(manifest("a"));
    *extension.operation.lock().unwrap() = Some(HostOperation::GetLanguage);
    let host = host_in("de", vec![extension.clone()]);
    host.request("a", command()).await.unwrap();
    assert_eq!(
        *extension.last_host_result.lock().unwrap(),
        Some(Ok(HostResponse::Language { tag: "de".into() }))
    );
}

#[tokio::test]
async fn a_newer_minor_contract_is_refused_by_an_older_host() {
    let host = ExtensionHost::new(
        Arc::new(FakeBackend::default()),
        HostOptions {
            contract: ContractVersion { major: 1, minor: 0 },
            ..options()
        },
    );
    host.load(vec![TestExtension::new(manifest("a"))]);
    let ExtensionStatus::Refused { reason } = status(&host, "a") else {
        panic!("should be refused")
    };
    assert_eq!(
        reason,
        "built for contract 1.2, but this application provides 1.0"
    );
}

struct Echo(Manifest);

#[async_trait]
impl Extension for Echo {
    fn manifest(&self) -> Manifest {
        self.0.clone()
    }

    async fn handle(
        &self,
        _: ExtensionRequest,
        _: Arc<dyn Host>,
    ) -> Result<ExtensionResponse, ExtensionError> {
        Ok(ExtensionResponse::Ui {
            root: UiNode::Text {
                text: "Pods".into(),
            },
        })
    }
}

#[tokio::test]
async fn interface_text_is_not_rewritten_even_when_it_matches_a_translation() {
    let manifest = translated_manifest("a", "Workloads", &[("de", "Pods", "Pods-de")]);
    let extension: Arc<dyn Extension> = Arc::new(Echo(manifest));
    let host = host_in("de", vec![extension]);
    let response = host.request("a", command()).await.unwrap();
    assert_eq!(
        response,
        ExtensionResponse::Ui {
            root: UiNode::Text {
                text: "Pods".into()
            }
        }
    );
}

fn change() -> HostOperation {
    HostOperation::RequestChange {
        request: ChangeRequest::Delete {
            target: ObjectRef {
                cluster: ClusterRef("a".into()),
                kind: pods(),
                namespace: Some("default".into()),
                name: "web".into(),
            },
        },
    }
}

fn reviewing(operation: HostOperation) -> (ExtensionHost, Arc<TestExtension>) {
    let mut declared = manifest("reviewer");
    declared.permissions = vec![Permission {
        kind: pods(),
        verbs: vec![Verb::Delete, Verb::Get],
    }];
    let extension = TestExtension::with(declared, Behavior::Succeed, Behavior::Succeed);
    *extension.operation.lock().unwrap() = Some(operation);
    let backend = Arc::new(FakeBackend {
        delay: Duration::from_millis(350),
        ..FakeBackend::default()
    });
    let host = ExtensionHost::new(backend, options());
    host.load(vec![extension.clone()]);
    (host, extension)
}

#[tokio::test]
async fn a_users_review_does_not_count_against_the_call_limit() {
    let (host, extension) = reviewing(change());

    let result = host
        .request(
            "reviewer",
            ExtensionRequest::RunCommand {
                command: "x".into(),
            },
        )
        .await;

    assert_eq!(result, Ok(ExtensionResponse::Done));
    assert_eq!(
        *extension.last_host_result.lock().unwrap(),
        Some(Ok(HostResponse::Done))
    );
}

#[tokio::test]
async fn any_other_slow_operation_still_counts() {
    let target = ObjectRef {
        cluster: ClusterRef("a".into()),
        kind: pods(),
        namespace: None,
        name: "web".into(),
    };
    let (host, _extension) = reviewing(HostOperation::GetObject { target });

    let result = host
        .request(
            "reviewer",
            ExtensionRequest::RunCommand {
                command: "x".into(),
            },
        )
        .await;

    assert!(
        matches!(result, Err(CallError::Failed { .. })),
        "{result:?}"
    );
}
