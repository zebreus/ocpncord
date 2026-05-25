use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};
use futures_core::Stream;
use ocpncord_backend::{
    BackendError, BackendEvent, EventCursor, EventEnvelope, EventScope, Result,
};
use serde::Deserialize;

/// Incremental SSE parser for streaming responses.
///
/// Accumulates raw bytes, finds SSE event boundaries (`\n\n`),
/// and parses complete events.
pub struct SseParser {
    buf: Vec<u8>,
    last_event_id: String,
    retry_ms: u64,
}

impl SseParser {
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            last_event_id: String::new(),
            retry_ms: 3000,
        }
    }

    /// Feed raw bytes and return any complete events parsed from the stream.
    pub fn feed(&mut self, data: &[u8]) -> Vec<Result<EventEnvelope>> {
        self.buf.extend_from_slice(data);
        let mut events = Vec::new();
        loop {
            match find_event_boundary(&self.buf) {
                Some((end, sep_len)) => {
                    let block = &self.buf[..end];
                    if let Ok(text) = core::str::from_utf8(block) {
                        for line in text.lines() {
                            let line = line.trim_end_matches('\r');
                            if let Some(id) = line.strip_prefix("id: ") {
                                self.last_event_id = id.to_owned();
                            } else if let Some(ms) = line.strip_prefix("retry: ") {
                                if let Ok(ms) = ms.parse::<u64>() {
                                    self.retry_ms = ms;
                                }
                            }
                        }
                    }
                    if let Some(event) = parse_sse_block(block) {
                        events.push(event);
                    }
                    self.buf.drain(..end + sep_len);
                }
                None => break,
            }
        }
        events
    }

    pub fn last_event_id(&self) -> &str {
        &self.last_event_id
    }

    pub fn retry_ms(&self) -> u64 {
        self.retry_ms
    }
}

impl Default for SseParser {
    fn default() -> Self {
        Self::new()
    }
}

enum SseSource {
    PreParsed {
        events: Vec<Result<EventEnvelope>>,
        pos: usize,
    },
    /// HTTP request in flight — lazily resolved on first poll.
    Pending(Pin<Box<dyn Future<Output = Vec<Result<EventEnvelope>>>>>),
    Live {
        state: Rc<RefCell<LiveState>>,
        driver: Pin<Box<dyn Future<Output = ()>>>,
    },
}

struct LiveState {
    queue: VecDeque<Result<EventEnvelope>>,
    done: bool,
}

#[derive(Clone)]
pub struct BufferedStreamSink {
    state: Rc<RefCell<LiveState>>,
}

impl BufferedStreamSink {
    pub fn push(&self, event: Result<EventEnvelope>) {
        self.state.borrow_mut().queue.push_back(event);
    }

    pub fn extend<I>(&self, events: I)
    where
        I: IntoIterator<Item = Result<EventEnvelope>>,
    {
        self.state.borrow_mut().queue.extend(events);
    }

    pub fn finish(&self) {
        self.state.borrow_mut().done = true;
    }
}

/// A stream that yields [`EventEnvelope`]s.
///
/// Can be either pre-parsed from a completed HTTP response body, or
/// backed by a lazy future that performs the HTTP request on first poll.
pub struct BufferedStream {
    source: SseSource,
}

impl BufferedStream {
    pub fn new(events: Vec<Result<EventEnvelope>>) -> Self {
        Self {
            source: SseSource::PreParsed { events, pos: 0 },
        }
    }

    /// Create a stream that lazily evaluates the HTTP request on first poll.
    pub fn from_pending(fut: Pin<Box<dyn Future<Output = Vec<Result<EventEnvelope>>>>>) -> Self {
        Self {
            source: SseSource::Pending(fut),
        }
    }

    pub fn live<F, B>(builder: B) -> Self
    where
        F: Future<Output = ()> + 'static,
        B: FnOnce(BufferedStreamSink) -> F,
    {
        let state = Rc::new(RefCell::new(LiveState {
            queue: VecDeque::new(),
            done: false,
        }));
        let sink = BufferedStreamSink {
            state: state.clone(),
        };
        let driver = Box::pin(builder(sink));
        Self {
            source: SseSource::Live { state, driver },
        }
    }

    /// Create an empty stream that immediately returns `None`.
    pub fn empty() -> Self {
        Self {
            source: SseSource::PreParsed {
                events: Vec::new(),
                pos: 0,
            },
        }
    }

    /// Parse all SSE events from a response body bytes.
    ///
    /// Each SSE event block is separated by `\n\n`:
    /// ```text
    /// event: message.part.updated
    /// data: {"part": {...}, "delta": "..."}
    ///
    /// event: session.status
    /// data: {"sessionID": "...", "status": {"type": "idle"}}
    /// ```
    pub fn parse_sse(body: &[u8]) -> Vec<Result<EventEnvelope>> {
        let mut events = Vec::new();
        let mut pos = 0;

        while pos < body.len() {
            match find_event_boundary(&body[pos..]) {
                Some((end, sep_len)) => {
                    let block = &body[pos..pos + end];
                    if let Some(event) = parse_sse_block(block) {
                        events.push(event);
                    }
                    pos += end + sep_len;
                }
                None => {
                    let block = &body[pos..];
                    if let Some(event) = parse_sse_block(block) {
                        events.push(event);
                    }
                    break;
                }
            }
        }

        events
    }
}

impl Stream for BufferedStream {
    type Item = Result<EventEnvelope>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = unsafe { self.get_unchecked_mut() };
        loop {
            match &mut this.source {
                SseSource::PreParsed { events, pos } => {
                    if *pos < events.len() {
                        let item = match &events[*pos] {
                            Ok(e) => Ok(e.clone()),
                            Err(e) => Err(e.clone()),
                        };
                        *pos += 1;
                        return Poll::Ready(Some(item));
                    } else {
                        return Poll::Ready(None);
                    }
                }
                SseSource::Pending(fut) => match fut.as_mut().poll(cx) {
                    Poll::Ready(events) => {
                        this.source = SseSource::PreParsed { events, pos: 0 };
                        continue;
                    }
                    Poll::Pending => return Poll::Pending,
                },
                SseSource::Live { state, driver } => {
                    if let Some(event) = state.borrow_mut().queue.pop_front() {
                        return Poll::Ready(Some(event));
                    }

                    if state.borrow().done {
                        return Poll::Ready(None);
                    }

                    if driver.as_mut().poll(cx).is_ready() {
                        state.borrow_mut().done = true;
                    }

                    if let Some(event) = state.borrow_mut().queue.pop_front() {
                        return Poll::Ready(Some(event));
                    }

                    if state.borrow().done {
                        return Poll::Ready(None);
                    }

                    return Poll::Pending;
                }
            }
        }
    }
}

