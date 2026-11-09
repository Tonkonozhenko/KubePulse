# Proposal

## Why

Two of the most common workload operations take manual YAML today. Restarting a workload's pods, to pick up a changed ConfigMap or Secret or recover from a bad state, means adding a restart-time annotation by hand or deleting pods one at a time and giving up the rolling update. Running a CronJob outside its schedule, to test it or catch up on a missed run, means writing a Job manifest by copying the CronJob's template.

## What Changes

- Add a Restart action to Deployments, StatefulSets, and DaemonSets, in the row actions column, the row's right-click menu, and the detail panel.
- Restart stamps the pod template with the current time, so the workload's controller replaces its pods with its own rollout strategy. No pod is deleted directly.
- Add a Run now action to CronJobs, in the same places. It creates a Job from the CronJob's job template, owned by the CronJob and marked as started by hand, and reports the new Job's name.
- Both actions work on several checked objects at once, next to Delete selected, when the table shows a kind that offers them.
- Both actions ask for confirmation naming the cluster and every object, the same way deletion does, and ask for a typed name on a protected cluster. They show no diff, because the user does not author the change. They become named exceptions to the diff review rule, next to deletion.
- The restart write carries the object's `resourceVersion`. On a conflict, the system re-reads the object and stamps it again, so it never overwrites anyone else's change.
- Running several objects reports progress and names each failure with the cluster's error, and a retry runs only what failed.

## Capabilities

### New Capabilities

- `general/workload-restart`: which kinds can be restarted, and how a restart is confirmed, written, and reported, for one object or several
- `general/cronjob-run`: starting a CronJob's Job by hand, and how it is confirmed, created, and reported, for one CronJob or several

### Modified Capabilities

- `general/resource-browsing`: the Row actions requirement lists Restart for Deployments, StatefulSets, and DaemonSets, and Run now for CronJobs

## Impact

- **Cluster:** `kubepulse-cluster` gains `restart_object` and `run_cronjob` in `objects.rs`.
- **UI:** `resource_browser.rs` gains the row actions and checked-row buttons, `detail.rs` gains the buttons, and `cluster_tab.rs` routes them to a confirmation dialog. `delete_dialog.rs` becomes one dialog shared by deletion, restart, and run. `services.rs`, `runtime.rs`, and the test fakes gain the two operations.
- **Contract:** none. Row actions are host-owned today, and the new ones join them.
- **Locale:** `locales/en.json` gains new keys, and `en-XA` is regenerated.
- **Architecture docs:** `CLAUDE.md` names restart and run as exceptions to the diff review rule, next to deletion.
- **Dependencies:** none.
