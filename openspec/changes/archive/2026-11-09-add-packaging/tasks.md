# Tasks

## 1. Configure packaging

- [x] 1.1 Install the tool with `cargo install cargo-packager --locked --version 0.11.8`, and verify `cargo packager --version` prints 0.11.8
- [x] 1.2 Add `description` and `[package.metadata.packager]` to `crates/kubepulse/Cargo.toml` as listed in `design.md`, and verify `cargo build --workspace --locked` passes with `Cargo.lock` unchanged
- [x] 1.3 Add the `deb` settings: `package-name`, `depends`, `desktop-template`, and `files`, and verify `cargo packager --release --formats deb` writes `kubepulse_0.1.0_amd64.deb` under `target/packager`
- [x] 1.4 Inspect the package, and verify `dpkg-deb -I` shows `Package: kubepulse`, version 0.1.0, and all declared dependencies, and `dpkg-deb -c` lists `/usr/bin/kubepulse`, the 256x256 hicolor icon, `/usr/share/applications/kubepulse.desktop`, and `/usr/share/doc/kubepulse/copyright`
- [x] 1.5 Extract the desktop entry from the package, and verify it matches `assets/kubepulse.desktop`, including `StartupWMClass=kubepulse`
- [x] 1.6 Check the runtime library list against `grep -a -o` on the release binary, and verify each library name found has a package in `depends`
- [x] 1.7 Add the packaging commands, the `--reinstall` note, and the pinned tool version to the Commands section of `CLAUDE.md`, and verify the documented command builds the package as written

## 2. Build every platform in CI

- [x] 2.1 Add a `package` job to `.github/workflows/ci.yml` with a matrix of `ubuntu-latest` with `deb`, `macos-latest` with `app,dmg`, and `windows-latest` with `nsis`, limited to push events, that installs the pinned tool and uploads `target/packager`, and verify the file parses with `yq`
- [ ] 2.2 After the change reaches `main`, verify the job passes on all three platforms and uploads a package from each, and fix any macOS or Windows icon or path errors it reveals

## 3. Install on this machine

These steps start the app but never connect a cluster, because default kubeconfig contexts can be work or production clusters.

- [x] 3.1 Have the user run `! sudo apt install ./target/packager/kubepulse_0.1.0_amd64.deb`, because it needs a password, and verify `dpkg -s kubepulse` shows `install ok installed` at version 0.1.0
- [x] 3.2 Run `kubepulse` from a terminal outside the repository, and verify the window opens with the context list and no errors on stderr
- [x] 3.3 Have the user start KubePulse from the desktop launcher, and verify it is listed with the logo, and that the running window appears under the KubePulse entry and not as an unknown application
- [x] 3.4 Have the user remove the package with `sudo apt remove kubepulse`, verify the binary, desktop entry, and icon are gone and the settings folder is unchanged, then reinstall so the app stays installed

## Workflow follow-up

- Run `openspec validate add-packaging --strict` after any edit to the artifacts.
- Archive the change once CI has packaged all three platforms.
