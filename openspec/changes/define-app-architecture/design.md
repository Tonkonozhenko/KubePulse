# Design

## Context

The repository holds a single-binary Rust scaffold (edition 2024) with no dependencies, so nothing existing constrains the architecture. See `proposal.md` for motivation and `specs/` for required behavior.

Three requirements shape almost every decision:

- 10k pods in each cluster, with hard frame-rate and memory limits.
- Isolated multi-cluster sessions.
- Plugins authored mostly by the maintainer today, with third-party plugins later.

Dependency state was checked on 2026-11-05:

| Dependency | Version | License | Notes |
|---|---|---|---|
| `gpui-kit` | 0.9.0 | Apache-2.0 | Includes table, code editor, diff, dock, tree, and charts. Performance claims are its own and unverified. |
| `gpui-pre` | 0.3.11 (exact pin) | Apache-2.0 | Frequent snapshots of upstream gpui. Any snapshot can break the API. |
| `kube` | 5.0.0 | Apache-2.0 | Kubernetes client and watcher runtime. |
| `alacritty_terminal` | 0.26.0 | Apache-2.0 | Terminal emulation core. |

## Goals / Non-Goals

**Goals:**

- Keep the UI thread free of network and disk work.
- Make core layers testable without a window or a real cluster.
- Keep the extension contract independent of how an extension is loaded.
- Confine UI toolkit types to one crate, so API churn stays contained.

**Non-Goals:**

- A sandboxed plugin loader, third-party plugin distribution, or a plugin marketplace. The contract is designed for them, but they are not built here.
- Cross-cluster aggregate views.
- Adding clusters by hand or importing kubeconfig files from other locations. For now, the cluster list is exactly what the default kubeconfig resolves to.
- Helm management, metrics charts, and node shells.
- Schema-aware completion in the editor. The first release relies on syntax checks and server-side dry runs.
- Technical license enforcement, telemetry, and auto-update.

## Decisions

### D1. Layered Cargo workspace

```text
kubepulse (binary, composition root)
├── kubepulse-ui ────────────┐  gpui-kit lives only here
├── kubepulse-builtin ───────┤  built-in extensions
│                            ▼
├── kubepulse-extension-host ──► kubepulse-extension-api
├── kubepulse-store               (Apache-2.0, no toolkit or client types)
└── kubepulse-cluster             kube and the async runtime live here
```

| Crate | Responsibility |
|---|---|
| `kubepulse-cluster` | Kubeconfig, sessions, auth, discovery, watches, exec, logs, port-forward |
| `kubepulse-store` | Per-cluster resource cache, row projection, sorted and filtered views |
| `kubepulse-extension-api` | The contract: manifests, contribution types, declarative UI tree, host operations |
| `kubepulse-extension-host` | Registry, permission enforcement, activation, failure isolation |
| `kubepulse-builtin` | Resource views, detail tabs, and actions. Depends only on the contract. |
| `kubepulse-ui` | Workspace, tabs, table, editor, terminal, and the declarative UI renderer |

CI asserts this dependency graph, so `kubepulse-builtin` cannot reach core crates and only `kubepulse-ui` depends on the toolkit.

*Alternative:* a single crate with modules. It is simpler at first, but nothing would stop built-ins from bypassing the contract, and that would break the path to a sandboxed loader.

### D2. `gpui-kit` on an exact `gpui-pre` pin

The kit supplies most of the UI surface the first release needs. Upgrades are deliberate, single commits that move the kit and every snapshot crate together.

*Alternatives:* the crates.io `gpui` release is over a year old. `gpui-ce` is active and stability-focused, but the kit does not target it, so the table, editor, diff, and dock would have to be built by hand.

### D3. Two execution worlds, connected by channels

Cluster and store work runs on a Tokio runtime on background threads. The UI runs on the toolkit's main thread. Commands flow down through channels, and state flows up as immutable, versioned snapshots. UI code never awaits network or disk work.

*Alternative:* running everything on the toolkit's executor. `kube` requires Tokio, and sharing a thread pool would let a cluster stall rendering.

### D4. One explicitly connected session per cluster

The cluster list comes only from parsing the kubeconfig, so it stays cheap with hundreds of contexts and needs no network access or credential helpers. A `ClusterSession` is created only by the user's connect action, and destroyed only by their disconnect action or application exit. Until then, a context is a plain list entry holding no client, tasks, or cache.

