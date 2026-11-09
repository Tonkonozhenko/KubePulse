# Design

## Context

See `proposal.md` for the motivation, and `specs/general/workload-restart/spec.md` and `specs/general/cronjob-run/spec.md` for required behavior. These facts about the current code shape the design:

- Row actions are host-owned. `RowAction` in `resource_browser.rs` lists them, `row_actions(kind)` picks them by kind, and each emits a `BrowserEvent`. `DetailView` emits a matching `DetailEvent`. `ClusterTab` handles both.
- `DeleteDialog` handles one object or several. It names the cluster and every object, asks for a typed name on a protected cluster, runs up to 8 deletions at once, keeps failed objects listed with their errors, retries only those, and stops starting new ones when cancelled.
- `SessionHandle` in `kubepulse-cluster/src/objects.rs` has `get_object`, `replace_object`, and `create_object`, and `object_error` maps HTTP 409 to `ObjectError::Conflict`. `ClusterServices` exposes these to the UI, and `tests/support` fakes them.
- `workspace.rs` already pushes kit notifications for errors.
- The architecture rule sends every change through dry run, diff review, confirmation, and a replace carrying `resourceVersion`, with deletion as the only exception.

## Goals / Non-Goals

**Goals:**

- One confirmation dialog serves deletion, restart, and run, so all three get the same protected-cluster rule, progress, and retry behavior.
- A restart changes exactly one annotation and never loses a concurrent change.
- A manual Job is indistinguishable from one the cluster's own tooling would create by hand.

**Non-Goals:**

- Restarting custom workload kinds. They have no common restart mechanism.
- Editing the Job before creating it. Run now is a straight copy, and the user changes what runs by editing the CronJob's template.
- Running these actions from the command palette.
- Watching the rollout or the Job's progress in the dialog. Tables and the detail panel already show both live.
- Rollback, Pause, Resume, Scale, or Suspend. Each needs its own design.
- Moving row actions onto the extension contract. Edit and Delete are host-owned, and the new actions follow them.

## Decisions

### Restart stamps the standard annotation on the pod template

The system sets `spec.template.metadata.annotations["kubectl.kubernetes.io/restartedAt"]` to the current UTC time in RFC 3339. Changing the pod template makes the controller roll out new pods with the workload's own strategy, and this key is what the cluster's own tooling writes, so other tools show the restart the same way.

Alternative: deleting the pods. That skips the rollout strategy and drops capacity at once for a Deployment.

### Restart reads, stamps, and replaces, retrying on conflict

`SessionHandle::restart_object(target)` reads the object, sets the annotation on the value it read, and replaces it with that value's `resourceVersion`. On `ObjectError::Conflict` it reads again and repeats, up to 3 attempts in all, and then returns the last conflict. A pure function `stamp_restart(&mut Value, time)` does the edit, so it can be unit tested without a cluster.

Retrying is safe because each attempt starts from the newest object and adds only the annotation. This keeps the architecture's rule that writes carry `resourceVersion`.

Alternative: a merge patch without `resourceVersion`. It needs no retry, but it would be the first write that does not carry `resourceVersion`.

### Run now copies the job template, as the cluster's tooling does

`SessionHandle::run_cronjob(target) -> String` reads the CronJob and creates a `batch/v1` Job built by a pure function `job_from_cronjob(&Value) -> Value`:

- `metadata.generateName` is `<cronjob>-manual-`, so the server picks a unique name, and the function returns the created name.
- `metadata.labels` and `metadata.annotations` come from `spec.jobTemplate.metadata`, plus the annotation `cronjob.kubernetes.io/instantiate: manual`.
- `metadata.ownerReferences` holds one controller reference to the CronJob, with its UID, so the Job shows the CronJob as owner and is cleaned up with it.
- `spec` is `spec.jobTemplate.spec` unchanged.

A CronJob name is at most 52 characters, and the server trims a `generateName` prefix so the generated name fits, so no client-side truncation is needed.

Alternative: a fixed name with a timestamp. It can collide when two runs start in the same second, and the server already solves uniqueness.

### Restart and run skip the dry run and the diff

Both are named exceptions, next to deletion: a confirmation naming the cluster and every object, then the write. The user does not author either change, so a diff would show only generated values, and the real write reports admission errors the same way a dry run would. `CLAUDE.md` is updated to name both exceptions.

### The delete dialog becomes an object operation dialog

`delete_dialog.rs` becomes `object_dialog.rs`, and `DeleteDialog` becomes `ObjectDialog`, taking an `Operation { Delete, Restart, Run }`. The operation chooses:

- The title, the confirm button's label and style (danger only for Delete), and the progress text.
- The call: `delete_object`, `restart_object`, or `run_cronjob`.
- Whether `NotFound` counts as success: only for Delete.
- What success does: Delete unchecks the objects and closes their detail panel. Restart leaves both alone. Run leaves both alone and, when the dialog closes, pushes one notification naming every created Job.

Run collects created Job names across retries, so a retry after a partial failure still names every Job. A retried run starts only the failed CronJobs, so no CronJob gets a second Job.

Everything else is shared: the object list, the typed-name rule, the concurrency limit, cancellation, and retry of failures.

Alternative: separate dialogs. They would copy about 300 lines each and let the confirmation rules drift.

### Kinds are matched by group and kind

`columns.rs` gains `is_restartable(kind)`, true for group `apps` with kind `Deployment`, `StatefulSet`, or `DaemonSet`, and `is_cronjob(kind)`, true for group `batch` with kind `CronJob`, next to `is_pod` and `is_service`.

`row_actions` returns `[Restart, Edit, Delete]` or `[Run, Edit, Delete]` for those kinds. The checked-rows bar shows Restart selected or Run selected for the same kinds. `DetailView` shows a Restart or Run now button for them.

`RowAction::Restart` uses `IconName::RotateCw`, which the cluster rail already uses for Retry. `RowAction::Run` uses a play icon from the kit's set, added to the host icon set if it is missing there. `BrowserEvent::Restart`/`Run(Vec<ObjectTarget>)` and `DetailEvent::Restart`/`Run(ObjectTarget)` reach `ClusterTab::confirm_operation`.

## Risks / Trade-offs

- **Risk** → A paused Deployment records the restart but does not roll out until resumed. Mitigation: the restart still succeeds as the cluster reports, and the detail panel shows the Deployment's conditions. The dialog does not try to predict this.
- **Risk** → Restarting many workloads at once replaces many pods at the same time. Mitigation: the confirmation lists every object, the protected-cluster rule applies, and each controller still honors its own surge and unavailability limits.
- **Risk** → Run now ignores the CronJob's `concurrencyPolicy`, so a manual Job can overlap a scheduled one. Mitigation: this matches how manual runs behave in the cluster's own tooling, and the spec does not promise otherwise.
- **Risk** → Manual Jobs count toward the CronJob's history limits, so the controller may delete older Jobs. Mitigation: owning the Job is what keeps it tied to the CronJob, and history limits are the user's own setting.
- **Risk** → An admission webhook rejects the stamped template or the new Job. Mitigation: the dialog shows the cluster's error for that object, and nothing changes.
- **Risk** → The annotation keys name the cluster's own tooling. Mitigation: they are protocol values that must match byte for byte, so they appear only as string constants, never as product references in text.

## Migration Plan

None. The change only adds actions, and nothing is stored.
