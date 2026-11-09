# Design

## Context

The repository has no website. Product text lives in `README.md`, the logo in `assets/`, and CI in `.github/workflows/` (`ci.yml`, `package.yml`, `release.yml`). Releases are published at `github.com/Tonkonozhenko/KubePulse/releases`, so the Pages site will be served at `https://tonkonozhenko.github.io/KubePulse/`, under a `/KubePulse/` path prefix. Requirements are in `specs/arch/website/spec.md`.

## Goals / Non-Goals

**Goals:**
- One hand-written page that is easy to edit and has no build step.
- Publishing that runs only when the site changes.

**Non-Goals:**
- Documentation pages, a blog, screenshots, or per-platform download buttons that resolve the newest asset names. The page links to the latest release instead.
- A custom domain.
- Translations. The page is English only.

## Decisions

**Plain HTML and CSS in `site/`, no generator.** One page does not justify a static-site generator, a Node toolchain, or a theme dependency. `site/index.html` and `site/style.css` are served as written. A generator was considered and rejected because it adds a build step and a dependency for no visible benefit.

**Relative URLs only.** The site is served under `/KubePulse/`, so links to the stylesheet and logo are relative (`style.css`, `logo.png`). This keeps the page working at that prefix and under any later custom domain without changes.

**System font stack, inline SVG or local images only.** Web fonts and icon CDNs would contact third parties, which the spec forbids. The page uses `system-ui` and friends, and the logo is a copy of `assets/logo-256.png` in `site/`. Copying rather than referencing `assets/` keeps the published artifact to the `site/` directory alone.

**Color tokens with `prefers-color-scheme`.** Colors are CSS custom properties on `:root`, redefined in a `prefers-color-scheme: dark` media query. No toggle and no script, so the page needs no JavaScript.

**Publish with the official Pages actions.** `.github/workflows/pages.yml` runs on pushes to `main` with a `paths` filter of `site/**` and the workflow file itself, plus `workflow_dispatch`. It uses `actions/configure-pages`, `actions/upload-pages-artifact` with `path: site`, and `actions/deploy-pages`, with `pages: write` and `id-token: write` permissions on the deploy job only and a `concurrency` group so a newer push replaces an in-flight deploy. Publishing from a `gh-pages` branch was rejected because it adds generated commits to the repository.

## Risks / Trade-offs

**Feature list drifts from the README** → The page keeps the feature list short and in the same order as the README's Features section, and the task list includes checking both when features change. Generating one from the other is not worth a build step for a handful of bullets.

**Pages is not enabled for the repository** → The first deploy fails until the repository's Pages source is set to GitHub Actions. The task list includes this one-time setting, and the failure message from `deploy-pages` names it.

## Migration Plan

1. Merge to `main`.
2. In the repository settings, set Pages → Source to GitHub Actions.
3. Run the workflow by hand once, or push a change under `site/`.

Rollback: disable Pages in the repository settings, or revert the commit.
