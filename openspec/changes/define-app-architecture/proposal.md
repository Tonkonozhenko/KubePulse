# Proposal

## Why

KubePulse is an empty Rust scaffold. It is meant to become a lightweight, native Kubernetes IDE, and the decisions that are expensive to reverse need to be made before feature work starts: how clusters are isolated, how 10k pods stay smooth, how plugins attach, and how the product is licensed.

The first release needs a Kubernetes desktop workflow that stays fast at real cluster scale, is safe to use across production and non-production clusters, and can grow through plugins without rewriting the core.

## What Changes

- Establish a layered architecture with a cluster layer, a state layer, an extension layer, and a UI layer, each in its own crate with enforced dependency direction.
- Add multi-cluster support, where every connected cluster is an isolated session and a failing cluster cannot affect the others.
- Add a resource browser that covers every resource kind the cluster serves, including custom resources, and stays smooth at 10k pods in each cluster.
- Add manifest editing with a server-side dry run, a diff review before every apply, and conflict detection.
- Add pod logs, pod shell, and port-forwarding.
- Add a tabbed workspace where each tab is bound to one cluster. Cross-cluster aggregate views are out of scope.
- Add an extension contract that built-in features use exclusively, so that a sandboxed third-party plugin loader can be added later without changing it.
- Support Linux, macOS, and Windows from the first release.
- License the product under Business Source License 1.1. Non-commercial use is free, commercial use requires a paid license, and each version converts to AGPL-3.0 four years after release.

Because of that license, KubePulse is source-available rather than open source until a version converts. Project materials must describe it that way.

## Capabilities

### New Capabilities

- `general/cluster-sessions`: discovering clusters, connecting, authenticating, and isolating each connected cluster in its own session.
- `general/resource-browsing`: listing, watching, sorting, filtering, and inspecting any resource kind at 10k pods per cluster.
- `general/manifest-editing`: editing resource manifests through validation, diff review, apply, and conflict handling.
- `pod/log`: streaming, following, and searching container logs.
- `pod/shell`: interactive terminal sessions inside containers.
- `pod/port-forwarding`: forwarding local ports to pods and services, and managing active forwards.
- `shell/workspace`: tabs, panes, one-cluster tab binding, workspace restore, and startup time.
- `shell/cluster-identity`: cluster names, colors, and protected clusters.
- `shell/command-palette`: keyboard-driven access to actions and navigation.
- `arch/supported-platforms`: supported operating systems and display servers.
- `arch/extension-system`: contribution points, declarative extension UI, permissions, activation, and contract versioning.
- `general/licensing`: the license terms shown to users and the rules that keep the distribution compliant with them.

### Modified Capabilities

None. There are no existing specs.

## Impact

- **Code:** replaces the scaffold in `src/main.rs` with a Cargo workspace. The package is renamed from `KubePulse` to `kubepulse`, because Rust package names are conventionally lowercase.
- **Dependencies:** adds a Kubernetes client, a pinned UI component kit with its pinned UI engine, an async runtime, and a terminal emulation library. All must be compatible with the chosen license, so GPL-licensed dependencies are not allowed.
- **Distribution:** requires signed builds and CI for Linux, macOS, and Windows.
- **Contributors:** a contributor license agreement is required, because paid licenses can only cover code the project is entitled to relicense.
- **Legal:** the license parameters need legal review before the first public release. A trademark for the name is recommended.
