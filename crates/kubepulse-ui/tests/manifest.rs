mod support;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use gpui_kit::{
    AppContext as _, Entity, Focusable as _, MouseButton, SharedString, TestAppContext,
    VisualTestContext, WindowOptions,
    component::ActiveTheme as _,
    component::highlighter::LanguageRegistry,
    test::{ClickOptions, TestWindowExt as _},
};
use kubepulse_cluster::{
    ConnectionState, ObjectError, ObjectTarget, ResourceKind, UpdateAccess, WriteMode, manifest,
};
use kubepulse_extension_api::{
    CONTRACT_VERSION, ChangeRequest, ClusterRef, Contributions, Extension, ExtensionError,
    ExtensionRequest, ExtensionResponse, Host, HostOperation, HostResponse, KindRef, Manifest,
    ObjectRef, Permission, SurfaceRequest, Verb,
};
use kubepulse_extension_host::{ExtensionHost, HostOptions};
use kubepulse_ui::{
    cluster_model::ClusterModel,
    identity::{ClusterKey, ClusterSettings, IdentityStore},
    manifest_tab::{ManifestRequest, ManifestTab},
    preferences::{Appearance, Preferences, SharedPreferences},
    runtime::{ClusterBackend, ClusterState, ClusterStatus, ConnectedClusters},
    services::SharedServices,
    settings_tab,
    shell_state::{SharedShellState, ShellState},
    workspace::Workspace,
};
use serde_json::{Value, json};
use support::{FakeServices, connected, kind, wait_until};
use tokio::{
    runtime::{Builder, Runtime},
    sync::{mpsc, watch},
};

fn config_map() -> ResourceKind {
    kind("", "ConfigMap", "configmaps", true)
}

fn secret() -> ResourceKind {
    kind("", "Secret", "secrets", true)
}

fn target(kind: ResourceKind) -> ObjectTarget {
    ObjectTarget {
        kind,
        namespace: Some("default".into()),
        name: "app".into(),
    }
}

fn live(version: &str, data: Value) -> Value {
    json!({
        "apiVersion": "v1",
        "kind": "ConfigMap",
        "metadata": {
            "name": "app",
            "namespace": "default",
            "uid": "1234",
            "resourceVersion": version,
            "managedFields": [{"manager": "kubectl"}],
        },
        "data": data,
        "status": {"phase": "Ready"},
    })
}

fn key() -> ClusterKey {
    ClusterKey::new("a", Some("https://a.example"))
}

struct Fixture {
    services: Arc<FakeServices>,
    workspace: Entity<Workspace>,
    _state: watch::Sender<Arc<ClusterState>>,
    requests: Option<mpsc::UnboundedSender<kubepulse_ui::runtime::HostRequest>>,
}

fn setup(
    cx: &mut TestAppContext,
    identities: IdentityStore,
    status: ClusterStatus,
) -> (Fixture, &mut VisualTestContext) {
    cx.executor().allow_parking();
    let (state, receiver) = watch::channel(Arc::new(ClusterState {
        version: 1,
        clusters: vec![status],
        errors: vec![],
    }));
    let (sender, _commands) = mpsc::unbounded_channel();
    let services = Arc::new(FakeServices::idle());
    let shared = services.clone();
    let model = cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_reduce_motion(true);
        cx.set_global(SharedServices(shared));
        let model = cx.new(|cx| ClusterModel::new(receiver, sender, identities, cx));
        let shell = cx.new(|_| ShellState::default());
        cx.set_global(SharedShellState(shell));
        settings_tab::init(Preferences::default(), None, cx);
        model
    });
    let (requests, listening) = mpsc::unbounded_channel();
    let window_model = model.clone();
    let (handle, workspace) = cx
        .update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| {
                    let workspace = Workspace::new(window_model, None, window, cx);
                    workspace.listen(listening, window, cx);
                    workspace
                })
            })
        })
        .unwrap();
    let cx = VisualTestContext::from_window(handle, cx).into_mut();
    cx.simulate_resize(gpui_kit::size(gpui_kit::px(1400.), gpui_kit::px(900.)));
    cx.run_until_parked();
    let fixture = Fixture {
        services,
        workspace,
        _state: state,
        requests: Some(requests),
    };
    (fixture, cx)
}

