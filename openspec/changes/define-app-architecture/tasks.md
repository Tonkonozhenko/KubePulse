# Tasks

## 1. Workspace and project foundations

- [ ] 1.1 Replace the scaffold with a Cargo workspace containing the crates from design D1, rename the package to `kubepulse`, and verify `cargo build --workspace` succeeds
- [ ] 1.2 Add a CI check that enforces the crate dependency graph from D1, and verify it fails on a test branch where `kubepulse-builtin` depends on `kubepulse-store`
- [ ] 1.3 Set up CI builds and tests on Linux, macOS, and Windows, and verify all three pass on the empty workspace
- [ ] 1.4 Add the root `LICENSE` with the BUSL 1.1 parameters from D17 and the Apache-2.0 license on `kubepulse-extension-api`, and verify each crate's manifest declares the intended license
- [ ] 1.5 Add a license check in CI with a GPL-family exclusion, and verify a deliberately added GPL dependency fails it
- [ ] 1.6 Add a contributor license agreement check to pull requests and a README licensing section that says source-available, and verify the check blocks a pull request from a contributor who has not accepted

## 2. Feasibility gate

Stop after this group if any target is missed, and revisit the design before starting feature work.

- [ ] 2.1 Build a throwaway window with the pinned `gpui-kit` table and synthetic data for 10,000 pods, and record the frame rate while scrolling on Linux, macOS, and Windows
- [ ] 2.2 Drive the same table with 1,000 row changes per second for 30 seconds, and verify it stays at 60 frames per second or higher on the reference machine
- [ ] 2.3 Load a 5,000-line YAML document in the kit's code editor with highlighting and the diff component, and verify typing stays responsive on all three platforms
- [ ] 2.4 Render an `alacritty_terminal` grid in a custom element running a full-screen program, and verify rendering and input work on all three platforms
- [ ] 2.5 Measure memory with the 10k-pod prototype, and record whether the limits in `general/resource-browsing` hold, updating the spec if they need to change
- [ ] 2.6 Write the gate results in `docs/feasibility.md`, including any fallback chosen for a missed target, and verify each target has a measured value

## 3. Cluster sessions

- [ ] 3.1 Implement kubeconfig discovery with `KUBECONFIG` merging, file watching, and per-file error reporting, and verify with unit tests using fixture files
- [ ] 3.2 Implement the session state machine and task tree from D4, and verify with tests for each state transition against a fake API server
- [ ] 3.3 Implement auth through kubeconfig mechanisms with timeouts and cancellation, and verify a slow credential helper can be cancelled without blocking other sessions
- [ ] 3.4 Implement reconnection with backoff, and verify a fake API server outage moves the session to degraded and back to connected
- [ ] 3.5 Implement API discovery, including custom resource changes, and verify a kind added to the fake server appears without reconnecting
- [ ] 3.6 Add a test that scans log output during authentication, and verify no credentials appear in it
- [ ] 3.7 Document the session lifecycle in `docs/architecture/cluster-sessions.md`, and verify it matches the implemented states

## 4. Resource store

- [ ] 4.1 Implement reference-counted watches with pagination, bookmarks, and the release delay from D5, and verify a test confirms two views share one watch
- [ ] 4.2 Implement the row transform and string interning from D6, and verify with a benchmark that measures memory per pod
- [ ] 4.3 Implement view queries for filter and sort with once-per-frame snapshot publication from D7, and verify 10k rows sort and filter within 100 milliseconds in a benchmark
- [ ] 4.4 Add the synthetic load generator from D18, and verify the store converges within 2 seconds after a 1,000-changes-per-second burst
- [ ] 4.5 Implement namespace-scoped fallback for forbidden cluster-wide lists, and verify it against a fake server that denies them
- [ ] 4.6 Add scale benchmarks to CI with regression thresholds, and verify a deliberate slowdown fails them

## 5. Extension contract and host

- [ ] 5.1 Define manifest, contribution, declarative UI, and host operation types in `kubepulse-extension-api`, and verify round-trip serialization tests pass for every type
- [ ] 5.2 Implement the registry with enable and disable behavior and the built-in loader, and verify a test extension's contributions appear and disappear without a restart
- [ ] 5.3 Implement capability handles that enforce declared permissions, and verify undeclared operations are refused in tests
- [ ] 5.4 Implement activation triggers, and verify a test extension stays inactive until its trigger fires
- [ ] 5.5 Implement failure handling with panic capture, time limits, and auto-disable, and verify a panicking test extension leaves the host running
- [ ] 5.6 Implement contract version checks, and verify an incompatible manifest is refused with a clear message
- [ ] 5.7 Write the extension authoring guide in `docs/extensions.md`, and verify its example extension builds

## 6. Application shell

