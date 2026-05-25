#![cfg_attr(not(test), no_std)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
#![allow(async_fn_in_trait)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use futures_core::Stream;

mod types;
pub use types::*;

#[cfg(feature = "mock")]
pub mod mock;

// --- Backend event (yielded by streams) ---

#[derive(Debug, Clone)]
pub enum BackendEvent {
    Error {
        message: String,
    },
    SessionCreated {
        session: Session,
    },
    SessionDeleted {
        session_id: SessionId,
    },
    SessionUpdated {
        session: Session,
    },
    SessionStatus {
        session_id: SessionId,
        status: SessionStatus,
    },
    SessionIdle {
        session_id: SessionId,
    },
    SessionError {
        session_id: SessionId,
        error: ServerError,
    },
    SessionDiff {
        session_id: SessionId,
        diff: Vec<SnapshotFileDiff>,
    },
    SessionCompacted {
        session_id: SessionId,
    },
    MessageUpdated {
        session_id: SessionId,
        message: Message,
    },
    MessageRemoved {
        session_id: SessionId,
        message_id: MessageId,
    },
    MessagePartUpdated {
        session_id: SessionId,
        message_id: MessageId,
        part_id: String,
        part: Part,
    },
    MessagePartDelta {
        session_id: SessionId,
        message_id: MessageId,
        part_id: String,
        field: String,
        delta: String,
    },
    MessagePartRemoved {
        session_id: SessionId,
        message_id: MessageId,
        part_id: String,
    },
    PermissionAsked {
        request: PermissionRequest,
    },
    PermissionReplied {
        session_id: SessionId,
        request_id: String,
        reply: String,
    },
    QuestionAsked {
        request: QuestionRequest,
    },
    QuestionRejected {
        session_id: SessionId,
        request_id: String,
    },
    QuestionReplied {
        session_id: SessionId,
        request_id: String,
        answers: Vec<Vec<String>>,
    },
    CommandExecuted {
        name: String,
        session_id: SessionId,
        arguments: String,
        message_id: String,
    },
    FileEdited {
        file: String,
    },
    FileWatcherUpdated {
        file: String,
        event: String,
    },
    PtyCreated {
        info: Pty,
    },
    PtyUpdated {
        info: Pty,
    },
    PtyDeleted {
        id: String,
    },
    PtyExited {
        id: String,
        exit_code: i32,
    },
    LspDiagnostics {
        server_id: String,
        path: String,
    },
    LspUpdated,
    McpBrowserOpenFailed {
        mcp_name: String,
        url: String,
    },
    McpToolsChanged {
        server: String,
    },
    InstallationUpdateAvailable {
        version: String,
    },
    InstallationUpdated {
        version: String,
    },
    WorkspaceReady {
        name: String,
    },
    WorkspaceFailed {
        message: String,
    },
    WorktreeReady {
        name: String,
        branch: String,
    },
    WorktreeFailed {
        message: String,
    },
    VcsBranchUpdated {
        branch: String,
    },
    TodoUpdated {
        session_id: SessionId,
        todos: Vec<Todo>,
    },
    TuiPromptAppend {
        text: String,
    },
    TuiCommandExecute {
        command: String,
    },
    TuiToastShow {
        message: String,
        variant: String,
        title: Option<String>,
        duration: Option<u64>,
    },
    TuiSessionSelect {
        session_id: SessionId,
    },
    ProjectUpdated(Project),
    ServerConnected,
    GlobalDisposed,
    ServerInstanceDisposed {
        directory: String,
    },
}

// --- Backend error ---

#[derive(Debug, Clone)]
pub enum BackendError {
    Connection { message: String },
    Api { status: u16, message: String },
    Timeout,
    Parse { message: String },
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connection { message } => write!(f, "connection error: {message}"),
            Self::Api { status, message } => {
                write!(f, "api error ({status}): {message}")
            }
            Self::Timeout => write!(f, "request timed out"),
            Self::Parse { message } => write!(f, "parse error: {message}"),
        }
    }
}

