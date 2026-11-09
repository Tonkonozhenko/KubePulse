# Tasks

## 1. Namespaces in snapshots

- [x] 1.1 Add `Snapshot::namespaces` from D4, computed once per publish before filtering, and verify with a test in `crates/kubepulse-store/tests/views.rs` that a view restricted to one namespace still reports every namespace in the kind, and that a deleted object's namespace goes once it is empty

## 2. Per-cluster selection

- [x] 2.1 Add the selection, `selected_namespaces`, `set_namespaces`, and `NamespacesChanged` to `ClusterModel` from D1, and verify with UI tests that setting a selection affects only its `ClusterKey`, that setting an equal one emits nothing, and that it survives a disconnected state
- [x] 2.2 Change `ResourceBrowser::open` to take the key and model and to start from the cluster's selection, update `cluster_tab.rs` and `examples/soak.rs`, and verify with a UI test that opening Deployments after selecting on Pods shows only that namespace with no unfiltered snapshot in between
- [x] 2.3 Follow `NamespacesChanged` in namespaced browsers from D2, and verify with a UI test that two tabs on one cluster update together, a tab on another cluster does not, and the originating browser's search text is kept

## 3. Multiselect control

- [x] 3.1 Replace the namespace input with the combobox from D5 on namespaced kinds only, port `selecting_namespaces_shows_only_those_namespaces`, and verify with UI tests for selecting, clearing, the all-namespaces placeholder, searching, and a namespaced custom kind
- [x] 3.2 Leave cluster-scoped browsers out per D3, and verify with UI tests that a selection does not hide Nodes, that no control renders there, and that the selection applies again on returning to a namespaced kind
- [x] 3.3 Open the `Namespace` view and build the option union from D4 and D6, and verify with UI tests that a namespace with no objects of the kind is offered, that one created while open appears with the selection intact and can still be deselected, and that a deleted selected namespace stays selected and goes once removed
- [x] 3.4 Show the incomplete-list footer from D5, and verify with UI tests for a denied `Namespace` view offering the table's namespaces, and for a cluster that does not serve the kind

## 4. Checks

- [x] 4.1 Run `cargo test --workspace`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all --check`, `python3 scripts/check-crate-graph.py`, and `openspec validate add-namespace-multiselect --strict`, and verify each passes, including the existing 10k-row browsing tests
- [x] 4.2 Launch the app against a real cluster on Linux, and verify selecting namespaces across kinds and tabs, clearing, and hiding on cluster-scoped kinds
- [x] 4.3 Run the `soak` example, and verify memory and row updates stay within the limits in `general/resource-browsing` with the extra view open, or record the result and a mitigation in `design.md`

## Workflow follow-up

- Archive `define-app-architecture` before archiving this change, because this change modifies its `general/resource-browsing` spec.
