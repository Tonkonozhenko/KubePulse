//! Changes one object: edit its YAML, validate with a dry run, review the diff,
//! then write it with the version it was opened at. Extensions' changes take
//! this same path, so nothing reaches a cluster without the user's confirmation.

use std::sync::Arc;

use gpui_kit::{
    assets::IconName,
    base::input::Diagnostic,
    component::{
        ActiveTheme as _, Disableable as _, Icon, Sizable as _,
        button::Button,
        diff::{Diff, DiffFile, DiffState},
        dock::{self, PanelEvent},
        input::{Editor, EditorState, Input, InputEvent, InputState, Position},
    },
    prelude::FluentBuilder as _,
    *,
};
use kubepulse_cluster::{
    ManifestError, ObjectError, ObjectTarget, UpdateAccess, WriteMode, manifest,
};
use kubepulse_extension_api::ChangeOutcome;
use rust_i18n::t;
use serde_json::Value;
use tokio::sync::oneshot;

use crate::{
    cluster_model::ClusterModel,
    columns::{kind_icon, kind_ref, kind_title},
    icons::view_glyph,
    identity::ClusterKey,
    services::{ClusterServices, Reply, SharedServices},
    shell_state::{SharedShellState, ShellState, close_button},
    tab_connection::{activate_tab, connection_notes},
};

pub const PANEL_NAME: &str = "ManifestTab";

/// What the tab was opened to do.
#[derive(Clone, Debug, PartialEq)]
pub enum ManifestRequest {
    Edit,
    /// An extension's new version of an existing object.
    Update(Value),
    /// An extension's new object.
    Create(Value),
    /// An extension's request to delete the object.
    Delete,
}

struct Review {
    diff: Entity<DiffState>,
    patch: String,
    submitted: Value,
    applying: bool,
}