- [ ] 6.1 Build the main window, dock-based tabs and panes, and sidebar, and verify with headless UI tests covering splitting and tab moves
- [ ] 6.2 Connect the UI to the background runtime using the channels and snapshots from D3, and verify a UI test stays responsive while a fake cluster stalls
- [ ] 6.3 Implement the cluster list, connection states, and connect and disconnect actions, including the active-work warning, and verify with UI tests against a fake cluster
- [ ] 6.4 Implement cluster identity and protected clusters with the keys from D16, and verify a reused context name on another server does not inherit settings
- [ ] 6.5 Implement workspace persistence and restore without automatic reconnection, and verify a restart test restores the layout in a disconnected state
- [ ] 6.6 Implement the command palette, including extension commands, and verify opening a kind works using only the keyboard
- [ ] 6.7 Add a startup time check, and verify the window appears within 1 second on the reference machine with unreachable clusters

## 7. Resource browsing

- [ ] 7.1 Implement the declarative UI renderer for all contract elements, including the unknown-element placeholder, and verify with UI tests for each element
- [ ] 7.2 Implement the generic resource table with declarative columns, namespace selection, sorting, filtering, and stable selection, and verify with UI tests at 10k rows
- [ ] 7.3 Implement printer columns for custom resources, and verify a fixture custom resource shows its declared columns
- [ ] 7.4 Implement computed columns for visible rows only, and verify scrolling does not block on a deliberately slow extension
- [ ] 7.5 Add built-in extensions for workload, network, config, storage, and access-control kinds, and verify each kind lists with its default columns
- [ ] 7.6 Implement the detail view with events and owner navigation, and verify with a UI test on a Deployment-owned pod
- [ ] 7.7 Implement hidden Secret values and deletion confirmation, and verify with UI tests covering a protected cluster
- [ ] 7.8 Verify the memory limits and the 5-minute watch release with a 10-minute soak test on the reference machine

## 8. Manifest editing

- [ ] 8.1 Implement manifest loading with stripping and restoring of server-managed fields, and verify with round-trip unit tests
- [ ] 8.2 Implement the editor surface with YAML highlighting, search, undo, and syntax errors, and verify a syntax error disables applying
- [ ] 8.3 Implement the dry-run step, and verify a rejected change shows the server's message and leaves the cluster unchanged
- [ ] 8.4 Implement diff review with target details and the no-change case, and verify an admission mutation appears in the diff using a fake server
- [ ] 8.5 Implement apply with `resourceVersion` checks and conflict recovery, and verify a concurrent change is refused and can be reapplied on the newer version
- [ ] 8.6 Implement read-only mode, the Secret reveal confirmation, and unsaved-edit protection, and verify with UI tests
- [ ] 8.7 Route extension mutations through this flow, and verify a test extension cannot change a resource without user confirmation

## 9. Pod logs

- [ ] 9.1 Implement container selection, init containers, and previous-instance logs, and verify with a fake server
- [ ] 9.2 Implement streaming into the ring buffer from D14, with the dropped-lines notice, and verify memory stays bounded at 10,000 lines per second
- [ ] 9.3 Implement the log surface with follow behavior, timestamps, and search, and verify with UI tests while lines stream
- [ ] 9.4 Implement stream resume with duplicate removal, and verify an interrupted fake stream shows no duplicated lines

## 10. Pod shell

- [ ] 10.1 Implement the exec stream with shell discovery and the fallback command prompt, and verify against a simulated cluster, including a container with no shell
- [ ] 10.2 Implement the terminal element from D13 with colors, scrollback, copy and paste, and resizing, and verify a full-screen program works on all three platforms
- [ ] 10.3 Implement session-end handling, reconnect, and the target display, and verify a dropped connection keeps the scrollback

## 11. Port-forwarding

- [ ] 11.1 Implement pod forwards with loopback-only binding and automatic port selection, and verify a test confirms the listener is not reachable on non-loopback addresses
- [ ] 11.2 Implement service forwards through EndpointSlices with re-resolution, and verify replacing the backend pod reconnects
- [ ] 11.3 Implement the global forward registry and its view, and verify forwards survive closing their tab and stop when their cluster disconnects
- [ ] 11.4 Implement failure reporting and restart, and verify a deleted target pod marks the forward as failed

## 12. Release readiness

- [ ] 12.1 Add the About screen with the license text and third-party notices, and verify both are present in release builds
- [ ] 12.2 Package signed builds for Linux, macOS, and Windows, and verify each installs and starts on a clean machine
- [ ] 12.3 Verify no license check or usage reporting exists by running the application offline and capturing its outbound traffic
- [ ] 12.4 Run the end-to-end workflow on all three platforms against simulated 10k-pod clusters: connect two clusters, browse, edit with diff, follow logs, open a shell, and forward a port

## Workflow follow-up

- Have a lawyer review the BUSL Additional Use Grant, the AGPL-3.0 Change License, and the contributor agreement before the first public release.
- Register a trademark for the name.
- Decide commercial pricing and how licenses are sold.
- Archive the change after the project's review requirements are satisfied.