/// Find the end of the first SSE event block.
///
/// Returns `(offset, separator_len)` where `separator_len` is 2 for `\n\n`
/// or 4 for `\r\n\r\n`.
pub(crate) fn find_event_boundary(data: &[u8]) -> Option<(usize, usize)> {
    if let Some(pos) = data.windows(2).position(|w| w == b"\n\n") {
        return Some((pos, 2));
    }
    if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
        return Some((pos, 4));
    }
    None
}

fn parse_json<'a, T: Deserialize<'a>>(data: &'a str) -> core::result::Result<T, BackendError> {
    serde_json::from_str(data).map_err(|e| BackendError::Parse {
        message: alloc::format!("parse error: {e}"),
    })
}

fn parse_backend_event_value(value: &serde_json::Value) -> Option<Result<EventEnvelope>> {
    let scope = parse_event_scope(value);
    let mut cursor = None;

    // Try nested GlobalEvent format first; fall back to flat Event format.
    let payload = value.get("payload").unwrap_or(value);
    let event_type = payload.get("type").and_then(|v| v.as_str())?;

    // Some server events arrive wrapped as { type: "sync", syncEvent: { type:
    // "event.type.version", data: {...} } }. Unwrap to the inner event type
    // and use `syncEvent.data` as properties.
    let (event_type, props) = if event_type == "sync" {
        let sync_event = payload.get("syncEvent")?;
        cursor = parse_sync_cursor(payload, sync_event);
        let inner_type = sync_event.get("type").and_then(|v| v.as_str())?;
        // Strip the version suffix (e.g. "message.part.updated.1" → "message.part.updated")
        let inner_type = inner_type
            .rsplit_once('.')
            .map_or(inner_type, |(base, _)| base);
        let inner_data = sync_event.get("data")?;
        (inner_type, inner_data)
    } else {
        let props = payload.get("properties")?;
        (event_type, props)
    };

    let parsed = match event_type {
        "message.part.updated" => parse_part_updated_value(props),
        "message.updated" => parse_message_updated_value(props),
        "session.idle" => {
            let session_id = props.get("sessionID").and_then(|v| v.as_str())?;
            Some(Ok(BackendEvent::SessionIdle {
                session_id: session_id.to_owned(),
            }))
        }
        "session.error" => {
            let session_id = props.get("sessionID").and_then(|v| v.as_str())?;
            let error = props
                .get("error")
                .and_then(|v| serde_json::from_value(v.clone()).ok())?;
            Some(Ok(BackendEvent::SessionError {
                session_id: session_id.to_owned(),
                error,
            }))
        }
        "session.status" => {
            let session_id = props.get("sessionID").and_then(|v| v.as_str())?;
            let status = props
                .get("status")
                .and_then(|v| serde_json::from_value(v.clone()).ok())?;
            Some(Ok(BackendEvent::SessionStatus {
                session_id: session_id.to_owned(),
                status,
            }))
        }
        "server.connected" => Some(Ok(BackendEvent::ServerConnected)),
        "global.disposed" => Some(Ok(BackendEvent::GlobalDisposed)),
        "session.created" => {
            let session = props
                .get("info")
                .and_then(|v| serde_json::from_value(v.clone()).ok())?;
            Some(Ok(BackendEvent::SessionCreated { session }))
        }
        "session.updated" => {
            let session = props
                .get("info")
                .and_then(|v| serde_json::from_value(v.clone()).ok())?;
            Some(Ok(BackendEvent::SessionUpdated { session }))
        }
        "session.deleted" => {
            let session_id = props.get("sessionID").and_then(|v| v.as_str())?;
            Some(Ok(BackendEvent::SessionDeleted {
                session_id: session_id.to_owned(),
            }))
        }
        "session.diff" => {
            let session_id = props.get("sessionID").and_then(|v| v.as_str())?;
            let diff = props
                .get("diff")
                .and_then(|v| serde_json::from_value(v.clone()).ok())?;
            Some(Ok(BackendEvent::SessionDiff {
                session_id: session_id.to_owned(),
                diff,
            }))
        }
        "session.compacted" => {
            let session_id = props.get("sessionID").and_then(|v| v.as_str())?;
            Some(Ok(BackendEvent::SessionCompacted {
                session_id: session_id.to_owned(),
            }))
        }
        "message.part.delta" => {
            let session_id = props.get("sessionID").and_then(|v| v.as_str())?;
            let message_id = props.get("messageID").and_then(|v| v.as_str())?;
            let part_id = props.get("partID").and_then(|v| v.as_str())?;
            let field = props.get("field").and_then(|v| v.as_str())?;
            let delta = props.get("delta").and_then(|v| v.as_str())?;
            Some(Ok(BackendEvent::MessagePartDelta {
                session_id: session_id.to_owned(),
                message_id: message_id.to_owned(),
                part_id: part_id.to_owned(),
                field: field.to_owned(),
                delta: delta.to_owned(),
            }))
        }
        "message.part.removed" => {
            let session_id = props.get("sessionID").and_then(|v| v.as_str())?;
            let message_id = props.get("messageID").and_then(|v| v.as_str())?;
            let part_id = props.get("partID").and_then(|v| v.as_str())?;
            Some(Ok(BackendEvent::MessagePartRemoved {
                session_id: session_id.to_owned(),
                message_id: message_id.to_owned(),
                part_id: part_id.to_owned(),
            }))
        }
        "permission.asked" => {
            let request = serde_json::from_value(props.clone()).ok()?;
            Some(Ok(BackendEvent::PermissionAsked { request }))
        }
        "permission.replied" => {
            let session_id = props.get("sessionID").and_then(|v| v.as_str())?;
            let request_id = props.get("requestID").and_then(|v| v.as_str())?;
            let reply = props.get("reply").and_then(|v| v.as_str())?;
            Some(Ok(BackendEvent::PermissionReplied {
                session_id: session_id.to_owned(),
                request_id: request_id.to_owned(),
                reply: reply.to_owned(),
            }))
        }
        "question.asked" => {
            let request = serde_json::from_value(props.clone()).ok()?;
            Some(Ok(BackendEvent::QuestionAsked { request }))
        }
        "question.rejected" => {
            let session_id = props.get("sessionID").and_then(|v| v.as_str())?;
            let request_id = props.get("requestID").and_then(|v| v.as_str())?;
            Some(Ok(BackendEvent::QuestionRejected {
                session_id: session_id.to_owned(),
                request_id: request_id.to_owned(),
            }))
        }
        "question.replied" => {
            let session_id = props.get("sessionID").and_then(|v| v.as_str())?;
            let request_id = props.get("requestID").and_then(|v| v.as_str())?;
            let answers = props
                .get("answers")
                .and_then(|v| serde_json::from_value(v.clone()).ok())?;
            Some(Ok(BackendEvent::QuestionReplied {
                session_id: session_id.to_owned(),
                request_id: request_id.to_owned(),
                answers,
            }))
        }
        "command.executed" => {
            let name = props.get("name").and_then(|v| v.as_str())?;
            let session_id = props.get("sessionID").and_then(|v| v.as_str())?;
            let arguments = props.get("arguments").and_then(|v| v.as_str())?;
            let message_id = props.get("messageID").and_then(|v| v.as_str())?;
            Some(Ok(BackendEvent::CommandExecuted {
                name: name.to_owned(),
                session_id: session_id.to_owned(),
                arguments: arguments.to_owned(),
                message_id: message_id.to_owned(),
            }))
        }
        "file.edited" => {
            let file = props.get("file").and_then(|v| v.as_str())?;
            Some(Ok(BackendEvent::FileEdited {
                file: file.to_owned(),
            }))
        }
        "file.watcher.updated" => {
            let file = props.get("file").and_then(|v| v.as_str())?;
            let event = props.get("event").and_then(|v| v.as_str())?;
            Some(Ok(BackendEvent::FileWatcherUpdated {
                file: file.to_owned(),
                event: event.to_owned(),
            }))
        }
        "pty.created" => {
            let info = props
                .get("info")
                .and_then(|v| serde_json::from_value(v.clone()).ok())?;
            Some(Ok(BackendEvent::PtyCreated { info }))
        }
        "pty.updated" => {
            let info = props
                .get("info")
                .and_then(|v| serde_json::from_value(v.clone()).ok())?;
            Some(Ok(BackendEvent::PtyUpdated { info }))
        }
        "pty.deleted" => {
            let id = props.get("id").and_then(|v| v.as_str())?;
            Some(Ok(BackendEvent::PtyDeleted { id: id.to_owned() }))
        }
        "pty.exited" => {
            let id = props.get("id").and_then(|v| v.as_str())?;
            let exit_code = props.get("exitCode").and_then(|v| v.as_i64())?;
            Some(Ok(BackendEvent::PtyExited {
                id: id.to_owned(),
                exit_code: exit_code as i32,
            }))
        }
        "lsp.client.diagnostics" => {
            let server_id = props.get("serverID").and_then(|v| v.as_str())?;
            let path = props.get("path").and_then(|v| v.as_str())?;
            Some(Ok(BackendEvent::LspDiagnostics {
                server_id: server_id.to_owned(),
                path: path.to_owned(),
            }))
        }
        "lsp.updated" => Some(Ok(BackendEvent::LspUpdated)),
        "mcp.browser.open.failed" => {
            let mcp_name = props.get("mcpName").and_then(|v| v.as_str())?;
            let url = props.get("url").and_then(|v| v.as_str())?;
            Some(Ok(BackendEvent::McpBrowserOpenFailed {
                mcp_name: mcp_name.to_owned(),
                url: url.to_owned(),
            }))
        }
        "mcp.tools.changed" => {
            let server = props.get("server").and_then(|v| v.as_str())?;
            Some(Ok(BackendEvent::McpToolsChanged {
                server: server.to_owned(),
            }))
        }
        "installation.update-available" => {
            let version = props.get("version").and_then(|v| v.as_str())?;
            Some(Ok(BackendEvent::InstallationUpdateAvailable {
                version: version.to_owned(),
            }))
        }
        "installation.updated" => {
            let version = props.get("version").and_then(|v| v.as_str())?;
            Some(Ok(BackendEvent::InstallationUpdated {
                version: version.to_owned(),
            }))
        }
        "workspace.ready" => {
            let name = props.get("name").and_then(|v| v.as_str())?;
            Some(Ok(BackendEvent::WorkspaceReady {
                name: name.to_owned(),
            }))
        }
        "workspace.failed" => {
            let message = props.get("message").and_then(|v| v.as_str())?;
            Some(Ok(BackendEvent::WorkspaceFailed {
                message: message.to_owned(),
            }))
        }
        "worktree.ready" => {
            let name = props.get("name").and_then(|v| v.as_str())?;
            let branch = props.get("branch").and_then(|v| v.as_str())?;
            Some(Ok(BackendEvent::WorktreeReady {
                name: name.to_owned(),
                branch: branch.to_owned(),
            }))
        }
        "worktree.failed" => {
            let message = props.get("message").and_then(|v| v.as_str())?;
            Some(Ok(BackendEvent::WorktreeFailed {
                message: message.to_owned(),
            }))
        }
        "vcs.branch.updated" => {
            let branch = props.get("branch").and_then(|v| v.as_str())?;
            Some(Ok(BackendEvent::VcsBranchUpdated {
                branch: branch.to_owned(),
            }))
        }
        "todo.updated" => {
            let session_id = props.get("sessionID").and_then(|v| v.as_str())?;
            let todos = props
                .get("todos")
                .and_then(|v| serde_json::from_value(v.clone()).ok())?;
            Some(Ok(BackendEvent::TodoUpdated {
                session_id: session_id.to_owned(),
                todos,
            }))
        }
        "tui.prompt.append" => {
            let text = props.get("text").and_then(|v| v.as_str())?;
            Some(Ok(BackendEvent::TuiPromptAppend {
                text: text.to_owned(),
            }))
        }
        "tui.command.execute" => {
            let command = props.get("command").and_then(|v| v.as_str())?;
            Some(Ok(BackendEvent::TuiCommandExecute {
                command: command.to_owned(),
            }))
        }
        "tui.toast.show" => {
            let message = props.get("message").and_then(|v| v.as_str())?;
            let variant = props.get("variant").and_then(|v| v.as_str())?;
            let title = props
                .get("title")
                .and_then(|v| v.as_str())
                .map(|s| s.to_owned());
            let duration = props.get("duration").and_then(|v| v.as_u64());
            Some(Ok(BackendEvent::TuiToastShow {
                message: message.to_owned(),
                variant: variant.to_owned(),
                title,
                duration,
            }))
        }
        "tui.session.select" => {
            let session_id = props.get("sessionID").and_then(|v| v.as_str())?;
            Some(Ok(BackendEvent::TuiSessionSelect {
                session_id: session_id.to_owned(),
            }))
        }
        "project.updated" => {
            let id = props.get("id").and_then(|v| v.as_str())?;
            let worktree = props.get("worktree").and_then(|v| v.as_str())?;
            let name = props
                .get("name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_owned());
            Some(Ok(BackendEvent::ProjectUpdated(
                ocpncord_backend::Project {
                    id: id.to_owned(),
                    worktree: worktree.to_owned(),
                    name,
                },
            )))
        }
        "server.instance.disposed" => {
            let directory = props.get("directory").and_then(|v| v.as_str())?;
            Some(Ok(BackendEvent::ServerInstanceDisposed {
                directory: directory.to_owned(),
            }))
        }

        // --- Versioned sync payload variants carried by the event stream ---
        "message.updated.1" => {
            let session_id = props.get("sessionID").and_then(|v| v.as_str())?;
            let message = props
                .get("info")
                .and_then(|v| serde_json::from_value(v.clone()).ok())?;
            Some(Ok(BackendEvent::MessageUpdated {
                session_id: session_id.to_owned(),
                message,
            }))
        }
        "message.removed.1" => {
            let session_id = props.get("sessionID").and_then(|v| v.as_str())?;
            let message_id = props.get("messageID").and_then(|v| v.as_str())?;
            Some(Ok(BackendEvent::MessageRemoved {
                session_id: session_id.to_owned(),
                message_id: message_id.to_owned(),
            }))
        }
        "message.part.updated.1" => parse_part_updated_value(props),
        "message.part.removed.1" => {
            let session_id = props.get("sessionID").and_then(|v| v.as_str())?;
            let message_id = props.get("messageID").and_then(|v| v.as_str())?;
            let part_id = props.get("partID").and_then(|v| v.as_str())?;
            Some(Ok(BackendEvent::MessagePartRemoved {
                session_id: session_id.to_owned(),
                message_id: message_id.to_owned(),
                part_id: part_id.to_owned(),
            }))
        }
        "session.created.1" | "session.updated.1" | "session.deleted.1" => {
            parse_wrapped_session_event(event_type, props)
        }

        _ => None,
    };

    parsed.map(|result| {
        result.map(|event| EventEnvelope {
            event,
            scope,
            cursor,
        })
    })
}

fn parse_event_scope(value: &serde_json::Value) -> EventScope {
    EventScope {
        directory: value
            .get("directory")
            .and_then(|v| v.as_str())
            .map(|s| s.to_owned()),
        workspace: value
            .get("workspace")
            .and_then(|v| v.as_str())
            .map(|s| s.to_owned()),
        project: value
            .get("project")
            .and_then(|v| v.as_str())
            .map(|s| s.to_owned()),
    }
}

fn string_field(value: &serde_json::Value, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| value.get(*name).and_then(|v| v.as_str()))
        .map(|s| s.to_owned())
}

