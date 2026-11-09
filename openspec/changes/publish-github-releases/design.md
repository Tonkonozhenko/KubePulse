# Design

## Context

`ci.yml` has a `package` job that runs on pushes to `main`. It builds with `cargo-packager` 0.11.8 on `ubuntu-latest` (`deb`), `macos-latest` (`app,dmg`), and `windows-latest` (`nsis`), and uploads `target/packager/*` as workflow artifacts. The workspace version is `0.1.0` in the root `Cargo.toml`, and the repository has no tags yet. Requirements are in `specs/arch/release/spec.md`.

## Goals / Non-Goals

**Goals:**
- One packaging definition shared by CI and releases.
- Publishing is all or nothing: a release appears only with every installer and the checksum file.

**Non-Goals:**
- Code signing and notarization for macOS and Windows. Installers stay unsigned until signing keys exist, which needs its own change.
- Extra architectures. Releases ship what the hosted runners build: x86_64 Linux and Windows, and arm64 macOS.
- Auto-update or an update check inside the application, which would contact a network service.
- Publishing to package repositories.

## Decisions

### Reusable packaging workflow
Move the `package` job into `.github/workflows/package.yml` with `on: workflow_call`, and call it from both `ci.yml` and `release.yml`. Each matrix leg uploads its `target/packager` output as an artifact named `kubepulse-<os>`, as today. Copying the job into `release.yml` was rejected because the two copies would drift, and releases would ship installers built differently from the ones CI tests.

The `.app` bundle is a directory, so it cannot be attached to a release directly. The release attaches the `.dmg`, which contains it. Attached files are the regular files at the top of each artifact, which excludes the `.app` directory and the `.cargo-packager` cache.

### Tag trigger and version check
`release.yml` runs on `push` of tags matching `v*`. Its first job reads the workspace version with `cargo metadata --no-deps --format-version 1 | jq` and compares it with the tag minus the `v` prefix, failing with both values in the message. Packaging `needs` this job, so a mismatched tag builds nothing.

### Publishing with the `gh` CLI
The final job `needs` the packaging workflow, downloads every artifact, writes `SHA256SUMS` with `sha256sum`, and runs `gh release create "$tag" --verify-tag --generate-notes` with the files attached. `gh release create` fails when a release for the tag exists, which keeps published releases unchanged without extra checks. The CLI is preinstalled on hosted runners, so a third-party release action is not needed. Only this job gets `permissions: contents: write`; the rest of the workflow keeps `contents: read`.

`--generate-notes` builds notes from the commits and pull requests since the previous tag. It is the only notes source until the project keeps a changelog.

### Pre-release detection
A tag containing `-` after the version is a pre-release, following semantic versioning. The publish job passes `--prerelease` for those and `--latest` otherwise.

### Tests are not rerun
The release workflow does not rerun the CI test jobs. Releases are tagged on commits of `main`, where CI has already run, and running tests again would double the release time.

## Risks / Trade-offs

**A tag is pushed on a commit whose CI failed** → Cutting a release is documented as tagging a `main` commit with a green CI run. A bad release is deleted by hand and the version bumped.

**macOS and Windows warn before opening unsigned installers** → Accepted for the first releases. Signing is a later change that needs its own keys and secrets.

**The hosted runner images change, for example `macos-latest` moving to a new macOS version** → The packaging workflow is the same one CI runs on every push to `main`, so a runner change breaks CI before it breaks a release.

**A partial failure leaves a pushed tag with no release** → Nothing is published, so the fix is to delete the tag, fix the cause, and push the tag again.

## Migration Plan

1. Land the workflows on `main` and confirm CI still packages on push.
2. Push `v0.1.0` on that commit to publish the first release.

Rollback is deleting `release.yml` and any release it created.
