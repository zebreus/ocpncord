use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};
use futures_core::Stream;
use opencode_backend::{BackendError, BackendEvent, Result};
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
    pub fn feed(&mut self, data: &[u8]) -> Vec<Result<BackendEvent>> {
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
        events: Vec<Result<BackendEvent>>,
        pos: usize,
    },
    /// HTTP request in flight — lazily resolved on first poll.
    Pending(Pin<Box<dyn Future<Output = Vec<Result<BackendEvent>>>>>),
}

/// A stream that yields [`BackendEvent`]s.
///
/// Can be either pre-parsed from a completed HTTP response body, or
/// backed by a lazy future that performs the HTTP request on first poll.
pub struct BufferedStream {
    source: SseSource,
}

impl BufferedStream {
    pub fn new(events: Vec<Result<BackendEvent>>) -> Self {
        Self {
            source: SseSource::PreParsed { events, pos: 0 },
        }
    }

    /// Create a stream that lazily evaluates the HTTP request on first poll.
    pub fn from_pending(fut: Pin<Box<dyn Future<Output = Vec<Result<BackendEvent>>>>>) -> Self {
        Self {
            source: SseSource::Pending(fut),
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
    pub fn parse_sse(body: &[u8]) -> Vec<Result<BackendEvent>> {
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
    type Item = Result<BackendEvent>;

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

/// Parse a single SSE block into a `BackendEvent`.
///
/// The SSE `data:` field contains a JSON envelope:
/// ```json
/// {"directory": "/path", "payload": {"type": "message.part.updated", "properties": {...}}}
/// ```
fn parse_sse_block(block: &[u8]) -> Option<Result<BackendEvent>> {
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

    // Parse the outer envelope: {"directory": "...", "payload": {"type": "...", "properties": {...}}}
    let value: serde_json::Value = parse_json::<serde_json::Value>(data).ok()?;
    let payload = value.get("payload")?;
    let event_type = payload.get("type").and_then(|v| v.as_str())?;
    let props = payload.get("properties")?;

    match event_type {
        "message.part.updated" => parse_part_updated_value(props),
        "message.updated" => Some(Ok(BackendEvent::Done)),
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
            let status_type = props
                .get("status")
                .and_then(|s| s.get("type"))
                .and_then(|v| v.as_str())?;
            if status_type == "idle" {
                Some(Ok(BackendEvent::Done))
            } else {
                Some(Ok(BackendEvent::SessionIdle {
                    session_id: session_id.to_owned(),
                }))
            }
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
                opencode_backend::Project {
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

        // --- Sync event stream variants (from /global/sync-event) ---
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
        "message.part.updated.1" => {
            let part = props
                .get("part")
                .and_then(|v| serde_json::from_value(v.clone()).ok())?;
            Some(Ok(BackendEvent::Part { part, delta: None }))
        }
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
            // Sync session events wrap data differently
            parse_sync_session_event(event_type, props)
        }

        _ => None,
    }
}

fn parse_sync_session_event(
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

/// Parse a `message.part.updated` event data JSON into a `BackendEvent::Part`.
fn parse_part_updated_value(props: &serde_json::Value) -> Option<Result<BackendEvent>> {
    let session_id = props.get("sessionID").and_then(|v| v.as_str())?;
    let part = props
        .get("part")
        .and_then(|v| serde_json::from_value(v.clone()).ok())?;
    Some(Ok(BackendEvent::MessagePartUpdated {
        session_id: session_id.to_owned(),
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

    #[test]
    fn parse_text_part() {
        let data = wrap_sse_data("message.part.updated", "{\"sessionID\":\"ses1\",\"part\":{\"id\":\"prt1\",\"sessionID\":\"ses1\",\"messageID\":\"msg1\",\"type\":\"text\",\"text\":\"Hello\"},\"time\":0}");
        let sse = format!("event: message.part.updated\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        match &events[0] {
            Ok(BackendEvent::MessagePartUpdated { part, .. }) => {
                assert!(matches!(part, opencode_backend::Part::Text(_)));
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
        match &events[0] {
            Ok(BackendEvent::MessagePartUpdated { part, .. }) => {
                assert!(matches!(part, opencode_backend::Part::Tool(_)));
            }
            _ => panic!("expected tool part"),
        }
    }

    #[test]
    fn parse_done_event() {
        let data = wrap_sse_data("message.updated", "{\"sessionID\":\"ses1\",\"info\":{\"id\":\"msg1\",\"sessionID\":\"ses1\",\"role\":\"assistant\",\"time\":{\"created\":0,\"updated\":0},\"parentID\":\"\",\"summary\":null,\"cost\":0,\"tokens\":{\"total\":0,\"input\":0,\"output\":0,\"reasoning\":0},\"finishReason\":null}}");
        let sse = format!("event: message.updated\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], Ok(BackendEvent::Done)));
    }

    #[test]
    fn parse_server_connected() {
        let data = wrap_sse_data("server.connected", "{}");
        let sse = format!("event: server.connected\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], Ok(BackendEvent::ServerConnected)));
    }

    #[test]
    fn parse_session_created() {
        let data = wrap_sse_data("session.created", "{\"sessionID\":\"ses123\",\"info\":{\"id\":\"ses123\",\"title\":\"Test\",\"projectID\":\"proj1\",\"directory\":\"/tmp\",\"slug\":\"\",\"version\":\"1\",\"time\":{\"created\":0,\"updated\":0}}}");
        let sse = format!("event: session.created\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        match &events[0] {
            Ok(BackendEvent::SessionCreated { session }) => {
                assert_eq!(session.id, "ses123");
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
        match &events[0] {
            Ok(BackendEvent::SessionDeleted { session_id }) => {
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
        match &events[0] {
            Ok(BackendEvent::SessionIdle { session_id }) => {
                assert_eq!(session_id, "ses123");
            }
            _ => panic!("expected session.idle"),
        }
    }

    #[test]
    fn parse_session_status_idle_is_done() {
        let data = wrap_sse_data(
            "session.status",
            "{\"sessionID\":\"ses123\",\"status\":{\"type\":\"idle\"}}",
        );
        let sse = format!("event: session.status\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], Ok(BackendEvent::Done)));
    }

    #[test]
    fn parse_session_error() {
        let data = wrap_sse_data("session.error", "{\"sessionID\":\"ses123\",\"error\":{\"name\":\"APIError\",\"data\":{\"message\":\"fail\"}}}");
        let sse = format!("event: session.error\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        match &events[0] {
            Ok(BackendEvent::SessionError { session_id, .. }) => {
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
        match &events[0] {
            Ok(BackendEvent::PermissionAsked { request }) => {
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
        match &events[0] {
            Ok(BackendEvent::QuestionAsked { request }) => {
                assert_eq!(request.questions[0].question, "Proceed?");
            }
            _ => panic!("expected question.asked"),
        }
    }

    #[test]
    fn parse_command_executed() {
        let data = wrap_sse_data("command.executed", "{\"name\":\"build\",\"sessionID\":\"ses1\",\"arguments\":\"--release\",\"messageID\":\"msg1\"}");
        let sse = format!("event: command.executed\ndata: {data}\n\n");
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 1);
        match &events[0] {
            Ok(BackendEvent::CommandExecuted { name, .. }) => {
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
        match &events[0] {
            Ok(BackendEvent::FileEdited { file }) => {
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
        match &events[0] {
            Ok(BackendEvent::PtyCreated { info }) => {
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
        match &events[0] {
            Ok(BackendEvent::LspDiagnostics { server_id, path }) => {
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
        match &events[0] {
            Ok(BackendEvent::McpToolsChanged { server }) => {
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
        match &events[0] {
            Ok(BackendEvent::InstallationUpdateAvailable { version }) => {
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
        match &events[0] {
            Ok(BackendEvent::WorkspaceReady { name }) => {
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
        match &events[0] {
            Ok(BackendEvent::TodoUpdated { todos, .. }) => {
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
        match &events[0] {
            Ok(BackendEvent::TuiToastShow {
                message, variant, ..
            }) => {
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
        assert!(matches!(events[0], Ok(BackendEvent::GlobalDisposed)));
    }

    #[test]
    fn parse_multiple_events() {
        let d1 = wrap_sse_data("message.part.updated", "{\"sessionID\":\"ses1\",\"part\":{\"id\":\"prt1\",\"sessionID\":\"ses1\",\"messageID\":\"msg1\",\"type\":\"text\",\"text\":\"One\"},\"time\":0}");
        let d2 = wrap_sse_data("message.part.updated", "{\"sessionID\":\"ses1\",\"part\":{\"id\":\"prt2\",\"sessionID\":\"ses1\",\"messageID\":\"msg1\",\"type\":\"text\",\"text\":\"Two\"},\"time\":0}");
        let d3 = wrap_sse_data("message.updated", "{\"sessionID\":\"ses1\",\"info\":{\"id\":\"msg1\",\"sessionID\":\"ses1\",\"role\":\"assistant\",\"time\":{\"created\":0,\"updated\":0},\"parentID\":\"\",\"summary\":null,\"cost\":0,\"tokens\":{\"total\":0,\"input\":0,\"output\":0,\"reasoning\":0},\"finishReason\":null}}");
        let sse = format!(
            "event: message.part.updated\ndata: {d1}\n\nevent: message.part.updated\ndata: {d2}\n\nevent: message.updated\ndata: {d3}\n\n"
        );
        let events = BufferedStream::parse_sse(sse.as_bytes());
        assert_eq!(events.len(), 3);
        assert!(matches!(events[2], Ok(BackendEvent::Done)));
    }

    #[test]
    fn parse_no_events_for_empty_body() {
        let events = BufferedStream::parse_sse(b"");
        assert!(events.is_empty());
    }
}