enum Stage {
    /// A Secret's values appear only after the user asks for them.
    AwaitingReveal,
    Loading,
    LoadFailed(String),
    Editing,
    Validating,
    Review(Review),
    Conflict {
        newer: Value,
        diff: Entity<DiffState>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Notice {
    Info(String),
    Error(String),
}

fn diff_state(patch: &str, cx: &mut Context<ManifestTab>) -> Result<Entity<DiffState>, String> {
    let files = DiffFile::parse(patch).map_err(|error| error.to_string())?;
    Ok(cx.new(|cx| DiffState::new(files, cx)))
}

fn syntax_summary(error: &ManifestError) -> String {
    match error {
        ManifestError::Syntax {
            line,
            column,
            message,
        } => t!(
            "manifest.syntax_error",
            line = line,
            column = column,
            error = message
        )
        .to_string(),
        ManifestError::NotAMapping => t!("manifest.not_a_mapping").to_string(),
    }
}

pub struct ManifestTab {
    key: ClusterKey,
    target: ObjectTarget,
    request: ManifestRequest,
    /// Dropping this unanswered tells the extension that the user declined.
    reply: Option<oneshot::Sender<ChangeOutcome>>,
    model: Entity<ClusterModel>,
    shell: Entity<ShellState>,
    services: Arc<dyn ClusterServices>,
    editor: Entity<EditorState>,
    typed_name: Entity<InputState>,
    stage: Stage,
    /// The whole object as last read, which carries its `resourceVersion`.
    opened: Value,
    /// The text that counts as no change.
    baseline: String,
    /// `None` while the latest text is still being checked.
    parsed: Option<Result<Value, ManifestError>>,
    read_only: Option<String>,
    notice: Option<Notice>,
    focus: FocusHandle,
    _syntax_check: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl ManifestTab {
    pub fn new(
        key: ClusterKey,
        target: ObjectTarget,
        request: ManifestRequest,
        reply: Option<oneshot::Sender<ChangeOutcome>>,
        model: Entity<ClusterModel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let services = cx.global::<SharedServices>().0.clone();
        let shell = cx.global::<SharedShellState>().0.clone();
        let editor = cx.new(|cx| EditorState::new(window, cx).language("yaml"));
        let typed_name = cx
            .new(|cx| InputState::new(window, cx).placeholder(t!("detail.type_name").to_string()));
        let subscriptions = vec![
            cx.subscribe(&editor, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.check_syntax(cx);
                }
            }),
            cx.subscribe(&typed_name, |_, _, _: &InputEvent, cx| cx.notify()),
            cx.observe(&model, |_, _, cx| cx.notify()),
        ];
        let is_secret = target.kind.group.is_empty() && target.kind.kind == "Secret";
        let reveals = is_secret && !matches!(request, ManifestRequest::Create(_));
        let mut tab = Self {
            key,
            target,
            request,
            reply,
            model,
            shell,
            services,
            editor,
            typed_name,
            stage: Stage::AwaitingReveal,
            opened: Value::Null,
            baseline: String::new(),
            parsed: None,
            read_only: None,
            notice: None,
            focus: cx.focus_handle(),
            _syntax_check: None,
            _subscriptions: subscriptions,
        };
        if !reveals {
            tab.load(window, cx);
        }
        tab
    }

    pub fn key(&self) -> &ClusterKey {
        &self.key
    }

    pub fn target(&self) -> &ObjectTarget {
        &self.target
    }

    pub fn text(&self, cx: &App) -> String {
        self.editor.read(cx).value().to_string()
    }

    /// Whether the text can go to a dry run.
    pub fn can_review(&self, cx: &App) -> bool {
        self.on_its_cluster(cx)
            && self.read_only.is_none()
            && matches!(self.stage, Stage::Editing)
            && matches!(self.parsed, Some(Ok(_)))
            && self.text(cx) != self.baseline
    }

    pub fn editor(&self) -> &Entity<EditorState> {
        &self.editor
    }

    /// The latest result or failure shown to the user.
    pub fn message(&self) -> Option<&str> {
        self.notice.as_ref().map(|notice| match notice {
            Notice::Info(text) | Notice::Error(text) => text.as_str(),
        })
    }

    pub fn typed_name(&self) -> &Entity<InputState> {
        &self.typed_name
    }

    pub fn read_only(&self) -> Option<&str> {
        self.read_only.as_deref()
    }

    /// The change under review, as a unified patch.
    pub fn reviewed_patch(&self) -> Option<&str> {
        match &self.stage {
            Stage::Review(review) => Some(&review.patch),
            _ => None,
        }
    }

    pub fn has_unapplied_changes(&self, cx: &App) -> bool {
        self.read_only.is_none()
            && self.request != ManifestRequest::Delete
            && !matches!(self.stage, Stage::AwaitingReveal | Stage::Loading)
            && !matches!(self.stage, Stage::LoadFailed(_))
            && self.text(cx) != self.baseline
    }

    /// A context moved to another server is a different cluster, and nothing
    /// opened here may be written there.
    fn on_its_cluster(&self, cx: &App) -> bool {
        self.model.read(cx).same_server(&self.key)
    }

    fn activate(&self, cx: &mut Context<Self>) {
        activate_tab(cx.entity_id(), &self.key, &self.model, &self.shell, cx);
    }

    fn context(&self) -> &str {
        &self.key.context
    }

    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let ManifestRequest::Create(manifest) = &self.request {
            let edited = manifest::strip(manifest);
            self.set_text(&manifest::to_yaml(&edited), window, cx);
            self.submit(edited, window, cx);
            return;
        }
        self.stage = Stage::Loading;
        let context = self.context().to_owned();
        let object = self.services.get_object(&context, self.target.clone());
        let access = self.services.update_access(&context, self.target.clone());
        cx.spawn_in(window, async move |this, cx| {
            let (object, access) = (object.await, access.await);
            let _ = this.update_in(cx, |this, window, cx| {
                this.loaded(object, access, window, cx);
            });
        })
        .detach();
        cx.notify();
    }

    fn loaded(
        &mut self,
        object: Result<Value, ObjectError>,
        access: Result<UpdateAccess, ObjectError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let object = match object {
            Ok(object) => object,
            Err(error) => {
                let reason = error.to_string();
                self.send_outcome(ChangeOutcome::Failed {
                    reason: reason.clone(),
                });
                self.stage = Stage::LoadFailed(reason);
                cx.notify();
                return;
            }
        };
        self.baseline = manifest::to_yaml(&manifest::strip(&object));
        self.opened = object;
        match self.request.clone() {
            ManifestRequest::Edit => {
                self.read_only = self.read_only_reason(&access);
                self.set_text(&self.baseline.clone(), window, cx);
                self.stage = Stage::Editing;
                if let Err(error) = access {
                    let error = t!("manifest.access_unknown", error = error).to_string();
                    self.notice = Some(Notice::Error(error));
                }
            }
            ManifestRequest::Update(proposed) => {
                let edited = manifest::strip(&proposed);
                self.set_text(&manifest::to_yaml(&edited), window, cx);
                self.submit(edited, window, cx);
            }
            ManifestRequest::Delete => {
                let submitted = self.opened.clone();
                self.show_review(manifest::patch(&self.baseline, ""), submitted, cx);
            }
            ManifestRequest::Create(_) => unreachable!("creating reads nothing"),
        }
        cx.notify();
    }

