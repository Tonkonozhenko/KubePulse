# Proposal

## Why

KubePulse has no public page that explains what it is. The only description is the repository README, which mixes the pitch with build and install instructions. A short landing page on GitHub Pages gives people one link that tells them what the product does, which platforms it runs on, and where to download it.

## What Changes

- A static, single-page site in a new top-level `site/` directory. It says what KubePulse is, lists its main features, states that it is open source under Apache-2.0 with no accounts or telemetry, and links to the latest release and the source repository.
- The page uses only files in `site/`: no external fonts, scripts, stylesheets, or analytics, so visiting it sends no requests to third parties.
- The page works at phone width and follows the visitor's light or dark preference.
- A new GitHub Actions workflow publishes `site/` to GitHub Pages when a change to the site is pushed to `main`, and can be run by hand.

## Capabilities

### New Capabilities

- `arch/website`: the public project page, what it must say, what it may load, and how it is published.

### Modified Capabilities

None.

## Impact

- New `site/` directory with `index.html`, a stylesheet, and the logo copied from `assets/`.
- New `.github/workflows/pages.yml`.
- `README.md`: a link to the published page.
- The repository's Pages source must be set to GitHub Actions once, in the repository settings.
- No Rust code or dependencies change.
