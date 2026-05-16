use alloc::string::String;
use alloc::vec::Vec;
use core::pin::Pin;
use core::task::{Context, Poll};
use futures_core::Stream;
use opencode_backend::{BackendError, BackendEvent, Part, Result};
use serde::Deserialize;

#[cfg(feature = "std")]
use crate::sse_stream::TcpSseStream;

enum SseSource {
    PreParsed {
        events: Vec<Result<BackendEvent>>,
        pos: usize,
    },
    #[cfg(feature = "std")]
    Direct(TcpSseStream),
}

/// A stream that yields [`BackendEvent`]s.
///
/// Can be either pre-parsed from a completed HTTP response body (used by `prompt`
/// and `command`), or a real-time SSE stream (used by `subscribe` on `std`).
pub struct BufferedStream {
    source: SseSource,
}

impl BufferedStream {
    pub fn new(events: Vec<Result<BackendEvent>>) -> Self {
        Self {
            source: SseSource::PreParsed { events, pos: 0 },
        }
    }

    #[cfg(feature = "std")]
    pub fn from_tcp_stream(stream: TcpSseStream) -> Self {
        Self {
            source: SseSource::Direct(stream),
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
        match &mut this.source {
            SseSource::PreParsed { events, pos } => {
                if *pos < events.len() {
                    let item = match &events[*pos] {
                        Ok(e) => Ok(e.clone()),
                        Err(e) => Err(e.clone()),
                    };
                    *pos += 1;
                    Poll::Ready(Some(item))
                } else {
                    Poll::Ready(None)
                }
            }
            #[cfg(feature = "std")]
            SseSource::Direct(stream) => Pin::new(stream).poll_next(cx),
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
fn parse_sse_block(block: &[u8]) -> Option<Result<BackendEvent>> {
    let text = core::str::from_utf8(block).ok()?;

    let mut event_type = "";
    let mut data = "";

    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(value) = line.strip_prefix("event: ") {
            event_type = value;
        } else if let Some(value) = line.strip_prefix("data: ") {
            data = value;
        }
    }

    if data.is_empty() {
        return None;
    }

    match event_type {
        "message.part.updated" => parse_part_updated(data),
        "message.updated" => Some(Ok(BackendEvent::Done)),
        "session.idle" => Some(parse_json::<SessionIdWrap>(data)
            .map(|w| BackendEvent::SessionIdle { session_id: w.session_id })),
        "session.error" => Some(parse_json::<SessionErrorWrap>(data)
            .map(|w| BackendEvent::SessionError { session_id: w.session_id, error: w.error })),
        "session.status" => Some(parse_json::<SessionStatusWrap>(data)
            .map(|w| {
                if w.status.status_type == "idle" {
                    BackendEvent::Done
                } else {
                    BackendEvent::SessionIdle { session_id: w.session_id }
                }
            })),
        "server.connected" => Some(Ok(BackendEvent::ServerConnected)),
        "global.disposed" => Some(Ok(BackendEvent::GlobalDisposed)),
        "session.created" => Some(parse_json::<SessionCreatedWrap>(data)
            .map(|w| BackendEvent::SessionCreated { session: w.info })),
        "session.updated" => Some(parse_json::<SessionUpdatedWrap>(data)
            .map(|w| BackendEvent::SessionUpdated { session: w.info })),
        "session.deleted" => Some(parse_json::<SessionDeletedWrap>(data)
            .map(|w| BackendEvent::SessionDeleted { session_id: w.session_id })),
        "session.diff" => Some(parse_json::<SessionDiffWrap>(data)
            .map(|w| BackendEvent::SessionDiff { session_id: w.session_id, diff: w.diff })),
        "session.compacted" => Some(parse_json::<SessionCompactedWrap>(data)
            .map(|w| BackendEvent::SessionCompacted { session_id: w.session_id })),
        "message.part.delta" => Some(parse_json::<PartDeltaWrap>(data)
            .map(|w| BackendEvent::MessagePartDelta {
                session_id: w.session_id,
                message_id: w.message_id,
                part_id: w.part_id,
                field: w.field,
                delta: w.delta,
            })),
        "message.part.removed" => Some(parse_json::<PartRemovedWrap>(data)
            .map(|w| BackendEvent::MessagePartRemoved {
                session_id: w.session_id,
                message_id: w.message_id,
                part_id: w.part_id,
            })),
        "permission.asked" => Some(parse_json::<PermissionAskedWrap>(data)
            .map(|w| BackendEvent::PermissionAsked { request: w.request })),
        "permission.replied" => Some(parse_json::<PermissionRepliedWrap>(data)
            .map(|w| BackendEvent::PermissionReplied {
                session_id: w.session_id,
                request_id: w.request_id,
                reply: w.reply,
            })),
        "question.asked" => Some(parse_json::<QuestionAskedWrap>(data)
            .map(|w| BackendEvent::QuestionAsked { request: w.request })),
        "question.rejected" => Some(parse_json::<QuestionRejectedWrap>(data)
            .map(|w| BackendEvent::QuestionRejected {
                session_id: w.session_id,
                request_id: w.request_id,
            })),
        "question.replied" => Some(parse_json::<QuestionRepliedWrap>(data)
            .map(|w| BackendEvent::QuestionReplied {
                session_id: w.session_id,
                request_id: w.request_id,
                answers: w.answers,
            })),
        "command.executed" => Some(parse_json::<CommandExecutedWrap>(data)
            .map(|w| BackendEvent::CommandExecuted {
                name: w.name,
                session_id: w.session_id,
                arguments: w.arguments,
                message_id: w.message_id,
            })),
        "file.edited" => Some(parse_json::<FileEditedWrap>(data)
            .map(|w| BackendEvent::FileEdited { file: w.file })),
        "file.watcher.updated" => Some(parse_json::<FileWatcherWrap>(data)
            .map(|w| BackendEvent::FileWatcherUpdated { file: w.file, event: w.event })),
        "pty.created" => Some(parse_json::<PtyCreatedWrap>(data)
            .map(|w| BackendEvent::PtyCreated { info: w.info })),
        "pty.updated" => Some(parse_json::<PtyUpdatedWrap>(data)
            .map(|w| BackendEvent::PtyUpdated { info: w.info })),
        "pty.deleted" => Some(parse_json::<PtyDeletedWrap>(data)
            .map(|w| BackendEvent::PtyDeleted { id: w.id })),
        "pty.exited" => Some(parse_json::<PtyExitedWrap>(data)
            .map(|w| BackendEvent::PtyExited { id: w.id, exit_code: w.exit_code })),
        "lsp.client.diagnostics" => Some(parse_json::<LspDiagnosticsWrap>(data)
            .map(|w| BackendEvent::LspDiagnostics { server_id: w.server_id, path: w.path })),
        "lsp.updated" => Some(Ok(BackendEvent::LspUpdated)),
        "mcp.browser.open.failed" => Some(parse_json::<McpBrowserFailedWrap>(data)
            .map(|w| BackendEvent::McpBrowserOpenFailed { mcp_name: w.mcp_name, url: w.url })),
        "mcp.tools.changed" => Some(parse_json::<McpToolsChangedWrap>(data)
            .map(|w| BackendEvent::McpToolsChanged { server: w.server })),
        "installation.update-available" => Some(parse_json::<InstallationUpdateWrap>(data)
            .map(|w| BackendEvent::InstallationUpdateAvailable { version: w.version })),
        "installation.updated" => Some(parse_json::<InstallationUpdatedWrap>(data)
            .map(|w| BackendEvent::InstallationUpdated { version: w.version })),
        "workspace.ready" => Some(parse_json::<WorkspaceReadyWrap>(data)
            .map(|w| BackendEvent::WorkspaceReady { name: w.name })),
        "workspace.failed" => Some(parse_json::<WorkspaceFailedWrap>(data)
            .map(|w| BackendEvent::WorkspaceFailed { message: w.message })),
        "worktree.ready" => Some(parse_json::<WorktreeReadyWrap>(data)
            .map(|w| BackendEvent::WorktreeReady { name: w.name, branch: w.branch })),
        "worktree.failed" => Some(parse_json::<WorktreeFailedWrap>(data)
            .map(|w| BackendEvent::WorktreeFailed { message: w.message })),
        "vcs.branch.updated" => Some(parse_json::<VcsBranchWrap>(data)
            .map(|w| BackendEvent::VcsBranchUpdated { branch: w.branch })),
        "todo.updated" => Some(parse_json::<TodoUpdatedWrap>(data)
            .map(|w| BackendEvent::TodoUpdated { session_id: w.session_id, todos: w.todos })),
        "tui.prompt.append" => Some(parse_json::<TuiPromptAppendWrap>(data)
            .map(|w| BackendEvent::TuiPromptAppend { text: w.text })),
        "tui.command.execute" => Some(parse_json::<TuiCommandExecuteWrap>(data)
            .map(|w| BackendEvent::TuiCommandExecute { command: w.command })),
        "tui.toast.show" => Some(parse_json::<TuiToastShowWrap>(data)
            .map(|w| BackendEvent::TuiToastShow {
                message: w.message,
                variant: w.variant,
                title: w.title,
                duration: w.duration,
            })),
        "tui.session.select" => Some(parse_json::<TuiSessionSelectWrap>(data)
            .map(|w| BackendEvent::TuiSessionSelect { session_id: w.session_id })),
        "project.updated" => Some(parse_json::<ProjectUpdatedWrap>(data)
            .map(|w| BackendEvent::ProjectUpdated(opencode_backend::Project {
                id: w.id,
                worktree: w.worktree,
                name: w.name,
            }))),
        "server.instance.disposed" => Some(parse_json::<ServerInstanceDisposedWrap>(data)
            .map(|w| BackendEvent::ServerInstanceDisposed { directory: w.directory })),

        // --- Sync event stream variants (from /global/sync-event) ---
        "session.created.1" => Some(parse_json::<SyncSessionWrap>(data)
            .map(|w| BackendEvent::SessionCreated { session: w.data.info })),
        "session.updated.1" => Some(parse_json::<SyncSessionWrap>(data)
            .map(|w| BackendEvent::SessionUpdated { session: w.data.info })),
        "session.deleted.1" => Some(parse_json::<SyncSessionWrap>(data)
            .map(|w| BackendEvent::SessionDeleted { session_id: w.data.session_id })),
        "message.removed.1" => Some(parse_json::<SyncMessageRemovedWrap>(data)
            .map(|w| BackendEvent::MessageRemoved { session_id: w.data.session_id, message_id: w.data.message_id })),
        "message.part.updated.1" => Some(parse_json::<SyncPartWrap>(data)
            .map(|w| BackendEvent::Part { part: w.data.part, delta: None })),
        "message.part.removed.1" => Some(parse_json::<SyncPartRemovedWrap>(data)
            .map(|w| BackendEvent::MessagePartRemoved { session_id: w.data.session_id, message_id: w.data.message_id, part_id: w.data.part_id })),

        _ => None,
    }
}

#[derive(Deserialize)]
struct SessionIdWrap {
    #[serde(rename = "sessionID")]
    session_id: opencode_backend::SessionId,
}

#[derive(Deserialize)]
struct SessionErrorWrap {
    #[serde(rename = "sessionID")]
    session_id: opencode_backend::SessionId,
    error: opencode_backend::ServerError,
}

#[derive(Deserialize)]
struct SessionStatusWrap {
    #[serde(rename = "sessionID")]
    session_id: opencode_backend::SessionId,
    status: opencode_backend::SessionStatus,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct SessionCreatedWrap {
    #[serde(rename = "sessionID")]
    session_id: opencode_backend::SessionId,
    info: opencode_backend::Session,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct SessionUpdatedWrap {
    #[serde(rename = "sessionID")]
    session_id: opencode_backend::SessionId,
    info: opencode_backend::Session,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct SessionDeletedWrap {
    #[serde(rename = "sessionID")]
    session_id: opencode_backend::SessionId,
    info: opencode_backend::Session,
}

#[derive(Deserialize)]
struct SessionDiffWrap {
    #[serde(rename = "sessionID")]
    session_id: opencode_backend::SessionId,
    diff: Vec<opencode_backend::SnapshotFileDiff>,
}

#[derive(Deserialize)]
struct SessionCompactedWrap {
    #[serde(rename = "sessionID")]
    session_id: opencode_backend::SessionId,
}

#[derive(Deserialize)]
struct PartDeltaWrap {
    #[serde(rename = "sessionID")]
    session_id: opencode_backend::SessionId,
    #[serde(rename = "messageID")]
    message_id: opencode_backend::MessageId,
    #[serde(rename = "partID")]
    part_id: String,
    field: String,
    delta: String,
}

#[derive(Deserialize)]
struct PartRemovedWrap {
    #[serde(rename = "sessionID")]
    session_id: opencode_backend::SessionId,
    #[serde(rename = "messageID")]
    message_id: opencode_backend::MessageId,
    #[serde(rename = "partID")]
    part_id: String,
}

#[derive(Deserialize)]
struct PermissionAskedWrap {
    #[serde(flatten)]
    request: opencode_backend::PermissionRequest,
}

#[derive(Deserialize)]
struct PermissionRepliedWrap {
    #[serde(rename = "sessionID")]
    session_id: opencode_backend::SessionId,
    #[serde(rename = "requestID")]
    request_id: String,
    reply: String,
}

#[derive(Deserialize)]
struct QuestionAskedWrap {
    #[serde(flatten)]
    request: opencode_backend::QuestionRequest,
}

#[derive(Deserialize)]
struct QuestionRejectedWrap {
    #[serde(rename = "sessionID")]
    session_id: opencode_backend::SessionId,
    #[serde(rename = "requestID")]
    request_id: String,
}

#[derive(Deserialize)]
struct QuestionRepliedWrap {
    #[serde(rename = "sessionID")]
    session_id: opencode_backend::SessionId,
    #[serde(rename = "requestID")]
    request_id: String,
    answers: Vec<String>,
}

#[derive(Deserialize)]
struct CommandExecutedWrap {
    name: String,
    #[serde(rename = "sessionID")]
    session_id: opencode_backend::SessionId,
    arguments: String,
    #[serde(rename = "messageID")]
    message_id: opencode_backend::MessageId,
}

#[derive(Deserialize)]
struct FileEditedWrap {
    file: String,
}

#[derive(Deserialize)]
struct FileWatcherWrap {
    file: String,
    event: String,
}

#[derive(Deserialize)]
struct PtyCreatedWrap {
    info: opencode_backend::Pty,
}

#[derive(Deserialize)]
struct PtyUpdatedWrap {
    info: opencode_backend::Pty,
}

#[derive(Deserialize)]
struct TodoUpdatedWrap {
    #[serde(rename = "sessionID")]
    session_id: opencode_backend::SessionId,
    todos: Vec<opencode_backend::Todo>,
}

#[derive(Deserialize)]
struct PtyDeletedWrap {
    id: String,
}

#[derive(Deserialize)]
struct PtyExitedWrap {
    id: String,
    #[serde(rename = "exitCode")]
    exit_code: i32,
}

#[derive(Deserialize)]
struct LspDiagnosticsWrap {
    #[serde(rename = "serverID")]
    server_id: String,
    path: String,
}

#[derive(Deserialize)]
struct McpBrowserFailedWrap {
    #[serde(rename = "mcpName")]
    mcp_name: String,
    url: String,
}

#[derive(Deserialize)]
struct McpToolsChangedWrap {
    server: String,
}

#[derive(Deserialize)]
struct InstallationUpdateWrap {
    version: String,
}

#[derive(Deserialize)]
struct InstallationUpdatedWrap {
    version: String,
}

#[derive(Deserialize)]
struct WorkspaceReadyWrap {
    name: String,
}

#[derive(Deserialize)]
struct WorkspaceFailedWrap {
    message: String,
}

#[derive(Deserialize)]
struct WorktreeReadyWrap {
    name: String,
    branch: String,
}

#[derive(Deserialize)]
struct WorktreeFailedWrap {
    message: String,
}

#[derive(Deserialize)]
struct VcsBranchWrap {
    branch: String,
}

#[derive(Deserialize)]
struct TuiPromptAppendWrap {
    text: String,
}

#[derive(Deserialize)]
struct TuiCommandExecuteWrap {
    command: String,
}

#[derive(Deserialize)]
struct TuiToastShowWrap {
    message: String,
    variant: String,
    title: Option<String>,
    duration: Option<u64>,
}

#[derive(Deserialize)]
struct TuiSessionSelectWrap {
    #[serde(rename = "sessionID")]
    session_id: opencode_backend::SessionId,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectUpdatedWrap {
    id: String,
    worktree: String,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct ServerInstanceDisposedWrap {
    directory: String,
}

// --- Sync event stream wrap types (from /global/sync-event) ---

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncSessionWrap {
    data: SyncSessionDataWrap,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncSessionDataWrap {
    #[serde(rename = "sessionID")]
    session_id: opencode_backend::SessionId,
    info: opencode_backend::Session,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncMessageWrap {
    data: SyncMessageDataWrap,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncMessageDataWrap {
    #[serde(rename = "sessionID")]
    session_id: opencode_backend::SessionId,
    info: opencode_backend::Message,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncMessageRemovedWrap {
    data: SyncMessageRemovedDataWrap,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncMessageRemovedDataWrap {
    #[serde(rename = "sessionID")]
    session_id: opencode_backend::SessionId,
    #[serde(rename = "messageID")]
    message_id: opencode_backend::MessageId,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncPartWrap {
    data: SyncPartDataWrap,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncPartDataWrap {
    #[serde(rename = "sessionID")]
    session_id: opencode_backend::SessionId,
    part: opencode_backend::Part,
    time: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncPartRemovedWrap {
    data: SyncPartRemovedDataWrap,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncPartRemovedDataWrap {
    #[serde(rename = "sessionID")]
    session_id: opencode_backend::SessionId,
    #[serde(rename = "messageID")]
    message_id: opencode_backend::MessageId,
    #[serde(rename = "partID")]
    part_id: String,
}

/// Parse a `message.part.updated` event data JSON into a `BackendEvent::Part`.
fn parse_part_updated(data: &str) -> Option<Result<BackendEvent>> {
    #[derive(Deserialize)]
    struct PartUpdated {
        part: Part,
        delta: Option<String>,
    }

    match parse_json::<PartUpdated>(data) {
        Ok(parsed) => Some(Ok(BackendEvent::Part {
            part: parsed.part,
            delta: parsed.delta,
        })),
        Err(e) => Some(Err(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_text_part() {
        let sse = b"event: message.part.updated\ndata: {\"part\":{\"type\":\"text\",\"text\":\"Hello\"},\"delta\":\"Hello\"}\n\n";
        let events = BufferedStream::parse_sse(sse);
        assert_eq!(events.len(), 1);
        match &events[0] {
            Ok(BackendEvent::Part { part, delta }) => {
                assert!(matches!(part, Part::Text(_)));
                assert_eq!(delta.as_deref(), Some("Hello"));
            }
            _ => panic!("expected text part"),
        }
    }

    #[test]
    fn parse_tool_part() {
        let sse = b"event: message.part.updated\ndata: {\"part\":{\"type\":\"tool\",\"tool\":\"bash\",\"state\":{\"status\":\"running\",\"input\":{},\"time\":{\"start\":0}}}}\n\n";
        let events = BufferedStream::parse_sse(sse);
        assert_eq!(events.len(), 1);
        match &events[0] {
            Ok(BackendEvent::Part { part, .. }) => {
                assert!(matches!(part, Part::Tool(_)));
            }
            _ => panic!("expected tool part"),
        }
    }

    #[test]
    fn parse_done_event() {
        let sse = b"event: message.updated\ndata: {}\n\n";
        let events = BufferedStream::parse_sse(sse);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], Ok(BackendEvent::Done)));
    }

    #[test]
    fn parse_server_connected() {
        let sse = b"event: server.connected\ndata: {}\n\n";
        let events = BufferedStream::parse_sse(sse);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], Ok(BackendEvent::ServerConnected)));
    }

    #[test]
    fn parse_session_created() {
        let sse = b"event: session.created\ndata: {\"sessionID\":\"ses123\",\"info\":{\"id\":\"ses123\",\"title\":\"Test\",\"projectID\":\"proj1\",\"directory\":\"/tmp\",\"slug\":\"\",\"version\":\"1\",\"time\":{\"created\":0,\"updated\":0}}}\n\n";
        let events = BufferedStream::parse_sse(sse);
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
        let sse = b"event: session.deleted\ndata: {\"sessionID\":\"ses123\",\"info\":{\"id\":\"ses123\",\"title\":\"Test\",\"projectID\":\"proj1\",\"directory\":\"/tmp\",\"slug\":\"\",\"version\":\"1\",\"time\":{\"created\":0,\"updated\":0}}}\n\n";
        let events = BufferedStream::parse_sse(sse);
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
        let sse = b"event: session.idle\ndata: {\"sessionID\":\"ses123\"}\n\n";
        let events = BufferedStream::parse_sse(sse);
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
        let sse = b"event: session.status\ndata: {\"sessionID\":\"ses123\",\"status\":{\"type\":\"idle\"}}\n\n";
        let events = BufferedStream::parse_sse(sse);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], Ok(BackendEvent::Done)));
    }

    #[test]
    fn parse_session_error() {
        let sse = b"event: session.error\ndata: {\"sessionID\":\"ses123\",\"error\":{\"name\":\"APIError\",\"data\":{\"message\":\"fail\"}}}\n\n";
        let events = BufferedStream::parse_sse(sse);
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
        let sse = b"event: permission.asked\ndata: {\"id\":\"per1\",\"sessionID\":\"ses1\",\"permission\":\"bash\",\"patterns\":[],\"metadata\":{},\"always\":[]}\n\n";
        let events = BufferedStream::parse_sse(sse);
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
        let sse = b"event: question.asked\ndata: {\"id\":\"que1\",\"sessionID\":\"ses1\",\"questions\":[{\"question\":\"Proceed?\",\"header\":\"Confirm\",\"options\":[{\"label\":\"Yes\",\"description\":\"yes\"},{\"label\":\"No\",\"description\":\"no\"}]}]}\n\n";
        let events = BufferedStream::parse_sse(sse);
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
        let sse = b"event: command.executed\ndata: {\"name\":\"build\",\"sessionID\":\"ses1\",\"arguments\":\"--release\",\"messageID\":\"msg1\"}\n\n";
        let events = BufferedStream::parse_sse(sse);
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
        let sse = b"event: file.edited\ndata: {\"file\":\"src/main.rs\"}\n\n";
        let events = BufferedStream::parse_sse(sse);
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
        let sse = b"event: pty.created\ndata: {\"info\":{\"id\":\"pty1\",\"title\":\"bash\",\"command\":\"bash\",\"args\":[],\"cwd\":\"/tmp\",\"status\":\"running\",\"pid\":1234}}\n\n";
        let events = BufferedStream::parse_sse(sse);
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
        let sse = b"event: lsp.client.diagnostics\ndata: {\"serverID\":\"rust-analyzer\",\"path\":\"src/main.rs\"}\n\n";
        let events = BufferedStream::parse_sse(sse);
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
        let sse = b"event: mcp.tools.changed\ndata: {\"server\":\"my-mcp\"}\n\n";
        let events = BufferedStream::parse_sse(sse);
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
        let sse = b"event: installation.update-available\ndata: {\"version\":\"1.2.3\"}\n\n";
        let events = BufferedStream::parse_sse(sse);
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
        let sse = b"event: workspace.ready\ndata: {\"name\":\"my-project\"}\n\n";
        let events = BufferedStream::parse_sse(sse);
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
        let sse = b"event: todo.updated\ndata: {\"sessionID\":\"ses1\",\"todos\":[{\"content\":\"Fix bug\",\"status\":\"pending\",\"priority\":\"high\"}]}\n\n";
        let events = BufferedStream::parse_sse(sse);
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
        let sse = b"event: tui.toast.show\ndata: {\"message\":\"Done!\",\"variant\":\"success\"}\n\n";
        let events = BufferedStream::parse_sse(sse);
        assert_eq!(events.len(), 1);
        match &events[0] {
            Ok(BackendEvent::TuiToastShow { message, variant, .. }) => {
                assert_eq!(message, "Done!");
                assert_eq!(variant, "success");
            }
            _ => panic!("expected tui.toast.show"),
        }
    }

    #[test]
    fn parse_global_disposed() {
        let sse = b"event: global.disposed\ndata: {}\n\n";
        let events = BufferedStream::parse_sse(sse);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], Ok(BackendEvent::GlobalDisposed)));
    }

    #[test]
    fn parse_multiple_events() {
        let sse = b"\
event: message.part.updated
data: {\"part\":{\"type\":\"text\",\"text\":\"One\"},\"delta\":\"One\"}

event: message.part.updated
data: {\"part\":{\"type\":\"text\",\"text\":\"Two\"},\"delta\":\"Two\"}

event: message.updated
data: {}

";
        let events = BufferedStream::parse_sse(sse);
        assert_eq!(events.len(), 3);
        assert!(matches!(events[2], Ok(BackendEvent::Done)));
    }

    #[test]
    fn parse_no_events_for_empty_body() {
        let events = BufferedStream::parse_sse(b"");
        assert!(events.is_empty());
    }
}
