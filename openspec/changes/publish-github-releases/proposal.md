# Proposal

## Why

CI already builds a `.deb`, a `.app` and `.dmg`, and an NSIS installer on every push to `main`, but they are kept only as short-lived workflow artifacts. Users have no stable place to download a versioned build, so nobody can install KubePulse without building it from source.

## What Changes

- Add a release workflow that runs when a version tag such as `v0.1.0` is pushed.
- Refuse to release when the tag does not match the workspace version.
- Build the installers for Linux, macOS, and Windows from the tagged commit, using the same packaging as CI.
- Publish one release for the tag with every installer and a SHA-256 checksum file attached, and release notes generated from the commits since the previous tag.
- Mark versions with a pre-release suffix, such as `0.2.0-beta.1`, as pre-releases.
- Move the packaging steps out of `ci.yml` into a reusable workflow, so CI and releases build packages the same way.

## Capabilities

### New Capabilities
- `arch/release`: how a tagged version becomes a published release, what each release contains, and how users verify the downloads.

### Modified Capabilities

None. The installers themselves are unchanged and stay defined by `arch/packaging`.

## Impact

- New `.github/workflows/release.yml` and `.github/workflows/package.yml`.
- `.github/workflows/ci.yml`: the `package` job calls the reusable workflow.
- The release job needs `contents: write` on the repository token. No other secrets are added.
- `CLAUDE.md`: document how to cut a release.
- No change to crates, dependencies, or the packaged application.