fn cluster() -> (IdentityStore, ClusterStatus) {
    let served = vec![config_map(), secret()];
    (IdentityStore::default(), connected("a", served))
}

fn with_object(cx: &mut TestAppContext, object: Value) -> (Fixture, &mut VisualTestContext) {
    let (identities, status) = cluster();
    let (fixture, cx) = setup(cx, identities, status);
    fixture
        .services
        .add_object(&config_map(), Some("default"), object);
    (fixture, cx)
}

fn open(fixture: &Fixture, kind: ResourceKind, cx: &mut VisualTestContext) -> Entity<ManifestTab> {
    let tab = cx.update(|window, cx| {
        fixture.workspace.update(cx, |workspace, cx| {
            workspace.open_editor(key(), target(kind), ManifestRequest::Edit, None, window, cx)
        })
    });
    settle(cx);
    tab
}

fn open_editing(fixture: &Fixture, cx: &mut VisualTestContext) -> Entity<ManifestTab> {
    let tab = open(fixture, config_map(), cx);
    wait_until(cx, "the editor", |cx| exists("manifest-review", cx));
    tab
}

fn settle(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.render_frame(cx));
    cx.run_until_parked();
}

fn exists(id: &str, cx: &mut VisualTestContext) -> bool {
    let id = SharedString::from(id.to_owned());
    cx.update(|window, cx| {
        window.render_frame(cx);
        window.try_find(id).is_some()
    })
}

fn click(id: &str, cx: &mut VisualTestContext) {
    let id = SharedString::from(id.to_owned());
    cx.update(|window, cx| window.click(id, cx));
    settle(cx);
}

fn type_text(tab: &Entity<ManifestTab>, text: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| tab.update(cx, |tab, cx| tab.set_text(text, window, cx)));
    wait_until(cx, "the syntax check", |cx| {
        cx.update(|_, cx| tab.read(cx).text(cx) == text)
    });
    settle(cx);
}

fn edited(data: Value) -> String {
    manifest::to_yaml(&manifest::strip(&live("1", data)))
}

fn dry_run_writes(services: &FakeServices) -> Vec<Value> {
    writes_with(services, WriteMode::DryRun)
}

fn applied_writes(services: &FakeServices) -> Vec<Value> {
    writes_with(services, WriteMode::Apply)
}

fn writes_with(services: &FakeServices, mode: WriteMode) -> Vec<Value> {
    let writes = services.writes.lock().unwrap();
    writes
        .iter()
        .filter(|(written, _)| *written == mode)
        .map(|(_, object)| object.clone())
        .collect()
}

fn review_edit(fixture: &Fixture, tab: &Entity<ManifestTab>, cx: &mut VisualTestContext) {
    type_text(tab, &edited(json!({"key": "new"})), cx);
    click("manifest-review", cx);
    let services = fixture.services.clone();
    wait_until(cx, "the dry run", |_| !dry_run_writes(&services).is_empty());
    settle(cx);
}

fn patch(tab: &Entity<ManifestTab>, cx: &mut VisualTestContext) -> String {
    cx.update(|_, cx| tab.read(cx).reviewed_patch().unwrap_or_default().to_owned())
}

fn message(tab: &Entity<ManifestTab>, cx: &mut VisualTestContext) -> String {
    cx.update(|_, cx| tab.read(cx).message().unwrap_or_default().to_owned())
}

#[gpui_kit::test]
fn a_manifest_opens_without_server_managed_fields(cx: &mut TestAppContext) {
    let (fixture, cx) = with_object(cx, live("1", json!({"key": "old"})));

    let tab = open_editing(&fixture, cx);

    let text = cx.update(|_, cx| tab.read(cx).text(cx));
    assert_eq!(text, edited(json!({"key": "old"})));
    assert!(text.contains("key: old"), "{text}");
    for hidden in ["status", "managedFields", "resourceVersion", "uid"] {
        assert!(!text.contains(hidden), "{hidden} in {text}");
    }
}

