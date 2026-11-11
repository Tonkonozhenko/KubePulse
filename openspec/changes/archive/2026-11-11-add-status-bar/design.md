# Design: Status Bar

## Context

See proposal.md for why.

- `Workspace::render` lays out the rail, the sidebar, and the dock in a row, with nothing below them.
- `ShellState::selected()` gives the selected cluster's `ClusterKey`, and the selection already follows the active tab. `ClusterModel` gives its `identity(key)` (name, color, `protected`), `connection(context)`, and `status(context).active_work`, the labels of the shells and forwards a disconnect would stop.
- `tab_connection::connection_notes` builds, for every cluster-bound tab, a state row, a server-changed note, a failure reason, a stale note, and a Connect button. The rail marks a protected cluster with a `rail-protected` icon.
- `SharedForwards` holds the `ForwardRegistry`, whose entries carry their cluster. The `OpenPortForwards` action opens the forwards list.
- The actions column is `ACTIONS_PADDING (24) + ACTION_WIDTH (28) × n`. Its buttons are xsmall icon buttons (`size_5`, 1.25 rem), in a `gap_1` (0.25 rem) row, inside the table's 8 px cell padding on each side.

## Goals / Non-Goals

**Goals:**
- A single status bar view, observing the models it reads, with no new state of its own.

**Non-Goals:**
- Removing the name and state from the logs, shell, and editor tabs, which the identity rule requires on every editor and shell.
- Showing the server version, the kubeconfig user, or the namespace selection.
- Making the bar's items configurable.

## Decisions

### D1: `StatusBar` view under the three columns

`Workspace::render` becomes a column: the existing row (`flex_1`), then `StatusBar`, which is one line tall, `flex_none`, and has a top border. `StatusBar::new` observes `ClusterModel`, `ShellState`, and the forward registry, and calls `cx.notify()` when any of them changes. It renders from the selection, left to right:

1. a color dot (`rgb(identity.color)`), the name, and for a protected cluster the same shield icon as the rail;
2. the state mark and label, reusing `tab_connection::state_mark` and `state_label` (made `pub`);
3. for Failed or Degraded, the reason (`flex_1`, `min_w_0`, `text_ellipsis`, with the full reason in a tooltip), and for Degraded also `tab.stale`;
4. on the right, the running-work count (`status.work`, shown only when non-zero, with a tooltip listing `active_work` one per line);
5. on the right, the forwards count (`status.forwards`, shown only when non-zero), a ghost xsmall button that dispatches `OpenPortForwards`.

Ids `status-cluster`, `status-state`, `status-reason`, `status-work`, and `status-forwards` let tests find each item.

*Alternative:* a status bar per pane. Rejected: the window has one selected cluster, and the user asked for one bar at the bottom.

### D2: Connection notes split by placement

`connection_notes` gains a parameter, `Notes::All` or `Notes::Actions`. The logs, shell, and editor tabs pass `All` and keep their current notes. The list view passes `Actions`, which yields only the server-changed note and the Connect button: what the user must act on inside the tab when the table cannot show. The list view also stops rendering `identity.name` and keeps the color stripe.

*Alternative:* move every note out of all tabs. Rejected for now: it breaks the identity rule for editors and shells, and it was not asked for.

### D3: Actions width from the button sizes

`actions_width(n, rem) = n × 1.25 rem + (n − 1) × 0.25 rem + 2 × CELL_PADDING`. This replaces `ACTIONS_PADDING` and `ACTION_WIDTH`. It is computed in `ResourceBrowser::new` from `window.rem_size()`, so it follows the theme's font size like the buttons do. `ACTION_BUTTON_REMS` and `ACTION_GAP_REMS` name the kit sizes, next to the other table constants.

## Risks / Trade-offs

- **The kit's xsmall button size changes on a kit upgrade, and buttons overflow the column** → A test compares the column width with the rendered actions row's bounds, so an upgrade that changes the size fails the test.
- **The work labels come from the cluster crate and are written in English** → They are already shown as-is in the disconnect warning. This change only counts and lists them.
