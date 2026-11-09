# Design

## Context

See `proposal.md` for motivation and `specs/general/resource-browsing/spec.md` for required behavior.

Today `ResourceBrowser` owns a text input, parses it into `ViewQuery::namespaces`, and sends that to its store view. An empty set means every namespace. Filtering already happens in the store's per-kind task against a shared cache, so namespace selection costs no extra watches. Each kind change in a preview tab builds a new `ResourceBrowser`, which is why the value is lost today.

`ViewQuery::matches` excludes rows with no namespace when the set is non-empty, so a cluster-scoped table given a selection would show nothing. That is the current behavior of the field on those kinds.

## Goals / Non-Goals

**Goals:**

- One selection per cluster, held in UI state and pushed to each table's `ViewQuery`.
- Options that are correct for users with and without permission to list namespaces.
- No added work on the UI thread proportional to row count.

**Non-Goals:**

- Server-side namespace filtering or namespace-scoped watches. The cache must hold all rows anyway, because two tabs can share a kind and selections can change instantly.
- A new control. The kit's multi-select combobox covers this.

## Decisions

### D1: The selection lives in `ClusterModel`

`ClusterModel` gains `namespaces: HashMap<ClusterKey, BTreeSet<String>>`, with `selected_namespaces(&key)` and `set_namespaces(key, set, cx)`. A real change emits `ClusterModelEvent::NamespacesChanged(key)`.

The model is already a global that lives for the whole run, is keyed per cluster, and is available to tabs rebuilt from a saved layout. Holding the selection there gives session-only lifetime with no extra code, and it survives disconnects because sessions do not own it. Keying by `ClusterKey` rather than context means a context repointed at another server does not inherit a selection.

Alternatives: a `ShellState` field would work but mixes cluster data into tab bookkeeping. A store-level selection would push UI state down the crate graph, and it would be lost on disconnect, because the store is dropped.

### D2: Browsers read the selection on open and follow the event

`ResourceBrowser::open` takes the `ClusterKey` and model in place of the context, and builds its first `ViewQuery` from the model, so a new kind never flashes all namespaces. It subscribes to the model with `cx.subscribe_in`, because updating the combobox needs a window. On `NamespacesChanged` for its key it updates the query and, if the combobox differs, the combobox.

The browser that made the change skips the combobox update, because `set_selected_values` clears the search text the user is typing in. Comparing as sets covers this without tracking who originated a change.

### D3: Cluster-scoped browsers do not take part

The combobox field is `Option<Entity<ComboboxState<SearchableVec<String>>>>`, set only when `kind.namespaced`. A cluster-scoped browser has no combobox, no subscription, and always leaves `query.namespaces` empty. No `ViewQuery::matches` change is needed.

### D4: Options are the union of three sources

Options are the sorted union of the cluster's `Namespace` objects, the namespaces present in this kind's cache, and the current selection. There is no branch on permissions, and each source covers a case:

- The `Namespace` objects offer namespaces with no objects of this kind.
- The kind's namespaces cover users who cannot list namespaces. Rows can only come from namespaces the session watches, so nothing the old text field could match is lost.
- The selection keeps deleted namespaces selected and offered, and it covers the moment before the first list completes.

The `Namespace` objects come from a second `View` held by each namespaced browser, opened with `ClusterServices::open_view`. It reuses the shared, reference-counted watch, and it is dropped with the browser, so no lifecycle is added. The kind is found by group `""` and kind `Namespace` in `ClusterStatus::resources`. No new service method is needed.

The kind's namespaces cannot come from `Snapshot::rows`, because those are already filtered: selecting one namespace would remove the others from the list. `Snapshot` therefore gains `namespaces: Arc<Vec<String>>`, the sorted distinct namespaces before filtering. `publish` computes it once per publish, and every view shares it. Cost is one pass over rows that `publish` already walks to filter and sort, at most once per frame.

Alternative: maintaining a namespace count map in `Cache` avoids that pass, but adds bookkeeping to every event path, including resyncs. It is not worth it unless measurement says so.

### D5: The control uses the kit's default trigger

`Combobox` with `.multiple(true)`, `.searchable(true)`, `.cleanable(true)`, and the placeholder "All namespaces". The default trigger shows names joined by commas and truncates them. `Change` events apply each toggle at once, so the table updates while the menu is open. A custom trigger would lose the clear button and caret, which the kit renders only for the default.

When the `Namespace` view reports `denied`, or the cluster does not serve the kind, a `.footer` says the list may be incomplete. `Snapshot::denied` is otherwise not shown for this view: the user did not ask for this list, and the table is still usable.

### D6: Refreshing options re-applies the selection

The browser recomputes the union when either snapshot's namespaces differ from the last, so a pod churning in place does nothing. It calls `set_items`, then `set_selected_values`, because the combobox stores selections with list indices that shift when a namespace is inserted, and deselecting goes by index.

## Risks / Trade-offs

- **A new namespace clears search text in an open menu, because `set_selected_values` clears it** → Accepted. Namespaces change rarely, and the kit has no way to read whether the menu is open.
- **A forgotten selection hides objects on another kind** → The control is on every namespaced table and the summary already reads "N of M objects". Nothing persists past a restart.
- **`publish` gains a pass over rows on every frame during a burst** → It is linear next to an existing sort. The 10k-pod tests cover it, and D4's count map is the fallback if the frame budget suffers.
- **A cluster with thousands of namespaces makes the menu long** → The list is virtualized by the kit and searchable.
- **`set_selected_values` ignores values not in the list** → D4 always includes the selection in the options, and `set_items` runs first.

## Migration Plan

No migration. Nothing is persisted, and `ResourceBrowser::set_namespaces` changes signature for its in-repo callers only.
