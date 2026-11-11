# Proposal

## Why

Above every table, the list view spends two rows on the cluster's name and its connection state, and the tab title already shows the name. That space belongs to the table. Meanwhile, the state of the cluster the user is working in has no fixed place in the window, and the cluster's running work (shells and port-forwards) shows only in the disconnect warning. The actions column also reserves 28 px per button plus 24 px, which leaves empty space beside 20 px buttons.

## What Changes

- Add a **status bar** along the bottom of the main window. It describes the selected cluster, which follows the active tab:
  - its color and name, emphasized when the cluster is protected;
  - its connection state;
  - when the connection failed or is being retried, the reason (with full text on hover) and a note that the data shown is the last known;
  - its running work: a count of its shells and port-forwards, the work a disconnect would stop, listed on hover;
  - its active port-forwards: a count that opens the forwards list.
- The **list view** shows only the cluster's color stripe at its top. The name and state rows move to the status bar. A tab whose cluster is not connected still shows the Connect button and the server-changed note in its body, where the table would be.
- The **actions column** fits its buttons: their combined width, the gaps between them, and the cell padding.

## Capabilities

### New Capabilities
- `shell/status-bar`: The window's status bar and what it shows about the selected cluster.

### Modified Capabilities
- `general/table-columns`: The actions column's fixed width is derived from its buttons.

## Impact

- `crates/kubepulse-ui/src/workspace.rs`: lays the status bar out under the three columns.
- A new `crates/kubepulse-ui/src/status_bar.rs`: the bar's view, which reads `ClusterModel`, `ShellState`, `IdentityStore`, and `SharedForwards`.
- `crates/kubepulse-ui/src/cluster_tab.rs` and `tab_connection.rs`: the list view drops its name and state rows and keeps the Connect button and server-changed note.
- `crates/kubepulse-ui/src/resource_browser.rs`: the actions column width.
- `locales/en.json`, regenerated `en-XA`: status bar text.
- Tests: `tests/shell.rs` and `tests/browsing.rs`.
- The logs, shell, and editor tabs keep their own cluster name and state, which the identity rule requires on every editor and shell.