#[gpui_kit::test]
fn the_editor_highlights_yaml_and_undoes_typing(cx: &mut TestAppContext) {
    let (fixture, cx) = with_object(cx, live("1", json!({"key": "old"})));
    let tab = open_editing(&fixture, cx);
    assert!(LanguageRegistry::singleton().language("yaml").is_some());

    let before = cx.update(|_, cx| tab.read(cx).text(cx));
    let editor = cx.update(|_, cx| tab.read(cx).editor().clone());
    cx.update(|window, cx| editor.read(cx).focus_handle(cx).focus(window, cx));
    cx.simulate_input("x");
    settle(cx);
    assert_ne!(cx.update(|_, cx| tab.read(cx).text(cx)), before);
    assert!(exists("manifest-unapplied", cx));

    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-z"
    } else {
        "ctrl-z"
    });
    settle(cx);
    assert_eq!(cx.update(|_, cx| tab.read(cx).text(cx)), before);
    assert!(!exists("manifest-unapplied", cx));
}

#[gpui_kit::test]
fn highlighting_follows_the_active_theme(cx: &mut TestAppContext) {
    let (fixture, cx) = with_object(cx, live("1", json!({"key": "old"})));
    open_editing(&fixture, cx);
    let appearance = |cx: &mut VisualTestContext, mode| {
        let preferences = cx.update(|_, cx| cx.global::<SharedPreferences>().0.clone());
        cx.update(|_, cx| {
            preferences.update(cx, |model, cx| {
                model.change(|p: &mut Preferences| p.appearance = mode, cx);
            });
        });
        cx.run_until_parked();
        cx.update(|_, cx| cx.theme().highlight_theme.clone())
    };

    let light = appearance(cx, Appearance::Light);
    let dark = appearance(cx, Appearance::Dark);

    assert_ne!(light.name, dark.name);
}

#[gpui_kit::test]
fn a_syntax_error_is_reported_and_stops_the_review(cx: &mut TestAppContext) {
    let (fixture, cx) = with_object(cx, live("1", json!({"key": "old"})));
    let tab = open_editing(&fixture, cx);

    type_text(&tab, "a: 1\nb: [\n", cx);
    wait_until(cx, "the error", |cx| exists("manifest-syntax-error", cx));

    assert!(!cx.update(|_, cx| tab.read(cx).can_review(cx)));
    let editor = cx.update(|_, cx| tab.read(cx).editor().clone());
    let marked = cx.update(|_, cx| editor.read(cx).diagnostics().map_or(0, |set| set.len()));
    assert_eq!(marked, 1);
    click("manifest-review", cx);
    assert!(dry_run_writes(&fixture.services).is_empty());

    type_text(&tab, &edited(json!({"key": "fixed"})), cx);
    wait_until(cx, "the error to clear", |cx| {
        !exists("manifest-syntax-error", cx)
    });
    assert!(cx.update(|_, cx| tab.read(cx).can_review(cx)));
}

#[gpui_kit::test]
fn a_rejected_change_shows_the_servers_message_and_writes_nothing(cx: &mut TestAppContext) {
    let (fixture, cx) = with_object(cx, live("1", json!({"key": "old"})));
    let tab = open_editing(&fixture, cx);
    fixture
        .services
        .write_results
        .lock()
        .unwrap()
        .push(Err(ObjectError::Failed("data.key: must be shorter".into())));

    review_edit(&fixture, &tab, cx);

    assert!(exists("manifest-error", cx));
    assert!(message(&tab, cx).contains("data.key: must be shorter"));
    assert!(!exists("review-apply", cx));
    assert!(applied_writes(&fixture.services).is_empty());
    assert_eq!(
        cx.update(|_, cx| tab.read(cx).text(cx)),
        edited(json!({"key": "new"})),
        "the user's text stays"
    );
}

