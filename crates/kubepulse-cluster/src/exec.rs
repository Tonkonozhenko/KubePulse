//! Interactive exec sessions. Output is passed on as raw bytes, because only the
//! terminal knows what they mean.

use std::future::pending;

use futures::{FutureExt as _, SinkExt as _};
use k8s_openapi::{api::core::v1::Pod, apimachinery::pkg::apis::meta::v1::Status};
use kube::{
    Api, Client,
    api::{AttachParams, TerminalSize},
};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    sync::{mpsc, watch},
};
use tokio_util::sync::{CancellationToken, DropGuard};

use crate::resource_watch::SessionHandle;

/// Tried in this order, so the friendliest shell a container has wins.
const SHELLS: [&str; 3] = ["bash", "ash", "sh"];
const CHUNK_BYTES: usize = 64 * 1024;
const QUEUED_CHUNKS: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecRequest {
    pub namespace: String,
    pub pod: String,
    pub container: String,
    /// What to run. Without one, the session looks for a shell.
    pub command: Option<Vec<String>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecSize {
    pub cols: u16,
    pub rows: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExecState {
    Connecting,
    Running,
    Exited(i32),
    /// The connection ended without the process reporting an exit.
    Dropped,
    NoShell,
    Failed(String),
}

/// The server's end of an [`ExecStream`].
pub struct ExecFeed {
    output: mpsc::Sender<Vec<u8>>,
    input: mpsc::UnboundedReceiver<Vec<u8>>,
    size: watch::Receiver<ExecSize>,
    state: watch::Sender<ExecState>,
}

impl ExecFeed {
    /// Delivers output, for a session that does not come from a server.
    pub fn output(&self, bytes: Vec<u8>) {
        self.output.try_send(bytes).expect("the reader keeps up");
    }

    /// Whether the other end let the session go.
    pub fn abandoned(&self) -> bool {
        self.output.is_closed()
    }

    pub fn set_state(&self, state: ExecState) {
        self.state.send_replace(state);
    }

    /// Everything typed so far that has not been read yet.
    pub fn typed(&mut self) -> Vec<u8> {
        let mut typed = Vec::new();
        while let Ok(bytes) = self.input.try_recv() {
            typed.extend(bytes);
        }
        typed
    }

    pub fn size(&self) -> ExecSize {
        *self.size.borrow()
    }
}

/// A live session. Dropping it ends the session.
pub struct ExecStream {
    output: Option<mpsc::Receiver<Vec<u8>>>,
    input: mpsc::UnboundedSender<Vec<u8>>,
    size: watch::Sender<ExecSize>,
    state: watch::Receiver<ExecState>,
    _stop: Option<DropGuard>,
}

impl ExecStream {
    /// The bytes the container writes, which only one reader can take.
    pub fn take_output(&mut self) -> Option<mpsc::Receiver<Vec<u8>>> {
        self.output.take()
    }

    pub fn input(&self) -> mpsc::UnboundedSender<Vec<u8>> {
        self.input.clone()
    }

    pub fn resize(&self, size: ExecSize) {
        self.size.send_replace(size);
    }

    /// Closes when the session is gone, which a disconnect does without saying
    /// how the process ended.
    pub fn subscribe(&self) -> watch::Receiver<ExecState> {
        self.state.clone()
    }
}

pub fn exec_channel(size: ExecSize) -> (ExecFeed, ExecStream) {
    let (output, output_receiver) = mpsc::channel(QUEUED_CHUNKS);
    let (input, input_receiver) = mpsc::unbounded_channel();
    let (size, size_receiver) = watch::channel(size);
    let (state, state_receiver) = watch::channel(ExecState::Connecting);
    let feed = ExecFeed {
        output,
        input: input_receiver,
        size: size_receiver,
        state,
    };
    let stream = ExecStream {
        output: Some(output_receiver),
        input,
        size,
        state: state_receiver,
        _stop: None,
    };
    (feed, stream)
}

fn reports_missing_binary(message: &str) -> bool {
    message.contains("executable file not found") || message.contains("no such file or directory")
}

fn succeeded(status: &Status) -> bool {
    status.status.as_deref() == Some("Success")
}

fn exit_code(status: &Status) -> Option<i32> {
    let causes = status.details.as_ref()?.causes.as_ref()?;
    let cause = causes
        .iter()
        .find(|cause| cause.reason.as_deref() == Some("ExitCode"))?;
    cause.message.as_deref()?.parse().ok()
}

fn status_message(status: Status) -> String {
    status
        .message
        .or(status.reason)
        .unwrap_or_else(|| format!("{:?}", status.status))
}

fn request_failure(error: kube::Error) -> ExecState {
    match error {
        kube::Error::Api(status) => ExecState::Failed(status.message),
        other => ExecState::Failed(other.to_string()),
    }
}

/// How a process ended, from the status the server sends last.
fn end_state(status: Option<Status>) -> ExecState {
    let Some(status) = status else {
        return ExecState::Dropped;
    };
    if succeeded(&status) {
        return ExecState::Exited(0);
    }
    match exit_code(&status) {
        Some(code) => ExecState::Exited(code),
        None => ExecState::Failed(status_message(status)),
    }
}

/// Whether a probe found a shell. Anything other than the shell being absent,
/// such as a denial, stops the search, because the next shell would meet it too.
fn probe_result(status: Option<Status>) -> Result<bool, ExecState> {
    let Some(status) = status else {
        return Err(ExecState::Dropped);
    };
    let message = status.message.as_deref().unwrap_or_default();
    if succeeded(&status) {
        Ok(true)
    } else if exit_code(&status).is_some() || reports_missing_binary(message) {
        Ok(false)
    } else {
        Err(ExecState::Failed(status_message(status)))
    }
}

async fn shell_exists(
    api: &Api<Pod>,
    request: &ExecRequest,
    shell: &str,
) -> Result<bool, ExecState> {
    let params = AttachParams::default()
        .container(request.container.clone())
        .stderr(false);
    let process = api
        .exec(&request.pod, [shell, "-c", "exit 0"], &params)
        .await;
    let mut process = match process {
        Ok(process) => process,
        Err(kube::Error::Api(status)) if reports_missing_binary(&status.message) => {
            return Ok(false);
        }
        Err(error) => return Err(request_failure(error)),
    };
    let status = process.take_status().expect("the status is taken once");
    probe_result(status.await)
}

/// Each try runs without a terminal and only exits, so nothing a failed try
/// prints reaches the user.
async fn find_shell(api: &Api<Pod>, request: &ExecRequest) -> Result<&'static str, ExecState> {
    for shell in SHELLS {
        if shell_exists(api, request, shell).await? {
            return Ok(shell);
        }
    }
    Err(ExecState::NoShell)
}

async fn attach(api: &Api<Pod>, request: &ExecRequest, command: Vec<String>, feed: &mut ExecFeed) {
    let params = AttachParams::interactive_tty()
        .container(request.container.clone())
        .max_stdout_buf_size(CHUNK_BYTES)
        .max_stdin_buf_size(CHUNK_BYTES);
    let mut process = match api.exec(&request.pod, command, &params).await {
        Ok(process) => process,
        Err(error) => return feed.set_state(request_failure(error)),
    };
    let status = process.take_status().expect("the status is taken once");
    let mut stdout = process.stdout().expect("the session asked for stdout");
    let mut stdin = process.stdin().expect("the session asked for stdin");
    let mut sizes = process
        .terminal_size()
        .expect("the session asked for a terminal");
    feed.set_state(ExecState::Running);

    let ExecFeed {
        output,
        input,
        size,
        state,
    } = feed;
    let write_input = async {
        while let Some(bytes) = input.recv().await {
            if stdin.write_all(&bytes).await.is_err() {
                break;
            }
        }
        pending::<()>().await;
    };
    let send_sizes = async {
        loop {
            let ExecSize { cols, rows } = *size.borrow_and_update();
            let size_message = TerminalSize {
                width: cols,
                height: rows,
            };
            if sizes.send(size_message).await.is_err() || size.changed().await.is_err() {
                pending::<()>().await;
            }
        }
    };
    let mut chunk = vec![0; CHUNK_BYTES];
    tokio::pin!(status, write_input, send_sizes);
    // A broken connection never ends the output, because the client library
    // only closes it on a clean end. The status does resolve then.
    let ended = loop {
        tokio::select! {
            read = stdout.read(&mut chunk) => {
                let read = read.unwrap_or(0);
                if read == 0 {
                    break status.await;
                }
                let _ = output.send(chunk[..read].to_vec()).await;
            }
            ended = &mut status => break ended,
            () = &mut write_input => {}
            () = &mut send_sizes => {}
        }
    };
    // Whatever the status overtook is already in the buffer.
    while let Some(Ok(read @ 1..)) = stdout.read(&mut chunk).now_or_never() {
        let _ = output.send(chunk[..read].to_vec()).await;
    }
    state.send_replace(end_state(ended));
}

/// Runs one session to its end, reporting its progress through `feed`.
pub async fn run_exec(client: Client, request: ExecRequest, mut feed: ExecFeed) {
    let api = Api::namespaced(client, &request.namespace);
    let command = match request.command.clone() {
        Some(command) => command,
        None => match find_shell(&api, &request).await {
            Ok(shell) => vec![shell.to_owned()],
            Err(state) => return feed.set_state(state),
        },
    };
    attach(&api, &request, command, &mut feed).await;
}

impl SessionHandle {
    /// Starts a session in this session's tree. Disconnecting warns about it
    /// through `work`, and ends it.
    pub fn open_exec(&self, request: ExecRequest, size: ExecSize, label: &str) -> ExecStream {
        let (feed, mut stream) = exec_channel(size);
        let stop = CancellationToken::new();
        stream._stop = Some(stop.clone().drop_guard());
        let work = self.track_work(label);
        self.spawn_stream(stop, async move |client| {
            let _work = work;
            run_exec(client, request, feed).await;
        });
        stream
    }
}

#[cfg(test)]
mod tests {
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::{StatusCause, StatusDetails};

    use super::*;

    fn exited(code: &str) -> Status {
        Status {
            status: Some("Failure".into()),
            reason: Some("NonZeroExitCode".into()),
            message: Some("command terminated with non-zero exit code".into()),
            details: Some(StatusDetails {
                causes: Some(vec![StatusCause {
                    reason: Some("ExitCode".into()),
                    message: Some(code.into()),
                    ..StatusCause::default()
                }]),
                ..StatusDetails::default()
            }),
            ..Status::default()
        }
    }

    fn failed(message: &str) -> Status {
        Status {
            status: Some("Failure".into()),
            reason: Some("InternalError".into()),
            message: Some(message.into()),
            ..Status::default()
        }
    }

    fn success() -> Status {
        Status {
            status: Some("Success".into()),
            ..Status::default()
        }
    }

    #[test]
    fn a_process_ends_with_its_exit_code() {
        assert_eq!(end_state(Some(success())), ExecState::Exited(0));
        assert_eq!(end_state(Some(exited("130"))), ExecState::Exited(130));
    }

    #[test]
    fn a_session_with_no_final_status_dropped() {
        assert_eq!(end_state(None), ExecState::Dropped);
    }

    #[test]
    fn a_failure_shows_what_the_cluster_said() {
        assert_eq!(
            end_state(Some(failed("container not found"))),
            ExecState::Failed("container not found".into())
        );
    }

    #[test]
    fn a_shell_that_cannot_start_means_trying_the_next() {
        assert_eq!(probe_result(Some(exited("127"))), Ok(false));
        let missing = failed(r#"exec: "bash": executable file not found in $PATH: unknown"#);
        assert_eq!(probe_result(Some(missing)), Ok(false));
    }

    #[test]
    fn a_stopped_container_is_not_a_missing_shell() {
        assert_eq!(
            probe_result(Some(failed("container not found"))),
            Err(ExecState::Failed("container not found".into()))
        );
    }
}
