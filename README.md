# KubePulse

A lightweight, native Kubernetes IDE for Linux, macOS, and Windows, written in Rust.

KubePulse is in early development. Pre-built installers are published on the [releases page](https://github.com/Tonkonozhenko/KubePulse/releases), and you should expect missing features and breaking changes between versions.

## Features

- Lists every context in your default kubeconfig without touching the network. You connect and disconnect each cluster yourself.
- Resource browsing designed for clusters with 10,000 pods each.
- Pod logs, container shells, and port forwarding.
- Manifest editing with a dry run, a diff, and confirmation before every change.
- Several clusters open side by side, each marked with its own identity color.
- A command palette, light and dark themes, and an extension system that the built-in features use too.
- No accounts, license keys, or telemetry.

## Installing

Download the installer for your platform from the [latest release](https://github.com/Tonkonozhenko/KubePulse/releases/latest).

| Platform | File |
|---|---|
| Linux (Debian, Ubuntu, and derivatives) | `.deb` |
| macOS | `.dmg` |
| Windows | `-setup.exe` |

### Linux

```bash
sudo apt install ./kubepulse_<version>_amd64.deb
```

The package pulls in its runtime libraries and adds KubePulse to the application launcher. It runs under both Wayland and X11. You can also start it with `kubepulse` in a terminal.

### macOS

Open the `.dmg` and drag KubePulse into Applications. The app isn't signed yet, so the first time you open it, Control-click it in Applications and choose **Open**.

### Windows

Run the `-setup.exe` installer. The installer isn't signed yet, so Windows may show a SmartScreen warning: choose **More info**, then **Run anyway**.

### Verifying a download

Each release includes a `SHA256SUMS` file. On Linux, run this in the folder that holds the download:

```bash
sha256sum --check --ignore-missing SHA256SUMS
```

On macOS, use `shasum -a 256 -c --ignore-missing SHA256SUMS` instead.

## Building from source

You need a stable Rust toolchain. Linux also needs the development headers:

```bash
sudo apt install libfontconfig1-dev libfreetype-dev libwayland-dev \
  libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev
```

Then build, test, and run:

```bash
cargo build --workspace
cargo test --workspace
cargo run
```

## Licensing

KubePulse is open source under the [Apache License, Version 2.0](LICENSE). Licenses of third-party dependencies are listed in [assets/third-party-notices.txt](assets/third-party-notices.txt).

## Contributing

Contributions are licensed under the Apache License, Version 2.0, as its section 5 describes.