#[gpui_kit::test]
fn the_diff_shows_admission_changes_and_nothing_applies_until_confirmed(cx: &mut TestAppContext) {
    let (fixture, cx) = with_object(cx, live("1", json!({"key": "old"})));
    let tab = open_editing(&fixture, cx);
    let mut mutated = live("2", json!({"key": "new"}));
    mutated["metadata"]["labels"] = json!({"injected-by": "admission"});
    fixture
        .services
        .write_results
        .lock()
        .unwrap()
        .push(Ok(mutated));

    review_edit(&fixture, &tab, cx);

    let diff = patch(&tab, cx);
    assert!(diff.contains("+  key: new"), "{diff}");
    assert!(diff.contains("-  key: old"), "{diff}");
    assert!(diff.contains("+    injected-by: admission"), "{diff}");
    assert!(exists("manifest-target", cx));
    assert!(applied_writes(&fixture.services).is_empty());

    click("review-apply", cx);
    let services = fixture.services.clone();
    wait_until(cx, "the write", |_| !applied_writes(&services).is_empty());

    let sent = &applied_writes(&fixture.services)[0];
    assert_eq!(sent["metadata"]["resourceVersion"], "1");
    assert_eq!(sent["data"]["key"], "new");
    wait_until(cx, "the result", |cx| exists("manifest-notice", cx));
}

#[gpui_kit::test]
fn a_change_the_server_ignores_reports_nothing_to_apply(cx: &mut TestAppContext) {
    let original = live("1", json!({"key": "old"}));
    let (fixture, cx) = with_object(cx, original.clone());
    let tab = open_editing(&fixture, cx);
    fixture
        .services
        .write_results
        .lock()
        .unwrap()
        .push(Ok(original));

    review_edit(&fixture, &tab, cx);

    assert!(exists("manifest-notice", cx));
    assert!(!exists("review-apply", cx));
    assert!(cx.update(|_, cx| tab.read(cx).reviewed_patch().is_none()));
}

#[gpui_kit::test]
fn a_failed_apply_keeps_the_users_text(cx: &mut TestAppContext) {
    let (fixture, cx) = with_object(cx, live("1", json!({"key": "old"})));
    let tab = open_editing(&fixture, cx);
    review_edit(&fixture, &tab, cx);
    fixture
        .services
        .write_results
        .lock()
        .unwrap()
        .push(Err(ObjectError::Failed("the server is unavailable".into())));

    click("review-apply", cx);
    wait_until(cx, "the error", |cx| exists("manifest-error", cx));

    assert!(message(&tab, cx).contains("the server is unavailable"));
    assert_eq!(
        cx.update(|_, cx| tab.read(cx).text(cx)),
        edited(json!({"key": "new"}))
    );
}

#[gpui_kit::test]
fn a_concurrent_change_is_refused_and_can_be_reapplied_on_the_newer_version(
    cx: &mut TestAppContext,
) {
    let (fixture, cx) = with_object(cx, live("1", json!({"key": "old", "other": "old"})));
    let tab = open_editing(&fixture, cx);
    type_text(&tab, &edited(json!({"key": "mine", "other": "old"})), cx);
    click("manifest-review", cx);
    let services = fixture.services.clone();
    wait_until(cx, "the review", |cx| exists("review-apply", cx));

    // Another actor changes `other` before the user confirms.
    let newer = live("2", json!({"key": "old", "other": "theirs"}));
    services.objects.lock().unwrap()[0].1 = newer;
    services
        .write_results
        .lock()
        .unwrap()
        .push(Err(ObjectError::Conflict(
            "the object has been modified".into(),
        )));
    click("review-apply", cx);
    wait_until(cx, "the conflict", |cx| exists("manifest-conflict", cx));
    assert_eq!(applied_writes(&services).len(), 1, "the server refused it");

    click("conflict-reapply", cx);
    wait_until(cx, "the new review", |cx| exists("review-apply", cx));

    let again = dry_run_writes(&services).pop().unwrap();
    assert_eq!(again["metadata"]["resourceVersion"], "2");
    assert_eq!(again["data"], json!({"key": "mine", "other": "theirs"}));
    let diff = patch(&tab, cx);
    assert!(diff.contains("+  key: mine"), "{diff}");
    assert!(
        !diff.contains("-  other"),
        "their change is the baseline: {diff}"
    );
    assert!(
        !diff.contains("+  other"),
        "their change is the baseline: {diff}"
    );

    click("review-apply", cx);
    wait_until(cx, "the write", |_| applied_writes(&services).len() == 2);
    let sent = applied_writes(&services).pop().unwrap();
    assert_eq!(sent["metadata"]["resourceVersion"], "2");
}

