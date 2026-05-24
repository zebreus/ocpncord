use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

// --- Identifiers ---

pub type SessionId = String;
pub type MessageId = String;

// --- Health ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    pub healthy: bool,
    pub version: String,
}

// --- Session time ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTime {
    pub created: u64,
    pub updated: u64,
}

// --- Sessions ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: SessionId,
    pub title: String,
    #[serde(rename = "projectID")]
    pub project_id: String,
    pub directory: String,
    #[serde(rename = "parentID")]
    pub parent_id: Option<SessionId>,
    pub time: SessionTime,
    pub slug: String,
    pub version: String,
    #[serde(default, rename = "workspaceID")]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub summary: Option<SessionSummary>,
    #[serde(default)]
    pub share: Option<SessionShare>,
    #[serde(default)]
    pub permission: Option<PermissionRuleset>,
    #[serde(default)]
    pub revert: Option<SessionRevert>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub additions: u32,
    pub deletions: u32,
    pub files: u32,
    #[serde(default)]
    pub diffs: Vec<SnapshotFileDiff>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionShare {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRevert {
    pub message_id: String,
    #[serde(default, rename = "partID")]
    pub part_id: Option<String>,
    #[serde(default)]
    pub snapshot: Option<String>,
    #[serde(default)]
    pub diff: Option<String>,
}

// --- Message time ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageTime {
    pub created: u64,
    #[serde(default)]
    pub completed: Option<u64>,
}

