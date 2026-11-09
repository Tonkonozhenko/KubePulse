# Tasks

## 1. Cluster operations

- [x] 1.1 Add `stamp_restart(&mut Value, time)` to `kubepulse-cluster/src/objects.rs`, setting `spec.template.metadata.annotations["kubectl.kubernetes.io/restartedAt"]` and creating missing maps, with unit tests that it adds the key to a template with no annotations, replaces an earlier time, and leaves every other field unchanged; verify with `cargo test -p kubepulse-cluster --lib objects`
- [x] 1.2 Add `SessionHandle::restart_object(target)` that reads, stamps, and replaces with the read `resourceVersion`, retrying on `ObjectError::Conflict` up to 3 attempts in all; verify with a test in `crates/kubepulse-cluster/tests/object_writes.rs` against the fake API that a conflict on the first replace is retried on the newer object and keeps its change, and that three conflicts return `Conflict`
- [x] 1.3 Add `job_from_cronjob(&Value) -> Value` building a `batch/v1` Job with `generateName` `<cronjob>-manual-`, the template's labels and annotations plus `cronjob.kubernetes.io/instantiate: manual`, a controller owner reference to the CronJob with its UID, and the template's `spec`, with unit tests for each of those fields and a template with no metadata; verify with `cargo test -p kubepulse-cluster --lib objects`
- [x] 1.4 Add `SessionHandle::run_cronjob(target) -> String` that reads the CronJob, creates the Job in its namespace, and returns the created name; verify with a test in `object_writes.rs` that the created Job carries the owner reference and the server-generated name is returned
- [x] 1.5 Add `restart_object` and `run_cronjob` to `ClusterServices` in `services.rs`, implement them in `runtime.rs`, and record calls in the fake in `crates/kubepulse-ui/tests/support/mod.rs`, with a way to make chosen targets fail; verify `cargo build --workspace --all-targets` succeeds

## 2. Shared confirmation dialog

- [x] 2.1 Rename `delete_dialog.rs` to `object_dialog.rs` and `DeleteDialog` to `ObjectDialog`, taking `Operation::Delete` with no behavior change, and update `cluster_tab.rs`; verify every existing deletion test in `crates/kubepulse-ui/tests/browsing.rs` passes unchanged apart from names
- [x] 2.2 Add `Operation::Restart` and `Operation::Run`, choosing title, confirm label and style (danger only for Delete), progress text, the service call, whether `NotFound` is success (Delete only), and the success callback; add the keys to `crates/kubepulse-ui/locales/en.json` without plurals, run `python3 scripts/check-i18n.py --write`, and verify `python3 scripts/check-i18n.py` passes
- [x] 2.3 For Run, collect created Job names across retries and push one notification naming them when the dialog closes on success; verify with a test that runs three CronJobs with one failing, retries after clearing the failure, and finds one notification naming all three Jobs and only one run call per CronJob
- [x] 2.4 Add tests for Restart and Run covering: nothing happens until confirm, Enter does not confirm, protected cluster asks for the object's name for one and the cluster's name for several, a failure stays listed with the cluster's error while others succeed, retry runs only failures, cancel starts no more, a missing object is a failure, and a successful restart keeps rows checked and the detail panel open; verify `cargo test -p kubepulse-ui --test browsing` passes

## 3. Actions in tables and the detail panel

- [x] 3.1 Add `is_restartable` and `is_cronjob` to `columns.rs`, with unit tests that only `apps` Deployment, StatefulSet, and DaemonSet, and only `batch` CronJob match, and a same-named kind in another group does not; verify with `cargo test -p kubepulse-ui --lib columns`
- [x] 3.2 Add `RowAction::Restart` (`IconName::RotateCw`) and `RowAction::Run` (a play icon, added to the host icon set if missing), return them from `row_actions`, emit `BrowserEvent::Restart` and `BrowserEvent::Run`, and route them in `cluster_tab.rs`; verify with tests that a Deployments row and its right-click menu offer Restart, a CronJobs row offers Run now, a Pods row offers neither, and using either does not open the detail panel or change checked rows
- [x] 3.3 Show Restart selected and Run selected next to Delete selected for those kinds; verify with tests that checking two Deployments and choosing Restart selected opens a confirmation listing both, and that a Pods table shows neither button
- [x] 3.4 Add Restart and Run now buttons to `DetailView` for those kinds, emitting `DetailEvent::Restart` and `DetailEvent::Run`; verify with tests that a StatefulSet's panel offers Restart, a CronJob's offers Run now, and a Job's offers neither

## 4. Documentation

- [x] 4.1 Update the Cluster changes rule in `CLAUDE.md` to name restart and run now as exceptions next to deletion, each after an explicit confirmation naming the cluster, namespace, and objects, and verify the text reads correctly

## 5. Integration

- [x] 5.1 Run `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --all --check`, `python3 scripts/check-crate-graph.py`, and `python3 scripts/check-i18n.py`, and verify all pass
- [x] 5.2 Add ignored tests to `crates/kubepulse/tests/real_cluster.rs` that restart a fixture Deployment and wait for a new ReplicaSet with the annotation, and run a fixture CronJob and wait for a Job owned by it; verify they pass on the `kubepulse-e2e` minikube profile with the command in `CLAUDE.md`
