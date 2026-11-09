//! Shells on this machine, fed through the same stream as container sessions.

use std::io::{Read, Write};

use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use tokio::{select, sync::watch, task::spawn_blocking};
use tokio_util::sync::CancellationToken;

use crate::exec::{CHUNK_BYTES, ExecFeed, ExecSize, ExecState, ExecStream, exec_channel};

/// Starts the user's login shell in their home directory. Must be called
/// inside the Tokio runtime. Dropping the stream ends the shell.
pub fn open_local_shell(size: ExecSize) -> ExecStream {
    let mut command = CommandBuilder::new_default_prog();
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    open(command, size)
}

fn open(command: CommandBuilder, size: ExecSize) -> ExecStream {
    let (feed, mut stream) = exec_channel(size);
    let stop = CancellationToken::new();
    stream.stop_on_drop(stop.clone());
    tokio::spawn(run(command, feed, stop));
    stream
}

fn pty_size(size: ExecSize) -> PtySize {
    PtySize {
        rows: size.rows,
        cols: size.cols,
        ..PtySize::default()
    }
}

fn fail(state: &watch::Sender<ExecState>, error: &dyn std::fmt::Display) {
    state.send_replace(ExecState::Failed(error.to_string()));
}

/// Ends the session by closing every handle on the terminal, as closing a
/// terminal window does. The system then hangs up the terminal, which ends the
/// program in front and makes the shell end the jobs it started.
async fn run(command: CommandBuilder, feed: ExecFeed, stop: CancellationToken) {
    let ExecFeed {
        output,
        mut input,
        mut size,
        state,
    } = feed;
    let pair = match native_pty_system().openpty(pty_size(*size.borrow())) {
        Ok(pair) => pair,
        Err(error) => return fail(&state, &format!("{error:#}")),
    };
    let mut child = match pair.slave.spawn_command(command) {
        Ok(child) => child,
        Err(error) => return fail(&state, &format!("{error:#}")),
    };
    // The reader only sees the end of output once no slave handle is left open.
    drop(pair.slave);
    let master = pair.master;
    let (mut reader, mut writer, _stopping) = match handles(&*master) {
        Ok(handles) => handles,
        Err(error) => {
            let _ = child.kill();
            return fail(&state, &error);
        }
    };
    state.send_replace(ExecState::Running);

    let mut reading = spawn_blocking(move || {
        let mut buffer = vec![0; CHUNK_BYTES];
        while let Ok(read @ 1..) = reader.read(&mut buffer) {
            if output.blocking_send(buffer[..read].to_vec()).is_err() {
                return;
            }
        }
    });
    // A writer that types end-of-input when dropped would let the shell exit
    // before the hangup, without ending its jobs, so it is kept until the end.
    let _writing = spawn_blocking(move || {
        while let Some(bytes) = input.blocking_recv() {
            if writer
                .write_all(&bytes)
                .and_then(|()| writer.flush())
                .is_err()
            {
                break;
            }
        }
        writer
    });
    let mut waiting = spawn_blocking(move || child.wait());

    loop {
        select! {
            () = stop.cancelled() => return,
            Ok(()) = size.changed() => {
                let size = pty_size(*size.borrow_and_update());
                if let Err(error) = master.resize(size) {
                    tracing::warn!(%error, "could not resize the local shell");
                }
            }
            ended = &mut waiting => {
                state.send_replace(match ended.expect("the waiter does not panic") {
                    Ok(status) => ExecState::Exited(status.exit_code() as i32),
                    Err(error) => ExecState::Failed(error.to_string()),
                });
                break;
            }
        }
    }
    // Programs the shell left running can keep the terminal open, so output is
    // read until the tab lets the session go.
    select! {
        _ = &mut reading => {}
        () = stop.cancelled() => {}
    }
}

type Handles<Stopping> = (Box<dyn Read + Send>, Box<dyn Write + Send>, Stopping);

/// A read or write blocked on the terminal would keep it open after the
/// session ends, so each waits for the terminal or for the returned pipe end
/// to close, whichever comes first.
#[cfg(unix)]
fn handles(master: &dyn MasterPty) -> std::io::Result<Handles<std::io::PipeWriter>> {
    use std::os::fd::BorrowedFd;

    let fd = master
        .as_raw_fd()
        .expect("a terminal master has a descriptor");
    // The master outlives this borrow.
    let fd = unsafe { BorrowedFd::borrow_raw(fd) };
    let (stopped, stopping) = std::io::pipe()?;
    let clone = || -> std::io::Result<_> {
        Ok(Stoppable {
            terminal: fd.try_clone_to_owned()?.into(),
            stopped: stopped.try_clone()?,
        })
    };
    let (reader, writer) = (clone()?, clone()?);
    Ok((Box::new(reader), Box::new(writer), stopping))
}

#[cfg(windows)]
fn handles(master: &dyn MasterPty) -> Result<Handles<()>, String> {
    let reader = master.try_clone_reader();
    let writer = master.take_writer();
    match (reader, writer) {
        (Ok(reader), Ok(writer)) => Ok((reader, writer, ())),
        (Err(error), _) | (_, Err(error)) => Err(format!("{error:#}")),
    }
}

#[cfg(unix)]
struct Stoppable {
    terminal: std::fs::File,
    stopped: std::io::PipeReader,
}