#[gpui_kit::test]
fn an_object_the_user_cannot_update_opens_read_only_and_says_why(cx: &mut TestAppContext) {
    let (fixture, cx) = with_object(cx, live("1", json!({"key": "old"})));
    *fixture.services.access.lock().unwrap() = Some(UpdateAccess {
        allowed: false,
        reason: Some("no rule allows update".into()),
    });

    let tab = open(&fixture, config_map(), cx);
    wait_until(cx, "the notice", |cx| exists("manifest-read-only", cx));

    let reason = cx.update(|_, cx| tab.read(cx).read_only().unwrap_or_default().to_owned());
    assert!(reason.contains("no rule allows update"), "{reason}");
    assert!(!exists("manifest-review", cx));
    assert!(
        cx.update(|_, cx| tab.read(cx).text(cx))
            .contains("key: old")
    );
}

#[gpui_kit::test]
fn a_kind_without_update_opens_read_only(cx: &mut TestAppContext) {
    let (fixture, cx) = with_object(cx, live("1", json!({"key": "old"})));
    let mut kind = config_map();
    kind.verbs = vec!["get".into()];
    fixture
        .services
        .add_object(&kind, Some("default"), live("1", json!({})));

    open(&fixture, kind, cx);

    wait_until(cx, "the notice", |cx| exists("manifest-read-only", cx));
}

#[gpui_kit::test]
fn a_secret_stays_hidden_until_the_user_reveals_it(cx: &mut TestAppContext) {
    let (identities, status) = cluster();
    let (fixture, cx) = setup(cx, identities, status);
    let object = json!({
        "apiVersion": "v1",
        "kind": "Secret",
        "metadata": {"name": "app", "namespace": "default"},
        "data": {"password": "aHVudGVyMg=="},
    });
    fixture
        .services
        .add_object(&secret(), Some("default"), object);

    let tab = open(&fixture, secret(), cx);

    assert!(exists("manifest-reveal-prompt", cx));
    assert_eq!(cx.update(|_, cx| tab.read(cx).text(cx)), "");

    click("manifest-reveal", cx);
    wait_until(cx, "the manifest", |cx| exists("manifest-review", cx));
    assert!(
        cx.update(|_, cx| tab.read(cx).text(cx))
            .contains("aHVudGVyMg==")
    );
}

fn close_active(tab: &Entity<ManifestTab>, cx: &mut VisualTestContext) {
    let id = tab.entity_id();
    let shell = cx.update(|_, cx| cx.global::<SharedShellState>().0.clone());
    cx.update(|_, cx| shell.update(cx, |shell, cx| shell.request_close(id, cx)));
    settle(cx);
}

fn is_open(tab: &Entity<ManifestTab>, cx: &mut VisualTestContext) -> bool {
    let id = tab.entity_id();
    let shell = cx.update(|_, cx| cx.global::<SharedShellState>().0.clone());
    cx.update(|_, cx| shell.read(cx).editor(id).is_some())
}

#[gpui_kit::test]
fn a_middle_click_with_unapplied_changes_asks_first(cx: &mut TestAppContext) {
    let (fixture, cx) = with_object(cx, live("1", json!({"key": "old"})));
    let tab = open_editing(&fixture, cx);
    type_text(&tab, &edited(json!({"key": "new"})), cx);

    let title = SharedString::from(format!("tab-title-{}", tab.entity_id()));
    let options = ClickOptions::new().with_button(MouseButton::Middle);
    cx.update(|window, cx| window.click_with_options(title, options, cx));
    settle(cx);

    assert!(exists("discard-confirmation", cx));
    assert!(is_open(&tab, cx));
}

#[gpui_kit::test]
fn closing_with_unapplied_changes_asks_first(cx: &mut TestAppContext) {
    let (fixture, cx) = with_object(cx, live("1", json!({"key": "old"})));
    let tab = open_editing(&fixture, cx);
    type_text(&tab, &edited(json!({"key": "new"})), cx);

    close_active(&tab, cx);
    assert!(exists("discard-confirmation", cx));
    assert!(is_open(&tab, cx));

    click("discard-cancel", cx);
    assert!(is_open(&tab, cx), "the text stays");

    close_active(&tab, cx);
    click("discard-confirm", cx);
    assert!(!is_open(&tab, cx));
}