fn parse_sync_cursor(
    envelope: &serde_json::Value,
    sync_event: &serde_json::Value,
) -> Option<EventCursor> {
    let event_id = string_field(sync_event, &["id"]).or_else(|| string_field(envelope, &["id"]));
    let aggregate_id = string_field(sync_event, &["aggregateID", "aggregate_id"]);
    let seq = sync_event.get("seq").and_then(|v| v.as_u64());

    if event_id.is_some() || aggregate_id.is_some() || seq.is_some() {
        Some(EventCursor {
            event_id,
            aggregate_id,
            seq,
        })
    } else {
        None
    }
}

pub(crate) fn parse_sync_history_record(
    value: &serde_json::Value,
) -> Option<Result<EventEnvelope>> {
    let mut wrapped = serde_json::Map::new();
    wrapped.insert(
        alloc::string::String::from("type"),
        serde_json::Value::String(alloc::string::String::from("sync")),
    );
    if let Some(id) = value.get("id") {
        wrapped.insert(alloc::string::String::from("id"), id.clone());
    }
    wrapped.insert(alloc::string::String::from("syncEvent"), value.clone());
    parse_backend_event_value(&serde_json::Value::Object(wrapped))
}

/// Parse a single SSE block into a `BackendEvent`.
///
/// The SSE `data:` field can contain one of three formats:
///
/// **Bus event (GlobalEvent) format** — emitted by `BusEvent`s (e.g. `message.part.delta`):
/// ```json
/// {"directory": "/path", "payload": {"type": "message.part.delta", "properties": {...}}}
/// ```
///
/// **Wrapped wire payload format** — emitted with `type: "sync"` envelopes
/// (e.g. `message.part.updated`, `session.created`):
/// ```json
/// {"directory": "/path", "payload": {"type": "sync", "syncEvent": {"type": "message.part.updated.1", "data": {...}}}}
/// ```
///
/// **Flat (Event) format** — accepted for wire compatibility:
/// ```json
/// {"type": "message.part.updated", "properties": {...}}
/// ```
fn parse_sse_block(block: &[u8]) -> Option<Result<EventEnvelope>> {
    let text = core::str::from_utf8(block).ok()?;

    let mut data = "";
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(value) = line.strip_prefix("data: ") {
            data = value;
        }
    }

    if data.is_empty() {
        return None;
    }

    let value: serde_json::Value = parse_json::<serde_json::Value>(data).ok()?;
    parse_backend_event_value(&value)
}