    fn read_only_reason(&self, access: &Result<UpdateAccess, ObjectError>) -> Option<String> {
        if !self.target.kind.verbs.iter().any(|verb| verb == "update") {
            return Some(t!("manifest.kind_read_only").to_string());
        }
        match access {
            Ok(UpdateAccess {
                allowed: false,
                reason: Some(reason),
            }) => Some(t!("manifest.read_only_because", reason = reason).to_string()),
            Ok(UpdateAccess { allowed: false, .. }) => Some(t!("manifest.read_only").to_string()),
            _ => None,
        }
    }

    pub fn set_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.editor
            .update(cx, |editor, cx| editor.set_value(text, window, cx));
        self.check_syntax(cx);
    }

    /// Parses away from the UI thread, so that a large document types smoothly.
    fn check_syntax(&mut self, cx: &mut Context<Self>) {
        self.parsed = None;
        self.notice = None;
        let text = self.text(cx);
        let task = cx.spawn(async move |this, cx| {
            let parsed = cx
                .background_spawn(async move { manifest::parse(&text) })
                .await;
            let _ = this.update(cx, |this, cx| this.syntax_checked(parsed, cx));
        });
        self._syntax_check = Some(task);
        cx.notify();
    }

    fn syntax_checked(&mut self, parsed: Result<Value, ManifestError>, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            let text = editor.text().clone();
            let Some(diagnostics) = editor.diagnostics_mut() else {
                return;
            };
            diagnostics.reset(&text);
            if let Err(ManifestError::Syntax {
                line,
                column,
                message,
            }) = &parsed
            {
                let line = u32::try_from(line.saturating_sub(1)).unwrap_or(u32::MAX);
                let character = u32::try_from(column.saturating_sub(1)).unwrap_or(u32::MAX);
                let start = Position { line, character };
                let end = Position {
                    line,
                    character: character + 1,
                };
                diagnostics.push(Diagnostic::new(start..end, message.clone()));
            }
            cx.notify();
        });
        self.parsed = Some(parsed);
        cx.notify();
    }

    fn write(&self, object: Value, mode: WriteMode) -> Reply<Value> {
        let (context, target) = (self.context(), self.target.clone());
        if matches!(self.request, ManifestRequest::Create(_)) {
            self.services.create_object(context, target, object, mode)
        } else {
            self.services.replace_object(context, target, object, mode)
        }
    }

    fn start_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_review(cx) {
            return;
        }
        if let Some(Ok(edited)) = self.parsed.clone() {
            self.submit(edited, window, cx);
        }
    }

    /// Asks the server what this version would become. The result is what the
    /// user reviews, so changes made by admission rules are visible.
    fn submit(&mut self, edited: Value, window: &mut Window, cx: &mut Context<Self>) {
        let submitted = if self.opened.is_null() {
            edited
        } else {
            manifest::restore(edited, &self.opened)
        };
        self.stage = Stage::Validating;
        self.notice = None;
        let reply = self.write(submitted.clone(), WriteMode::DryRun);
        cx.spawn_in(window, async move |this, cx| {
            let result = reply.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.validated(submitted, result, window, cx);
            });
        })
        .detach();
        cx.notify();
    }

    fn validated(
        &mut self,
        submitted: Value,
        result: Result<Value, ObjectError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(accepted) => {
                let old = if self.opened.is_null() {
                    String::new()
                } else {
                    manifest::to_yaml(&manifest::strip(&self.opened))
                };
                let new = manifest::to_yaml(&manifest::strip(&accepted));
                if old == new {
                    self.stage = Stage::Editing;
                    self.notice = Some(Notice::Info(t!("manifest.no_change").to_string()));
                } else {
                    self.show_review(manifest::patch(&old, &new), submitted, cx);
                }
            }
            Err(ObjectError::Conflict(_)) => self.open_conflict(window, cx),
            Err(error) => {
                self.stage = Stage::Editing;
                let error = t!("manifest.rejected", error = error).to_string();
                self.notice = Some(Notice::Error(error));
            }
        }
        cx.notify();
    }

    fn show_review(&mut self, patch: String, submitted: Value, cx: &mut Context<Self>) {
        match diff_state(&patch, cx) {
            Ok(diff) => {
                self.stage = Stage::Review(Review {
                    diff,
                    patch,
                    submitted,
                    applying: false,
                });
            }
            Err(error) => {
                self.stage = Stage::Editing;
                let error = t!("manifest.diff_failed", error = error).to_string();
                self.notice = Some(Notice::Error(error));
            }
        }
    }

    fn back_to_editing(&mut self, cx: &mut Context<Self>) {
        self.stage = Stage::Editing;
        cx.notify();
    }

    pub fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.on_its_cluster(cx) {
            return;
        }
        let Stage::Review(review) = &mut self.stage else {
            return;
        };
        review.applying = true;
        let submitted = review.submitted.clone();
        self.notice = None;
        if self.request == ManifestRequest::Delete {
            let reply = self
                .services
                .delete_object(self.context(), self.target.clone());
            cx.spawn_in(window, async move |this, cx| {
                let result = reply.await;
                let _ = this.update_in(cx, |this, _, cx| this.deleted(result, cx));
            })
            .detach();
        } else {
            let reply = self.write(submitted, WriteMode::Apply);
            cx.spawn_in(window, async move |this, cx| {
                let result = reply.await;
                let _ = this.update_in(cx, |this, window, cx| {
                    this.applied(result, window, cx);
                });
            })
            .detach();
        }
        cx.notify();
    }

    fn applied(
        &mut self,
        result: Result<Value, ObjectError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(object) => {
                self.baseline = manifest::to_yaml(&manifest::strip(&object));
                self.opened = object;
                self.request = ManifestRequest::Edit;
                self.set_text(&self.baseline.clone(), window, cx);
                self.stage = Stage::Editing;
                self.notice = Some(Notice::Info(t!("manifest.applied").to_string()));
                self.send_outcome(ChangeOutcome::Applied);
            }
            Err(ObjectError::Conflict(_)) => self.open_conflict(window, cx),
            Err(error) => {
                self.stage = Stage::Editing;
                let error = t!("manifest.apply_failed", error = error).to_string();
                self.notice = Some(Notice::Error(error));
            }
        }
        cx.notify();
    }

    fn deleted(&mut self, result: Result<(), ObjectError>, cx: &mut Context<Self>) {
        match result {
            Ok(()) => {
                self.send_outcome(ChangeOutcome::Applied);
                let id = cx.entity_id();
                self.shell
                    .update(cx, |shell, cx| shell.request_close(id, cx));
            }
            Err(error) => {
                if let Stage::Review(review) = &mut self.stage {
                    review.applying = false;
                }
                let error = t!("detail.delete_failed", error = error).to_string();
                self.notice = Some(Notice::Error(error));
            }
        }
        cx.notify();
    }

    fn send_outcome(&mut self, outcome: ChangeOutcome) {
        if let Some(reply) = self.reply.take() {
            // The extension may have stopped waiting.
            let _ = reply.send(outcome);
        }
    }

    /// The object moved on after it was opened. Reads the newer version, so the
    /// user can see what changed before deciding.
    fn open_conflict(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let reply = self
            .services
            .get_object(self.context(), self.target.clone());
        cx.spawn_in(window, async move |this, cx| {
            let newer = reply.await;
            let _ = this.update_in(cx, |this, _, cx| this.show_conflict(newer, cx));
        })
        .detach();
    }

    fn show_conflict(&mut self, newer: Result<Value, ObjectError>, cx: &mut Context<Self>) {
        let shown = newer.map_err(|error| error.to_string()).and_then(|newer| {
            let old = manifest::to_yaml(&manifest::strip(&self.opened));
            let new = manifest::to_yaml(&manifest::strip(&newer));
            let diff = diff_state(&manifest::patch(&old, &new), cx)?;
            Ok(Stage::Conflict { newer, diff })
        });
        match shown {
            Ok(stage) => self.stage = stage,
            Err(error) => {
                self.stage = Stage::Editing;
                let error = t!("manifest.conflict_unread", error = error).to_string();
                self.notice = Some(Notice::Error(error));
            }
        }
        cx.notify();
    }

    /// Moves the user's edits onto the newer version, then reviews them again.
    fn reapply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.on_its_cluster(cx) {
            return;
        }
        let Stage::Conflict { newer, .. } = &self.stage else {
            return;
        };
        let newer = newer.clone();
        let ours = match manifest::parse(&self.text(cx)) {
            Ok(ours) => ours,
            Err(error) => {
                self.stage = Stage::Editing;
                self.notice = Some(Notice::Error(syntax_summary(&error)));
                cx.notify();
                return;
            }
        };
        let theirs = manifest::strip(&newer);
        let merged = manifest::rebase(&manifest::strip(&self.opened), &ours, &theirs);
        self.opened = newer;
        self.baseline = manifest::to_yaml(&theirs);
        self.set_text(&manifest::to_yaml(&merged), window, cx);
        self.submit(merged, window, cx);
    }

    fn render_target(&self, cx: &App) -> impl IntoElement + use<> {
        let identity = self.model.read(cx).identity(&self.key);
        let namespace = self
            .target
            .namespace
            .clone()
            .unwrap_or_else(|| t!("detail.cluster_wide").to_string());
        div()
            .id("manifest-target")
            .flex()
            .flex_wrap()
            .gap_3()
            .child(
                div()
                    .text_color(rgb(identity.color))
                    .child(t!("detail.cluster", name = identity.name).to_string()),
            )
            .when(identity.protected, |this| {
                this.child(t!("cluster.protected_tag").to_string())
            })
            .child(t!("detail.namespace_line", namespace = namespace).to_string())
            .child(format!("{} {}", self.target.kind.kind, self.target.name))
            .test_support()
    }

    fn render_notice(&self) -> Option<impl IntoElement + use<>> {
        self.notice.as_ref().map(|notice| match notice {
            Notice::Info(text) => div()
                .id("manifest-notice")
                .child(text.clone())
                .test_support(),
            Notice::Error(text) => div()
                .id("manifest-error")
                .child(text.clone())
                .test_support(),
        })
    }

    fn render_editing(&self, cx: &mut Context<Self>) -> AnyElement {
        let syntax = match &self.parsed {
            Some(Err(error)) => Some(syntax_summary(error)),
            _ => None,
        };
        let read_only = self.read_only.is_some();
        div()
            .flex()
            .flex_col()
            .flex_1()
            .w_full()
            .gap_2()
            .overflow_hidden()
            .children(syntax.map(|syntax| {
                div()
                    .id("manifest-syntax-error")
                    .child(syntax)
                    .test_support()
            }))
            .child(
                div().flex_1().w_full().overflow_hidden().child(
                    Editor::new(&self.editor)
                        .readonly(read_only)
                        .h(relative(1.)),
                ),
            )
            .when(!read_only, |this| {
                this.child(
                    Button::new("manifest-review")
                        .icon(IconName::Eye)
                        .label(t!("manifest.review").to_string())
                        .disabled(!self.can_review(cx))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.start_review(window, cx);
                        })),
                )
            })
            .into_any_element()
    }

    fn render_review(&self, review: &Review, cx: &mut Context<Self>) -> AnyElement {
        let delete = self.request == ManifestRequest::Delete;
        let protected = self.model.read(cx).identity(&self.key).protected;
        let needs_name = delete && protected;
        let name_matches = self.typed_name.read(cx).value().as_ref() == self.target.name.as_str();
        let (id, label, icon) = if delete {
            ("review-delete", t!("detail.delete"), IconName::Trash)
        } else {
            ("review-apply", t!("manifest.apply"), IconName::Check)
        };
        let confirm = Button::new(id)
            .icon(icon)
            .label(label.to_string())
            .disabled(review.applying || (needs_name && !name_matches))
            .on_click(cx.listener(|this, _, window, cx| this.apply(window, cx)));
        let back = (!delete).then(|| {
            Button::new("review-back")
                .label(t!("manifest.back").to_string())
                .on_click(cx.listener(|this, _, _, cx| this.back_to_editing(cx)))
        });
        div()
            .id("manifest-review-panel")
            .flex()
            .flex_col()
            .flex_1()
            .w_full()
            .gap_2()
            .overflow_hidden()
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .overflow_hidden()
                    .child(Diff::new(&review.diff)),
            )
            .children(needs_name.then(|| t!("detail.protected").to_string()))
            .children(needs_name.then(|| Input::new(&self.typed_name)))
            .child(div().flex().gap_2().children(back).child(confirm))
            .test_support()
            .into_any_element()
    }

    fn render_conflict(&self, diff: &Entity<DiffState>, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("manifest-conflict")
            .flex()
            .flex_col()
            .flex_1()
            .w_full()
            .gap_2()
            .overflow_hidden()
            .child(t!("manifest.conflict").to_string())
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .overflow_hidden()
                    .child(Diff::new(diff)),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Button::new("conflict-back")
                            .label(t!("manifest.back").to_string())
                            .on_click(cx.listener(|this, _, _, cx| this.back_to_editing(cx))),
                    )
                    .child(
                        Button::new("conflict-reapply")
                            .icon(IconName::RotateCw)
                            .label(t!("manifest.reapply").to_string())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.reapply(window, cx);
                            })),
                    ),
            )
            .test_support()
            .into_any_element()
    }

    fn render_stage(&self, cx: &mut Context<Self>) -> AnyElement {
        match &self.stage {
            Stage::AwaitingReveal => div()
                .id("manifest-reveal-prompt")
                .flex()
                .flex_col()
                .items_start()
                .gap_2()
                .child(t!("manifest.reveal_prompt").to_string())
                .child(
                    Button::new("manifest-reveal")
                        .icon(IconName::Eye)
                        .label(t!("detail.reveal").to_string())
                        .on_click(cx.listener(|this, _, window, cx| this.load(window, cx))),
                )
                .test_support()
                .into_any_element(),
            Stage::Loading | Stage::Validating => div()
                .child(t!("common.loading").to_string())
                .into_any_element(),
            Stage::LoadFailed(reason) => div()
                .id("manifest-load-error")
                .child(t!("detail.load_failed", error = reason).to_string())
                .test_support()
                .into_any_element(),
            Stage::Editing => self.render_editing(cx),
            Stage::Review(review) => self.render_review(review, cx),
            Stage::Conflict { diff, .. } => self.render_conflict(diff, cx),
        }
    }
}