impl fmt::Display for BackendEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Error { message } => write!(f, "error: {message}"),
            Self::SessionCreated { session } => write!(f, "session.created: {}", session.id),
            Self::SessionDeleted { session_id } => write!(f, "session.deleted: {session_id}"),
            Self::SessionUpdated { session } => write!(f, "session.updated: {}", session.id),
            Self::SessionStatus { session_id, status } => {
                write!(f, "session.status: {session_id}: {}", status.status_type)
            }
            Self::SessionIdle { session_id } => write!(f, "session.idle: {session_id}"),
            Self::SessionError { session_id, error } => {
                write!(f, "session.error: {session_id}: {error:?}")
            }
            Self::SessionDiff { session_id, .. } => write!(f, "session.diff: {session_id}"),
            Self::SessionCompacted { session_id } => write!(f, "session.compacted: {session_id}"),
            Self::MessageUpdated { session_id, .. } => write!(f, "message.updated: {session_id}"),
            Self::MessageRemoved {
                session_id,
                message_id,
            } => write!(f, "message.removed: {session_id}/{message_id}"),
            Self::MessagePartUpdated {
                session_id,
                message_id,
                part_id,
                ..
            } => {
                write!(
                    f,
                    "message.part.updated: {session_id}/{message_id}/{part_id}"
                )
            }
            Self::MessagePartDelta {
                session_id,
                message_id,
                part_id,
                ..
            } => write!(f, "message.part.delta: {session_id}/{message_id}/{part_id}"),
            Self::MessagePartRemoved {
                session_id,
                message_id,
                part_id,
            } => write!(
                f,
                "message.part.removed: {session_id}/{message_id}/{part_id}"
            ),
            Self::PermissionAsked { request } => {
                write!(f, "permission.asked: {}", request.permission)
            }
            Self::PermissionReplied {
                session_id,
                request_id,
                reply,
            } => write!(f, "permission.replied: {session_id}/{request_id}: {reply}"),
            Self::QuestionAsked { request } => write!(f, "question.asked: {}", request.id),
            Self::QuestionRejected {
                session_id,
                request_id,
            } => write!(f, "question.rejected: {session_id}/{request_id}"),
            Self::QuestionReplied {
                session_id,
                request_id,
                ..
            } => write!(f, "question.replied: {session_id}/{request_id}"),
            Self::CommandExecuted {
                name, session_id, ..
            } => write!(f, "command.executed: {name} ({session_id})"),
            Self::FileEdited { file } => write!(f, "file.edited: {file}"),
            Self::FileWatcherUpdated { file, event } => {
                write!(f, "file.watcher.updated: {file} ({event})")
            }
            Self::PtyCreated { info } => write!(f, "pty.created: {}", info.id),
            Self::PtyUpdated { info } => write!(f, "pty.updated: {}", info.id),
            Self::PtyDeleted { id } => write!(f, "pty.deleted: {id}"),
            Self::PtyExited { id, exit_code } => write!(f, "pty.exited: {id} ({exit_code})"),
            Self::LspDiagnostics { server_id, path } => {
                write!(f, "lsp.diagnostics: {server_id}: {path}")
            }
            Self::LspUpdated => write!(f, "lsp.updated"),
            Self::McpBrowserOpenFailed { mcp_name, url } => {
                write!(f, "mcp.browser.open.failed: {mcp_name}: {url}")
            }
            Self::McpToolsChanged { server } => write!(f, "mcp.tools.changed: {server}"),
            Self::InstallationUpdateAvailable { version } => {
                write!(f, "installation.update-available: {version}")
            }
            Self::InstallationUpdated { version } => write!(f, "installation.updated: {version}"),
            Self::WorkspaceReady { name } => write!(f, "workspace.ready: {name}"),
            Self::WorkspaceFailed { message } => write!(f, "workspace.failed: {message}"),
            Self::WorktreeReady { name, branch } => write!(f, "worktree.ready: {name} ({branch})"),
            Self::WorktreeFailed { message } => write!(f, "worktree.failed: {message}"),
            Self::VcsBranchUpdated { branch } => write!(f, "vcs.branch.updated: {branch}"),
            Self::TodoUpdated { session_id, .. } => write!(f, "todo.updated: {session_id}"),
            Self::TuiPromptAppend { text } => write!(f, "tui.prompt.append: {text}"),
            Self::TuiCommandExecute { command } => write!(f, "tui.command.execute: {command}"),
            Self::TuiToastShow {
                message, variant, ..
            } => write!(f, "tui.toast.show: [{variant}] {message}"),
            Self::TuiSessionSelect { session_id } => write!(f, "tui.session.select: {session_id}"),
            Self::ProjectUpdated(project) => write!(f, "project.updated: {}", project.id),
            Self::ServerConnected => write!(f, "server.connected"),
            Self::GlobalDisposed => write!(f, "global.disposed"),
            Self::ServerInstanceDisposed { directory } => {
                write!(f, "server.instance.disposed: {directory}")
            }
        }
    }
}

