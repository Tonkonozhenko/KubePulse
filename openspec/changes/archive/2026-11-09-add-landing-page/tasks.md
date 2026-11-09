# Tasks

## 1. Page

- [x] 1.1 Copy `assets/logo-256.png` to `site/logo.png` and verify the file exists with `file site/logo.png`
- [x] 1.2 Write `site/index.html` with a header (logo, name, one-line description naming Linux, macOS, and Windows, an early-development note, and download and source links to `https://github.com/Tonkonozhenko/KubePulse/releases/latest` and `https://github.com/Tonkonozhenko/KubePulse`), a feature list matching the README's Features section, a license and privacy section naming Apache-2.0 and no accounts, license keys, or telemetry, and a footer; use only relative URLs for local files, and verify with `grep -Eo '(src|href)="[^"]*"' site/index.html` that every local reference is relative and every external one is a link (`<a href>`) to the repository
- [x] 1.3 Write `site/style.css` with color tokens on `:root`, a `prefers-color-scheme: dark` override, a system font stack, a readable max width, and a 16px side gutter; verify `grep -E '@import|url\(http|fonts\.' site/style.css` finds nothing
- [x] 1.4 Open `site/index.html` in a browser at 360px and at desktop width, in light and dark system themes, and verify there is no sideways scrolling, text is readable in both themes, and the browser's network panel shows no requests to other hosts

## 2. Publishing

- [x] 2.1 Add `.github/workflows/pages.yml` triggered on pushes to `main` with paths `site/**` and `.github/workflows/pages.yml`, and on `workflow_dispatch`; one job using `actions/checkout`, `actions/configure-pages`, `actions/upload-pages-artifact` with `path: site`, and `actions/deploy-pages`, with `pages: write` and `id-token: write`, the `github-pages` environment, and a `pages` concurrency group; verify with `yq '.on, .jobs' .github/workflows/pages.yml` that triggers, permissions, and steps are as described
- [x] 2.2 Add a link to `https://tonkonozhenko.github.io/KubePulse/` near the top of `README.md` and verify it renders in the README preview

## 3. Release check

- [ ] 3.1 After merging, set the repository's Pages source to GitHub Actions, run the Pages workflow by hand, and verify `https://tonkonozhenko.github.io/KubePulse/` shows the page with its logo and styles
