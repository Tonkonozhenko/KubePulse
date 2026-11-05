# Proposal

## Why

Namespace scoping is a free-text, comma-separated field on each table. The user has to know and type exact names, a typo silently shows nothing, and the choice is lost when the next kind opens, even though working in a namespace usually spans several kinds. The field also appears on cluster-scoped kinds, where any value hides every object.

## What Changes

- Replace the text field with a searchable multiselect in the table toolbar, listing the cluster's namespaces and updating live.
- Show it on every namespaced kind, including custom resources, and hide it on cluster-scoped kinds, which always show all their objects.
- Make the selection per cluster: it applies to every namespaced kind and every tab on that cluster, so changing it in one place changes it everywhere.
- Keep the selection for the life of the application, including across disconnecting and reconnecting. Every launch starts on all namespaces, and nothing is saved to disk.
- Keep working when the user cannot list namespaces, by offering the namespaces seen in the table and saying why the list is partial.
- Keep a selected namespace that has been deleted until the user removes it, so a selection never widens on its own.

## Out of Scope

- Saving the selection across restarts, or named namespace sets.
- Fetching less from the cluster. Selection still filters the shared per-kind cache, and watches stay cluster-wide where permitted.
- Reaching namespaces the session does not already watch. Users who cannot list namespaces see what the existing fallback watches provide.
- Exposing the selection to extensions, or choosing namespaces from the command palette.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `general/resource-browsing`: changes how namespaces are chosen, and adds per-cluster selection, cluster-scoped behavior, and where the choices come from. This capability is defined by the in-flight change `define-app-architecture` and is not yet in `openspec/specs/`.

## Impact

- **Code:** `kubepulse-ui` changes `resource_browser.rs`, `cluster_model.rs`, `cluster_tab.rs`, and `examples/soak.rs`. `kubepulse-store` adds the namespaces present in a kind to each snapshot.
- **Contract and files:** the extension contract, saved layouts, and settings files do not change.
- **Dependencies:** none. The multiselect comes from the kit already in use.
- **Dependencies between changes:** this builds on `add-three-column-layout`, whose preview tabs replace the table when the kind changes. `define-app-architecture` must be archived first, because this change modifies its `general/resource-browsing` spec. Whichever of this change and `add-themes-and-i18n` lands second handles this change's new strings in the catalogs.
