# Tasks

## 1. Shared packaging workflow

- [x] 1.1 Create `.github/workflows/package.yml` with `on: workflow_call`, holding the matrix, system libraries, toolchain, cache, `cargo-packager` 0.11.8 install, packaging, and artifact upload moved from the `package` job in `ci.yml`; verify `yq '.on' .github/workflows/package.yml` shows `workflow_call`
- [x] 1.2 Replace the `package` job in `ci.yml` with a job that `uses: ./.github/workflows/package.yml`, keeping `if: github.event_name == 'push'`; verify after pushing to `main` that the CI run uploads the three `kubepulse-<os>` artifacts as before

## 2. Release workflow

- [x] 2.1 Create `.github/workflows/release.yml` triggered on `push` of tags `v*`, with top-level `permissions: contents: read`; verify `yq '.on' .github/workflows/release.yml` shows only the tag trigger
- [x] 2.2 Add a `version` job that compares the tag without `v` against the workspace version from `cargo metadata --no-deps --format-version 1 | jq`, failing with both values in the message; verify the comparison command prints `0.1.0` locally
- [x] 2.3 Add a `package` job that `needs: version` and calls `./.github/workflows/package.yml`; verify with `yq '.jobs.package' .github/workflows/release.yml`
- [x] 2.4 Add a `publish` job that `needs: package`, has `permissions: contents: write`, downloads all artifacts, collects their top-level regular files, writes `SHA256SUMS`, and runs `gh release create "$tag" --verify-tag --generate-notes` with `--prerelease` for tags containing `-` and `--latest` otherwise; verify with `yq '.jobs.publish' .github/workflows/release.yml` that only this job has write permission
- [x] 2.5 Add a "Releases" section to `CLAUDE.md` describing how to cut a release: bump the workspace version, wait for green CI on `main`, then push `v<version>`; and how to recover from a failed run by deleting and re-pushing the tag

## 3. First release

- [x] 3.1 After the user confirms, push the tag `v0.1.0` on a `main` commit with green CI; verify the release `v0.1.0` has the `.deb`, `.dmg`, Windows installer, and `SHA256SUMS`, is marked latest, and that `sha256sum -c SHA256SUMS` passes on the downloaded files

## Workflow follow-up

- Archive the change after the first release is published.
