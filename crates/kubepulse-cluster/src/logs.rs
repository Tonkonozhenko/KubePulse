//! Container logs. Lines go into a bounded ring buffer in the background, and
//! views read only the lines they show, so log volume never reaches the UI
//! thread.

use std::{
    collections::VecDeque,
    mem::size_of,
    ops::Range,
    sync::{Arc, Mutex},
    time::Duration,
};

use futures::{AsyncBufRead, AsyncBufReadExt as _};
use k8s_openapi::{
    api::core::v1::{ContainerStatus, Pod},
    jiff::Timestamp,
};
use kube::{Api, api::LogParams};
use serde_json::Value;
use tokio::{sync::watch, time};
use tokio_util::sync::{CancellationToken, DropGuard};

use crate::{objects::object_error, resource_watch::SessionHandle};

const MAX_LINES: usize = 100_000;
const MAX_BYTES: usize = 64 * 1024 * 1024;
/// The bookkeeping each line costs beyond its text.
const LINE_OVERHEAD: usize = size_of::<LogLine>() + 2 * size_of::<usize>();
const FRAME: Duration = Duration::from_millis(16);
const RETRY_DELAY: Duration = Duration::from_secs(1);

/// One line as the cluster sent it, with its timestamp when it has one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogLine {
    raw: Arc<str>,
    text_start: usize,
}

impl LogLine {
    pub fn new(raw: &str) -> Self {
        let text_start = split_timestamp(raw).map_or(0, |(_, text)| raw.len() - text.len());
        Self {
            raw: raw.into(),
            text_start,
        }
    }

    pub fn timestamp(&self) -> Option<&str> {
        (self.text_start > 0).then(|| self.raw[..self.text_start].trim_end())
    }

    pub fn text(&self) -> &str {
        &self.raw[self.text_start..]
    }

    fn cost(&self) -> usize {
        self.raw.len() + LINE_OVERHEAD
    }
}

fn split_timestamp(raw: &str) -> Option<(Timestamp, &str)> {
    let (time, text) = raw.split_once(' ').unwrap_or((raw, ""));
    Some((time.parse().ok()?, text))
}

#[derive(Debug, Default)]
struct Retained {
    lines: VecDeque<LogLine>,
    bytes: usize,
    /// The sequence number of the oldest retained line, which is also how many
    /// lines were dropped.
    first: u64,
}

#[derive(Debug)]
pub struct LogBuffer {
    retained: Mutex<Retained>,
    max_lines: usize,
    max_bytes: usize,
}

impl Default for LogBuffer {
    fn default() -> Self {
        Self::with_limits(MAX_LINES, MAX_BYTES)
    }
}

