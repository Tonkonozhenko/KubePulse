//! Exec sessions against a simulated API server, which speaks the same channel
//! protocol over a WebSocket as a real one.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use futures::{SinkExt as _, StreamExt as _};
use kube::{Client, Config};
use kubepulse_cluster::{ExecRequest, ExecSize, ExecState, ExecStream, exec_channel, run_exec};
use serde_json::{Value, json};
use tokio::{net::TcpListener, sync::mpsc, time::timeout};
use tokio_tungstenite::{
    WebSocketStream, accept_hdr_async,
    tungstenite::{
        Message,
        handshake::server::{ErrorResponse, Request, Response},
        http::{StatusCode, header},
    },
};

const WAIT: Duration = Duration::from_secs(20);
const STDIN: u8 = 0;
const STDOUT: u8 = 1;
const STATUS: u8 = 3;
const RESIZE: u8 = 4;

const DENIED: &str = "pods \"web\" is forbidden: cannot create resource \"pods/exec\"";

/// What the simulated container and API server do.
#[derive(Clone)]
struct Cluster {
    shells: &'static [&'static str],
    deny: bool,
    /// Ends interactive sessions by closing the socket without a status.
    drop_sessions: bool,
    commands: Arc<Mutex<Vec<Vec<String>>>>,
    sizes: Arc<Mutex<Vec<Value>>>,
}

impl Cluster {
    fn with_shells(shells: &'static [&'static str]) -> Self {
        Self {
            shells,
            deny: false,
            drop_sessions: false,
            commands: Arc::default(),
            sizes: Arc::default(),
        }
    }

    fn commands(&self) -> Vec<Vec<String>> {
        self.commands.lock().unwrap().clone()
    }

    async fn serve(self) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                let cluster = self.clone();
                tokio::spawn(async move {
                    let mut command = Vec::new();
                    let deny = cluster.deny;
                    #[allow(clippy::result_large_err, reason = "the library sets this type")]
                    let accept = |request: &Request, response: Response| {
                        command = query_commands(request.uri().query().unwrap_or_default());
                        if deny {
                            return Err(denial());
                        }
                        Ok(accepted(response))
                    };
                    if let Ok(socket) = accept_hdr_async(socket, accept).await {
                        cluster.commands.lock().unwrap().push(command.clone());
                        cluster.run(socket, command).await;
                    }
                });
            }
        });
        format!("http://{address}")
    }

    async fn run(&self, mut socket: WebSocketStream<tokio::net::TcpStream>, command: Vec<String>) {
        let shell = command[0].as_str();
        if command.len() > 1 {
            let status = if self.shells.contains(&shell) {
                json!({"status": "Success"})
            } else {
                exit_status(127)
            };
            send(&mut socket, STATUS, status.to_string().as_bytes()).await;
            return;
        }
        send(&mut socket, STDOUT, format!("{shell}$ ").as_bytes()).await;
        while let Some(Ok(message)) = socket.next().await {
            let Message::Binary(data) = message else {
                continue;
            };
            match data[0] {
                STDIN if &data[1..] == b"exit 3\n" => {
                    let status = exit_status(3).to_string();
                    send(&mut socket, STATUS, status.as_bytes()).await;
                    return;
                }
                STDIN if &data[1..] == b"cut\n" && self.drop_sessions => return,
                STDIN => send(&mut socket, STDOUT, &data[1..]).await,
                RESIZE => {
                    let size = serde_json::from_slice(&data[1..]).unwrap();
                    self.sizes.lock().unwrap().push(size);
                }
                _ => {}
            }
        }
    }
}

fn exit_status(code: i32) -> Value {
    json!({
        "status": "Failure",
        "reason": "NonZeroExitCode",
        "message": format!("command terminated with non-zero exit code: {code}"),
        "details": {"causes": [{"reason": "ExitCode", "message": code.to_string()}]},
    })
}

async fn send(socket: &mut WebSocketStream<tokio::net::TcpStream>, channel: u8, bytes: &[u8]) {
    let mut frame = vec![channel];
    frame.extend_from_slice(bytes);
    let _ = socket.send(Message::Binary(frame.into())).await;
}

fn query_commands(query: &str) -> Vec<String> {
    query
        .split('&')
        .filter_map(|pair| pair.strip_prefix("command="))
        .map(|word| {
            word.replace("%20", " ")
                .replace("%2F", "/")
                .replace('+', " ")
        })
        .collect()
}

fn accepted(mut response: Response) -> Response {
    response.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        "v4.channel.k8s.io".parse().unwrap(),
    );
    response
}

fn denial() -> ErrorResponse {
    let body = json!({
        "kind": "Status", "apiVersion": "v1", "metadata": {},
        "status": "Failure", "reason": "Forbidden", "message": DENIED, "code": 403,
    });
    let response = Response::builder()
        .status(StatusCode::FORBIDDEN)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Some(body.to_string()))
        .unwrap();
    ErrorResponse::from(response)
}

fn request(command: Option<Vec<String>>) -> ExecRequest {
    ExecRequest {
        namespace: "default".into(),
        pod: "web".into(),
        container: "app".into(),
        command,
    }
}