fn parse_wrapped_session_event(
    event_type: &str,
    props: &serde_json::Value,
) -> Option<Result<BackendEvent>> {
    match event_type {
        "session.created.1" | "session.updated.1" => {
            let session = props
                .get("info")
                .and_then(|v| serde_json::from_value(v.clone()).ok())?;
            Some(Ok(BackendEvent::SessionCreated { session }))
        }
        "session.deleted.1" => {
            let session_id = props.get("sessionID").and_then(|v| v.as_str())?;
            Some(Ok(BackendEvent::SessionDeleted {
                session_id: session_id.to_owned(),
            }))
        }
        _ => None,
    }
}

/// Parse a `message.updated` event into the normalized transcript event.
fn parse_message_updated_value(props: &serde_json::Value) -> Option<Result<BackendEvent>> {
    let session_id = props.get("sessionID").and_then(|v| v.as_str())?;
    let message = props
        .get("info")
        .and_then(|v| serde_json::from_value(v.clone()).ok())?;
    Some(Ok(BackendEvent::MessageUpdated {
        session_id: session_id.to_owned(),
        message,
    }))
}

/// Parse a `message.part.updated` event data JSON into a
/// `BackendEvent::MessagePartUpdated`.
///
/// The `sessionID` may be at `properties.sessionID` (wrapped format) or
/// inside the `part` object at `properties.part.sessionID` (live SSE format).
fn parse_part_updated_value(props: &serde_json::Value) -> Option<Result<BackendEvent>> {
    let part: ocpncord_backend::Part = props
        .get("part")
        .and_then(|v| serde_json::from_value(v.clone()).ok())?;
    let message_id = part.message_id()?;
    let part_id = part.id()?;
    let session_id = props
        .get("sessionID")
        .and_then(|v| v.as_str())
        .or_else(|| {
            props
                .get("part")
                .and_then(|p| p.get("sessionID"))
                .and_then(|v| v.as_str())
        })?;
    Some(Ok(BackendEvent::MessagePartUpdated {
        session_id: session_id.to_owned(),
        message_id: message_id.to_owned(),
        part_id: part_id.to_owned(),
        part,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to wrap event data in the GlobalEvent envelope.
    fn wrap_sse_data(event_type: &str, properties_json: &str) -> String {
        format!(
            "{{\"directory\":\"/tmp\",\"payload\":{{\"type\":\"{event_type}\",\"properties\":{properties_json}}}}}"
        )
    }

    fn event_at(events: &[Result<EventEnvelope>], index: usize) -> &BackendEvent {
        &events[index].as_ref().expect("event should parse").event
    }

    fn envelope_at(events: &[Result<EventEnvelope>], index: usize) -> &EventEnvelope {
        events[index].as_ref().expect("event should parse")
    }

    #[test]
    fn parse_text_part() {
        let data = wrap_sse_data("message.part.updated", "{\"sessionID\":\"ses1\",\"part\":{\"id\":\"prt1\",\"sessionID\":\"ses1\",\"messageID\":\"msg1\",\"type\":\"text\",\"text\":\"Hello\"},\"time\":0}");
        let sse = format!("event: message.part.updated\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        match event_at(&events, 0) {
            BackendEvent::MessagePartUpdated {
                message_id,
                part_id,
                part,
                ..
            } => {
                assert_eq!(message_id, "msg1");
                assert_eq!(part_id, "prt1");
                assert!(matches!(part, ocpncord_backend::Part::Text(_)));
            }
            _ => panic!("expected text part"),
        }
    }

    #[test]
    fn parse_tool_part() {
        let data = wrap_sse_data("message.part.updated", "{\"sessionID\":\"ses1\",\"part\":{\"id\":\"prt1\",\"sessionID\":\"ses1\",\"messageID\":\"msg1\",\"type\":\"tool\",\"callID\":\"call1\",\"tool\":\"bash\",\"state\":{\"status\":\"pending\",\"input\":{},\"raw\":\"\"}},\"time\":0}");
        let sse = format!("event: message.part.updated\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        match event_at(&events, 0) {
            BackendEvent::MessagePartUpdated {
                message_id,
                part_id,
                part,
                ..
            } => {
                assert_eq!(message_id, "msg1");
                assert_eq!(part_id, "prt1");
                assert!(matches!(part, ocpncord_backend::Part::Tool(_)));
            }
            _ => panic!("expected tool part"),
        }
    }

    #[test]
    fn parse_message_updated_for_assistant_message() {
        let data = wrap_sse_data("message.updated", "{\"sessionID\":\"ses1\",\"info\":{\"id\":\"msg1\",\"sessionID\":\"ses1\",\"role\":\"assistant\",\"time\":{\"created\":0},\"parentID\":\"msg0\",\"modelID\":\"model-1\",\"providerID\":\"provider-1\",\"mode\":\"build\",\"agent\":\"builder\",\"path\":{\"cwd\":\"/tmp\",\"root\":\"/repo\"},\"summary\":true,\"cost\":0,\"tokens\":{\"total\":4,\"input\":1,\"output\":2,\"reasoning\":1,\"cache\":{\"read\":0,\"write\":0}},\"structured\":{\"answer\":42},\"variant\":\"thinking\",\"finish\":\"stop\"}}");
        let sse = format!("event: message.updated\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        match event_at(&events, 0) {
            BackendEvent::MessageUpdated {
                message: ocpncord_backend::Message::Assistant(message),
                ..
            } => {
                assert_eq!(message.parent_id.as_deref(), Some("msg0"));
                assert_eq!(
                    message.path.as_ref().map(|path| path.cwd.as_str()),
                    Some("/tmp")
                );
                assert_eq!(message.summary, Some(true));
                assert_eq!(
                    message.tokens.as_ref().and_then(|tokens| tokens.total),
                    Some(4.0)
                );
                assert_eq!(message.variant.as_deref(), Some("thinking"));
                assert_eq!(message.finish.as_deref(), Some("stop"));
                assert_eq!(
                    message
                        .structured
                        .as_ref()
                        .and_then(|value| value.get("answer"))
                        .and_then(|value| value.as_i64()),
                    Some(42)
                );
            }
            other => panic!("expected assistant message.updated, got {other:?}"),
        }
    }

    #[test]
    fn parse_user_message_updated_produces_transcript_event() {
        let data = wrap_sse_data("message.updated", "{\"sessionID\":\"ses1\",\"info\":{\"id\":\"msg1\",\"sessionID\":\"ses1\",\"role\":\"user\",\"time\":{\"created\":0},\"format\":{\"type\":\"json_schema\",\"schema\":{\"type\":\"object\"},\"retryCount\":2},\"summary\":{\"title\":\"T\",\"body\":\"B\",\"diffs\":[]},\"agent\":\"build\",\"model\":{\"providerID\":\"anthropic\",\"modelID\":\"claude-sonnet\",\"variant\":\"fast\"},\"system\":\"system prompt\",\"tools\":{\"grep\":true}}}");
        let sse = format!("event: message.updated\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        match event_at(&events, 0) {
            BackendEvent::MessageUpdated {
                message: ocpncord_backend::Message::User(message),
                ..
            } => {
                assert_eq!(message.model.variant.as_deref(), Some("fast"));
                assert_eq!(message.system.as_deref(), Some("system prompt"));
                assert_eq!(
                    message.tools.as_ref().and_then(|tools| tools.get("grep")),
                    Some(&true)
                );
                assert_eq!(
                    message
                        .summary
                        .as_ref()
                        .and_then(|summary| summary.title.as_deref()),
                    Some("T")
                );
                assert!(matches!(
                    message.format,
                    Some(ocpncord_backend::OutputFormat::JsonSchema(_))
                ));
            }
            other => panic!("expected user message.updated, got {other:?}"),
        }
    }

    #[test]
    fn parse_server_connected() {
        let data = wrap_sse_data("server.connected", "{}");
        let sse = format!("event: server.connected\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        assert!(matches!(
            event_at(&events, 0),
            BackendEvent::ServerConnected
        ));
        assert_eq!(
            envelope_at(&events, 0).scope.directory.as_deref(),
            Some("/tmp")
        );
    }

    #[test]
    fn parse_session_created() {
        let data = wrap_sse_data("session.created", "{\"sessionID\":\"ses123\",\"info\":{\"id\":\"ses123\",\"title\":\"Test\",\"projectID\":\"proj1\",\"directory\":\"/tmp\",\"path\":\"/repo\",\"slug\":\"\",\"summary\":{\"additions\":1,\"deletions\":2,\"files\":3},\"cost\":4.5,\"tokens\":{\"input\":1,\"output\":2,\"reasoning\":3,\"cache\":{\"read\":4,\"write\":5}},\"agent\":\"builder\",\"model\":{\"id\":\"model-1\",\"providerID\":\"provider-1\",\"variant\":\"thinking\"},\"version\":\"1\",\"time\":{\"created\":0,\"updated\":0,\"compacting\":9,\"archived\":10}}}");
        let sse = format!("event: session.created\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        match event_at(&events, 0) {
            BackendEvent::SessionCreated { session } => {
                assert_eq!(session.id, "ses123");
                assert_eq!(session.path.as_deref(), Some("/repo"));
                assert_eq!(session.cost, Some(4.5));
                assert_eq!(
                    session.tokens.as_ref().map(|tokens| tokens.cache.write),
                    Some(5.0)
                );
                assert_eq!(session.agent.as_deref(), Some("builder"));
                assert_eq!(
                    session.model.as_ref().map(|model| model.id.as_str()),
                    Some("model-1")
                );
                assert_eq!(session.time.compacting, Some(9));
                assert_eq!(session.time.archived, Some(10));
            }
            _ => panic!("expected session.created"),
        }
    }

    #[test]
    fn parse_session_deleted() {
        let data = wrap_sse_data("session.deleted", "{\"sessionID\":\"ses123\",\"info\":{\"id\":\"ses123\",\"title\":\"Test\",\"projectID\":\"proj1\",\"directory\":\"/tmp\",\"slug\":\"\",\"version\":\"1\",\"time\":{\"created\":0,\"updated\":0}}}");
        let sse = format!("event: session.deleted\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        match event_at(&events, 0) {
            BackendEvent::SessionDeleted { session_id } => {
                assert_eq!(session_id, "ses123");
            }
            _ => panic!("expected session.deleted"),
        }
    }

    #[test]
    fn parse_session_idle() {
        let data = wrap_sse_data("session.idle", "{\"sessionID\":\"ses123\"}");
        let sse = format!("event: session.idle\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        match event_at(&events, 0) {
            BackendEvent::SessionIdle { session_id } => {
                assert_eq!(session_id, "ses123");
            }
            _ => panic!("expected session.idle"),
        }
    }

    #[test]
    fn parse_session_status_idle() {
        let data = wrap_sse_data(
            "session.status",
            "{\"sessionID\":\"ses123\",\"status\":{\"type\":\"idle\"}}",
        );
        let sse = format!("event: session.status\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        match event_at(&events, 0) {
            BackendEvent::SessionStatus { session_id, status } => {
                assert_eq!(session_id, "ses123");
                assert_eq!(status.status_type, "idle");
            }
            _ => panic!("expected session.status"),
        }
    }

    #[test]
    fn parse_session_status_busy() {
        let data = wrap_sse_data(
            "session.status",
            "{\"sessionID\":\"ses123\",\"status\":{\"type\":\"busy\"}}",
        );
        let sse = format!("event: session.status\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        match event_at(&events, 0) {
            BackendEvent::SessionStatus { session_id, status } => {
                assert_eq!(session_id, "ses123");
                assert_eq!(status.status_type, "busy");
                assert_eq!(status.attempt, None);
                assert_eq!(status.message.as_deref(), None);
                assert_eq!(status.next, None);
            }
            _ => panic!("expected session.status"),
        }
    }

    #[test]
    fn parse_session_status_retry() {
        let data = wrap_sse_data(
            "session.status",
            "{\"sessionID\":\"ses123\",\"status\":{\"type\":\"retry\",\"attempt\":2,\"message\":\"backing off\",\"next\":42}}",
        );
        let sse = format!("event: session.status\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        match event_at(&events, 0) {
            BackendEvent::SessionStatus { session_id, status } => {
                assert_eq!(session_id, "ses123");
                assert_eq!(status.status_type, "retry");
                assert_eq!(status.attempt, Some(2));
                assert_eq!(status.message.as_deref(), Some("backing off"));
                assert_eq!(status.next, Some(42));
            }
            _ => panic!("expected session.status"),
        }
    }

    #[test]
    fn parse_session_error() {
        let data = wrap_sse_data("session.error", "{\"sessionID\":\"ses123\",\"error\":{\"name\":\"APIError\",\"data\":{\"message\":\"fail\"}}}");
        let sse = format!("event: session.error\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        match event_at(&events, 0) {
            BackendEvent::SessionError { session_id, .. } => {
                assert_eq!(session_id, "ses123");
            }
            _ => panic!("expected session.error"),
        }
    }

    #[test]
    fn parse_permission_asked() {
        let data = wrap_sse_data("permission.asked", "{\"id\":\"per1\",\"sessionID\":\"ses1\",\"permission\":\"bash\",\"patterns\":[],\"metadata\":{},\"always\":[]}");
        let sse = format!("event: permission.asked\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        match event_at(&events, 0) {
            BackendEvent::PermissionAsked { request } => {
                assert_eq!(request.permission, "bash");
            }
            _ => panic!("expected permission.asked"),
        }
    }

    #[test]
    fn parse_question_asked() {
        let data = wrap_sse_data("question.asked", "{\"id\":\"que1\",\"sessionID\":\"ses1\",\"questions\":[{\"question\":\"Proceed?\",\"header\":\"Confirm\",\"options\":[{\"label\":\"Yes\",\"description\":\"yes\"},{\"label\":\"No\",\"description\":\"no\"}]}]}");
        let sse = format!("event: question.asked\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        match event_at(&events, 0) {
            BackendEvent::QuestionAsked { request } => {
                assert_eq!(request.questions[0].question, "Proceed?");
            }
            _ => panic!("expected question.asked"),
        }
    }

    #[test]
    fn parse_question_replied_with_nested_answers() {
        let data = wrap_sse_data(
            "question.replied",
            "{\"sessionID\":\"ses1\",\"requestID\":\"que1\",\"answers\":[[\"Yes\"],[\"custom\",\"extra\"]]}",
        );
        let sse = format!("event: question.replied\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        match event_at(&events, 0) {
            BackendEvent::QuestionReplied {
                session_id,
                request_id,
                answers,
            } => {
                assert_eq!(session_id, "ses1");
                assert_eq!(request_id, "que1");
                assert_eq!(
                    answers,
                    &alloc::vec![
                        alloc::vec!["Yes".to_string()],
                        alloc::vec!["custom".to_string(), "extra".to_string()]
                    ]
                );
            }
            _ => panic!("expected question.replied"),
        }
    }

    #[test]
    fn parse_command_executed() {
        let data = wrap_sse_data("command.executed", "{\"name\":\"build\",\"sessionID\":\"ses1\",\"arguments\":\"--release\",\"messageID\":\"msg1\"}");
        let sse = format!("event: command.executed\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        match event_at(&events, 0) {
            BackendEvent::CommandExecuted { name, .. } => {
                assert_eq!(name, "build");
            }
            _ => panic!("expected command.executed"),
        }
    }

    #[test]
    fn parse_file_edited() {
        let data = wrap_sse_data("file.edited", "{\"file\":\"src/main.rs\"}");
        let sse = format!("event: file.edited\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        match event_at(&events, 0) {
            BackendEvent::FileEdited { file } => {
                assert_eq!(file, "src/main.rs");
            }
            _ => panic!("expected file.edited"),
        }
    }

    #[test]
    fn parse_pty_created() {
        let data = wrap_sse_data("pty.created", "{\"info\":{\"id\":\"pty1\",\"title\":\"bash\",\"command\":\"bash\",\"args\":[],\"cwd\":\"/tmp\",\"status\":\"running\",\"pid\":1234}}");
        let sse = format!("event: pty.created\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        match event_at(&events, 0) {
            BackendEvent::PtyCreated { info } => {
                assert_eq!(info.id, "pty1");
            }
            _ => panic!("expected pty.created"),
        }
    }

    #[test]
    fn parse_lsp_diagnostics() {
        let data = wrap_sse_data(
            "lsp.client.diagnostics",
            "{\"serverID\":\"rust-analyzer\",\"path\":\"src/main.rs\"}",
        );
        let sse = format!("event: lsp.client.diagnostics\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        match event_at(&events, 0) {
            BackendEvent::LspDiagnostics { server_id, path } => {
                assert_eq!(server_id, "rust-analyzer");
                assert_eq!(path, "src/main.rs");
            }
            _ => panic!("expected lsp.diagnostics"),
        }
    }

    #[test]
    fn parse_mcp_tools_changed() {
        let data = wrap_sse_data("mcp.tools.changed", "{\"server\":\"my-mcp\"}");
        let sse = format!("event: mcp.tools.changed\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        match event_at(&events, 0) {
            BackendEvent::McpToolsChanged { server } => {
                assert_eq!(server, "my-mcp");
            }
            _ => panic!("expected mcp.tools.changed"),
        }
    }

    #[test]
    fn parse_installation_update() {
        let data = wrap_sse_data("installation.update-available", "{\"version\":\"1.2.3\"}");
        let sse = format!("event: installation.update-available\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        match event_at(&events, 0) {
            BackendEvent::InstallationUpdateAvailable { version } => {
                assert_eq!(version, "1.2.3");
            }
            _ => panic!("expected installation.update-available"),
        }
    }

    #[test]
    fn parse_workspace_ready() {
        let data = wrap_sse_data("workspace.ready", "{\"name\":\"my-project\"}");
        let sse = format!("event: workspace.ready\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        match event_at(&events, 0) {
            BackendEvent::WorkspaceReady { name } => {
                assert_eq!(name, "my-project");
            }
            _ => panic!("expected workspace.ready"),
        }
    }

    #[test]
    fn parse_todo_updated() {
        let data = wrap_sse_data("todo.updated", "{\"sessionID\":\"ses1\",\"todos\":[{\"content\":\"Fix bug\",\"status\":\"pending\",\"priority\":\"high\"}]}");
        let sse = format!("event: todo.updated\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        match event_at(&events, 0) {
            BackendEvent::TodoUpdated { todos, .. } => {
                assert_eq!(todos[0].content, "Fix bug");
            }
            _ => panic!("expected todo.updated"),
        }
    }

    #[test]
    fn parse_tui_toast() {
        let data = wrap_sse_data(
            "tui.toast.show",
            "{\"message\":\"Done!\",\"variant\":\"success\"}",
        );
        let sse = format!("event: tui.toast.show\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        match event_at(&events, 0) {
            BackendEvent::TuiToastShow {
                message, variant, ..
            } => {
                assert_eq!(message, "Done!");
                assert_eq!(variant, "success");
            }
            _ => panic!("expected tui.toast.show"),
        }
    }

    #[test]
    fn parse_global_disposed() {
        let data = wrap_sse_data("global.disposed", "{}");
        let sse = format!("event: global.disposed\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        assert!(matches!(event_at(&events, 0), BackendEvent::GlobalDisposed));
    }

    #[test]
    fn parse_multiple_events() {
        let d1 = wrap_sse_data("message.part.updated", "{\"sessionID\":\"ses1\",\"part\":{\"id\":\"prt1\",\"sessionID\":\"ses1\",\"messageID\":\"msg1\",\"type\":\"text\",\"text\":\"One\"},\"time\":0}");
        let d2 = wrap_sse_data("message.part.updated", "{\"sessionID\":\"ses1\",\"part\":{\"id\":\"prt2\",\"sessionID\":\"ses1\",\"messageID\":\"msg1\",\"type\":\"text\",\"text\":\"Two\"},\"time\":0}");
        let d3 = wrap_sse_data("message.updated", "{\"sessionID\":\"ses1\",\"info\":{\"id\":\"msg1\",\"sessionID\":\"ses1\",\"role\":\"assistant\",\"time\":{\"created\":0},\"parentID\":\"msg0\",\"modelID\":\"model-1\",\"providerID\":\"provider-1\",\"mode\":\"build\",\"agent\":\"builder\",\"path\":{\"cwd\":\"/tmp\",\"root\":\"/repo\"},\"cost\":0,\"tokens\":{\"total\":0,\"input\":0,\"output\":0,\"reasoning\":0,\"cache\":{\"read\":0,\"write\":0}},\"finish\":\"stop\"}}");
        let sse = format!(
            "event: message.part.updated\ndata: {d1}\n\nevent: message.part.updated\ndata: {d2}\n\nevent: message.updated\ndata: {d3}\n\n"
        );
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 3);
        assert!(matches!(
            event_at(&events, 2),
            BackendEvent::MessageUpdated { .. }
        ));
    }

    #[test]
    fn parse_no_events_for_empty_body() {
        let events = BufferedStream::parse_sse(b"");
        assert!(events.is_empty());
    }

    // --- Wrapped wire payload tests ---

    /// Helper for server payloads wrapped as
    /// `{ type: "sync", syncEvent: { type: "event.type.version", data: {...} } }`.
    fn wrap_wire_event(event_type: &str, version: u32, data_json: &str) -> String {
        format!(
            "{{\"directory\":\"/tmp\",\"payload\":{{\"type\":\"sync\",\"syncEvent\":{{\"type\":\"{event_type}.{version}\",\"id\":\"evt_test\",\"seq\":0,\"aggregateID\":\"ses1\",\"data\":{data_json}}}}}}}"
        )
    }

    #[test]
    fn parse_wrapped_message_part_updated_text() {
        let data = wrap_wire_event(
            "message.part.updated",
            1,
            "{\"sessionID\":\"ses1\",\"part\":{\"id\":\"prt1\",\"sessionID\":\"ses1\",\"messageID\":\"msg1\",\"type\":\"text\",\"text\":\"Hello from wrapped payload\"},\"time\":0}",
        );
        let sse = format!("event: message\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1, "wrapped payload should be parsed");
        match event_at(&events, 0) {
            BackendEvent::MessagePartUpdated {
                session_id, part, ..
            } => {
                assert_eq!(session_id, "ses1");
                assert!(matches!(part, ocpncord_backend::Part::Text(_)));
            }
            other => panic!("expected MessagePartUpdated, got {other:?}"),
        }
    }

    #[test]
    fn parse_wrapped_message_updated_as_transcript_event() {
        let data = wrap_wire_event(
            "message.updated",
            1,
            "{\"sessionID\":\"ses1\",\"info\":{\"id\":\"msg1\",\"sessionID\":\"ses1\",\"role\":\"assistant\",\"time\":{\"created\":0},\"parentID\":\"msg0\",\"modelID\":\"model-1\",\"providerID\":\"provider-1\",\"mode\":\"build\",\"agent\":\"builder\",\"path\":{\"cwd\":\"/tmp\",\"root\":\"/repo\"},\"cost\":0,\"tokens\":{\"total\":0,\"input\":0,\"output\":0,\"reasoning\":0,\"cache\":{\"read\":0,\"write\":0}},\"finish\":\"stop\"}}",
        );
        let sse = format!("event: message\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(
            events.len(),
            1,
            "wrapped message.updated should be parsed as MessageUpdated"
        );
        assert!(matches!(
            event_at(&events, 0),
            BackendEvent::MessageUpdated { .. }
        ));
        let cursor = envelope_at(&events, 0).cursor.as_ref().unwrap();
        assert_eq!(cursor.event_id.as_deref(), Some("evt_test"));
        assert_eq!(cursor.aggregate_id.as_deref(), Some("ses1"));
        assert_eq!(cursor.seq, Some(0));
    }

    #[test]
    fn parse_wrapped_session_created() {
        let data = wrap_wire_event(
            "session.created",
            1,
            "{\"sessionID\":\"ses123\",\"info\":{\"id\":\"ses123\",\"title\":\"Test\",\"projectID\":\"proj1\",\"directory\":\"/tmp\",\"slug\":\"\",\"version\":\"1\",\"time\":{\"created\":0,\"updated\":0}}}",
        );
        let sse = format!("event: message\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        match event_at(&events, 0) {
            BackendEvent::SessionCreated { session } => {
                assert_eq!(session.id, "ses123");
            }
            other => panic!("expected SessionCreated, got {other:?}"),
        }
    }

    #[test]
    fn parse_wrapped_message_part_removed() {
        let data = wrap_wire_event(
            "message.part.removed",
            1,
            "{\"sessionID\":\"ses1\",\"messageID\":\"msg1\",\"partID\":\"prt1\"}",
        );
        let sse = format!("event: message\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        match event_at(&events, 0) {
            BackendEvent::MessagePartRemoved {
                session_id,
                message_id,
                part_id,
            } => {
                assert_eq!(session_id, "ses1");
                assert_eq!(message_id, "msg1");
                assert_eq!(part_id, "prt1");
            }
            other => panic!("expected MessagePartRemoved, got {other:?}"),
        }
    }

    #[test]
    fn parse_wrapped_and_bus_events_interleaved() {
        // Real server can send wrapped payload events alongside BusEvents.
        // Both should parse correctly.
        let wrapped = wrap_wire_event(
            "message.part.updated",
            1,
            "{\"sessionID\":\"ses1\",\"part\":{\"id\":\"prt1\",\"sessionID\":\"ses1\",\"messageID\":\"msg1\",\"type\":\"text\",\"text\":\"One\"},\"time\":0}",
        );
        let bus = wrap_sse_data(
            "message.part.delta",
            "{\"sessionID\":\"ses1\",\"messageID\":\"msg1\",\"partID\":\"prt1\",\"field\":\"text\",\"delta\":\" world\"}",
        );
        let done = wrap_wire_event(
            "message.updated",
            1,
            "{\"sessionID\":\"ses1\",\"info\":{\"id\":\"msg1\",\"sessionID\":\"ses1\",\"role\":\"assistant\",\"time\":{\"created\":0},\"parentID\":\"msg0\",\"modelID\":\"model-1\",\"providerID\":\"provider-1\",\"mode\":\"build\",\"agent\":\"builder\",\"path\":{\"cwd\":\"/tmp\",\"root\":\"/repo\"},\"cost\":0,\"tokens\":{\"total\":0,\"input\":0,\"output\":0,\"reasoning\":0,\"cache\":{\"read\":0,\"write\":0}},\"finish\":\"stop\"}}",
        );
        let sse = format!(
            "event: message\ndata: {wrapped}\n\nevent: message\ndata: {bus}\n\nevent: message\ndata: {done}\n\n"
        );
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 3, "all three events should parse");
        assert!(matches!(
            event_at(&events, 0),
            BackendEvent::MessagePartUpdated { .. }
        ));
        assert!(matches!(
            event_at(&events, 1),
            BackendEvent::MessagePartDelta { .. }
        ));
        assert!(matches!(
            event_at(&events, 2),
            BackendEvent::MessageUpdated { .. }
        ));
    }

    #[test]
    fn parse_sync_history_record_with_cursor() {
        let record: serde_json::Value = serde_json::from_str(
            "{\"type\":\"message.updated.1\",\"id\":\"evt_1\",\"seq\":1,\"aggregateID\":\"ses1\",\"data\":{\"sessionID\":\"ses1\",\"info\":{\"id\":\"msg1\",\"sessionID\":\"ses1\",\"role\":\"assistant\",\"time\":{\"created\":0},\"parentID\":null,\"modelID\":\"mock/model\",\"providerID\":\"mock\",\"mode\":\"default\",\"agent\":\"build\",\"cost\":0.0}}}",
        )
        .unwrap();

        let event = parse_sync_history_record(&record)
            .expect("record should normalize")
            .expect("record should parse");

        assert!(matches!(event.event, BackendEvent::MessageUpdated { .. }));
        let cursor = event.cursor.expect("sync event should carry cursor");
        assert_eq!(cursor.event_id.as_deref(), Some("evt_1"));
        assert_eq!(cursor.aggregate_id.as_deref(), Some("ses1"));
        assert_eq!(cursor.seq, Some(1));
    }
}