impl LogBuffer {
    pub fn with_limits(max_lines: usize, max_bytes: usize) -> Self {
        Self {
            retained: Mutex::default(),
            max_lines,
            max_bytes,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Retained> {
        self.retained.lock().expect("log buffer lock")
    }

    pub fn push(&self, line: LogLine) {
        let mut retained = self.lock();
        retained.bytes += line.cost();
        retained.lines.push_back(line);
        while retained.lines.len() > self.max_lines || retained.bytes > self.max_bytes {
            let Some(oldest) = retained.lines.pop_front() else {
                break;
            };
            retained.bytes -= oldest.cost();
            retained.first += 1;
        }
    }

    /// The sequence numbers of the retained lines. Numbers never repeat, so
    /// they stay valid as older lines go.
    pub fn bounds(&self) -> Range<u64> {
        let retained = self.lock();
        retained.first..retained.first + retained.lines.len() as u64
    }

    pub fn dropped(&self) -> u64 {
        self.lock().first
    }

    pub fn retained_bytes(&self) -> usize {
        self.lock().bytes
    }

    /// Lines by sequence number, with those no longer retained left out.
    pub fn lines(&self, range: Range<u64>) -> Vec<(u64, LogLine)> {
        let retained = self.lock();
        let end = retained.first + retained.lines.len() as u64;
        let (start, end) = (range.start.max(retained.first), range.end.min(end));
        (start..end)
            .map(|seq| {
                let index = usize::try_from(seq - retained.first).expect("a retained index");
                (seq, retained.lines[index].clone())
            })
            .collect()
    }
}

/// Where `query` occurs in `text`. ASCII letters match in either case.
pub fn match_ranges(text: &str, query: &str) -> Vec<Range<usize>> {
    let (text, query) = (text.as_bytes(), query.as_bytes());
    let mut ranges = Vec::new();
    let mut start = 0;
    while !query.is_empty() && start + query.len() <= text.len() {
        if text[start..start + query.len()].eq_ignore_ascii_case(query) {
            ranges.push(start..start + query.len());
            start += query.len();
        } else {
            start += 1;
        }
    }
    ranges
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LogState {
    Connecting,
    Streaming,
    /// The connection dropped, and the stream will resume.
    Interrupted,
    /// The container stopped, or the whole log has been read.
    Ended,
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogStatus {
    pub state: LogState,
    /// Counts up as lines arrive, at most once per frame.
    pub published: u64,
    pub interruptions: u32,
}

/// The writing end of a [`LogStream`].
pub struct LogFeed {
    buffer: Arc<LogBuffer>,
    status: watch::Sender<LogStatus>,
}

impl LogFeed {
    pub fn push(&self, line: LogLine) {
        self.buffer.push(line);
    }

    /// Tells readers about the lines pushed since the last call.
    pub fn publish(&self) {
        let end = self.buffer.bounds().end;
        self.status.send_if_modified(|status| {
            let changed = status.published != end;
            status.published = end;
            changed
        });
    }

    pub fn set_state(&self, state: LogState) {
        self.publish();
        self.status.send_if_modified(|status| {
            let interrupted = state == LogState::Interrupted && status.state != state;
            let changed = status.state != state;
            status.interruptions += u32::from(interrupted);
            status.state = state;
            changed
        });
    }
}

/// A live log. Dropping it stops the stream.
pub struct LogStream {
    buffer: Arc<LogBuffer>,
    status: watch::Receiver<LogStatus>,
    _stop: Option<DropGuard>,
}

impl LogStream {
    pub fn buffer(&self) -> &Arc<LogBuffer> {
        &self.buffer
    }

    pub fn subscribe(&self) -> watch::Receiver<LogStatus> {
        self.status.clone()
    }
}

pub fn log_channel(buffer: LogBuffer) -> (LogFeed, LogStream) {
    let buffer = Arc::new(buffer);
    let (status, receiver) = watch::channel(LogStatus {
        state: LogState::Connecting,
        published: 0,
        interruptions: 0,
    });
    let feed = LogFeed {
        buffer: buffer.clone(),
        status,
    };
    let stream = LogStream {
        buffer,
        status: receiver,
        _stop: None,
    };
    (feed, stream)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogRequest {
    pub namespace: String,
    pub pod: String,
    pub container: String,
    pub previous: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContainerKind {
    Regular,
    Init,
    Ephemeral,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContainerInfo {
    pub name: String,
    pub kind: ContainerKind,
    /// Whether an earlier instance may have left logs.
    pub restarted: bool,
}

/// The containers of a pod object, with regular containers first.
pub fn pod_containers(pod: &Value) -> Vec<ContainerInfo> {
    let restarts = |statuses: &str, name: &str| {
        pod["status"][statuses].as_array().is_some_and(|statuses| {
            statuses.iter().any(|status| {
                status["name"] == name && status["restartCount"].as_i64().unwrap_or(0) > 0
            })
        })
    };
    let listed = [
        ("containers", "containerStatuses", ContainerKind::Regular),
        (
            "initContainers",
            "initContainerStatuses",
            ContainerKind::Init,
        ),
        (
            "ephemeralContainers",
            "ephemeralContainerStatuses",
            ContainerKind::Ephemeral,
        ),
    ];
    listed
        .into_iter()
        .flat_map(|(field, statuses, kind)| {
            let containers = pod["spec"][field].as_array().into_iter().flatten();
            containers.filter_map(move |container| {
                let name = container["name"].as_str()?;
                Some(ContainerInfo {
                    name: name.to_owned(),
                    kind,
                    restarted: restarts(statuses, name),
                })
            })
        })
        .collect()
}

/// Removes the lines a resumed stream repeats. It resumes from a whole second,
/// so it replays the lines from before the last one.
#[derive(Debug, Default)]
struct Resume {
    last: Option<Timestamp>,
    at_last: usize,
    skip: usize,
}

impl Resume {
    fn admit(&mut self, time: Option<Timestamp>) -> bool {
        let Some(time) = time else {
            return true;
        };
        match self.last.map(|last| time.cmp(&last)) {
            Some(std::cmp::Ordering::Less) => false,
            Some(std::cmp::Ordering::Equal) if self.skip > 0 => {
                self.skip -= 1;
                false
            }
            Some(std::cmp::Ordering::Equal) => {
                self.at_last += 1;
                true
            }
            _ => {
                self.last = Some(time);
                self.at_last = 1;
                self.skip = 0;
                true
            }
        }
    }

    /// Where to resume from. The client library rounds this to a whole second,
    /// so rounding down here keeps it from skipping ahead of the last line.
    fn since(&self) -> Option<Timestamp> {
        let last = self.last?;
        Some(Timestamp::new(last.as_second(), 0).expect("a whole second is in range"))
    }

    fn restart(&mut self) {
        self.skip = self.at_last;
    }
}

fn parse_line(bytes: &[u8]) -> Option<&[u8]> {
    let line = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    (!bytes.is_empty()).then_some(line)
}

async fn read_lines(
    reader: impl AsyncBufRead,
    resume: &mut Resume,
    feed: &LogFeed,
) -> std::io::Result<()> {
    let mut reader = std::pin::pin!(reader);
    let mut bytes = Vec::new();
    loop {
        bytes.clear();
        reader.as_mut().read_until(b'\n', &mut bytes).await?;
        let Some(line) = parse_line(&bytes) else {
            return Ok(());
        };
        let line = String::from_utf8_lossy(line);
        let time = split_timestamp(&line).map(|(time, _)| time);
        if resume.admit(time) {
            feed.push(LogLine::new(&line));
        }
    }
}

fn is_running(statuses: Option<&Vec<ContainerStatus>>, container: &str) -> bool {
    statuses.into_iter().flatten().any(|status| {
        status.name == container
            && status
                .state
                .as_ref()
                .is_some_and(|state| state.running.is_some())
    })
}

fn container_running(pod: &Pod, container: &str) -> bool {
    pod.status.as_ref().is_some_and(|status| {
        is_running(status.container_statuses.as_ref(), container)
            || is_running(status.init_container_statuses.as_ref(), container)
            || is_running(status.ephemeral_container_statuses.as_ref(), container)
    })
}

async fn pump(api: Api<Pod>, request: LogRequest, feed: &LogFeed) {
    let mut resume = Resume::default();
    let mut first = true;
    let mut last_attempt = false;
    loop {
        let params = LogParams {
            container: Some(request.container.clone()),
            follow: !request.previous,
            previous: request.previous,
            timestamps: true,
            since_time: resume.since(),
            tail_lines: first.then_some(MAX_LINES as i64),
            ..LogParams::default()
        };
        let complete = match api.log_stream(&request.pod, &params).await {
            Ok(reader) => {
                feed.set_state(LogState::Streaming);
                let read = read_lines(reader, &mut resume, feed).await;
                if let Err(error) = &read {
                    tracing::debug!(%error, "log stream broke");
                }
                read.is_ok()
            }
            Err(error) if first => {
                feed.set_state(LogState::Failed(object_error(error).to_string()));
                return;
            }
            Err(error) => {
                tracing::debug!(%error, "log stream did not resume");
                false
            }
        };
        first = false;
        if request.previous && complete {
            feed.set_state(LogState::Ended);
            return;
        }
        match api.get(&request.pod).await {
            // A stream that broke may have missed the container's last lines,
            // so it gets one more read.
            Ok(pod) if !container_running(&pod, &request.container) => {
                if complete || last_attempt {
                    feed.set_state(LogState::Ended);
                    return;
                }
                last_attempt = true;
                feed.set_state(LogState::Interrupted);
            }
            Err(kube::Error::Api(status)) if status.code == 404 => {
                feed.set_state(LogState::Ended);
                return;
            }
            _ => feed.set_state(LogState::Interrupted),
        }
        resume.restart();
        time::sleep(RETRY_DELAY).await;
    }
}

impl SessionHandle {
    /// Starts streaming in this session's tree. The stream stops when it is
    /// dropped or the session ends, and its lines stay readable either way.
    pub fn stream_logs(&self, request: LogRequest) -> LogStream {
        let (feed, mut stream) = log_channel(LogBuffer::default());
        let stop = CancellationToken::new();
        stream._stop = Some(stop.clone().drop_guard());
        self.spawn_stream(stop, async move |client| {
            let api = Api::namespaced(client, &request.namespace);
            let publish = async {
                let mut frames = time::interval(FRAME);
                loop {
                    frames.tick().await;
                    feed.publish();
                }
            };
            tokio::select! {
                () = pump(api, request, &feed) => {}
                () = publish => {}
            }
            feed.publish();
        });
        stream
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const AT: &str = "2026-01-01T00:00:01.500000000Z";

    fn at(seconds: i64, nanos: i32) -> Option<Timestamp> {
        Some(Timestamp::new(1_767_225_600 + seconds, nanos).unwrap())
    }

    #[test]
    fn a_line_splits_into_its_timestamp_and_text() {
        let line = LogLine::new(&format!("{AT} hello world"));
        assert_eq!(line.timestamp(), Some(AT));
        assert_eq!(line.text(), "hello world");
    }

    #[test]
    fn a_line_without_a_timestamp_is_all_text() {
        let line = LogLine::new("not a time");
        assert_eq!(line.timestamp(), None);
        assert_eq!(line.text(), "not a time");
    }

    #[test]
    fn the_oldest_lines_go_at_the_line_limit() {
        let buffer = LogBuffer::with_limits(3, usize::MAX);
        for number in 0..5 {
            buffer.push(LogLine::new(&number.to_string()));
        }

        assert_eq!(buffer.bounds(), 2..5);
        assert_eq!(buffer.dropped(), 2);
        let oldest = buffer.lines(0..3);
        assert_eq!(oldest.len(), 1);
        assert_eq!(oldest[0].0, 2);
        assert_eq!(oldest[0].1.text(), "2");
    }

    #[test]
    fn the_oldest_lines_go_at_the_byte_limit() {
        let line = LogLine::new("0123456789");
        let buffer = LogBuffer::with_limits(usize::MAX, line.cost() * 2);
        for _ in 0..3 {
            buffer.push(line.clone());
        }

        assert_eq!(buffer.bounds(), 1..3);
        assert_eq!(buffer.retained_bytes(), line.cost() * 2);
    }

    #[test]
    fn a_million_lines_stay_within_both_limits() {
        let buffer = LogBuffer::default();
        let line = LogLine::new(&format!("{AT} {}", "x".repeat(1024)));
        for _ in 0..1_000_000 {
            buffer.push(line.clone());
        }

        let retained = buffer.bounds();
        assert!(retained.end - retained.start <= MAX_LINES as u64);
        assert!(buffer.retained_bytes() <= MAX_BYTES);
        assert_eq!(buffer.dropped() + retained.end - retained.start, 1_000_000);
    }

    #[test]
    fn matches_ignore_ascii_case_and_do_not_overlap() {
        assert_eq!(match_ranges("aAa b", "aa"), vec![0..2]);
        assert_eq!(match_ranges("Error error", "ERROR"), vec![0..5, 6..11]);
        assert!(match_ranges("text", "").is_empty());
    }

    #[test]
    fn resuming_starts_at_or_before_the_last_line() {
        let mut resume = Resume::default();
        assert_eq!(resume.since(), None);
        resume.admit(at(2, 700_000_000));
        assert_eq!(resume.since(), at(2, 0));
    }

    #[test]
    fn a_resumed_stream_skips_what_it_already_delivered() {
        let mut resume = Resume::default();
        assert!(resume.admit(at(1, 0)));
        assert!(resume.admit(at(2, 500)));
        assert!(resume.admit(at(2, 500)), "two lines can share a time");

        resume.restart();
        assert!(!resume.admit(at(1, 0)), "replayed from the whole second");
        assert!(!resume.admit(at(2, 500)));
        assert!(!resume.admit(at(2, 500)));
        assert!(resume.admit(at(2, 500)), "a third line at that time is new");
        assert!(resume.admit(at(3, 0)));
    }

    #[test]
    fn containers_are_listed_with_their_kind_and_restarts() {
        let pod = json!({
            "spec": {
                "containers": [{"name": "web"}, {"name": "sidecar"}],
                "initContainers": [{"name": "setup"}],
            },
            "status": {
                "containerStatuses": [
                    {"name": "web", "restartCount": 2},
                    {"name": "sidecar", "restartCount": 0},
                ],
            },
        });

        let containers = pod_containers(&pod);

        let summary: Vec<_> = containers
            .iter()
            .map(|c| (c.name.as_str(), c.kind, c.restarted))
            .collect();
        assert_eq!(
            summary,
            [
                ("web", ContainerKind::Regular, true),
                ("sidecar", ContainerKind::Regular, false),
                ("setup", ContainerKind::Init, false),
            ]
        );
    }
}
