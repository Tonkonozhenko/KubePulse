//! Kubeconfig, sessions, auth, discovery, watches, exec, logs, and port-forwarding.

pub mod exec;
pub mod forward;
pub mod kubeconfig;
pub mod local_shell;
pub mod logs;
pub mod manager;
pub mod manifest;
pub mod objects;
pub mod resource_watch;
pub mod session;

pub use exec::{ExecFeed, ExecRequest, ExecSize, ExecState, ExecStream, exec_channel, run_exec};
pub use forward::{
    ForwardFailure, ForwardFeed, ForwardKind, ForwardRequest, ForwardState, PortForward,
    bind_local, forward_channel, run_forward, serve_forward,
};
pub use kubeconfig::{
    ClusterEntry, ClusterList, FileError, KubeconfigWatcher, default_paths, load,
};
pub use local_shell::open_local_shell;
pub use logs::{
    ContainerInfo, ContainerKind, LogBuffer, LogFeed, LogLine, LogRequest, LogState, LogStatus,
    LogStream, log_channel, match_ranges, pod_containers,
};
pub use manager::{SessionManager, UnknownContext};
pub use manifest::ManifestError;
pub use objects::{EventRecord, ObjectError, ObjectTarget, UpdateAccess, WriteMode};
pub use resource_watch::{OwnerReference, RawObject, SessionHandle, WatchEvent};
pub use session::{
    ClusterSession, ConnectionState, PrinterColumn, ResourceKind, SessionOptions, WorkGuard,
};