#[cfg(unix)]
impl Stoppable {
    fn wait(&self, events: libc::c_short) -> std::io::Result<()> {
        use std::os::fd::AsRawFd;

        let mut fds = [
            libc::pollfd {
                fd: self.terminal.as_raw_fd(),
                events,
                revents: 0,
            },
            libc::pollfd {
                fd: self.stopped.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        loop {
            // The descriptors stay open for the call.
            if unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) } >= 0 {
                break;
            }
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
        if fds[1].revents != 0 {
            return Err(std::io::ErrorKind::BrokenPipe.into());
        }
        Ok(())
    }
}

#[cfg(unix)]
impl Read for Stoppable {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.wait(libc::POLLIN)?;
        self.terminal.read(buffer)
    }
}

#[cfg(unix)]
impl Write for Stoppable {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.wait(libc::POLLOUT)?;
        self.terminal.write(bytes)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::{process::Command, time::Duration};

    use tokio::time::{sleep, timeout};

    use super::*;

    const SIZE: ExecSize = ExecSize { cols: 80, rows: 24 };
    const WAIT: Duration = Duration::from_secs(10);

    fn sh(script: &str) -> CommandBuilder {
        let mut command = CommandBuilder::new("sh");
        command.args(["-c", script]);
        command
    }

    async fn reach(stream: &ExecStream, wanted: impl Fn(&ExecState) -> bool) -> ExecState {
        let mut states = stream.subscribe();
        timeout(WAIT, states.wait_for(wanted))
            .await
            .expect("the state arrives in time")
            .expect("the session reports its state")
            .clone()
    }

    async fn read_until(stream: &mut ExecStream, done: impl Fn(&str) -> bool) -> String {
        let mut output = stream.take_output().expect("the output is not taken yet");
        let mut text = String::new();
        timeout(WAIT, async {
            while !done(&text) {
                let bytes = output.recv().await.expect("the shell writes more");
                text.push_str(&String::from_utf8_lossy(&bytes));
            }
        })
        .await
        .expect("the output arrives in time");
        text
    }

    fn alive(pid: &str) -> bool {
        Command::new("kill")
            .args(["-0", pid])
            .status()
            .unwrap()
            .success()
    }

    #[tokio::test]
    async fn typed_input_reaches_the_shell_and_the_exit_code_comes_back() {
        let mut stream = open(sh("read line; echo \"got $line\"; exit 3"), SIZE);
        reach(&stream, |state| *state == ExecState::Running).await;
        stream.input().send(b"hello\r".to_vec()).unwrap();
        read_until(&mut stream, |text| text.contains("got hello")).await;
        reach(&stream, |state| *state == ExecState::Exited(3)).await;
    }

    #[tokio::test]
    async fn a_resize_reaches_the_shell() {
        let mut stream = open(sh("read _; stty size; read _"), SIZE);
        reach(&stream, |state| *state == ExecState::Running).await;
        stream.resize(ExecSize {
            cols: 120,
            rows: 40,
        });
        sleep(Duration::from_millis(100)).await;
        stream.input().send(b"\r".to_vec()).unwrap();
        read_until(&mut stream, |text| text.contains("40 120")).await;
    }

    async fn ends_with_the_stream(shell: &str, args: &[&str], line: &str) {
        let mut command = CommandBuilder::new(shell);
        command.args(args);
        let mut stream = open(command, SIZE);
        stream
            .input()
            .send(format!("{line}\r").into_bytes())
            .unwrap();
        let text = read_until(&mut stream, |text| {
            text.split("pid42=")
                .nth(1)
                .is_some_and(|rest| rest.contains('.'))
        })
        .await;
        let pid = text
            .split("pid42=")
            .nth(1)
            .and_then(|rest| rest.split('.').next())
            .expect("the shell prints the pid")
            .to_owned();
        assert!(alive(&pid));
        // A hangup while zsh is still finishing a command can leave its jobs
        // running, so the terminal closes once the shell is idle, as a person
        // would close it.
        sleep(Duration::from_millis(300)).await;
        drop(stream);
        timeout(WAIT, async {
            while alive(&pid) {
                sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("the program ends");
    }

    const BASH: &[&str] = &["--norc", "--noprofile", "-i"];
    const ZSH: &[&str] = &["-f", "-i"];
    // Zsh expands `!` before `.`, so the job id is printed through a variable.
    const BACKGROUND: &str = "sleep 1000 & p=$! ; echo pid$((6*7))=$p.";
    const FOREGROUND: &str = "sh -c 'echo pid$((6*7))=$$.; exec sleep 1000'";

    #[tokio::test]
    async fn dropping_the_stream_ends_a_background_program() {
        ends_with_the_stream("bash", BASH, BACKGROUND).await;
    }

    #[tokio::test]
    async fn dropping_the_stream_ends_a_foreground_program() {
        ends_with_the_stream("bash", BASH, FOREGROUND).await;
    }

    #[tokio::test]
    async fn dropping_the_stream_ends_a_background_program_of_zsh() {
        ends_with_the_stream("zsh", ZSH, BACKGROUND).await;
    }

    #[tokio::test]
    async fn dropping_the_stream_ends_a_foreground_program_of_zsh() {
        ends_with_the_stream("zsh", ZSH, FOREGROUND).await;
    }

    #[tokio::test]
    async fn a_missing_program_fails_with_the_platform_error() {
        let stream = open(CommandBuilder::new("/nonexistent/shell"), SIZE);
        let state = reach(&stream, |state| matches!(state, ExecState::Failed(_))).await;
        let ExecState::Failed(message) = state else {
            unreachable!()
        };
        assert!(message.contains("/nonexistent/shell"), "{message}");
    }
}
