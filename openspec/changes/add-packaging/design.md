# Design

## Context

See `proposal.md` for motivation, and `specs/arch/packaging/spec.md` for required behavior.

The workspace has one binary, `kubepulse`, and no release profile of its own. Its window already sets `app_id` to `kubepulse`, and Windows embeds `assets/logo.ico` through `build.rs`. Themes, locales, and icons are compiled in, so the binary needs no resource files beside it.

`ldd` on the binary lists `libxcb`, `libxkbcommon`, `libxkbcommon-x11`, `libgcc_s`, and `libc`. The binary also opens `libwayland-client`, `libwayland-egl`, `libEGL`, and `libvulkan` at runtime, and `ldd` cannot see those.

The developer machine runs Ubuntu 26.04 on x86_64, with `dpkg-deb` and `apt` present and `cargo-packager` not yet installed. The repository has no git remote, so there is no homepage to declare.

## Goals / Non-Goals

**Goals:**

- One command per platform builds an installer from a clean checkout.
- The Linux package installs and removes cleanly through `apt`.
- macOS and Windows packaging is exercised by CI, not only written down.

**Non-Goals:**

- Signing, notarization, publishing, and automatic updates. Unsigned macOS and Windows packages show a warning on first launch.
- Compatibility across Linux distributions. A package runs on systems with at least the glibc of the machine that built it.

## Decisions

### Configure through `[package.metadata.packager]`

The configuration lives in `crates/kubepulse/Cargo.toml`, so name, version, and license come from Cargo and the version has one source.

*Alternative:* a root `Packager.toml` would repeat the version and could drift from the workspace.

Key settings:

| Setting | Value |
|---|---|
| `product-name` | `KubePulse` |
| `identifier` | `dev.kubepulse.app` |
| `before-packaging-command` | `cargo build --release --locked -p kubepulse` |
| `out-dir` | `target/packager` at the workspace root, already ignored by git |
| `binaries` | `kubepulse`, marked as the main binary |
| `icons` | `assets/logo-256.png` and `assets/logo.ico` |
| `license-file` | `LICENSE` |
| `category` | `DeveloperTool` |

Paths are written relative to wherever cargo-packager resolves them in a workspace, which the first packaging run confirms.

### Name the formats on the command line

Each platform runs `cargo packager --release --formats <list>`: `deb` on Linux, `app,dmg` on macOS, and `nsis` on Windows.

*Alternative:* omitting `--formats` uses the tool's platform default, which may include formats nobody asked for. Naming them keeps the output the same if that default changes.

### Use `.deb` for the local install

The machine is Ubuntu, and `apt` gives dependency resolution, clean removal, and upgrades. It is also the format whose failures are easiest to inspect, with `dpkg-deb -I` and `dpkg-deb -c`.

*Alternative:* AppImage works across distributions, but it needs FUSE 2, which this machine does not have, and it bundles graphics libraries that are safer taken from the host.

### Set the Debian package name explicitly

Without `deb.package-name`, the control file's `Package` field comes from kebab-casing the product name, which is not guaranteed to give `kubepulse`. Setting it keeps the package, the binary, and the `apt remove` command under one name.

### Reuse `assets/kubepulse.desktop` as the desktop template

`deb.desktop-template` points at the existing file, so the installed entry keeps `StartupWMClass`, which ties the window to the launcher entry. The tool's documented template variables don't cover that key. The file has no template variables, so it renders unchanged, and it already uses `Exec=kubepulse` and `Icon=kubepulse`, matching the tool's install paths.

### Declare runtime libraries by hand

`deb.depends` lists `libc6`, `libgcc-s1`, `libxcb1`, `libxkbcommon0`, `libxkbcommon-x11-0`, `libwayland-client0`, `libwayland-egl1`, `libegl1`, and `libvulkan1`.

*Alternative:* deriving the list from `ldd` or `dpkg-shlibdeps` would miss the four libraries opened at runtime, and those are the ones that fail only for users.

### Ship the license through `deb.files`

`license-file` is for installers that display a license, and the tool's `.deb` code shows no step that installs it. `deb.files` maps `LICENSE` to `/usr/share/doc/kubepulse/copyright`, the Debian location.

### Add one icon set instead of an `.icns` file

The tool builds the macOS `.icns` from the PNG in `icons`, and the `.deb` takes only PNGs and skips the rest. A 256px logo is enough for this stage, and no new asset is needed.

### Run packaging in CI on pushes to `main`

A `package` job in `ci.yml` runs on a three-OS matrix and uploads each result as a workflow artifact. It uses `if: github.event_name == 'push'`, because release builds are too slow to run on every pull request. It installs `cargo-packager` with `--locked --version 0.11.8`, so a tool update cannot change packages unnoticed.

*Alternative:* leaving macOS and Windows unbuilt would leave that configuration untested, against the project's rule of the same features on every platform.

## Risks / Trade-offs

- **The installed app starts from the launcher with the desktop session's environment, not the shell's** → `KUBECONFIG` exported in `.zshrc` is not visible, so `~/.kube/config` is used, and credential helpers on a shell-only `PATH` fail with a session error. Accepted for this change: the failure is visible and stays inside one cluster session. Fixing it means reading the login shell's environment, which is application code and a separate change.
- **Package install checks reach real clusters** → The checks only start the app, which lists contexts without network access, and never connect. Default kubeconfig contexts can be work or production clusters.
- **Same-version rebuilds do not upgrade through `apt`** → Reinstall with `apt install --reinstall`, as documented in `CLAUDE.md`.
- **`logo.ico` breaks the macOS icon build, because every entry in `icons` is decoded** → The CI job shows it, and the fix is to scope that entry to Windows through `nsis` settings.
- **`dev.kubepulse.app` is chosen without an owned domain** → macOS treats a changed identifier as a different app. Accepted while there is no release, and it must be settled before the first public one.
- **The hand-written dependency list drifts when the UI stack is upgraded** → The string scan in the tasks is repeated with each kit upgrade, and a missed library shows up as a startup failure.

## Migration Plan

Nothing to migrate. To roll back, remove the package with `sudo apt remove kubepulse`, and revert the commit.

## Open Questions

- What is the final application identifier? It can change at any time before the first public release.