pub type Result<T> = core::result::Result<T, BackendError>;

// --- The Backend trait ---

pub trait Backend {
    type EventStream: Stream<Item = Result<EventEnvelope>> + Unpin;

    fn server_url(&self) -> Option<&str> {
        None
    }

    async fn test_server_url(&mut self, _url: &str) -> Result<Health> {
        Err(BackendError::Connection {
            message: "server URL testing is not supported by this backend".into(),
        })
    }

    async fn set_server_url(&mut self, _url: &str) -> Result<()> {
        Err(BackendError::Connection {
            message: "server URL configuration is not supported by this backend".into(),
        })
    }

    async fn health(&mut self) -> Result<Health>;

    async fn list_agents(&mut self) -> Result<Vec<Agent>>;

    async fn list_sessions(&mut self) -> Result<Vec<Session>>;
    async fn get_session(&mut self, id: &SessionId) -> Result<Session>;
    async fn create_session(&mut self, title: &str, session_directory: &str) -> Result<Session>;
    async fn delete_session(&mut self, id: &SessionId) -> Result<()>;
    async fn update_session(&mut self, id: &SessionId, title: &str) -> Result<Session>;
    async fn children_sessions(&mut self, id: &SessionId) -> Result<Vec<Session>>;
    async fn abort_session(&mut self, id: &SessionId) -> Result<()>;

    async fn list_messages(&mut self, id: &SessionId) -> Result<Vec<MessageSummary>>;
    async fn get_message(
        &mut self,
        session_id: &SessionId,
        message_id: &MessageId,
    ) -> Result<MessageDetail>;

    async fn submit_prompt(
        &mut self,
        id: &SessionId,
        text: &str,
        agent: Option<&str>,
    ) -> Result<SubmissionReceipt>;

    async fn submit_command(
        &mut self,
        id: &SessionId,
        text: &str,
        agent: Option<&str>,
    ) -> Result<SubmissionReceipt>;

    async fn reply_permission(&mut self, reply: &PermissionReply) -> Result<()>;

    async fn reply_question(&mut self, reply: &QuestionReply) -> Result<()>;

    async fn reject_question(&mut self, request_id: &str) -> Result<()>;

    async fn find_text(&mut self, pattern: &str) -> Result<Vec<TextMatch>>;

    async fn subscribe_live(&mut self) -> Result<Self::EventStream>;

    async fn sync_history(&mut self, request: &SyncHistoryRequest) -> Result<SyncHistoryBatch>;

    async fn get_config(&mut self) -> Result<Config>;

    async fn list_models(&mut self) -> Result<Vec<ModelSummary>>;

    async fn set_auth(&mut self, provider: &str, api_key: &str) -> Result<()>;

    async fn set_config(&mut self, config: &Config) -> Result<Config>;

    async fn dispose(&mut self) -> Result<()>;

    async fn upgrade(&mut self) -> Result<()>;

    async fn log(&mut self, level: &str, message: &str) -> Result<()>;

    async fn remove_auth(&mut self, provider: &str) -> Result<()>;
}