#[gpui_kit::test]
fn closing_without_changes_does_not_ask(cx: &mut TestAppContext) {
    let (fixture, cx) = with_object(cx, live("1", json!({"key": "old"})));
    let tab = open_editing(&fixture, cx);

    close_active(&tab, cx);

    assert!(!exists("discard-confirmation", cx));
    assert!(!is_open(&tab, cx));
}

#[gpui_kit::test]
fn opening_one_object_twice_shares_a_tab(cx: &mut TestAppContext) {
    let (fixture, cx) = with_object(cx, live("1", json!({"key": "old"})));
    let first = open_editing(&fixture, cx);
    let second = open(&fixture, config_map(), cx);
    assert_eq!(first.entity_id(), second.entity_id());
}

/// Runs one host operation when asked, as a test extension's command.
struct Changer {
    operation: HostOperation,
}

fn config_maps() -> KindRef {
    KindRef::new("", "ConfigMap")
}

#[async_trait]
impl Extension for Changer {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: "example.changer".into(),
            name: "Changer".into(),
            version: "1.0.0".into(),
            contract: CONTRACT_VERSION,
            activation: vec![],
            contributions: Contributions::default(),
            permissions: vec![Permission {
                kind: config_maps(),
                verbs: vec![
                    Verb::Create,
                    Verb::Get,
                    Verb::Update,
                    Verb::Delete,
                    Verb::Logs,
                ],
            }],
            translations: Default::default(),
        }
    }

    async fn handle(
        &self,
        _: ExtensionRequest,
        host: Arc<dyn Host>,
    ) -> Result<ExtensionResponse, ExtensionError> {
        let response = host.call(self.operation.clone()).await?;
        match response {
            HostResponse::Change { outcome } => Err(ExtensionError::new(format!("{outcome:?}"))),
            _ => Ok(ExtensionResponse::Done),
        }
    }
}

struct Running {
    runtime: Runtime,
    host: ExtensionHost,
    /// The change outcome, once the extension has its answer.
    result: Arc<Mutex<Option<String>>>,
}

/// Starts an extension's request through the real host and backend. The
/// fixture's channel carries what the backend asks of the workspace.
fn run_extension(
    fixture: &mut Fixture,
    operation: HostOperation,
    cx: &mut VisualTestContext,
) -> Running {
    let runtime = Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let (backend, mut from_backend) = ClusterBackend::new(Arc::<ConnectedClusters>::default());
    let host = ExtensionHost::new(Arc::new(backend), HostOptions::default());
    host.load(vec![Arc::new(Changer { operation })]);
    let to_workspace = fixture.requests.take().expect("one extension per test");
    runtime.spawn(async move {
        while let Some(request) = from_backend.recv().await {
            let _ = to_workspace.send(request);
        }
    });
    let result = Arc::new(Mutex::new(None));
    let (running, written) = (host.clone(), result.clone());
    runtime.spawn(async move {
        let request = ExtensionRequest::RunCommand {
            command: "x".into(),
        };
        let answer = running.request("example.changer", request).await;
        *written.lock().unwrap() = Some(format!("{answer:?}"));
    });
    cx.run_until_parked();
    Running {
        runtime,
        host,
        result,
    }
}

fn object_ref() -> ObjectRef {
    ObjectRef {
        cluster: ClusterRef("a".into()),
        kind: config_maps(),
        namespace: Some("default".into()),
        name: "app".into(),
    }
}

fn update_request() -> HostOperation {
    HostOperation::RequestChange {
        request: ChangeRequest::Update {
            target: object_ref(),
            manifest: live("9", json!({"key": "from-extension"})),
        },
    }
}

fn outcome(running: &Running) -> Option<String> {
    running.result.lock().unwrap().clone()
}

fn wait_for_outcome(running: &Running, cx: &mut VisualTestContext) -> String {
    wait_until(cx, "the extension's answer", |_| outcome(running).is_some());
    outcome(running).unwrap()
}

