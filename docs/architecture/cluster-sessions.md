# Cluster sessions

This describes how `kubepulse-cluster` lists clusters and manages connections. The reasoning is in design decision D4.

## The cluster list

`KubeconfigWatcher` reads the files from `KUBECONFIG`, or `~/.kube/config` when it is unset, and merges them so that the first file to define a name wins. It publishes a versioned `ClusterList` and reloads when a file changes.

- A missing file is skipped.
- An invalid file appears in `ClusterList::errors`, and the other files still list. Error messages are summaries, because parser output can quote credentials.
- Listing only parses files. It makes no network requests and runs no credential helpers.

## Session lifecycle

A context has no session until the user connects it. `SessionManager` holds the sessions, and a context without one reports `Disconnected`.

| State | Meaning |
|---|---|
| `Disconnected` | No session exists. The context holds no client, tasks, or cache. |
| `Connecting` | The session is loading its configuration or waiting for the API server. |
| `Authenticating` | A credential helper is running. Contexts without one skip this state. |
| `Connected` | The API server answered, and health checks and discovery are running. |
| `Degraded` | A health check failed. Retries continue, and `reason` holds the latest failure. |
| `Failed` | The connection attempt ended. `reason` says why, and connecting again retries. |

```text
Disconnected ─► Connecting ─► Authenticating ─► Connecting ─► Connected ◄─► Degraded
                    │               │               │             │             │
                    ▼               ▼               ▼             └──────┬──────┘
                  Failed ◄──────────┴───────────────┘                    ▼
                                                                   Disconnected
```

Disconnecting works from any state. Nothing else ends a session: it stays connected when idle or when its last tab closes.

An answer of 403 or 404 to the first request counts as connected, because it proves the server is reachable and authorization failures are normal states. A 401 fails the connection.

## Task tree

Each session owns a `CancellationToken` and a `TaskTracker`. Work that belongs to a cluster starts through `ClusterSession::spawn`, and `disconnect` cancels the tree and waits for it to finish. A spawned task stops on cancellation even if it ignores its token.

## Timeouts and retries

The defaults are in `SessionOptions`.

| Setting | Default |
|---|---|
| Reaching the API server | 15 seconds |
| Credential helper | 120 seconds, which allows for a browser login |
| Health check interval | 10 seconds |
| Discovery refresh | Every 30 seconds, limited to 30 seconds, and again after reconnecting |
| Reconnection delay | 1 to 60 seconds, exponential with jitter |

## Credentials

Credentials stay in the kubeconfig and the client's memory, and nothing persists them. Helper output is dropped from failure reasons, and `ClusterList` has no `Debug` implementation because it holds credentials. `credential_logs.rs` scans TRACE-level logs for tokens.

## Known limit: a running helper cannot be stopped

The client library runs credential helpers as a blocking process and gives no handle to stop one. Cancelling or timing out detaches the session immediately, so the UI and other sessions are unaffected, but the helper process keeps running until it exits and its result is ignored.

Each abandoned helper holds a thread from Tokio's blocking pool until it exits. Application shutdown must use `Runtime::shutdown_background` or a timeout, so that it does not wait for one.
