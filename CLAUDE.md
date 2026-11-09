# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project status

KubePulse is a lightweight, native Kubernetes IDE written in Rust. The Cargo workspace exists, but its crates are mostly empty. The architecture below is planned in the OpenSpec change `openspec/changes/define-app-architecture/` and is only partly implemented, so check what exists before assuming a module does.

Read `design.md` in that change for the reasoning behind each decision. Its specs define required behavior, including the performance limits.

## Commands

```bash
cargo build --workspace                  # build
cargo run                                # run the app
cargo test --workspace                   # run all tests
cargo test --workspace <name>            # run tests whose name contains <name>
cargo test -p <crate> <name> -- --exact  # run one test by its full path
cargo clippy --workspace --all-targets   # lint
cargo fmt --all --check                  # check formatting
python3 scripts/check-crate-graph.py     # check the crate dependency graph
python3 scripts/check-i18n.py            # check catalogs and find literals in views
python3 scripts/check-i18n.py --write    # also regenerate the en-XA pseudo-locale
cargo deny check licenses                # check dependency licenses
cargo about generate --locked --fail -o assets/third-party-notices.txt about.hbs  # regenerate the third-party notices after dependency changes
```

Packaging uses `cargo-packager`, pinned to the version in `.github/workflows/ci.yml`. Packages are written to `target/packager`:

```bash
cargo install cargo-packager --locked --version 0.11.8
cargo packager --release --formats deb       # Linux
cargo packager --release --formats app,dmg   # macOS
cargo packager --release --formats nsis      # Windows
sudo apt install ./target/packager/kubepulse_0.1.0_amd64.deb             # install on Debian or Ubuntu
sudo apt install --reinstall ./target/packager/kubepulse_0.1.0_amd64.deb # replace a build of the same version
```

Cluster tests run in their own minikube profile, with its own kubeconfig:

```bash
KUBECONFIG=$PWD/target/e2e.kubeconfig minikube start -p kubepulse-e2e --driver=docker --cpus=2 --memory=2g
KUBEPULSE_E2E_KUBECONFIG=$PWD/target/e2e.kubeconfig cargo test -p kubepulse --test real_cluster -- --ignored
minikube delete -p kubepulse-e2e
```

OpenSpec:

```bash
openspec list                                  # in-flight changes
openspec list --specs                          # main specs
openspec status --change <name>                # artifact progress
openspec validate <name> --strict              # run after editing a change
```

Use the `/opsx:propose`, `/opsx:explore`, `/opsx:apply`, `/opsx:sync`, and `/opsx:archive` commands for the workflow. `/opsx:propose` plans only, so implementation starts with an explicit `/opsx:apply`.

## Planned architecture

Dependencies point downward, and CI enforces this graph with `scripts/check-crate-graph.py`:

| Crate | Responsibility |
|---|---|
| `kubepulse` | Binary and composition root |
| `kubepulse-ui` | Workspace, tabs, table, editor, terminal, declarative UI renderer. The only crate that uses the UI toolkit. |
| `kubepulse-builtin` | Built-in views, tabs, and actions. Depends only on the extension contract. |
| `kubepulse-extension-host` | Registry, permission enforcement, activation, failure isolation |
| `kubepulse-extension-api` | The contract. Apache-2.0, with no toolkit or Kubernetes client types. |
| `kubepulse-store` | Per-cluster cache, row projection, sorted and filtered views |
| `kubepulse-cluster` | Kubeconfig, sessions, auth, discovery, watches, exec, logs, port-forward |

These rules hold the design together:

- **UI thread:** it never awaits network or disk work. Cluster and store work runs on Tokio background threads. Commands go down through channels, and state comes up as immutable, versioned snapshots.
- **Cluster connection:** every context in the default kubeconfig is listed automatically, and listing never touches the network or runs credential helpers. The user connects and disconnects clusters explicitly: nothing connects on open, and nothing disconnects when idle or when the last tab closes.
- **Cluster isolation:** each cluster is a session with its own task tree. Cancelling the tree tears down everything it owns, including shells, logs, and port-forwards.
- **Scale:** the target is 10k pods in each cluster. Watches are shared and reference-counted per cluster and resource kind, the store keeps compact rows rather than full objects, and snapshots publish at most once per frame.
- **Extensions:** built-in features use the same contract as plugins, with no private access to core state. Every value crossing the contract is owned and serializable, so a sandboxed loader can be added later without changing it.
- **Extension UI:** extensions describe interfaces with host-defined elements and never receive toolkit types or credentials. Log, terminal, and editor surfaces are host-owned, so high-rate data stays out of the contract.
- **Cluster changes:** every change, including one from an extension, goes through dry run, diff review, user confirmation, then a replace carrying `resourceVersion`. Deleting is the exception: it runs after an explicit confirmation naming the cluster, namespace, and object.

## Constraints

- **Product names:** never name other products in specs, OpenSpec artifacts, code, or comments. Describe capabilities directly instead.
- **Licensing:** the product is open source under Apache-2.0.
- **Dependencies:** no GPL-family dependencies. That rules out reusing the code editor from gpui's parent project.
- **Reuse:** prefer good open-source libraries with compatible licenses over writing our own. Build something only when no suitable library exists.
- **Feasibility gate:** deferred. Skip the feasibility testing in task group 2 for now and do not treat it as blocking feature work.
- **UI stack:** use `gpui-kit` on an exact `gpui-pre` pin. Upgrade the kit and all snapshot crates together in one commit, because any snapshot can break the API. Do not substitute `gpui` or `gpui-ce`.
- **No phoning home:** no license keys, activation, or telemetry.
- **Platforms:** Linux (Wayland and X11), macOS, and Windows must have the same features.
- **Cluster testing:** run every test that needs a cluster in the separate `kubepulse-e2e` minikube profile. Never test against a context from the default kubeconfig, because those can be work or production clusters.

## Coding standards

- **Simplicity:** no defensive code, no extra abstraction layers.
- **Errors:** no fallbacks that hide failures. Show errors to the user or log them, and let a failure stop only its own cluster session or extension.
- **Comments:** none unless the code is non-obvious.
- **Language:** use the latest stable Rust features.
- **Tests:** do not write tests for obvious error checks.
- **Text:** user-visible text in `kubepulse-ui` comes from `locales/en.json` through `t!`, and titles in `kubepulse-builtin` need an entry in its `locales/en.json`. Avoid plurals, because catalogs have no plural rules. Show data from clusters and extensions exactly as received, after a translated summary if it is an error.
- **Colors:** views take colors from `cx.theme()`. The only literal colors are cluster identity colors.

## Agent flow

- Do not create temporary files to test something. Checked-in examples and tests are fine.
- Do shell-side data processing with `jq` (JSON) and `yq` (YAML). Never use inline Python or Perl scripts, including for in-place file edits. Checked-in scripts such as `scripts/check-crate-graph.py` are fine.

## Git

- Commit and push through a subagent running the Haiku model.
- Do not add a session link or Claude signature, such as `Co-Authored-By` or `Claude-Session`, to commit messages.

## OpenSpec conventions

- Group capabilities in domain directories, and do not repeat the directory in the name: `general/`, `pod/`, `shell/`, and `arch/`. For example, `pod/log` rather than `pod/pod-logs`.
- `pod/shell` is the container terminal. The `shell/` directory is the application shell: workspace, cluster identity, and command palette.
- In `design.md`, write risks as `**Risk** → Mitigation`, because Markdown reads `[Risk]` as a link label.