fn only_tab(cx: &mut VisualTestContext) -> Entity<ManifestTab> {
    let shell = cx.update(|_, cx| cx.global::<SharedShellState>().0.clone());
    cx.update(|_, cx| shell.read(cx).editor_of(&key(), &target(config_map())))
        .expect("the review tab")
}

#[gpui_kit::test]
fn an_extensions_change_waits_for_the_users_confirmation(cx: &mut TestAppContext) {
    let (mut fixture, cx) = with_object(cx, live("1", json!({"key": "old"})));

    let running = run_extension(&mut fixture, update_request(), cx);
    wait_until(cx, "the review", |cx| exists("review-apply", cx));

    assert!(applied_writes(&fixture.services).is_empty());
    assert_eq!(outcome(&running), None, "the extension is still waiting");
    let tab = only_tab(cx);
    assert!(patch(&tab, cx).contains("+  key: from-extension"));

    click("review-apply", cx);
    let answer = wait_for_outcome(&running, cx);

    assert!(answer.contains("Applied"), "{answer}");
    let sent = applied_writes(&fixture.services).pop().unwrap();
    assert_eq!(sent["metadata"]["resourceVersion"], "1", "the live version");
    drop(running.host);
    running.runtime.shutdown_background();
}

#[gpui_kit::test]
fn closing_an_extensions_review_declines_it(cx: &mut TestAppContext) {
    let (mut fixture, cx) = with_object(cx, live("1", json!({"key": "old"})));
    let running = run_extension(&mut fixture, update_request(), cx);
    wait_until(cx, "the review", |cx| exists("review-apply", cx));

    let tab = only_tab(cx);
    close_active(&tab, cx);
    click("discard-confirm", cx);
    let answer = wait_for_outcome(&running, cx);

    assert!(answer.contains("Declined"), "{answer}");
    assert!(applied_writes(&fixture.services).is_empty());
    running.runtime.shutdown_background();
}

#[gpui_kit::test]
fn an_extensions_delete_names_the_target_and_needs_the_typed_name_when_protected(
    cx: &mut TestAppContext,
) {
    let mut identities = IdentityStore::default();
    identities.update(key(), |settings: &mut ClusterSettings| {
        settings.protected = true
    });
    let (_, status) = cluster();
    let (mut fixture, cx) = setup(cx, identities, status);
    fixture.services.add_object(
        &config_map(),
        Some("default"),
        live("1", json!({"key": "old"})),
    );
    let operation = HostOperation::RequestChange {
        request: ChangeRequest::Delete {
            target: object_ref(),
        },
    };

    let running = run_extension(&mut fixture, operation, cx);
    wait_until(cx, "the review", |cx| exists("review-delete", cx));
    assert!(exists("manifest-target", cx));

    click("review-delete", cx);
    assert!(fixture.services.deleted.lock().unwrap().is_empty());

    let tab = only_tab(cx);
    let typed = cx.update(|_, cx| tab.read(cx).typed_name().clone());
    cx.update(|window, cx| typed.update(cx, |input, cx| input.set_value("app", window, cx)));
    settle(cx);
    click("review-delete", cx);
    let answer = wait_for_outcome(&running, cx);

    assert!(answer.contains("Applied"), "{answer}");
    assert_eq!(fixture.services.deleted.lock().unwrap().len(), 1);
    settle(cx);
    assert!(!is_open(&tab, cx));
    running.runtime.shutdown_background();
}

#[gpui_kit::test]
fn an_extensions_new_object_is_dry_run_then_created_after_confirmation(cx: &mut TestAppContext) {
    let (identities, status) = cluster();
    let (mut fixture, cx) = setup(cx, identities, status);
    let operation = HostOperation::RequestChange {
        request: ChangeRequest::Create {
            cluster: ClusterRef("a".into()),
            kind: config_maps(),
            manifest: live("", json!({"key": "new"})),
        },
    };

    let running = run_extension(&mut fixture, operation, cx);
    wait_until(cx, "the review", |cx| exists("review-apply", cx));
    assert!(applied_writes(&fixture.services).is_empty());

    click("review-apply", cx);
    let answer = wait_for_outcome(&running, cx);

    assert!(answer.contains("Applied"), "{answer}");
    let created = applied_writes(&fixture.services).pop().unwrap();
    assert_eq!(created["metadata"]["name"], "app");
    running.runtime.shutdown_background();
}