Automatic reconnection after a lost connection stays, because it restores a session the user chose to keep. Releasing an unused watch (D5) frees cached data but does not end the session.

*Alternatives:* connecting every context at startup would run credential helpers for clusters the user never opens and trigger login prompts. Connecting on open or disconnecting when idle would change a production connection without the user asking, and credential helpers could prompt unexpectedly.

A `ClusterSession` owns its client, auth state, connection state machine, watches, and streams in its own task tree. Cancelling that tree tears down everything, including shells, logs, and port-forwards, which satisfies disconnect cleanup and failure isolation.

Connection attempts and credential helpers run with a timeout and are cancellable. Reconnection uses exponential backoff with jitter.

When a cluster-wide list is forbidden, the session falls back to watches on the namespaces the user can access.

### D5. Shared, reference-counted watches

The store keeps one watch per cluster and resource kind, however many tabs use it. A watch starts on first use and stops 5 minutes after the last user leaves. Lists are paginated, and watches use bookmarks to avoid full relists.

*Alternative:* one watch per view, which multiplies load on the API server and duplicates memory.

### D6. Compact rows instead of full objects

Incoming objects pass through a transform that keeps metadata, labels, owner references, and the fields needed for columns. Repeated strings are interned. Everything else is dropped, and the full object is fetched when the user opens it.

Default columns for built-in kinds are declarative field paths supplied by extensions. Custom resources use the printer columns from their definition, evaluated locally.

*Alternative:* the server-side Table format. It is compact, but it cannot serve arbitrary field paths for extension columns.

### D7. Per-frame snapshot publication

Each open table has a view query with its own filter and sort, running in the background against the shared store. Changes accumulate in batches. A view publishes a new immutable snapshot at most once per frame, and the UI takes the latest one on each frame. Intermediate versions are skipped, so bursts lower update frequency rather than frame rate.

Rows are tracked by UID, which keeps selection stable as order changes.

### D8. The contract is built-ins first, message-shaped, and serializable

Extensions implement Rust traits, but every value crossing the boundary is an owned, serializable type, and all calls are asynchronous. Nothing crossing it references toolkit, client, or store types. A later loader only has to carry these same messages.

Extensions are compiled in for now. A sandboxed loader comes later, chosen when third-party plugins become real. WebAssembly components and `gpui-shell` are the candidates. `gpui-shell` matches the declarative model but is at an early milestone with no stable interface, so the choice stays deferred.

Round-trip serialization tests on every contract type keep this property honest.

### D9. Declarative UI with host-owned surfaces

Extensions return a tree of host-defined elements, and `kubepulse-ui` maps it to kit components. Log, terminal, and editor surfaces are host-owned elements that extensions open by reference, so high-rate data never crosses the contract.

Declarative columns are evaluated by the store. Computed columns are requested only for visible rows, in batches, with cached results.

### D10. Permissions are enforced for built-ins too

Each extension has a manifest declaring its contract version, activation triggers, contributions, and permissions. All host operations pass through a capability handle bound to those permissions. Built-ins go through the same check, so the enforcement path is exercised from day one.

Extension mutations are routed to the host's review flow from D12.

Built-in extensions run in the application's process, so failure isolation means catching panics at the host boundary and applying a time limit to calls. Release builds keep `panic=unwind`. Real memory isolation arrives with the sandboxed loader.

### D11. Reuse the kit's code editor

The editor is the kit's code editor with the YAML grammar. The upstream application's editor is GPL-licensed and cannot be used, and building a new editor would add substantial work that the kit's component may make unnecessary. The feasibility gate verifies this choice.

### D12. Edit flow: dry run, diff, replace with a version check

```text
open ─► strip server fields ─► edit ─► dry run ─► diff ─► confirm ─► replace
                                                                       │
                                     conflict: show newer version ◄────┘
```

- Status and bookkeeping metadata are removed for display and restored for submission.
- The diff compares the normalized live object with the normalized dry-run result, so admission changes are visible.
- Changes are applied by a full replace carrying the original `resourceVersion`, so the server rejects stale writes.
- Opening a Secret's manifest counts as an explicit reveal and asks for confirmation first.
- After a conflict, the user's edits are merged onto the newer version field by field, and lists merge as one value. Where both sides changed a value the user's wins, and the diff review shows it against the newer version.
- An extension's create, update, or delete opens the same tab. Time spent in that review does not count against the extension's call limit from D10.

