# Proposal

## Why

KubePulse only runs from a checkout through `cargo run`, so it has no installer, no entry in the desktop launcher, and no way to try it without a Rust toolchain. `assets/kubepulse.desktop` and the logo files exist, but nothing installs them. A repeatable packaging step is the first thing a usable release needs.

## What Changes

- Configure `cargo-packager` for the `kubepulse` binary, so one command builds a release installer.
- Produce a `.deb` on Linux, a `.app` and `.dmg` on macOS, and an NSIS installer on Windows.
- Install the binary, launcher entry, icon, and license on Linux, and declare the runtime libraries the app loads, including the ones it opens only at runtime.
- Build all three platforms' packages in CI on pushes to `main`, and keep them as workflow artifacts, because the macOS and Windows configuration cannot be checked from Linux.
- Document the packaging commands in `CLAUDE.md`.
- Build the `.deb` and install it on the developer's Ubuntu 26.04 machine, then confirm it launches from the desktop launcher.

Not included: code signing or notarization, publishing a release or a package repository, automatic updates, AppImage, Flatpak, Snap, Pacman, or MSI formats, and packaging for other CPU architectures.

## Capabilities

### New Capabilities

- `arch/packaging`: how the application is packaged and installed on each platform: formats, desktop integration, runtime dependencies, license, and removal.

### Modified Capabilities

None. Main specs are still empty, and packaging adds to `arch/supported-platforms` from `define-app-architecture` without changing its requirements.

## Impact

- `crates/kubepulse/Cargo.toml`: packager metadata and a package description.
- `assets/kubepulse.desktop`: used as the installed launcher entry, unchanged.
- `.github/workflows/ci.yml`: a new packaging job.
- `CLAUDE.md`: new commands.
- **Tooling:** `cargo-packager` 0.11.8 (Apache-2.0 OR MIT) is a build tool installed with `cargo install`. It is not a workspace dependency, so `Cargo.lock` and `cargo deny` are unaffected.
- **Application code:** no changes.
- **Developer machine:** installing the `.deb` needs `sudo` and adds a system package named `kubepulse`.