impl gpui_kit::base::dock::Panel for ManifestTab {
    fn panel_name(&self) -> &'static str {
        PANEL_NAME
    }

    fn closable(&self, _: &App) -> bool {
        false
    }

    fn on_added_to(
        &mut self,
        _: WeakEntity<gpui_kit::base::dock::TabGroup>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (id, key, target) = (cx.entity_id(), self.key.clone(), self.target.clone());
        let tab = cx.weak_entity();
        self.shell
            .update(cx, |shell, _| shell.add_editor(id, key, target, tab));
    }

    fn on_removed(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.reply = None;
        let id = cx.entity_id();
        self.shell.update(cx, |shell, cx| shell.remove_tab(id, cx));
    }

    fn set_active(&mut self, active: bool, _: &mut Window, cx: &mut Context<Self>) {
        if active {
            self.activate(cx);
        }
    }
}

impl dock::Panel for ManifestTab {
    fn title(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let identity = self.model.read(cx).identity(&self.key);
        let contributions = self.services.contributions();
        let kind = kind_ref(&self.target.kind);
        let unapplied = self.has_unapplied_changes(cx);
        div()
            .id(SharedString::from(format!("tab-title-{}", cx.entity_id())))
            .flex()
            .items_center()
            .gap_2()
            .child(div().size_2().rounded_full().bg(rgb(identity.color)))
            .child(Icon::new(view_glyph(kind_icon(&kind, &contributions))).xsmall())
            .child(kind_title(&kind, &contributions))
            .child(self.target.name.clone())
            .child(identity.name)
            .when(identity.protected, |this| {
                this.child(t!("cluster.protected_tag").to_string())
            })
            .when(unapplied, |this| {
                this.child(
                    div()
                        .id("manifest-unapplied")
                        .child(t!("manifest.unapplied_tag").to_string())
                        .test_support(),
                )
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.activate(cx)),
            )
            .child(close_button(cx.entity_id(), self.shell.clone()))
            .test_support()
    }
}

impl EventEmitter<PanelEvent> for ManifestTab {}

impl Focusable for ManifestTab {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ManifestTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let identity = self.model.read(cx).identity(&self.key);
        let muted = cx.theme().muted_foreground;
        div()
            .id("manifest-tab")
            .capture_any_mouse_down(cx.listener(|this, _, _, cx| this.activate(cx)))
            .size_full()
            .flex()
            .flex_col()
            .items_start()
            .gap_2()
            .p_2()
            .child(div().h_1().w_full().flex_none().bg(rgb(identity.color)))
            .child(self.render_target(cx))
            .children(connection_notes(&self.key, &self.model, cx))
            .children(self.read_only.clone().map(|reason| {
                div()
                    .id("manifest-read-only")
                    .text_color(muted)
                    .child(reason)
                    .test_support()
            }))
            .children(self.render_notice())
            .when(self.on_its_cluster(cx), |this| {
                this.child(self.render_stage(cx))
            })
            .test_support()
    }
}