*Alternative:* server-side apply. A field the user deleted but another manager owns would survive, which is surprising for an editor showing the whole object. Forcing ownership could overwrite other actors' changes.

### D13. Terminal built on `alacritty_terminal`

The kit has no terminal, so a custom element renders the emulator's grid. Input, output, and resize events go through the Kubernetes exec stream. Shell discovery tries common shells in order, then falls back to asking the user for a command.

*Alternative:* the `gpui-terminal` crate. It is a small 0.1.0 release with little adoption, and a dependency that central should not rest on it.

### D14. Logs in a bounded ring buffer

Lines go into a per-stream ring buffer limited to 100,000 lines and 64 MiB, whichever is reached first, and render through a virtual list. Batches are delivered per frame. After a dropped stream, the reader resumes from the last timestamp and removes duplicates.

The client library rounds the resume time to the nearest second, which could skip lines, so the reader rounds it down itself and drops the replayed lines. A stream that broke gets one more read after its container stops, to recover the last lines. Lines over 4 KiB are cut for display only, because shaping them stalls a frame, and stay whole in the buffer and in search.

### D15. Port-forwards belong to sessions, not tabs

A global registry lists forwards across sessions, each owned by its cluster session. Listeners bind only to loopback. A service forward resolves its target port and a ready pod through EndpointSlices, then re-resolves if that pod disappears.

### D16. Local persistence without secrets

Layout, cluster display settings, and preferences are stored in the platform's configuration directory. Cluster settings are keyed by context name and server URL, so a reused name on another server does not inherit a color or a protected flag.

### D17. License parameters

| Parameter | Value |
|---|---|
| License | Business Source License 1.1 |
| Additional Use Grant | Non-commercial production use |
| Change Date | Fourth anniversary of each version's first public release |
| Change License | AGPL-3.0 |

`kubepulse-extension-api` is Apache-2.0, so extensions are not tied to the application's license. License checks run in CI with an allowlist that excludes GPL-family licenses.

*Alternatives:* AGPL with a paid license is real open source, but companies could use it internally without paying. FSL explicitly permits internal use. PolyForm Noncommercial never converts to open source.

### D18. Performance definitions

The reference machine is a 4-core laptop with 16 GB of RAM and an integrated GPU. Scale tests use a synthetic generator that drives the store and table with 10k pods and update bursts, plus a fake API server for watch behavior. A simulated cluster covers end-to-end checks before each release.

## Risks / Trade-offs

- **`gpui-pre` snapshots break the API** → Pin exactly, confine toolkit use to `kubepulse-ui`, and upgrade deliberately.
- **The kit depends on a single vendor** → Keep the exact pin, and vendor the kit if maintenance stops. Everything used is Apache-2.0.
- **The kit's table or editor misses the targets** → The feasibility gate tests them first. The fallback is a custom table or editor built on `gpui-base`, which adds schedule but not architectural change.
- **The toolkit behaves differently across platforms** → Run the gate and CI on all three from the start.
- **Built-ins bypass the contract** → CI enforces the dependency graph, and built-ins have no other route.
- **In-process extensions can block or corrupt memory** → Acceptable while they are first-party. Third-party extensions wait for the sandboxed loader.
- **The contract is designed before any sandboxed loader exists** → Serialization tests catch shape problems, but a real loader may still expose gaps. The contract version allows a controlled break.
- **The memory limits are estimates** → They are provisional until the gate measures real numbers, and any change must go back through the spec.
- **A closed-source non-commercial fork is not prevented before conversion** → Accepted limit of the license. Commercial forks still need a license, and the trademark protects the name.
- **Companies may not pay** → No technical enforcement is planned. Sales rely on license terms and direct selling.

## Migration Plan

There is no existing behavior or data to migrate. The scaffold package `KubePulse` is replaced by the workspace, and rollback is a git revert.

## Open Questions

- What exact wording defines "non-commercial" in the Additional Use Grant? This needs legal review before the first public release, but does not change the build.
- Which sandboxed loader comes later? Deferred until third-party plugin work starts.