struct Session {
    stream: ExecStream,
    output: mpsc::Receiver<Vec<u8>>,
}

impl Session {
    async fn start(cluster: Cluster, command: Option<Vec<String>>) -> Self {
        let url = cluster.serve().await;
        let client = Client::try_from(Config::new(url.parse().unwrap())).unwrap();
        let (feed, mut stream) = exec_channel(ExecSize { cols: 90, rows: 30 });
        let output = stream.take_output().unwrap();
        tokio::spawn(run_exec(client, request(command), feed));
        Self { stream, output }
    }

    fn send(&self, text: &str) {
        self.stream.input().send(text.as_bytes().to_vec()).unwrap();
    }

    async fn read_until(&mut self, wanted: &str) -> String {
        let mut text = String::new();
        timeout(WAIT, async {
            while !text.contains(wanted) {
                let bytes = self.output.recv().await.expect("output ended early");
                text.push_str(&String::from_utf8_lossy(&bytes));
            }
        })
        .await
        .unwrap_or_else(|_| panic!("no {wanted:?} in {text:?}"));
        text
    }

    async fn ended(&self) -> ExecState {
        let mut states = self.stream.subscribe();
        let state =
            states.wait_for(|state| !matches!(state, ExecState::Connecting | ExecState::Running));
        timeout(WAIT, state).await.unwrap().unwrap().clone()
    }

    async fn wait_running(&self) {
        let mut states = self.stream.subscribe();
        let state = states.wait_for(|state| *state == ExecState::Running);
        timeout(WAIT, state).await.unwrap().unwrap();
    }
}

fn probe(shell: &str) -> Vec<String> {
    vec![shell.to_owned(), "-c".to_owned(), "exit 0".to_owned()]
}

#[tokio::test]
async fn the_first_shell_a_container_has_opens() {
    let cluster = Cluster::with_shells(&["ash", "sh"]);
    let mut session = Session::start(cluster.clone(), None).await;

    session.read_until("ash$ ").await;

    assert_eq!(
        cluster.commands(),
        [probe("bash"), probe("ash"), vec!["ash".to_owned()]],
        "bash was tried, and sh was not needed"
    );
}

#[tokio::test]
async fn a_failed_try_shows_nothing_in_the_terminal() {
    let cluster = Cluster::with_shells(&["sh"]);
    let mut session = Session::start(cluster, None).await;

    let text = session.read_until("sh$ ").await;

    assert_eq!(text, "sh$ ");
}

#[tokio::test]
async fn a_container_with_no_shell_is_reported() {
    let cluster = Cluster::with_shells(&[]);
    let session = Session::start(cluster.clone(), None).await;

    assert_eq!(session.ended().await, ExecState::NoShell);
    assert_eq!(cluster.commands().len(), 3, "only the three tries ran");
}

#[tokio::test]
async fn an_entered_command_runs_without_looking_for_a_shell() {
    let cluster = Cluster::with_shells(&[]);
    let command = vec!["/app/console".to_owned()];
    let mut session = Session::start(cluster.clone(), Some(command.clone())).await;

    session.read_until("/app/console$ ").await;

    assert_eq!(cluster.commands(), [command]);
}

#[tokio::test]
async fn a_denial_shows_the_clusters_own_message() {
    let cluster = Cluster {
        deny: true,
        ..Cluster::with_shells(&["bash"])
    };
    let session = Session::start(cluster, None).await;

    assert_eq!(session.ended().await, ExecState::Failed(DENIED.to_owned()));
}

#[tokio::test]
async fn typed_input_reaches_the_container_and_output_comes_back() {
    let mut session = Session::start(Cluster::with_shells(&["bash"]), None).await;
    session.read_until("bash$ ").await;

    session.send("hello\n");

    session.read_until("hello\n").await;
}

#[tokio::test]
async fn the_container_learns_the_size_and_every_change() {
    let cluster = Cluster::with_shells(&["bash"]);
    let mut session = Session::start(cluster.clone(), None).await;
    session.read_until("bash$ ").await;

    session.stream.resize(ExecSize {
        cols: 120,
        rows: 40,
    });
    session.send("sync\n");
    session.read_until("sync\n").await;

    let sized = |width: u16, height: u16| json!({"Width": width, "Height": height});
    timeout(WAIT, async {
        while cluster.sizes.lock().unwrap().len() < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        *cluster.sizes.lock().unwrap(),
        [sized(90, 30), sized(120, 40)]
    );
}

#[tokio::test]
async fn an_exit_reports_its_code() {
    let mut session = Session::start(Cluster::with_shells(&["bash"]), None).await;
    session.read_until("bash$ ").await;

    session.send("exit 3\n");

    assert_eq!(session.ended().await, ExecState::Exited(3));
}

#[tokio::test]
async fn a_cut_connection_is_not_mistaken_for_an_exit() {
    let cluster = Cluster {
        drop_sessions: true,
        ..Cluster::with_shells(&["bash"])
    };
    let mut session = Session::start(cluster, None).await;
    session.read_until("bash$ ").await;
    session.wait_running().await;

    session.send("cut\n");

    assert_eq!(session.ended().await, ExecState::Dropped);
}
