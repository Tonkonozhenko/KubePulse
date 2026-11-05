# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project status

KubePulse is a lightweight, native Kubernetes IDE written in Rust. The code is still a single-package `cargo new` scaffold. The architecture below is planned in the OpenSpec change `openspec/changes/define-app-architecture/` and is not implemented yet, so check what exists before assuming a crate or module does.

Read `design.md` in that change for the reasoning behind each decision. Its specs define required behavior, including the performance limits.

## Commands

```bash
cargo build                  # build
cargo run                    # run the app
cargo test                   # run all tests
cargo test <name>            # run tests whose name contains <name>
cargo test <name> -- --exact # run one test by its full path
cargo clippy --all-targets   # lint
cargo fmt --check            # check formatting
```

Once the workspace from `design.md` exists, add `--workspace` to run across all crates or `-p <crate>` to target one.

OpenSpec:

```bash
openspec list                                  # in-flight changes
openspec list --specs                          # main specs
openspec status --change <name>                # artifact progress
openspec validate <name> --strict              # run after editing a change
```

Use the `/opsx:propose`, `/opsx:explore`, `/opsx:apply`, `/opsx:sync`, and `/opsx:archive` commands for the workflow. `/opsx:propose` plans only, so implementation starts with an explicit `/opsx:apply`.

## Planned architecture

Dependencies point downward, and CI is to enforce this graph:

| Crate | Responsibility |
|---|---|
| `kubepulse` | Binary and composition root |
| `kubepulse-ui` | Workspace, tabs, table, editor, terminal, declarative UI renderer. The only crate that uses the UI toolkit. |
| `kubepulse-builtin` | Built-in views, tabs, and actions. Depends only on the extension contract. |
| `kubepulse-extension-host` | Registry, loaders, permission enforcement, activation, failure isolation |
| `kubepulse-extension-api` | The contract. Apache-2.0, with no toolkit or Kubernetes client types. |
| `kubepulse-store` | Per-cluster cache, row projection, sorted and filtered views |
| `kubepulse-cluster` | Kubeconfig, sessions, auth, discovery, watches, exec, logs, port-forward |

These rules hold the design together:

- **UI thread:** it never awaits network or disk work. Cluster and store work runs on Tokio background threads. Commands go down through channels, and state comes up as immutable, versioned snapshots.
- **Cluster isolation:** each cluster is a session with its own task tree. Cancelling the tree tears down everything it owns, including shells, logs, and port-forwards.
- **Scale:** the target is 10k pods in each cluster. Watches are shared and reference-counted per cluster and resource kind, the store keeps compact rows rather than full objects, and snapshots publish at most once per frame.
- **Extensions:** built-in features use the same contract as plugins, with no private access to core state. Every value crossing the contract is owned and serializable, so a sandboxed loader can be added later without changing it.
- **Extension UI:** extensions describe interfaces with host-defined elements and never receive toolkit types or credentials. Log, terminal, and editor surfaces are host-owned, so high-rate data stays out of the contract.
- **Cluster changes:** every change, including one from an extension, goes through dry run, diff review, user confirmation, then a replace carrying `resourceVersion`.

## Constraints

- **Product names:** never name other products in specs, OpenSpec artifacts, code, or comments. Describe capabilities directly instead.
- **Licensing:** the product is Business Source License 1.1 with free non-commercial use, and each version converts to AGPL-3.0 after four years. Call it source-available, never open source.
- **Dependencies:** no GPL-family dependencies. That rules out reusing the code editor from gpui's parent project.
- **UI stack:** use `gpui-kit` on an exact `gpui-pre` pin. Upgrade the kit and all snapshot crates together in one commit, because any snapshot can break the API. Do not substitute `gpui` or `gpui-ce`.
- **No phoning home:** no license keys, activation, or telemetry.
- **Platforms:** Linux (Wayland and X11), macOS, and Windows must have the same features.

## OpenSpec conventions

- Group capabilities in domain directories, and do not repeat the directory in the name: `general/`, `pod/`, `shell/`, and `arch/`. For example, `pod/log` rather than `pod/pod-logs`.
- `pod/shell` is the container terminal. The `shell/` directory is the application shell: workspace, cluster identity, and command palette.
- In `design.md`, write risks as `**Risk** → Mitigation`, because Markdown reads `[Risk]` as a link label.