#[gpui_kit::test]
fn an_extension_opens_the_editor_surface(cx: &mut TestAppContext) {
    let (mut fixture, cx) = with_object(cx, live("1", json!({"key": "old"})));
    let operation = HostOperation::OpenSurface {
        request: SurfaceRequest::Editor {
            target: object_ref(),
        },
    };

    let running = run_extension(&mut fixture, operation, cx);
    wait_until(cx, "the editor", |cx| exists("manifest-review", cx));

    let answer = wait_for_outcome(&running, cx);
    assert!(answer.contains("Done"), "{answer}");
    running.runtime.shutdown_background();
}

#[gpui_kit::test]
fn a_change_on_a_disconnected_cluster_fails_without_a_tab(cx: &mut TestAppContext) {
    let (identities, mut status) = cluster();
    status.state = ConnectionState::Disconnected;
    let (mut fixture, cx) = setup(cx, identities, status);

    let running = run_extension(&mut fixture, update_request(), cx);
    let answer = wait_for_outcome(&running, cx);

    assert!(answer.contains("not connected"), "{answer}");
    assert!(fixture.services.writes.lock().unwrap().is_empty());
    running.runtime.shutdown_background();
}

#[gpui_kit::test]
fn a_failed_access_check_is_shown_and_leaves_the_editor_usable(cx: &mut TestAppContext) {
    let (fixture, cx) = with_object(cx, live("1", json!({"key": "old"})));
    *fixture.services.access_failure.lock().unwrap() = Some("the review was refused".into());

    let tab = open_editing(&fixture, cx);

    assert!(message(&tab, cx).contains("the review was refused"));
    assert!(cx.update(|_, cx| tab.read(cx).read_only().is_none()));
}

#[gpui_kit::test]
fn a_context_moved_to_another_server_blocks_applying(cx: &mut TestAppContext) {
    let (fixture, cx) = with_object(cx, live("1", json!({"key": "old"})));
    let tab = open_editing(&fixture, cx);
    review_edit(&fixture, &tab, cx);
    assert!(exists("review-apply", cx));

    let (_, mut moved) = cluster();
    moved.entry.server = Some("https://other.example".into());
    fixture._state.send_replace(Arc::new(ClusterState {
        version: 2,
        clusters: vec![moved],
        errors: vec![],
    }));
    settle(cx);

    assert!(exists("tab-server-changed", cx));
    assert!(!exists("review-apply", cx));
    cx.update(|window, cx| tab.update(cx, |tab, cx| tab.apply(window, cx)));
    settle(cx);
    assert!(applied_writes(&fixture.services).is_empty());
    assert!(!cx.update(|_, cx| tab.read(cx).can_review(cx)));
}

#[gpui_kit::test]
fn clicking_an_editor_selects_its_cluster(cx: &mut TestAppContext) {
    let (fixture, cx) = with_object(cx, live("1", json!({"key": "old"})));
    open_editing(&fixture, cx);
    let shell = cx.update(|_, cx| cx.global::<SharedShellState>().0.clone());
    cx.update(|_, cx| {
        shell.update(cx, |shell, cx| shell.select(ClusterKey::new("b", None), cx));
    });

    click("manifest-tab", cx);

    assert_eq!(
        cx.update(|_, cx| shell.read(cx).selected().cloned()),
        Some(key())
    );
}

#[gpui_kit::test]
fn an_extension_opens_the_log_surface_for_a_pod_only(cx: &mut TestAppContext) {
    let (identities, status) = cluster();
    let (mut fixture, cx) = setup(cx, identities, status);
    let operation = HostOperation::OpenSurface {
        request: SurfaceRequest::Logs {
            target: object_ref(),
            container: None,
        },
    };

    let running = run_extension(&mut fixture, operation, cx);
    let answer = wait_for_outcome(&running, cx);

    assert!(answer.contains("only pods have logs"), "{answer}");
    running.runtime.shutdown_background();
}
