# Tasks

## 1. Render icons

- [ ] 1.1 Register `AllAssets` in `app.rs`, and verify in the running app that rail buttons, chevrons, and kit component icons appear
- [ ] 1.2 Switch `kubepulse-ui` to `gpui_kit::assets::IconName`, and replace the `"icons/shield.svg"` string with `IconName::Shield`

## 2. Contract

- [ ] 2.1 Add `ViewIcon` and `ResourceViewContribution.icon`, and bump the contract to 1.2
- [ ] 2.2 Add round-trip tests for a missing icon and an unknown icon name
- [ ] 2.3 Declare the icon for each built-in view in `kubepulse-builtin`
- [ ] 2.4 Document `icon` in `docs/extensions.md`

## 3. Kind icons

- [ ] 3.1 Add `icons.rs` with `view_glyph` and `state_mark`
- [ ] 3.2 Add `icon` to `NavigationEntry` and add `kind_icon`, with unit tests for declared and undeclared kinds
- [ ] 3.3 Show the icon in sidebar entries
- [ ] 3.4 Show the icon in cluster tab titles, and the settings icon in the settings tab title

## 4. Actions and states

- [ ] 4.1 Add icons to connect, disconnect, retry, and cluster context menu actions
- [ ] 4.2 Add icons to delete, reveal, and hide in `detail.rs`
- [ ] 4.3 Add icons to the protection toggle, open folder, restart now, and open in new tab
- [ ] 4.4 Replace the `●` choice marker in `declarative.rs` with a check icon
- [ ] 4.5 Add prefix icons to the filter and label selector inputs, leaving the namespace control alone
- [ ] 4.6 Use `state_mark` in the rail and sidebar header, with colors from `cx.theme()`

## 5. Verify

- [ ] 5.1 Run `cargo test --workspace`, `cargo clippy --workspace --all-targets`, and `cargo fmt --all --check`
- [ ] 5.2 Run `python3 scripts/check-i18n.py` and `python3 scripts/check-crate-graph.py`
- [ ] 5.3 Check icons in the running app in light and dark modes, including long cluster names in tab titles
- [ ] 5.4 Measure the release binary size before and after, and record it in the commit message