// --- Messages ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSummary {
    pub id: MessageId,
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    pub role: MessageRole,
    pub time: MessageTime,
    #[serde(rename = "parentID", default)]
    pub parent_id: Option<MessageId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageDetail {
    pub info: MessageSummary,
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MessageRole {
    User,
    Assistant,
}

// --- Parts ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextPart {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningPart {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolPart {
    pub tool: String,
    pub state: ToolState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum ToolState {
    Pending {
        input: BTreeMap<String, String>,
        raw: String,
    },
    Running {
        input: BTreeMap<String, String>,
        #[serde(default)]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<BTreeMap<String, String>>,
        #[serde(default)]
        time: Option<ToolTime>,
    },
    Completed {
        input: BTreeMap<String, String>,
        output: String,
        title: String,
        metadata: BTreeMap<String, String>,
        time: ToolTimeCompleted,
        #[serde(default)]
        attachments: Vec<FilePart>,
    },
    Error {
        input: BTreeMap<String, String>,
        error: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<BTreeMap<String, String>>,
        time: ToolTimeCompleted,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolTime {
    pub start: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolTimeCompleted {
    pub start: u64,
    pub end: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepStartPart {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,
    #[serde(rename = "sessionID", default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepFinishPart {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,
    #[serde(rename = "sessionID", default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePart {
    pub mime: String,
    pub url: String,
    #[serde(default)]
    pub filename: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotPart {
    pub snapshot: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchPart {
    pub hash: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPart {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtaskPart {
    pub prompt: String,
    pub description: String,
    pub agent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryPart {
    pub attempt: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionPart {
    pub auto: bool,
    #[serde(default)]
    pub overflow: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum Part {
    Text(TextPart),
    Reasoning(ReasoningPart),
    Tool(ToolPart),
    #[serde(rename = "step-start")]
    StepStart(StepStartPart),
    #[serde(rename = "step-finish")]
    StepFinish(StepFinishPart),
    File(FilePart),
    Snapshot(SnapshotPart),
    Patch(PatchPart),
    Agent(AgentPart),
    Subtask(SubtaskPart),
    Retry(RetryPart),
    Compaction(CompactionPart),
}

// --- Agents ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub mode: AgentMode,
    #[serde(default)]
    pub native: Option<bool>,
    #[serde(default)]
    pub hidden: Option<bool>,
    #[serde(default)]
    pub model: Option<AgentModel>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub variant: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub steps: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentMode {
    Primary,
    Subagent,
    #[serde(rename = "all")]
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentModel {
    pub model_id: String,
    pub provider_id: String,
}

// --- Search ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextValue {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextMatch {
    pub path: TextValue,
    pub line_number: u64,
    pub lines: TextValue,
}

// --- Config ---

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub provider: BTreeMap<String, ProviderConfig>,
    #[serde(default)]
    pub agent: BTreeMap<String, AgentConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentConfig {
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderConfig {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub models: BTreeMap<String, ModelConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelConfig {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub reasoning: Option<bool>,
    #[serde(default, rename = "tool_call")]
    pub tool_call: Option<bool>,
}

// --- Models ---

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModelSummary {
    pub id: String,
    #[serde(rename = "providerID")]
    pub provider_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub capabilities: Option<ModelCapabilities>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModelCapabilities {
    #[serde(default)]
    pub reasoning: Option<bool>,
    #[serde(default, rename = "toolcall", alias = "tools")]
    pub tool_call: Option<bool>,
    #[serde(default)]
    pub attachment: Option<bool>,
}

// --- Message response (raw from server) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageResponse {
    pub info: MessageSummary,
    pub parts: Vec<Part>,
}

// --- Message (union) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Message {
    User(UserMessage),
    Assistant(AssistantMessage),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserMessage {
    pub id: MessageId,
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    pub role: MessageRole,
    pub time: MessageTime,
    pub agent: String,
    pub model: UserMessageModel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserMessageModel {
    #[serde(rename = "providerID")]
    pub provider_id: String,
    #[serde(rename = "modelID")]
    pub model_id: String,
    #[serde(default)]
    pub variant: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantMessage {
    pub id: MessageId,
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    pub role: MessageRole,
    pub time: MessageTime,
    #[serde(rename = "parentID", default)]
    pub parent_id: Option<MessageId>,
    #[serde(rename = "modelID")]
    pub model_id: String,
    #[serde(rename = "providerID")]
    pub provider_id: String,
    pub mode: String,
    pub agent: String,
    pub cost: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmissionReceipt {
    pub info: AssistantMessage,
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EventScope {
    pub directory: Option<String>,
    pub workspace: Option<String>,
    pub project: Option<String>,
}

impl EventScope {
    pub fn instance(directory: Option<String>, workspace: Option<String>) -> Self {
        Self {
            directory,
            workspace,
            project: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventCursor {
    pub event_id: Option<String>,
    pub aggregate_id: Option<String>,
    pub seq: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct EventEnvelope {
    pub event: crate::BackendEvent,
    pub scope: EventScope,
    pub cursor: Option<EventCursor>,
}

impl EventEnvelope {
    pub fn new(event: crate::BackendEvent) -> Self {
        Self {
            event,
            scope: EventScope::default(),
            cursor: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SyncHistoryRequest {
    pub scope: EventScope,
    pub known_sequences: BTreeMap<String, u64>,
}

impl Default for SyncHistoryRequest {
    fn default() -> Self {
        Self {
            scope: EventScope::default(),
            known_sequences: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SyncHistoryBatch {
    pub envelopes: Vec<EventEnvelope>,
    pub known_sequences: BTreeMap<String, u64>,
}

// --- Request body types ---

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSessionBody<'a> {
    pub title: &'a str,
    pub directory: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSessionBody<'a> {
    pub title: &'a str,
}

#[derive(Serialize)]
pub struct TextPartBody<'a> {
    #[serde(rename = "type")]
    pub type_: &'a str,
    pub text: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptBody<'a> {
    pub parts: &'a [TextPartBody<'a>],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<&'a str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandBody<'a> {
    pub command: &'a str,
    pub arguments: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<&'a str>,
}

#[derive(Serialize)]
pub struct AuthBody<'a> {
    #[serde(rename = "type")]
    pub type_: &'a str,
    pub key: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigBody<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<&'a str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogBody<'a> {
    pub level: &'a str,
    pub message: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionReplyBody<'a> {
    pub reply: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionReplyBody<'a> {
    pub answers: &'a [Vec<String>],
}

// --- Error responses ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BadRequestError {
    pub data: String,
    pub errors: Vec<BTreeMap<String, String>>,
    pub success: bool,
}

// --- SSE Event data types ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStatus {
    #[serde(rename = "type")]
    pub status_type: String,
    #[serde(default)]
    pub attempt: Option<u32>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub next: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotFileDiff {
    pub file: String,
    pub patch: String,
    pub additions: u32,
    pub deletions: u32,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequest {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    pub permission: String,
    pub patterns: Vec<String>,
    pub metadata: BTreeMap<String, String>,
    #[serde(default)]
    pub always: Vec<String>,
    #[serde(default)]
    pub tool: Option<PermissionToolInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionToolInfo {
    #[serde(rename = "messageID")]
    pub message_id: String,
    #[serde(rename = "callID")]
    pub call_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionReply {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    #[serde(rename = "requestID")]
    pub request_id: String,
    pub reply: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionRequest {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    pub questions: Vec<QuestionInfo>,
    #[serde(default)]
    pub tool: Option<PermissionToolInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionInfo {
    pub question: String,
    pub header: String,
    pub options: Vec<QuestionOption>,
    #[serde(default)]
    pub multiple: bool,
    #[serde(default = "default_true")]
    pub custom: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionReply {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    #[serde(rename = "requestID")]
    pub request_id: String,
    pub answers: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pty {
    pub id: String,
    pub title: String,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub status: PtyStatus,
    pub pid: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PtyStatus {
    Running,
    Exited,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Todo {
    pub content: String,
    pub status: String,
    pub priority: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub worktree: String,
    #[serde(default)]
    pub name: Option<String>,
}

// --- Permission ruleset ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRule {
    pub permission: String,
    pub pattern: String,
    pub action: String,
}

pub type PermissionRuleset = Vec<PermissionRule>;

// --- Server error (polymorphic) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerError {
    pub name: String,
    pub data: alloc::collections::BTreeMap<String, alloc::string::String>,
}

// --- GlobalEvent envelope ---

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalEvent {
    pub directory: String,
    pub payload: ServerEvent,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum ServerEvent {
    #[serde(rename = "server.connected")]
    ServerConnected {},
    #[serde(rename = "global.disposed")]
    GlobalDisposed {},
    #[serde(rename = "session.created")]
    SessionCreated {
        #[serde(rename = "sessionID")]
        session_id: SessionId,
        info: Session,
    },
    #[serde(rename = "session.updated")]
    SessionUpdated {
        #[serde(rename = "sessionID")]
        session_id: SessionId,
        info: Session,
    },
    #[serde(rename = "session.deleted")]
    SessionDeleted {
        #[serde(rename = "sessionID")]
        session_id: SessionId,
        info: Session,
    },
    #[serde(rename = "session.status")]
    SessionStatus {
        #[serde(rename = "sessionID")]
        session_id: SessionId,
        status: SessionStatus,
    },
    #[serde(rename = "session.idle")]
    SessionIdle {
        #[serde(rename = "sessionID")]
        session_id: SessionId,
    },
    #[serde(rename = "session.error")]
    SessionError {
        #[serde(rename = "sessionID")]
        session_id: SessionId,
        error: ServerError,
    },
    #[serde(rename = "session.diff")]
    SessionDiff {
        #[serde(rename = "sessionID")]
        session_id: SessionId,
        diff: Vec<SnapshotFileDiff>,
    },
    #[serde(rename = "session.compacted")]
    SessionCompacted {
        #[serde(rename = "sessionID")]
        session_id: SessionId,
    },
    #[serde(rename = "message.updated")]
    MessageUpdated {
        #[serde(rename = "sessionID")]
        session_id: SessionId,
        info: Message,
    },
    #[serde(rename = "message.removed")]
    MessageRemoved {
        #[serde(rename = "sessionID")]
        session_id: SessionId,
        #[serde(rename = "messageID")]
        message_id: MessageId,
    },
    #[serde(rename = "message.part.updated")]
    MessagePartUpdated {
        #[serde(rename = "sessionID")]
        session_id: SessionId,
        part: Part,
        time: u64,
    },
    #[serde(rename = "message.part.delta")]
    MessagePartDelta {
        #[serde(rename = "sessionID")]
        session_id: SessionId,
        #[serde(rename = "messageID")]
        message_id: MessageId,
        #[serde(rename = "partID")]
        part_id: String,
        field: String,
        delta: String,
    },
    #[serde(rename = "message.part.removed")]
    MessagePartRemoved {
        #[serde(rename = "sessionID")]
        session_id: SessionId,
        #[serde(rename = "messageID")]
        message_id: MessageId,
        #[serde(rename = "partID")]
        part_id: String,
    },
    #[serde(rename = "permission.asked")]
    PermissionAsked {
        #[serde(flatten)]
        request: PermissionRequest,
    },
    #[serde(rename = "permission.replied")]
    PermissionReplied {
        #[serde(flatten)]
        reply: PermissionReply,
    },
    #[serde(rename = "question.asked")]
    QuestionAsked {
        #[serde(flatten)]
        request: QuestionRequest,
    },
    #[serde(rename = "question.rejected")]
    QuestionRejected {
        #[serde(rename = "sessionID")]
        session_id: SessionId,
        #[serde(rename = "requestID")]
        request_id: String,
    },
    #[serde(rename = "question.replied")]
    QuestionReplied {
        #[serde(flatten)]
        reply: QuestionReply,
    },
    #[serde(rename = "command.executed")]
    CommandExecuted {
        name: String,
        #[serde(rename = "sessionID")]
        session_id: SessionId,
        arguments: String,
        #[serde(rename = "messageID")]
        message_id: String,
    },
    #[serde(rename = "file.edited")]
    FileEdited { file: String },
    #[serde(rename = "file.watcher.updated")]
    FileWatcherUpdated { file: String, event: String },
    #[serde(rename = "pty.created")]
    PtyCreated { info: Pty },
    #[serde(rename = "pty.updated")]
    PtyUpdated { info: Pty },
    #[serde(rename = "pty.deleted")]
    PtyDeleted { id: String },
    #[serde(rename = "pty.exited")]
    PtyExited {
        id: String,
        #[serde(rename = "exitCode")]
        exit_code: i32,
    },
    #[serde(rename = "lsp.client.diagnostics")]
    LspDiagnostics {
        #[serde(rename = "serverID")]
        server_id: String,
        path: String,
    },
    #[serde(rename = "lsp.updated")]
    LspUpdated {},
    #[serde(rename = "mcp.browser.open.failed")]
    McpBrowserOpenFailed {
        #[serde(rename = "mcpName")]
        mcp_name: String,
        url: String,
    },
    #[serde(rename = "mcp.tools.changed")]
    McpToolsChanged { server: String },
    #[serde(rename = "installation.update-available")]
    InstallationUpdateAvailable { version: String },
    #[serde(rename = "installation.updated")]
    InstallationUpdated { version: String },
    #[serde(rename = "workspace.ready")]
    WorkspaceReady { name: String },
    #[serde(rename = "workspace.failed")]
    WorkspaceFailed { message: String },
    #[serde(rename = "worktree.ready")]
    WorktreeReady { name: String, branch: String },
    #[serde(rename = "worktree.failed")]
    WorktreeFailed { message: String },
    #[serde(rename = "vcs.branch.updated")]
    VcsBranchUpdated { branch: String },
    #[serde(rename = "todo.updated")]
    TodoUpdated {
        #[serde(rename = "sessionID")]
        session_id: SessionId,
        todos: Vec<Todo>,
    },
    #[serde(rename = "tui.prompt.append")]
    TuiPromptAppend { text: String },
    #[serde(rename = "tui.command.execute")]
    TuiCommandExecute { command: String },
    #[serde(rename = "tui.toast.show")]
    TuiToastShow {
        message: String,
        variant: String,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        duration: Option<u64>,
    },
    #[serde(rename = "tui.session.select")]
    TuiSessionSelect {
        #[serde(rename = "sessionID")]
        session_id: SessionId,
    },
    #[serde(rename = "project.updated")]
    ProjectUpdated(Project),
    #[serde(rename = "server.instance.disposed")]
    ServerInstanceDisposed { directory: String },
    #[serde(other)]
    Unknown,
}
