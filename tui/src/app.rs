use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::Cell;
use core::future::Future;
use core::pin::Pin;
use core::task::Poll;

use futures_core::Stream;
use ocpncord_backend::{Backend, BackendEvent, EventEnvelope, EventScope};

use crate::chat::{
    loaded_messages_from_details, render_chat, user_loaded_message, ChatDisplayPolicy, ChatState,
    ChatTranscript, LoadedMessage, PartDisplayMode, PartKind,
};
use crate::command_palette::CommandPaletteModal;
use crate::event::{Event, Scancode};
use crate::key_chord::KeyChord;
use crate::modal::{
    HelpModal, Modal, ModelPickerModal, PermissionModal, QuestionModal, ServerConfigModal,
    SessionListModal,
};
use crate::prompt_bar::{InputMode, PromptBar};
use crate::theme::Theme;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Cell as TableCell, Clear, List, ListItem, ListState, Paragraph, Row,
    Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget, Table, Tabs, Widget, Wrap,
};

/// A toast notification displayed briefly in the top-right corner.
#[derive(Debug, Clone)]
pub struct Toast {
    pub title: Option<String>,
    pub message: String,
    pub variant: ToastVariant,
    pub created_at: u64,
    pub duration: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastVariant {
    Info,
    Success,
    Warning,
    Error,
}

const MAX_STORED_TOASTS: usize = 16;
const MAX_VISIBLE_TOASTS: usize = 5;
const LIVE_RECONNECT_DELAY_TICKS: u64 = 4;
const DEFAULT_SERVER_URL: &str = "http://localhost:4096";
const START_PAGE_LOGO: &str = r#"██████   ██████ ██████  ███    ██ ██████  ██████  ██████  ██████
██    ██ ██      ██   ██ ████   ██ ██      ██    ██ ██   ██ ██   ██
██    ██ ██      ██████  ██ ██  ██ ██      ██    ██ ██████  ██   ██
██    ██ ██      ██      ██  ██ ██ ██      ██    ██ ██   ██ ██   ██
██████   ██████ ██      ██   ████  ██████  ██████  ██   ██ ██████"#;
const START_PAGE_TIP: &str = "Tip: Ctrl+X H for help, Ctrl+X Q to quit";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    None,
    Quit,
    OpenModal(ModalId),
    CloseModal,
    CycleAgent,
    ExecuteCommand(String),
    Interrupt,
    SendMessage,
    OpenPalette,
    ScrollUp,
    ScrollDown,
    ScrollPageUp,
    ScrollPageDown,
    SelectModel(String),
    LoadSession(String),
    DeleteSession(String),
    RenameSession(String, String),
    AbortSession(String),
    OpenTerminal(String),
    CloseTerminal,
    ToggleSidePanel,
    ToggleSidePanelTab(Tab),
    SidePanelSelectTab(Tab),
    SetDisplayMode(PartKind, PartDisplayMode),
    ReplyPermission(String, String, PermissionReplyAction),
    ReplyQuestion(String, String, Vec<Vec<String>>),
    RejectQuestion(String),
    TestServerUrl(String),
    ApplyServerUrl(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    StartPage,
    Chat,
    Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalId {
    SessionList,
    Help,
    ModelPicker,
    CommandPalette,
    ServerConfig,
    DisplayConfig,
    PermissionApproval,
    QuestionApproval,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Diagnostics,
    Todos,
    Pane,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionReplyAction {
    Once,
    Always,
    Reject,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ActiveBlockingPrompt {
    Permission(String),
    Question(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmissionKind {
    Prompt,
    Command,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submission {
    pub kind: SubmissionKind,
    pub session_id: String,
    pub text: String,
    pub execution_text: String,
    pub agent: String,
}

impl Submission {
    fn prompt(session_id: String, text: String, agent: String) -> Self {
        Self {
            kind: SubmissionKind::Prompt,
            session_id,
            execution_text: text.clone(),
            text,
            agent,
        }
    }

    fn command(session_id: String, text: String, agent: String) -> Self {
        let execution_text = text
            .trim_start()
            .strip_prefix('!')
            .unwrap_or(text.as_str())
            .trim_start()
            .to_string();
        Self {
            kind: SubmissionKind::Command,
            session_id,
            execution_text,
            text,
            agent,
        }
    }
}

/// A single LSP diagnostic entry.
#[derive(Debug, Clone)]
pub struct LspDiagnostic {
    pub message: String,
    pub severity: Option<String>,
    pub line: u32,
    pub character: u32,
}
/// PTY terminal pane state (max 2000 lines).
#[derive(Debug, Clone)]
pub struct TerminalPane {
    pub pty_id: Option<String>,
    pub title: String,
    pub command: String,
    pub lines: alloc::collections::VecDeque<TermLine>,
    pub scroll: u16,
    pub status: ocpncord_backend::PtyStatus,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct TermLine {
    pub content: String,
    pub is_error: bool,
}

impl TerminalPane {
    pub fn new() -> Self {
        Self {
            pty_id: None,
            title: String::new(),
            command: String::new(),
            lines: alloc::collections::VecDeque::with_capacity(2000),
            scroll: 0,
            status: ocpncord_backend::PtyStatus::Running,
            exit_code: None,
        }
    }

    pub fn set_from_pty(&mut self, pty: &ocpncord_backend::Pty) {
        self.pty_id = Some(pty.id.clone());
        self.title = pty.title.clone();
        self.command = pty.command.clone();
        self.status = pty.status.clone();
    }
}

impl Default for TerminalPane {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
enum CreateSessionPurpose {
    Send {
        text: String,
        mode: InputMode,
        agent: String,
    },
    NewChat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TuiCommand {
    Models,
    NewSession,
    Sessions,
    Help,
    Todos,
    Diagnostics,
    Pty,
    Server,
    Display,
    Abort,
    Dispose,
    Upgrade,
    Exit,
}

impl TuiCommand {
    fn parse(text: &str) -> Option<Self> {
        match text.trim() {
            "/models" | "/config" => Some(Self::Models),
            "/new" => Some(Self::NewSession),
            "/sessions" => Some(Self::Sessions),
            "/help" => Some(Self::Help),
            "/todos" => Some(Self::Todos),
            "/diagnostics" => Some(Self::Diagnostics),
            "/pty" => Some(Self::Pty),
            "/server" => Some(Self::Server),
            "/display" => Some(Self::Display),
            "/abort" => Some(Self::Abort),
            "/dispose" => Some(Self::Dispose),
            "/upgrade" => Some(Self::Upgrade),
            "/exit" => Some(Self::Exit),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TuiCommandContext {
    clear_prompt: bool,
    toggle_panels: bool,
}

impl TuiCommandContext {
    fn typed() -> Self {
        Self {
            clear_prompt: true,
            toggle_panels: false,
        }
    }

    fn action() -> Self {
        Self {
            clear_prompt: false,
            toggle_panels: true,
        }
    }
}

#[derive(Debug, Clone)]
enum BackendOp {
    LoadAgents,
    Subscribe,
    SyncHistory {
        request: ocpncord_backend::SyncHistoryRequest,
    },
    CreateSession {
        title: String,
        session_directory: String,
        purpose: CreateSessionPurpose,
    },
    Submit {
        submission: Submission,
    },
    ListSessions,
    LoadSession {
        session_id: String,
    },
    DeleteSession {
        session_id: String,
    },
    OpenModelPicker {
        cached_models: Option<Vec<ocpncord_backend::ModelSummary>>,
    },
    TestServerUrl {
        url: String,
    },
    ApplyServerUrl {
        url: String,
    },
    SelectModel {
        model: String,
    },
    ReplyPermission {
        reply: ocpncord_backend::PermissionReply,
        message: String,
    },
    ReplyQuestion {
        reply: ocpncord_backend::QuestionReply,
    },
    RejectQuestion {
        request_id: String,
    },
    AbortSession {
        session_id: String,
    },
    Dispose,
    Upgrade,
    RenameSession {
        session_id: String,
        title: String,
    },
}

enum BackendOpResult<B: Backend> {
    Agents(ocpncord_backend::Result<Vec<ocpncord_backend::Agent>>),
    Subscribe(ocpncord_backend::Result<B::EventStream>),
    SyncHistory(ocpncord_backend::Result<ocpncord_backend::SyncHistoryBatch>),
    CreateSession {
        purpose: CreateSessionPurpose,
        result: ocpncord_backend::Result<ocpncord_backend::Session>,
    },
    Submit {
        submission: Submission,
        result: ocpncord_backend::Result<ocpncord_backend::SubmissionReceipt>,
    },
    ListSessions(ocpncord_backend::Result<Vec<ocpncord_backend::Session>>),
    LoadSession {
        result: ocpncord_backend::Result<(ocpncord_backend::Session, Vec<LoadedMessage>)>,
    },
    DeleteSession(ocpncord_backend::Result<Vec<ocpncord_backend::Session>>),
    OpenModelPicker {
        result: ocpncord_backend::Result<(
            ocpncord_backend::Config,
            Option<Vec<ocpncord_backend::ModelSummary>>,
        )>,
    },
    TestServerUrl {
        result: ocpncord_backend::Result<ocpncord_backend::Health>,
    },
    ApplyServerUrl {
        url: String,
        result: ocpncord_backend::Result<ocpncord_backend::Health>,
    },
    SelectModel {
        requested: String,
        result: ocpncord_backend::Result<ocpncord_backend::Config>,
    },
    PermissionReply {
        message: String,
        result: ocpncord_backend::Result<()>,
    },
    QuestionReply(ocpncord_backend::Result<()>),
    QuestionReject(ocpncord_backend::Result<()>),
    Abort(ocpncord_backend::Result<()>),
    Dispose(ocpncord_backend::Result<()>),
    Upgrade(ocpncord_backend::Result<()>),
    RenameSession(ocpncord_backend::Result<ocpncord_backend::Session>),
}

/// Top-level app state.
pub struct AppState {
    active_mode: AppMode,
    theme: Theme,
    key_chord: KeyChord,
    tick: u64,
    prompt_bar: PromptBar,
    chat_scroll: u16,
    active_session: Option<ocpncord_backend::Session>,
    draft: Option<String>,
    error: Option<String>,
    is_streaming: bool,
    response_indicator_until_tick: u64,
    chat: ChatState,
    active_submission: Option<Submission>,
    queued_submissions: Vec<Submission>,
    sync_known_sequences: BTreeMap<String, u64>,
    live_reconnect_at_tick: Option<u64>,
    pending_ops: alloc::collections::VecDeque<BackendOp>,
    active_modal: Option<Box<dyn Modal>>,
    active_blocking_prompt: Option<ActiveBlockingPrompt>,
    agents: Vec<ocpncord_backend::Agent>,
    active_agent: usize,
    // --- New fields for full API integration ---
    terminal: TerminalPane,
    model_cache: Option<Vec<ocpncord_backend::ModelSummary>>,
    // Permission & Question pending queues
    pending_permissions: alloc::collections::VecDeque<ocpncord_backend::PermissionRequest>,
    pending_questions: alloc::collections::VecDeque<ocpncord_backend::QuestionRequest>,
    // Toast notifications
    toasts: alloc::collections::VecDeque<Toast>,
    // Side panel state
    side_panel_visible: bool,
    side_panel_tab: Tab,
    side_panel_scroll: u16,
    // Track the screen before switching to Terminal for toggle back
    mode_before_terminal: AppMode,
    terminal_view_height: Cell<u16>,
    // LSP diagnostics cache: file_path -> diagnostics
    lsp_diagnostics: alloc::collections::BTreeMap<String, Vec<LspDiagnostic>>,
    // Todo cache
    todos: Vec<ocpncord_backend::Todo>,
    // Optional directory override for creating new sessions.
    session_directory_override: Option<String>,
    current_branch: Option<String>,
    current_server_url: String,
    display_policy: ChatDisplayPolicy,
}

impl AppState {
    pub fn new() -> Self {
        Self::new_with_server_url(DEFAULT_SERVER_URL.into())
    }

    pub fn new_with_server_url(current_server_url: String) -> Self {
        Self {
            active_mode: AppMode::StartPage,
            theme: Theme::default(),
            key_chord: KeyChord::new(),
            tick: 0,
            prompt_bar: PromptBar::new(),
            chat_scroll: 0,
            active_session: None,
            draft: None,
            error: None,
            is_streaming: false,
            response_indicator_until_tick: 0,
            chat: ChatState::new(),
            active_submission: None,
            queued_submissions: Vec::new(),
            sync_known_sequences: BTreeMap::new(),
            live_reconnect_at_tick: None,
            pending_ops: alloc::collections::VecDeque::new(),
            active_modal: None,
            active_blocking_prompt: None,
            agents: Vec::new(),
            active_agent: 0,
            terminal: TerminalPane::new(),
            model_cache: None,
            pending_permissions: alloc::collections::VecDeque::new(),
            pending_questions: alloc::collections::VecDeque::new(),
            toasts: alloc::collections::VecDeque::new(),
            side_panel_visible: false,
            side_panel_tab: Tab::Diagnostics,
            side_panel_scroll: 0,
            mode_before_terminal: AppMode::StartPage,
            terminal_view_height: Cell::new(10),
            lsp_diagnostics: alloc::collections::BTreeMap::new(),
            todos: Vec::new(),
            session_directory_override: None,
            current_branch: None,
            current_server_url,
            display_policy: ChatDisplayPolicy::default(),
        }
    }

    pub fn set_active_modal(&mut self, modal: Box<dyn Modal>) {
        self.active_modal = Some(modal);
        self.active_blocking_prompt = None;
    }

    pub fn active_modal(&self) -> Option<&dyn Modal> {
        self.active_modal.as_deref()
    }

    fn set_blocking_modal(&mut self, modal: Box<dyn Modal>, prompt: ActiveBlockingPrompt) {
        self.active_modal = Some(modal);
        self.active_blocking_prompt = Some(prompt);
    }

    fn clear_active_modal(&mut self) {
        self.active_modal = None;
        self.active_blocking_prompt = None;
    }

    fn push_toast(&mut self, toast: Toast) {
        if self.toasts.len() >= MAX_STORED_TOASTS {
            self.toasts.pop_front();
        }
        self.toasts.push_back(toast);
    }

    fn truncate_toast_text(display: &str, max_width: usize) -> String {
        if max_width == 0 {
            return String::new();
        }

        let chars: Vec<char> = display.chars().collect();
        if chars.len() <= max_width {
            return display.into();
        }

        if max_width <= 3 {
            return chars.into_iter().take(max_width).collect();
        }

        let mut truncated: String = chars.into_iter().take(max_width - 3).collect();
        truncated.push_str("...");
        truncated
    }

    fn render_toasts(&self, frame: &mut ratatui::Frame, area: Rect) {
        if self.active_modal.is_some() || area.width == 0 || area.height <= 2 {
            return;
        }

        let max_width = area.width.saturating_sub(2) as usize;
        if max_width == 0 {
            return;
        }

        let bottom_limit = area.y + area.height.saturating_sub(1);
        let mut toast_y = area.y + 2;

        for toast in self.toasts.iter().rev().take(MAX_VISIBLE_TOASTS) {
            if toast_y >= bottom_limit {
                break;
            }

            let style = match toast.variant {
                ToastVariant::Info => self.theme.toast_info,
                ToastVariant::Success => self.theme.toast_success,
                ToastVariant::Warning => self.theme.toast_warning,
                ToastVariant::Error => self.theme.toast_error,
            };
            let display = if let Some(ref title) = toast.title {
                alloc::format!(" {}: {} ", title, toast.message)
            } else {
                alloc::format!(" {} ", toast.message)
            };
            let display = Self::truncate_toast_text(&display, max_width);
            if display.is_empty() {
                continue;
            }

            let display_width = display.chars().count() as u16;
            if display_width == 0 {
                continue;
            }

            let x = area.x + area.width.saturating_sub(display_width.saturating_add(1));
            Text::from(display)
                .style(style)
                .render(Rect::new(x, toast_y, display_width, 1), frame.buffer_mut());
            toast_y += 1;
        }
    }

    fn queue_op(&mut self, op: BackendOp) {
        self.pending_ops.push_back(op);
    }

    fn active_session_scope(&self) -> EventScope {
        let Some(session) = &self.active_session else {
            return EventScope::default();
        };

        EventScope::instance(
            Some(session.directory.clone()),
            session.workspace_id.clone(),
        )
    }

    fn create_session_directory(&self) -> String {
        self.session_directory_override.clone().unwrap_or_default()
    }

    fn sync_history_request(&self) -> ocpncord_backend::SyncHistoryRequest {
        ocpncord_backend::SyncHistoryRequest {
            scope: self.active_session_scope(),
            known_sequences: self.sync_known_sequences.clone(),
        }
    }

    fn queue_sync_history(&mut self) {
        self.queue_op(BackendOp::SyncHistory {
            request: self.sync_history_request(),
        });
    }

    fn queue_live_resubscribe(&mut self) {
        self.live_reconnect_at_tick = None;
        self.queue_sync_history();
        self.queue_op(BackendOp::Subscribe);
    }

    fn schedule_live_reconnect(&mut self) {
        if self.live_reconnect_at_tick.is_none() {
            self.live_reconnect_at_tick =
                Some(self.tick.saturating_add(LIVE_RECONNECT_DELAY_TICKS));
        }
    }

    fn live_reconnect_due(&self) -> bool {
        self.live_reconnect_at_tick
            .map(|tick| self.tick >= tick)
            .unwrap_or(false)
    }

    fn envelope_matches_scope(&self, envelope: &EventEnvelope) -> bool {
        let wanted = self.active_session_scope();
        if let Some(directory) = wanted.directory.as_deref() {
            if let Some(event_directory) = envelope.scope.directory.as_deref() {
                return event_directory == directory;
            }
        }
        if let Some(workspace) = wanted.workspace.as_deref() {
            if let Some(event_workspace) = envelope.scope.workspace.as_deref() {
                return event_workspace == workspace;
            }
        }
        true
    }

    fn update_known_sequence(&mut self, envelope: &EventEnvelope) {
        let Some(cursor) = &envelope.cursor else {
            return;
        };
        let (Some(aggregate_id), Some(seq)) = (&cursor.aggregate_id, cursor.seq) else {
            return;
        };
        let entry = self
            .sync_known_sequences
            .entry(aggregate_id.clone())
            .or_insert(0);
        *entry = (*entry).max(seq);
    }

    fn apply_live_envelope(&mut self, envelope: EventEnvelope) -> bool {
        if !self.envelope_matches_scope(&envelope) {
            return true;
        }
        self.update_known_sequence(&envelope);
        self.handle_event(Event::Backend(envelope.event))
    }

    fn handle_sync_history_result(
        &mut self,
        result: ocpncord_backend::Result<ocpncord_backend::SyncHistoryBatch>,
    ) {
        match result {
            Ok(batch) => {
                for (aggregate_id, seq) in batch.known_sequences {
                    let entry = self.sync_known_sequences.entry(aggregate_id).or_insert(0);
                    *entry = (*entry).max(seq);
                }
                for envelope in batch.envelopes {
                    self.apply_live_envelope(envelope);
                }
            }
            Err(error) => {
                self.error = Some(alloc::format!("sync history: {error}"));
            }
        }
    }

    fn queue_startup(&mut self) {
        self.queue_op(BackendOp::LoadAgents);
        self.queue_live_resubscribe();
    }

    fn reset_for_server_switch(&mut self, url: String) {
        self.current_server_url = url;
        self.active_mode = AppMode::StartPage;
        self.prompt_bar.clear();
        self.chat_scroll = 0;
        self.active_session = None;
        self.draft = None;
        self.error = None;
        self.is_streaming = false;
        self.response_indicator_until_tick = 0;
        self.chat = ChatState::new();
        self.active_submission = None;
        self.queued_submissions.clear();
        self.sync_known_sequences.clear();
        self.live_reconnect_at_tick = None;
        self.pending_ops.clear();
        self.active_blocking_prompt = None;
        self.pending_permissions.clear();
        self.pending_questions.clear();
        self.agents.clear();
        self.active_agent = 0;
        self.model_cache = None;
        self.side_panel_visible = false;
        self.side_panel_scroll = 0;
        self.queue_startup();
    }

    pub fn active_mode(&self) -> AppMode {
        self.active_mode
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    pub fn draft(&self) -> Option<&str> {
        self.draft.as_deref()
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn active_session(&self) -> Option<&ocpncord_backend::Session> {
        self.active_session.as_ref()
    }

    pub fn set_session_directory_override(&mut self, session_directory: String) {
        self.session_directory_override = Some(session_directory);
    }

    pub fn prompt_text(&self) -> &str {
        self.prompt_bar.text()
    }

    pub fn active_agent_name(&self) -> &str {
        self.agents
            .get(self.active_agent)
            .map(|a| a.name.as_str())
            .unwrap_or("build")
    }

    fn active_agent_status(&self) -> (String, &'static str, String) {
        let Some(agent) = self.agents.get(self.active_agent) else {
            return ("build".into(), "primary", "default".into());
        };
        let mode = match agent.mode {
            ocpncord_backend::AgentMode::Primary => "primary",
            ocpncord_backend::AgentMode::Subagent => "subagent",
            ocpncord_backend::AgentMode::All => "all",
        };
        let model = agent
            .model
            .as_ref()
            .map(|model| alloc::format!("{}/{}", model.provider_id, model.model_id))
            .unwrap_or_else(|| "default".into());
        (agent.name.clone(), mode, model)
    }

    pub fn cycle_agent(&mut self) {
        if !self.agents.is_empty() {
            self.active_agent = (self.active_agent + 1) % self.agents.len();
        }
    }

    pub fn cycle_agent_back(&mut self) {
        if !self.agents.is_empty() {
            self.active_agent = self
                .active_agent
                .checked_sub(1)
                .unwrap_or(self.agents.len() - 1);
        }
    }

    fn apply_agent_result(
        &mut self,
        result: ocpncord_backend::Result<Vec<ocpncord_backend::Agent>>,
    ) {
        if let Ok(agents) = result {
            self.agents = agents
                .into_iter()
                .filter(|a| matches!(a.mode, ocpncord_backend::AgentMode::Primary))
                .collect();
        }
        if self.agents.is_empty() {
            self.agents = vec![
                ocpncord_backend::Agent {
                    name: "build".into(),
                    mode: ocpncord_backend::AgentMode::Primary,
                    description: None,
                    native: None,
                    hidden: None,
                    model: None,
                    color: None,
                    variant: None,
                    prompt: None,
                    steps: None,
                },
                ocpncord_backend::Agent {
                    name: "plan".into(),
                    mode: ocpncord_backend::AgentMode::Primary,
                    description: None,
                    native: None,
                    hidden: None,
                    model: None,
                    color: None,
                    variant: None,
                    prompt: None,
                    steps: None,
                },
            ];
        }
        self.active_agent = 0;
    }

    pub fn is_streaming(&self) -> bool {
        self.is_streaming
    }

    pub fn partial_parts(&self) -> &[ocpncord_backend::Part] {
        self.chat.partial_parts()
    }

    #[cfg(test)]
    pub(crate) fn messages(&self) -> &[LoadedMessage] {
        self.chat.messages()
    }

    pub fn active_submission(&self) -> Option<&Submission> {
        self.active_submission.as_ref()
    }

    pub fn queued_submissions(&self) -> &[Submission] {
        &self.queued_submissions
    }

    pub fn tick(&self) -> u64 {
        self.tick
    }

    fn mark_response_active(&mut self) {
        self.response_indicator_until_tick = self.tick.saturating_add(80);
    }

    fn should_show_response_indicator(&self) -> bool {
        self.is_streaming || self.response_indicator_until_tick > self.tick
    }

    fn queue_or_dispatch_submission(&mut self, submission: Submission, message: LoadedMessage) {
        if self.is_streaming || self.active_submission.is_some() {
            self.queued_submissions.push(submission);
            self.chat.queue_message(message);
            return;
        }

        self.start_submission(submission, message);
    }

    fn take_next_queued_submission(&mut self) -> Option<(Submission, LoadedMessage)> {
        if self.queued_submissions.is_empty() {
            return None;
        }

        let Some(message) = self.chat.pop_queued_message() else {
            return None;
        };

        if self.chat.has_partial_response() {
            self.chat.flush_partial_response(self.active_session_id());
        }

        Some((self.queued_submissions.remove(0), message))
    }

    fn start_submission(&mut self, submission: Submission, message: LoadedMessage) {
        self.chat.push_message(message);
        self.draft = Some(submission.text.clone());
        self.active_mode = AppMode::Chat;
        self.is_streaming = true;
        self.mark_response_active();
        self.chat.clear_partial_stream();
        self.queue_op(BackendOp::Submit { submission });
    }

    fn dispatch_next_queued_submission(&mut self) {
        let Some((submission, message)) = self.take_next_queued_submission() else {
            return;
        };

        self.start_submission(submission, message);
    }

    fn active_session_id(&self) -> Option<String> {
        self.active_session
            .as_ref()
            .map(|session| session.id.clone())
    }

    fn active_session_matches(&self, session_id: &str) -> bool {
        self.active_session.as_ref().map(|s| s.id.as_str()) == Some(session_id)
    }
    fn finish_streaming_response(&mut self) {
        let was_streaming = self.is_streaming;
        self.is_streaming = false;
        self.active_submission = None;

        if was_streaming {
            self.dispatch_next_queued_submission();
        }
    }

    fn apply_message_updated(&mut self, session_id: String, message: ocpncord_backend::Message) {
        if !self.active_session_matches(&session_id) {
            return;
        }

        if self.chat.apply_message_updated(message) {
            self.finish_streaming_response();
        }
    }

    fn apply_message_removed(&mut self, session_id: String, message_id: String) {
        if !self.active_session_matches(&session_id) {
            return;
        }
        self.chat.remove_message(&message_id);
    }

    fn open_permission_modal_if_idle(&mut self) -> bool {
        if self.active_modal.is_some() {
            return false;
        }

        let Some(request) = self.pending_permissions.front().cloned() else {
            return false;
        };

        let request_id = request.id.clone();
        self.set_blocking_modal(
            Box::new(PermissionModal::new(request)),
            ActiveBlockingPrompt::Permission(request_id),
        );
        true
    }

    fn open_question_modal_if_idle(&mut self) -> bool {
        if self.active_modal.is_some() {
            return false;
        }

        let Some(request) = self.pending_questions.front().cloned() else {
            return false;
        };

        let request_id = request.id.clone();
        self.set_blocking_modal(
            Box::new(QuestionModal::new(request)),
            ActiveBlockingPrompt::Question(request_id),
        );
        true
    }

    fn open_next_blocking_modal_if_idle(&mut self) -> bool {
        self.open_permission_modal_if_idle() || self.open_question_modal_if_idle()
    }

    fn remove_pending_permission_by_id(&mut self, request_id: &str) -> bool {
        let Some(index) = self
            .pending_permissions
            .iter()
            .position(|request| request.id == request_id)
        else {
            return false;
        };

        self.pending_permissions.remove(index);
        true
    }

    fn remove_pending_question_by_id(&mut self, request_id: &str) -> bool {
        let Some(index) = self
            .pending_questions
            .iter()
            .position(|request| request.id == request_id)
        else {
            return false;
        };

        self.pending_questions.remove(index);
        true
    }

    fn close_permission_modal_if_matches(&mut self, request_id: &str) {
        if matches!(
            self.active_blocking_prompt.as_ref(),
            Some(ActiveBlockingPrompt::Permission(active_id)) if active_id == request_id
        ) {
            self.clear_active_modal();
        }
    }

    fn close_question_modal_if_matches(&mut self, request_id: &str) {
        if matches!(
            self.active_blocking_prompt.as_ref(),
            Some(ActiveBlockingPrompt::Question(active_id)) if active_id == request_id
        ) {
            self.clear_active_modal();
        }
    }

    /// Returns `false` when the application should quit.
    pub fn handle_event(&mut self, event: Event) -> bool {
        match event {
            Event::Key(ref key) => {
                self.error = None;

                if let Some(ref mut modal) = self.active_modal {
                    let action = modal.handle_event(Event::Key(key.clone()));
                    match action {
                        Action::CloseModal => {
                            self.clear_active_modal();
                            self.open_next_blocking_modal_if_idle();
                        }
                        Action::None
                            if key.scancode == Scancode::Escape
                                && self.active_blocking_prompt.is_none() =>
                        {
                            self.clear_active_modal();
                            self.open_next_blocking_modal_if_idle();
                        }
                        Action::None => {}
                        other => {
                            return self.apply_action(Some(other));
                        }
                    }
                    return true;
                }

                if self.is_streaming {
                    if key.scancode == Scancode::Escape {
                        return self.handle_interrupt();
                    }
                    if key.scancode == Scancode::Char('c') && key.modifiers.ctrl {
                        return self.handle_interrupt();
                    }
                }

                if let Some(action) = self.key_chord.handle(key, self.tick) {
                    return self.apply_action(Some(action));
                }

                // KeyChord consumed the event (leader mode entered).
                // Prevent further processing (e.g. prompt_bar inserting a
                // literal 'x' into the input when Ctrl+X was pressed).
                if self.key_chord.is_leader_active() {
                    return true;
                }

                if key.scancode == crate::event::Scancode::Tab {
                    if key.modifiers.shift {
                        self.cycle_agent_back();
                    } else {
                        self.cycle_agent();
                    }
                    return true;
                }

                if key.scancode == Scancode::Escape {
                    if !self.prompt_bar.is_empty() {
                        self.prompt_bar.clear();
                    }
                    return true;
                }

                if self.active_mode == AppMode::Terminal {
                    let action = match key.scancode {
                        Scancode::Up => Some(Action::ScrollUp),
                        Scancode::Down => Some(Action::ScrollDown),
                        Scancode::PageUp => Some(Action::ScrollPageUp),
                        Scancode::PageDown => Some(Action::ScrollPageDown),
                        _ => None,
                    };
                    if action.is_some() {
                        return self.apply_action(action);
                    }
                    return true;
                }

                if self.active_mode == AppMode::Chat
                    || (self.side_panel_visible && self.side_panel_tab == Tab::Pane)
                {
                    let action = match key.scancode {
                        Scancode::Up => Some(Action::ScrollUp),
                        Scancode::Down => Some(Action::ScrollDown),
                        Scancode::PageUp => Some(Action::ScrollPageUp),
                        Scancode::PageDown => Some(Action::ScrollPageDown),
                        _ => None,
                    };
                    if action.is_some() {
                        return self.apply_action(action);
                    }
                }

                if let Some(action) = self.prompt_bar.handle_key(key) {
                    match action {
                        Action::SendMessage => {
                            return self.handle_send_message();
                        }
                        _ => {}
                    }
                }
            }
            Event::Backend(event) =>
            {
                #[allow(unreachable_patterns)]
                match event {
                    ocpncord_backend::BackendEvent::Error { message } => {
                        self.error = Some(message);
                        self.chat.clear_partial_stream();
                        self.is_streaming = false;
                        self.response_indicator_until_tick = 0;
                        self.active_submission = None;
                        self.dispatch_next_queued_submission();
                    }
                    ocpncord_backend::BackendEvent::SessionCreated { session } => {
                        let is_new = self
                            .active_session
                            .as_ref()
                            .map(|s| s.id != session.id)
                            .unwrap_or(true);
                        self.active_session = Some(session);
                        self.active_mode = AppMode::Chat;
                        if is_new {
                            self.chat.clear_messages();
                        }
                        self.error = None;
                    }
                    ocpncord_backend::BackendEvent::SessionUpdated { session } => {
                        if let Some(active) = &mut self.active_session {
                            if active.id == session.id {
                                *active = session;
                            }
                        }
                    }
                    ocpncord_backend::BackendEvent::SessionDeleted { .. } => {
                        self.active_session = None;
                        self.chat.clear_messages();
                        self.active_mode = AppMode::StartPage;
                    }
                    ocpncord_backend::BackendEvent::SessionIdle { .. } => {}
                    ocpncord_backend::BackendEvent::SessionError { error, .. } => {
                        self.error = Some(alloc::format!("Session error: {:?}", error));
                    }
                    ocpncord_backend::BackendEvent::SessionDiff { .. } => {}
                    ocpncord_backend::BackendEvent::SessionCompacted { .. } => {}
                    ocpncord_backend::BackendEvent::MessageUpdated {
                        session_id,
                        message,
                    } => {
                        self.apply_message_updated(session_id, message);
                    }
                    ocpncord_backend::BackendEvent::MessageRemoved {
                        session_id,
                        message_id,
                    } => self.apply_message_removed(session_id, message_id),
                    ocpncord_backend::BackendEvent::MessagePartUpdated {
                        session_id,
                        part_id,
                        ref part,
                        ..
                    } => {
                        if self.active_session.as_ref().map(|s| s.id.as_str())
                            == Some(session_id.as_str())
                        {
                            self.mark_response_active();
                            self.chat.merge_stream_part(Some(part_id), part.clone());
                        }
                    }
                    ocpncord_backend::BackendEvent::MessagePartDelta {
                        ref session_id,
                        part_id,
                        field,
                        delta,
                        ..
                    } if field == "text"
                        && self.active_session.as_ref().map(|s| s.id.as_str())
                            == Some(session_id.as_str()) =>
                    {
                        self.mark_response_active();
                        self.chat.merge_stream_delta(part_id, delta);
                    }
                    ocpncord_backend::BackendEvent::MessagePartDelta { .. } => {}
                    ocpncord_backend::BackendEvent::MessagePartRemoved {
                        session_id,
                        part_id,
                        ..
                    } => {
                        if self.active_session_matches(&session_id) {
                            self.chat.remove_stream_part(&part_id);
                        }
                    }
                    ocpncord_backend::BackendEvent::PermissionAsked { request } => {
                        self.pending_permissions.push_back(request);
                        self.open_next_blocking_modal_if_idle();
                    }
                    ocpncord_backend::BackendEvent::PermissionReplied { request_id, .. } => {
                        self.remove_pending_permission_by_id(&request_id);
                        self.close_permission_modal_if_matches(&request_id);
                        self.open_next_blocking_modal_if_idle();
                    }
                    ocpncord_backend::BackendEvent::QuestionAsked { request } => {
                        self.pending_questions.push_back(request);
                        self.open_next_blocking_modal_if_idle();
                    }
                    ocpncord_backend::BackendEvent::QuestionRejected { request_id, .. } => {
                        self.remove_pending_question_by_id(&request_id);
                        self.close_question_modal_if_matches(&request_id);
                        self.open_next_blocking_modal_if_idle();
                    }
                    ocpncord_backend::BackendEvent::QuestionReplied { request_id, .. } => {
                        self.remove_pending_question_by_id(&request_id);
                        self.close_question_modal_if_matches(&request_id);
                        self.open_next_blocking_modal_if_idle();
                    }
                    ocpncord_backend::BackendEvent::CommandExecuted {
                        name, arguments, ..
                    } => {
                        self.push_toast(Toast {
                            title: Some("Command".into()),
                            message: alloc::format!("{name} {arguments}"),
                            variant: ToastVariant::Info,
                            created_at: self.tick,
                            duration: 12,
                        });
                    }
                    ocpncord_backend::BackendEvent::FileEdited { file } => {
                        self.push_toast(Toast {
                            title: Some("File Edited".into()),
                            message: file,
                            variant: ToastVariant::Info,
                            created_at: self.tick,
                            duration: 8,
                        });
                    }
                    ocpncord_backend::BackendEvent::FileWatcherUpdated { file, event } => {
                        self.push_toast(Toast {
                            title: Some("File Watcher".into()),
                            message: alloc::format!("{file}: {event}"),
                            variant: ToastVariant::Info,
                            created_at: self.tick,
                            duration: 6,
                        });
                    }
                    ocpncord_backend::BackendEvent::PtyCreated { info } => {
                        self.terminal.set_from_pty(&info);
                        self.side_panel_tab = Tab::Pane;
                        self.active_mode = AppMode::Terminal;
                    }
                    ocpncord_backend::BackendEvent::PtyUpdated { .. } => {}
                    ocpncord_backend::BackendEvent::PtyDeleted { .. } => {
                        self.active_mode = AppMode::Chat;
                    }
                    ocpncord_backend::BackendEvent::PtyExited { exit_code, .. } => {
                        self.terminal.status = ocpncord_backend::PtyStatus::Exited;
                        self.terminal.exit_code = Some(exit_code);
                        self.push_toast(Toast {
                            title: Some("Terminal".into()),
                            message: alloc::format!("exit code: {exit_code}"),
                            variant: if exit_code == 0 {
                                ToastVariant::Success
                            } else {
                                ToastVariant::Error
                            },
                            created_at: self.tick,
                            duration: 8,
                        });
                    }
                    ocpncord_backend::BackendEvent::LspDiagnostics { path: _, .. } => {
                        self.side_panel_tab = Tab::Diagnostics;
                        self.side_panel_visible = true;
                    }
                    ocpncord_backend::BackendEvent::LspUpdated => {}
                    ocpncord_backend::BackendEvent::McpBrowserOpenFailed { mcp_name, url } => {
                        self.push_toast(Toast {
                            title: Some("MCP Error".into()),
                            message: alloc::format!("{mcp_name}: {url}"),
                            variant: ToastVariant::Error,
                            created_at: self.tick,
                            duration: 10,
                        });
                    }
                    ocpncord_backend::BackendEvent::McpToolsChanged { server } => {
                        self.push_toast(Toast {
                            title: Some("MCP Tools".into()),
                            message: alloc::format!("Updated from {server}"),
                            variant: ToastVariant::Info,
                            created_at: self.tick,
                            duration: 6,
                        });
                    }
                    ocpncord_backend::BackendEvent::InstallationUpdateAvailable { version } => {
                        self.push_toast(Toast {
                            title: Some("Update Available".into()),
                            message: alloc::format!("version {version}"),
                            variant: ToastVariant::Info,
                            created_at: self.tick,
                            duration: 12,
                        });
                    }
                    ocpncord_backend::BackendEvent::InstallationUpdated { version } => {
                        self.push_toast(Toast {
                            title: Some("Updated".into()),
                            message: alloc::format!("now on {version}"),
                            variant: ToastVariant::Success,
                            created_at: self.tick,
                            duration: 6,
                        });
                    }
                    ocpncord_backend::BackendEvent::WorkspaceReady { .. } => {}
                    ocpncord_backend::BackendEvent::WorkspaceFailed { message } => {
                        self.error = Some(alloc::format!("Workspace error: {message}"));
                    }
                    ocpncord_backend::BackendEvent::WorktreeReady { branch, .. } => {
                        self.current_branch = Some(branch);
                    }
                    ocpncord_backend::BackendEvent::WorktreeFailed { message } => {
                        self.error = Some(alloc::format!("Worktree error: {message}"));
                    }
                    ocpncord_backend::BackendEvent::VcsBranchUpdated { branch } => {
                        self.current_branch = Some(branch);
                    }
                    ocpncord_backend::BackendEvent::TodoUpdated { ref todos, .. } => {
                        self.todos = todos.clone();
                    }
                    ocpncord_backend::BackendEvent::TuiPromptAppend { ref text } => {
                        self.prompt_bar.append_text(text);
                    }
                    ocpncord_backend::BackendEvent::TuiCommandExecute { ref command } => {
                        if let Some(should_continue) =
                            self.handle_tui_command_text(command, TuiCommandContext::typed())
                        {
                            return should_continue;
                        }
                    }
                    ocpncord_backend::BackendEvent::TuiToastShow {
                        message,
                        variant,
                        title,
                        duration,
                    } => {
                        self.push_toast(Toast {
                            title,
                            message,
                            variant: match variant.as_str() {
                                "success" => ToastVariant::Success,
                                "warning" => ToastVariant::Warning,
                                "error" => ToastVariant::Error,
                                _ => ToastVariant::Info,
                            },
                            created_at: self.tick,
                            duration: duration.unwrap_or(6) as u64,
                        });
                    }
                    ocpncord_backend::BackendEvent::TuiSessionSelect { ref session_id } => {
                        self.queue_op(BackendOp::LoadSession {
                            session_id: session_id.clone(),
                        });
                    }
                    ocpncord_backend::BackendEvent::ServerConnected => {
                        self.push_toast(Toast {
                            title: Some("Server".into()),
                            message: "Connected".into(),
                            variant: ToastVariant::Success,
                            created_at: self.tick,
                            duration: 4,
                        });
                    }
                    ocpncord_backend::BackendEvent::GlobalDisposed => {
                        self.error = Some("Server disposed all instances".into());
                    }
                    ocpncord_backend::BackendEvent::ServerInstanceDisposed { directory } => {
                        self.push_toast(Toast {
                            title: Some("Instance Disposed".into()),
                            message: directory,
                            variant: ToastVariant::Warning,
                            created_at: self.tick,
                            duration: 8,
                        });
                    }
                    ocpncord_backend::BackendEvent::ProjectUpdated(_) => {}
                    _ => {}
                }
            }
            Event::Tick => {
                self.tick = self.tick.wrapping_add(1);
                let tick = self.tick;
                self.toasts
                    .retain(|toast| tick.saturating_sub(toast.created_at) <= toast.duration);
                while self.toasts.len() > MAX_STORED_TOASTS {
                    self.toasts.pop_front();
                }
                if let Some(action) = self.key_chord.tick(self.tick) {
                    return self.apply_action(Some(action));
                }
                if self.live_reconnect_due() {
                    self.queue_live_resubscribe();
                }
            }
            Event::Quit => return false,
        }
        true
    }

    fn handle_send_message(&mut self) -> bool {
        let text = self.prompt_bar.text().to_string();
        let mode = self.prompt_bar.input_mode();

        // Slash command routing
        if matches!(mode, InputMode::Command) {
            return self.handle_slash_command(&text);
        }

        let agent = self.active_agent_name().to_string();
        if let Some(session_id) = self.active_session_id() {
            self.prompt_bar.clear();
            let message = user_loaded_message(&text);
            let submission = match mode {
                InputMode::Shell => Submission::command(session_id, text.clone(), agent),
                _ => Submission::prompt(session_id, text.clone(), agent),
            };
            self.queue_or_dispatch_submission(submission, message);
        } else {
            self.queue_op(BackendOp::CreateSession {
                title: "Chat".into(),
                session_directory: self.create_session_directory(),
                purpose: CreateSessionPurpose::Send { text, mode, agent },
            });
        }

        true
    }

    fn handle_slash_command(&mut self, text: &str) -> bool {
        if let Some(should_continue) =
            self.handle_tui_command_text(text, TuiCommandContext::typed())
        {
            should_continue
        } else {
            self.handle_unknown_slash_command(text)
        }
    }

    fn handle_tui_command_text(&mut self, text: &str, context: TuiCommandContext) -> Option<bool> {
        TuiCommand::parse(text).map(|command| self.execute_tui_command(command, context))
    }

    fn execute_tui_command(&mut self, command: TuiCommand, context: TuiCommandContext) -> bool {
        match command {
            TuiCommand::Models => {
                if context.clear_prompt {
                    self.prompt_bar.clear();
                }
                self.open_model_picker();
                true
            }
            TuiCommand::NewSession => {
                if context.clear_prompt {
                    self.prompt_bar.clear();
                }
                self.clear_active_modal();
                self.queue_op(BackendOp::CreateSession {
                    title: "Chat".into(),
                    session_directory: self.create_session_directory(),
                    purpose: CreateSessionPurpose::NewChat,
                });
                true
            }
            TuiCommand::Sessions => {
                if context.clear_prompt {
                    self.prompt_bar.clear();
                }
                self.set_active_modal(Box::new(SessionListModal::new()));
                self.queue_op(BackendOp::ListSessions);
                true
            }
            TuiCommand::Help => {
                if context.clear_prompt {
                    self.prompt_bar.clear();
                }
                self.set_active_modal(Box::new(HelpModal::new()));
                true
            }
            TuiCommand::Todos => {
                if context.clear_prompt {
                    self.prompt_bar.clear();
                }
                self.clear_active_modal();
                if context.toggle_panels
                    && self.side_panel_visible
                    && self.side_panel_tab == Tab::Todos
                {
                    self.side_panel_visible = false;
                } else {
                    self.side_panel_visible = true;
                    self.side_panel_tab = Tab::Todos;
                    self.side_panel_scroll = 0;
                }
                true
            }
            TuiCommand::Diagnostics => {
                if context.clear_prompt {
                    self.prompt_bar.clear();
                }
                self.clear_active_modal();
                if context.toggle_panels
                    && self.side_panel_visible
                    && self.side_panel_tab == Tab::Diagnostics
                {
                    self.side_panel_visible = false;
                } else {
                    self.side_panel_visible = true;
                    self.side_panel_tab = Tab::Diagnostics;
                    self.side_panel_scroll = 0;
                }
                true
            }
            TuiCommand::Pty => {
                if context.clear_prompt {
                    self.prompt_bar.clear();
                }
                self.clear_active_modal();
                if context.toggle_panels
                    && self.side_panel_visible
                    && self.side_panel_tab == Tab::Pane
                {
                    self.side_panel_visible = false;
                } else {
                    self.side_panel_visible = true;
                    self.side_panel_tab = Tab::Pane;
                    self.side_panel_scroll = 0;
                }
                true
            }
            TuiCommand::Server => {
                if context.clear_prompt {
                    self.prompt_bar.clear();
                }
                self.set_active_modal(Box::new(ServerConfigModal::new(
                    self.current_server_url.clone(),
                )));
                true
            }
            TuiCommand::Display => {
                if context.clear_prompt {
                    self.prompt_bar.clear();
                }
                self.set_active_modal(Box::new(crate::modal::DisplayConfigModal::new(
                    self.display_policy,
                )));
                true
            }
            TuiCommand::Abort => {
                if let Some(session) = &self.active_session {
                    self.queue_op(BackendOp::AbortSession {
                        session_id: session.id.clone(),
                    });
                }
                true
            }
            TuiCommand::Dispose => {
                self.queue_op(BackendOp::Dispose);
                true
            }
            TuiCommand::Upgrade => {
                self.queue_op(BackendOp::Upgrade);
                true
            }
            TuiCommand::Exit => false,
        }
    }

    fn handle_unknown_slash_command(&mut self, text: &str) -> bool {
        let agent = self.active_agent_name().to_string();
        if let Some(session_id) = self.active_session_id() {
            self.prompt_bar.clear();
            let message = user_loaded_message(text);
            let submission = Submission::prompt(session_id, text.into(), agent);
            self.queue_or_dispatch_submission(submission, message);
        } else {
            self.queue_op(BackendOp::CreateSession {
                title: "Chat".into(),
                session_directory: self.create_session_directory(),
                purpose: CreateSessionPurpose::Send {
                    text: text.into(),
                    mode: InputMode::Normal,
                    agent,
                },
            });
        }

        true
    }

    fn handle_interrupt(&mut self) -> bool {
        if let Some(session) = &self.active_session {
            self.queue_op(BackendOp::AbortSession {
                session_id: session.id.clone(),
            });
        }
        self.is_streaming = false;
        self.response_indicator_until_tick = 0;
        self.chat.clear_partial_stream();
        self.active_submission = None;
        self.queued_submissions.clear();
        self.chat.clear_queued_messages();
        true
    }

    fn open_model_picker(&mut self) {
        self.active_modal = Some(Box::new(ModelPickerModal::new()));
        self.queue_op(BackendOp::OpenModelPicker {
            cached_models: self.model_cache.clone(),
        });
    }

    fn apply_action(&mut self, action: Option<Action>) -> bool {
        match action {
            Some(Action::Quit) => return false,
            Some(Action::CycleAgent) => self.cycle_agent(),
            Some(Action::ExecuteCommand(ref command)) => {
                if let Some(should_continue) =
                    self.handle_tui_command_text(command, TuiCommandContext::action())
                {
                    return should_continue;
                }
            }
            Some(Action::CloseModal) => {
                self.clear_active_modal();
                self.open_next_blocking_modal_if_idle();
            }
            Some(Action::OpenPalette) => {
                let modal = CommandPaletteModal::new(crate::command_palette::default_commands());
                self.set_active_modal(Box::new(modal));
            }
            Some(Action::OpenModal(ModalId::SessionList)) => {
                self.set_active_modal(Box::new(SessionListModal::new()));
                self.queue_op(BackendOp::ListSessions);
            }
            Some(Action::OpenModal(ModalId::ModelPicker)) => {
                self.open_model_picker();
            }
            Some(Action::OpenModal(ModalId::Help)) => {
                self.set_active_modal(Box::new(HelpModal::new()));
            }
            Some(Action::OpenModal(ModalId::ServerConfig)) => {
                self.set_active_modal(Box::new(ServerConfigModal::new(
                    self.current_server_url.clone(),
                )));
            }
            Some(Action::OpenModal(ModalId::DisplayConfig)) => {
                self.set_active_modal(Box::new(crate::modal::DisplayConfigModal::new(
                    self.display_policy,
                )));
            }
            Some(Action::OpenModal(ModalId::PermissionApproval)) => {
                self.open_permission_modal_if_idle();
            }
            Some(Action::OpenModal(ModalId::QuestionApproval)) => {
                self.open_question_modal_if_idle();
            }
            Some(Action::OpenModal(_)) => {}
            Some(Action::LoadSession(ref id)) => {
                self.queue_op(BackendOp::LoadSession {
                    session_id: id.clone(),
                });
            }
            Some(Action::DeleteSession(ref id)) => {
                self.set_active_modal(Box::new(SessionListModal::new()));
                self.queue_op(BackendOp::DeleteSession {
                    session_id: id.clone(),
                });
            }
            Some(Action::Interrupt) => {
                return self.handle_interrupt();
            }
            Some(Action::ScrollUp) => {
                if self.scroll_targets_terminal() {
                    self.scroll_terminal_up(1);
                } else {
                    self.chat_scroll = self.chat_scroll.saturating_add(1);
                }
            }
            Some(Action::ScrollDown) => {
                if self.scroll_targets_terminal() {
                    self.scroll_terminal_down(1);
                } else {
                    self.chat_scroll = self.chat_scroll.saturating_sub(1);
                }
            }
            Some(Action::ScrollPageUp) => {
                let amount = self.page_scroll_amount();
                if self.scroll_targets_terminal() {
                    self.scroll_terminal_up(amount);
                } else {
                    self.chat_scroll = self.chat_scroll.saturating_add(amount);
                }
            }
            Some(Action::ScrollPageDown) => {
                let amount = self.page_scroll_amount();
                if self.scroll_targets_terminal() {
                    self.scroll_terminal_down(amount);
                } else {
                    self.chat_scroll = self.chat_scroll.saturating_sub(amount);
                }
            }
            Some(Action::ToggleSidePanel) => {
                self.side_panel_visible = !self.side_panel_visible;
            }
            Some(Action::ToggleSidePanelTab(tab)) => {
                if self.side_panel_visible && self.side_panel_tab == tab {
                    self.side_panel_visible = false;
                } else {
                    self.side_panel_tab = tab;
                    self.side_panel_visible = true;
                    self.side_panel_scroll = 0;
                }
            }
            Some(Action::SidePanelSelectTab(tab)) => {
                self.side_panel_tab = tab;
                self.side_panel_scroll = 0;
            }
            Some(Action::SetDisplayMode(kind, mode)) => {
                self.display_policy.set_mode(kind, mode);
                if let Some(modal) = self
                    .active_modal
                    .as_deref_mut()
                    .and_then(|modal| modal.as_display_config_mut())
                {
                    modal.set_policy(self.display_policy);
                }
            }
            Some(Action::OpenTerminal(_pty_id)) => {
                if self.active_mode == AppMode::Terminal {
                    self.active_mode = self.mode_before_terminal;
                } else {
                    self.mode_before_terminal = self.active_mode;
                    self.active_mode = AppMode::Terminal;
                }
            }
            Some(Action::CloseTerminal) => {
                self.active_mode = AppMode::Chat;
            }
            Some(Action::ReplyPermission(ref session_id, ref request_id, reply_action)) => {
                let (reply_value, message) = match reply_action {
                    PermissionReplyAction::Once => ("once", "Permission allowed once"),
                    PermissionReplyAction::Always => ("always", "Permission allowed always"),
                    PermissionReplyAction::Reject => ("reject", "Permission denied"),
                };
                let reply = ocpncord_backend::PermissionReply {
                    session_id: session_id.clone(),
                    request_id: request_id.clone(),
                    reply: reply_value.into(),
                };
                self.remove_pending_permission_by_id(request_id);
                self.clear_active_modal();
                self.open_next_blocking_modal_if_idle();
                self.queue_op(BackendOp::ReplyPermission {
                    reply,
                    message: message.into(),
                });
            }
            Some(Action::ReplyQuestion(ref session_id, ref request_id, ref answers)) => {
                let reply = ocpncord_backend::QuestionReply {
                    session_id: session_id.clone(),
                    request_id: request_id.clone(),
                    answers: answers.clone(),
                };
                self.remove_pending_question_by_id(request_id);
                self.clear_active_modal();
                self.open_next_blocking_modal_if_idle();
                self.queue_op(BackendOp::ReplyQuestion { reply });
            }
            Some(Action::RejectQuestion(ref request_id)) => {
                self.remove_pending_question_by_id(request_id);
                self.clear_active_modal();
                self.open_next_blocking_modal_if_idle();
                self.queue_op(BackendOp::RejectQuestion {
                    request_id: request_id.clone(),
                });
            }
            Some(Action::SelectModel(ref model)) => {
                self.queue_op(BackendOp::SelectModel {
                    model: model.clone(),
                });
            }
            Some(Action::AbortSession(ref id)) => {
                self.queue_op(BackendOp::AbortSession {
                    session_id: id.clone(),
                });
            }
            Some(Action::RenameSession(ref id, ref title)) => {
                self.queue_op(BackendOp::RenameSession {
                    session_id: id.clone(),
                    title: title.clone(),
                });
            }
            Some(Action::TestServerUrl(ref url)) => {
                self.queue_op(BackendOp::TestServerUrl { url: url.clone() });
            }
            Some(Action::ApplyServerUrl(ref url)) => {
                self.queue_op(BackendOp::ApplyServerUrl { url: url.clone() });
            }
            _ => {}
        }
        true
    }

    fn handle_submit_result<B: Backend>(
        &mut self,
        submission: Submission,
        result: ocpncord_backend::Result<ocpncord_backend::SubmissionReceipt>,
    ) {
        match result {
            Ok(_) => {
                self.active_submission = Some(submission);
            }
            Err(e) => {
                self.error = Some(alloc::format!("{}", e));
                self.is_streaming = false;
                self.response_indicator_until_tick = 0;
                self.chat.clear_partial_stream();
                self.active_submission = None;
                self.dispatch_next_queued_submission();
            }
        }
    }

    fn handle_create_session_result(
        &mut self,
        purpose: CreateSessionPurpose,
        result: ocpncord_backend::Result<ocpncord_backend::Session>,
    ) {
        match result {
            Ok(session) => {
                self.active_session = Some(session);
                match purpose {
                    CreateSessionPurpose::Send { text, mode, agent } => {
                        self.prompt_bar.clear();
                        let session_id = self
                            .active_session
                            .as_ref()
                            .map(|session| session.id.clone())
                            .unwrap_or_default();
                        let message = user_loaded_message(&text);
                        let submission = match mode {
                            InputMode::Shell => Submission::command(session_id, text, agent),
                            _ => Submission::prompt(session_id, text, agent),
                        };
                        self.queue_or_dispatch_submission(submission, message);
                    }
                    CreateSessionPurpose::NewChat => {
                        self.prompt_bar.clear();
                        self.draft = None;
                        self.chat.clear_messages();
                        self.active_modal = None;
                        self.active_mode = AppMode::Chat;
                    }
                }
            }
            Err(e) => {
                self.error = Some(alloc::format!("{}", e));
            }
        }
    }

    fn handle_list_sessions(
        &mut self,
        result: ocpncord_backend::Result<Vec<ocpncord_backend::Session>>,
    ) {
        let mut modal = SessionListModal::new();
        match result {
            Ok(sessions) => modal.set_sessions(sessions),
            Err(e) => modal.set_error(alloc::format!("{}", e)),
        }
        self.set_active_modal(Box::new(modal));
    }

    fn handle_load_session(
        &mut self,
        result: ocpncord_backend::Result<(ocpncord_backend::Session, Vec<LoadedMessage>)>,
    ) {
        match result {
            Ok((session, messages)) => {
                self.active_session = Some(session);
                self.active_mode = AppMode::Chat;
                self.chat.replace_messages(messages);
                self.clear_active_modal();
                self.open_next_blocking_modal_if_idle();
            }
            Err(e) => self.error = Some(alloc::format!("{}", e)),
        }
    }

    fn handle_delete_session(
        &mut self,
        result: ocpncord_backend::Result<Vec<ocpncord_backend::Session>>,
    ) {
        self.handle_list_sessions(result);
    }

    fn handle_open_model_picker(
        &mut self,
        result: ocpncord_backend::Result<(
            ocpncord_backend::Config,
            Option<Vec<ocpncord_backend::ModelSummary>>,
        )>,
    ) {
        let mut modal = ModelPickerModal::new();
        match result {
            Ok((config, models)) => {
                if let Some(models) = models {
                    self.model_cache = Some(models);
                }
                if let Some(models) = self.model_cache.as_ref() {
                    modal.set_models_from_config(config, models);
                } else {
                    modal.set_config(config);
                }
            }
            Err(e) => modal.set_error(alloc::format!("{}", e)),
        }
        self.set_active_modal(Box::new(modal));
    }

    fn handle_test_server_url(
        &mut self,
        result: ocpncord_backend::Result<ocpncord_backend::Health>,
    ) {
        if let Some(modal) = self
            .active_modal
            .as_deref_mut()
            .and_then(|modal| modal.as_server_config_mut())
        {
            modal.set_test_result(result);
        } else if let Err(error) = result {
            self.error = Some(alloc::format!("{error}"));
        }
    }

    fn handle_apply_server_url_error(&mut self, error: ocpncord_backend::BackendError) {
        let message = alloc::format!("{error}");
        if let Some(modal) = self
            .active_modal
            .as_deref_mut()
            .and_then(|modal| modal.as_server_config_mut())
        {
            modal.set_apply_error(message);
        } else {
            self.error = Some(message);
        }
    }

    fn handle_select_model(
        &mut self,
        requested: String,
        result: ocpncord_backend::Result<ocpncord_backend::Config>,
    ) {
        match result {
            Ok(updated) => {
                let mut display_config = updated;
                if display_config.model.is_none() {
                    display_config.model = Some(requested);
                }

                if let Some(modal) = self
                    .active_modal
                    .as_deref_mut()
                    .and_then(|modal| modal.as_model_picker_mut())
                {
                    modal.update_current_from_config(&display_config);
                    return;
                }

                let mut modal = ModelPickerModal::new();
                if let Some(models) = self.model_cache.as_ref() {
                    modal.set_models_from_config(display_config, models);
                } else {
                    modal.set_config(display_config);
                }
                self.set_active_modal(Box::new(modal));
            }
            Err(e) => {
                let error = alloc::format!("{}", e);
                if let Some(modal) = self
                    .active_modal
                    .as_deref_mut()
                    .and_then(|modal| modal.as_model_picker_mut())
                {
                    modal.set_error(error);
                    return;
                }

                let mut modal = ModelPickerModal::new();
                modal.set_error(error);
                self.set_active_modal(Box::new(modal));
            }
        }
    }

    fn handle_permission_reply_result(
        &mut self,
        message: String,
        result: ocpncord_backend::Result<()>,
    ) {
        match result {
            Ok(()) => self.push_toast(Toast {
                title: Some("Permission".into()),
                message,
                variant: ToastVariant::Success,
                created_at: self.tick,
                duration: 6,
            }),
            Err(e) => self.push_toast(Toast {
                title: Some("Permission".into()),
                message: alloc::format!("{message}: {e}"),
                variant: ToastVariant::Error,
                created_at: self.tick,
                duration: 8,
            }),
        }
    }

    fn handle_question_reply_result(&mut self, result: ocpncord_backend::Result<()>) {
        match result {
            Ok(()) => self.push_toast(Toast {
                title: Some("Question".into()),
                message: "Answer submitted".into(),
                variant: ToastVariant::Success,
                created_at: self.tick,
                duration: 6,
            }),
            Err(e) => self.push_toast(Toast {
                title: Some("Question".into()),
                message: alloc::format!("Answer failed: {e}"),
                variant: ToastVariant::Error,
                created_at: self.tick,
                duration: 8,
            }),
        }
    }

    fn handle_question_reject_result(&mut self, result: ocpncord_backend::Result<()>) {
        match result {
            Ok(()) => self.push_toast(Toast {
                title: Some("Question".into()),
                message: "Question rejected".into(),
                variant: ToastVariant::Success,
                created_at: self.tick,
                duration: 6,
            }),
            Err(e) => self.push_toast(Toast {
                title: Some("Question".into()),
                message: alloc::format!("Reject failed: {e}"),
                variant: ToastVariant::Error,
                created_at: self.tick,
                duration: 8,
            }),
        }
    }

    fn handle_simple_result(&mut self, result: ocpncord_backend::Result<()>) {
        if let Err(e) = result {
            self.error = Some(alloc::format!("{}", e));
        }
    }

    fn handle_rename_session(
        &mut self,
        result: ocpncord_backend::Result<ocpncord_backend::Session>,
    ) {
        match result {
            Ok(session) => self.active_session = Some(session),
            Err(e) => self.error = Some(alloc::format!("{}", e)),
        }
    }

    fn page_scroll_amount(&self) -> u16 {
        self.terminal_view_height.get().saturating_sub(1).max(1)
    }

    fn scroll_targets_terminal(&self) -> bool {
        self.active_mode == AppMode::Terminal
            || (self.side_panel_visible && self.side_panel_tab == Tab::Pane)
    }

    fn max_terminal_scroll(&self) -> u16 {
        self.terminal
            .lines
            .len()
            .saturating_sub(self.terminal_view_height.get() as usize) as u16
    }

    fn scroll_terminal_up(&mut self, amount: u16) {
        let max_scroll = self.max_terminal_scroll();
        self.terminal.scroll = self.terminal.scroll.saturating_add(amount).min(max_scroll);
    }

    fn scroll_terminal_down(&mut self, amount: u16) {
        self.terminal.scroll = self.terminal.scroll.saturating_sub(amount);
    }

    fn render_status_line(&self, frame: &mut ratatui::Frame, area: Rect) {
        let (agent, mode, model) = self.active_agent_status();
        let mut spans = vec![
            Span::styled("[", self.theme.text_dim),
            Span::styled(agent, self.theme.text_accent),
            Span::styled("]  mode: ", self.theme.text_dim),
            Span::styled(mode, self.mode_status_style(mode)),
            Span::styled("  model: ", self.theme.text_dim),
            Span::styled(model, self.theme.text_dim),
        ];
        if self.should_show_response_indicator() {
            let spinner =
                ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"][(self.tick as usize / 3) % 10];
            spans.push(Span::styled("  ", self.theme.text_dim));
            spans.push(Span::styled(spinner, self.theme.text_accent));
            spans.push(Span::styled(" Agent is Responding...", self.theme.text_dim));
        }
        Line::from(spans).render(area, frame.buffer_mut());
    }

    fn mode_status_style(&self, mode: &str) -> Style {
        match mode {
            "primary" => self.theme.part_tool_done,
            "subagent" => self.theme.part_subtask,
            "all" => self.theme.text_accent,
            _ => self.theme.text_dim,
        }
    }

    fn render_start_page_logo(&self, frame: &mut ratatui::Frame, area: Rect) {
        let logo_height = START_PAGE_LOGO.lines().count() as u16;
        let tip_height = 1u16;
        let total_content_height = logo_height + tip_height;
        let start_y = area.height.saturating_sub(total_content_height) / 2;

        let logo_area = Rect::new(area.x, start_y, area.width, logo_height);
        Text::from(START_PAGE_LOGO)
            .style(self.theme.logo)
            .alignment(Alignment::Center)
            .render(logo_area, frame.buffer_mut());

        let tip_area = Rect::new(area.x, start_y + logo_height, area.width, tip_height);
        Text::from(START_PAGE_TIP)
            .style(self.theme.text_dim)
            .alignment(Alignment::Center)
            .render(tip_area, frame.buffer_mut());
    }

    pub fn render(&self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        let panel_width = if self.side_panel_visible {
            ((area.width as u32 * 30) / 100)
                .max(24)
                .min(area.width as u32) as u16
        } else {
            0
        };
        let chunks = if panel_width > 0 {
            Layout::new(
                Direction::Horizontal,
                [
                    Constraint::Min(0),
                    Constraint::Length(panel_width.min(area.width)),
                ],
            )
            .split(area)
        } else {
            Layout::new(Direction::Horizontal, [Constraint::Min(0)]).split(area)
        };
        let main_area = chunks[0];

        match self.active_mode {
            AppMode::StartPage => {
                self.render_start_page_logo(frame, main_area);
                let rows = Layout::new(
                    Direction::Vertical,
                    [
                        Constraint::Min(0),
                        Constraint::Length(1),
                        Constraint::Length(1),
                        Constraint::Length(6),
                    ],
                )
                .split(main_area);
                let prompt_row = rows[1];
                let status_row = rows[2];
                let prompt_area = Rect::new(
                    prompt_row.x + prompt_row.width.saturating_sub(50) / 2,
                    prompt_row.y,
                    50.min(prompt_row.width),
                    1,
                );
                self.prompt_bar.render(
                    prompt_area,
                    frame,
                    &self.theme,
                    self.is_streaming,
                    self.active_agent_name(),
                    self.tick,
                );
                let status_area = Rect::new(prompt_area.x, status_row.y, prompt_area.width, 1);
                self.render_status_line(frame, status_area);
            }
            AppMode::Chat => {
                let rows = Layout::new(
                    Direction::Vertical,
                    [
                        Constraint::Min(0),
                        Constraint::Length(1),
                        Constraint::Length(1),
                    ],
                )
                .split(main_area);
                render_chat(
                    frame,
                    &self.theme,
                    rows[0],
                    ChatTranscript {
                        messages: self.chat.messages(),
                        active_parts: self.chat.partial_parts(),
                        queued_messages: self.chat.queued_messages(),
                        is_streaming: self.is_streaming,
                        display_policy: &self.display_policy,
                    },
                    self.chat_scroll,
                );
                self.prompt_bar.render(
                    rows[1],
                    frame,
                    &self.theme,
                    self.is_streaming,
                    self.active_agent_name(),
                    self.tick,
                );
                self.render_status_line(frame, rows[2]);
            }
            AppMode::Terminal => {
                self.render_terminal(frame, main_area);
            }
        }

        if let Some(ref err) = self.error {
            let area = frame.area();
            let msg = alloc::format!(" Error: {} ", err);
            let msg_width = msg.len() as u16;
            let x = (area.width.saturating_sub(msg_width)) / 2;
            let err_area = Rect::new(x, area.height / 2, msg_width.min(area.width), 1);
            Text::from(msg.as_str())
                .style(self.theme.text_error)
                .render(err_area, frame.buffer_mut());
        }

        self.render_toasts(frame, frame.area());

        // Render side panel (right 30% when visible)
        if self.side_panel_visible {
            let panel_area = if chunks.len() > 1 { chunks[1] } else { area };
            self.render_side_panel(frame, panel_area);
        }

        if let Some(ref modal) = self.active_modal {
            let area = frame.area();
            let (modal_width, modal_height) = modal.preferred_size(area);
            let modal_x = area.x + (area.width.saturating_sub(modal_width)) / 2;
            let modal_y = area.y + (area.height.saturating_sub(modal_height)) / 2;
            let modal_area = Rect::new(modal_x, modal_y, modal_width, modal_height);
            Clear.render(modal_area, frame.buffer_mut());
            let block = Block::bordered()
                .style(Style::new())
                .border_type(BorderType::Rounded)
                .border_style(self.theme.border)
                .title_style(self.theme.text_accent)
                .title(modal.title());
            let content_area = block.inner(modal_area);
            Clear.render(content_area, frame.buffer_mut());
            block.render(modal_area, frame.buffer_mut());
            modal.render(frame, &self.theme, content_area);
        }
    }

    fn render_side_panel(&self, frame: &mut ratatui::Frame, area: Rect) {
        let panel = Block::bordered()
            .border_type(BorderType::Plain)
            .border_style(self.theme.side_panel_border)
            .style(self.theme.side_panel_bg);
        let panel_inner = panel.inner(area);
        panel.render(area, frame.buffer_mut());

        let rows = Layout::new(
            Direction::Vertical,
            [Constraint::Length(1), Constraint::Min(0)],
        )
        .split(panel_inner);
        let selected = match self.side_panel_tab {
            Tab::Diagnostics => 0,
            Tab::Todos => 1,
            Tab::Pane => 2,
        };
        let labels = if rows[0].width < 26 {
            ["Diag", "Todo", "Term"]
        } else {
            ["Diagnostics", "Todos", "Terminal"]
        };
        Tabs::new(labels)
            .select(selected)
            .style(self.theme.side_panel_tab_inactive)
            .highlight_style(self.theme.side_panel_tab_active)
            .render(rows[0], frame.buffer_mut());

        let content_area = rows[1];
        match self.side_panel_tab {
            Tab::Diagnostics => self.render_diagnostics_panel(frame, content_area),
            Tab::Todos => self.render_todos_panel(frame, content_area),
            Tab::Pane => self.render_terminal_panel(frame, content_area),
        }
    }

    fn render_diagnostics_panel(&self, frame: &mut ratatui::Frame, area: Rect) {
        if self.lsp_diagnostics.is_empty() {
            Text::from("No diagnostics")
                .style(self.theme.text_dim)
                .render(area, frame.buffer_mut());
            return;
        }

        let mut rows: Vec<Row<'_>> = Vec::new();
        for (file, diags) in self.lsp_diagnostics.iter() {
            rows.push(Row::new([
                TableCell::new(""),
                TableCell::new(""),
                TableCell::new(file.as_str()).style(self.theme.side_panel_title),
            ]));
            for diag in diags.iter() {
                let severity = diag.severity.as_deref().unwrap_or("info");
                let location = alloc::format!("{}:{}", diag.line, diag.character);
                rows.push(Row::new([
                    TableCell::new(severity).style(self.theme.text_accent),
                    TableCell::new(location),
                    TableCell::new(diag.message.as_str()),
                ]));
            }
        }

        Widget::render(
            Table::new(
                rows,
                [
                    Constraint::Length(7),
                    Constraint::Length(7),
                    Constraint::Min(8),
                ],
            )
            .column_spacing(1)
            .style(self.theme.text),
            area,
            frame.buffer_mut(),
        );
    }

    fn render_todos_panel(&self, frame: &mut ratatui::Frame, area: Rect) {
        if self.todos.is_empty() {
            Text::from("No todos")
                .style(self.theme.text_dim)
                .render(area, frame.buffer_mut());
            return;
        }

        let items: Vec<ListItem<'_>> = self
            .todos
            .iter()
            .map(|todo| {
                let checkbox = if todo.status == "completed" {
                    "[x]"
                } else {
                    "[ ]"
                };
                let style = if todo.status == "completed" {
                    self.theme.text_dim
                } else {
                    self.theme.text
                };
                ListItem::new(Line::from(alloc::format!("{} {}", checkbox, todo.content)))
                    .style(style)
            })
            .collect();
        Widget::render(List::new(items), area, frame.buffer_mut());
    }

    fn render_terminal_panel(&self, frame: &mut ratatui::Frame, area: Rect) {
        if self.terminal.pty_id.is_none() {
            Text::from("No terminal")
                .style(self.theme.text_dim)
                .render(area, frame.buffer_mut());
            return;
        }

        self.terminal_view_height.set(area.height);
        let start_idx =
            terminal_start_index(self.terminal.lines.len(), area.height, self.terminal.scroll);
        let items: Vec<ListItem<'_>> = self
            .terminal
            .lines
            .iter()
            .map(|line| {
                let style = if line.is_error {
                    self.theme.pty_error
                } else {
                    self.theme.pty_output
                };
                ListItem::new(Line::from(line.content.as_str())).style(style)
            })
            .collect();
        let mut state = ListState::default().with_offset(start_idx);
        StatefulWidget::render(List::new(items), area, frame.buffer_mut(), &mut state);

        if self.terminal.lines.len() > area.height as usize {
            let mut scrollbar = ScrollbarState::new(self.terminal.lines.len()).position(start_idx);
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .thumb_style(self.theme.scrollbar)
                .track_style(self.theme.text_dim)
                .render(area, frame.buffer_mut(), &mut scrollbar);
        }
    }

    fn render_terminal(&self, frame: &mut ratatui::Frame, area: Rect) {
        if self.terminal.pty_id.is_none() {
            let msg =
                "No active terminal. Run a shell command or wait for the agent to create one.";
            let y = area.y + area.height / 2;
            Paragraph::new(msg)
                .style(self.theme.text_dim)
                .alignment(ratatui::layout::Alignment::Center)
                .wrap(Wrap { trim: true })
                .render(
                    Rect::new(area.x, y, area.width, 2.min(area.height)),
                    frame.buffer_mut(),
                );
            Text::from("Ctrl+X T: back")
                .style(self.theme.pty_status_bar)
                .render(
                    Rect::new(area.x, area.height.saturating_sub(1), area.width, 1),
                    frame.buffer_mut(),
                );
        } else {
            let rows = Layout::new(
                Direction::Vertical,
                [Constraint::Min(0), Constraint::Length(1)],
            )
            .split(area);
            let terminal_area = rows[0];
            self.terminal_view_height.set(terminal_area.height);

            let start_idx = terminal_start_index(
                self.terminal.lines.len(),
                terminal_area.height,
                self.terminal.scroll,
            );
            let lines: Vec<Line<'_>> = self
                .terminal
                .lines
                .iter()
                .skip(start_idx)
                .take(terminal_area.height as usize)
                .map(|line| {
                    let style = if line.is_error {
                        self.theme.pty_error
                    } else {
                        self.theme.pty_output
                    };
                    Line::from(line.content.as_str()).style(style)
                })
                .collect();

            Paragraph::new(Text::from(lines))
                .wrap(Wrap { trim: false })
                .render(terminal_area, frame.buffer_mut());

            let status_area = rows[1];
            let status_str = format!(
                " {} {} | {} | {} | Exit: {} ",
                self.terminal.command,
                self.terminal.pty_id.as_deref().unwrap_or(""),
                if self.terminal.status == ocpncord_backend::PtyStatus::Running {
                    "running"
                } else {
                    "exited"
                },
                self.terminal.lines.len(),
                self.terminal
                    .exit_code
                    .map(|c| c.to_string())
                    .unwrap_or_default(),
            );
            Text::from(status_str)
                .style(self.theme.pty_status_bar)
                .render(status_area, frame.buffer_mut());
        }
    }
}

enum DriverEvent {
    Platform(Event),
    Live(Option<ocpncord_backend::Result<EventEnvelope>>),
    Operation,
    PlatformClosed,
}

type BackendOpFuture<'a, B> = Pin<Box<dyn Future<Output = BackendOpResult<B>> + 'a>>;

#[cfg(test)]
fn backend_op_future<'a, B: Backend>(backend: &'a mut B, op: BackendOp) -> BackendOpFuture<'a, B> {
    Box::pin(execute_backend_op(backend, op))
}

fn backend_op_future_from_ptr<'a, B: Backend + 'a>(
    backend: *mut B,
    op: BackendOp,
) -> BackendOpFuture<'a, B> {
    Box::pin(async move {
        // The driver creates at most one backend operation future at a time and
        // drops it before starting another. The raw pointer avoids making the
        // loop self-referential while preserving that single mutable owner.
        let backend = unsafe { &mut *backend };
        execute_backend_op(backend, op).await
    })
}

async fn execute_backend_op<B: Backend>(backend: &mut B, op: BackendOp) -> BackendOpResult<B> {
    match op {
        BackendOp::LoadAgents => BackendOpResult::Agents(backend.list_agents().await),
        BackendOp::Subscribe => BackendOpResult::Subscribe(backend.subscribe_live().await),
        BackendOp::SyncHistory { request } => {
            BackendOpResult::SyncHistory(backend.sync_history(&request).await)
        }
        BackendOp::CreateSession {
            title,
            session_directory,
            purpose,
        } => BackendOpResult::CreateSession {
            purpose,
            result: backend.create_session(&title, &session_directory).await,
        },
        BackendOp::Submit { submission } => {
            let result = match submission.kind {
                SubmissionKind::Prompt => {
                    backend
                        .submit_prompt(
                            &submission.session_id,
                            &submission.execution_text,
                            Some(&submission.agent),
                        )
                        .await
                }
                SubmissionKind::Command => {
                    backend
                        .submit_command(
                            &submission.session_id,
                            &submission.execution_text,
                            Some(&submission.agent),
                        )
                        .await
                }
            };
            BackendOpResult::Submit { submission, result }
        }
        BackendOp::ListSessions => BackendOpResult::ListSessions(backend.list_sessions().await),
        BackendOp::LoadSession { session_id } => {
            let result = async {
                let session = backend.get_session(&session_id).await?;
                let messages = load_messages(backend, &session_id).await?;
                Ok((session, messages))
            }
            .await;
            BackendOpResult::LoadSession { result }
        }
        BackendOp::DeleteSession { session_id } => {
            let result = async {
                backend.delete_session(&session_id).await?;
                backend.list_sessions().await
            }
            .await;
            BackendOpResult::DeleteSession(result)
        }
        BackendOp::OpenModelPicker { cached_models } => {
            let result = async {
                let config = backend.get_config().await?;
                let models = match cached_models {
                    Some(models) => Some(models),
                    None => match backend.list_models().await {
                        Ok(models) => Some(models),
                        Err(_) if !config.provider.is_empty() => None,
                        Err(error) => return Err(error),
                    },
                };
                Ok((config, models))
            }
            .await;
            BackendOpResult::OpenModelPicker { result }
        }
        BackendOp::TestServerUrl { url } => BackendOpResult::TestServerUrl {
            result: backend.test_server_url(&url).await,
        },
        BackendOp::ApplyServerUrl { url } => {
            let normalized_url = url.trim_end_matches('/').to_string();
            let result = async {
                let health = backend.test_server_url(&normalized_url).await?;
                backend.set_server_url(&normalized_url).await?;
                Ok(health)
            }
            .await;
            BackendOpResult::ApplyServerUrl {
                url: normalized_url,
                result,
            }
        }
        BackendOp::SelectModel { model } => {
            let requested = model.clone();
            let result = async {
                let mut config = backend.get_config().await?;
                config.model = Some(model);
                backend.set_config(&config).await
            }
            .await;
            BackendOpResult::SelectModel { requested, result }
        }
        BackendOp::ReplyPermission { reply, message } => {
            let result = backend.reply_permission(&reply).await;
            BackendOpResult::PermissionReply { message, result }
        }
        BackendOp::ReplyQuestion { reply } => {
            let result = backend.reply_question(&reply).await;
            BackendOpResult::QuestionReply(result)
        }
        BackendOp::RejectQuestion { request_id } => {
            let result = backend.reject_question(&request_id).await;
            BackendOpResult::QuestionReject(result)
        }
        BackendOp::AbortSession { session_id } => {
            BackendOpResult::Abort(backend.abort_session(&session_id).await)
        }
        BackendOp::Dispose => BackendOpResult::Dispose(backend.dispose().await),
        BackendOp::Upgrade => BackendOpResult::Upgrade(backend.upgrade().await),
        BackendOp::RenameSession { session_id, title } => {
            BackendOpResult::RenameSession(backend.update_session(&session_id, &title).await)
        }
    }
}

async fn load_messages<B: Backend>(
    backend: &mut B,
    session_id: &str,
) -> ocpncord_backend::Result<Vec<LoadedMessage>> {
    let session_id = session_id.to_string();
    let summaries = backend.list_messages(&session_id).await?;
    let mut details = Vec::new();
    for summary in summaries {
        if let Ok(detail) = backend.get_message(&session_id, &summary.id).await {
            details.push(detail);
        }
    }
    Ok(loaded_messages_from_details(details))
}

/// Fully async application driver owning UI state, platform events, backend
/// streams, and the Ratatui terminal.
pub struct App<B, E, T>
where
    B: Backend,
    E: Stream<Item = Event> + Unpin,
    T: ratatui_core::backend::Backend,
{
    state: AppState,
    backend: B,
    events: E,
    live_events: Option<B::EventStream>,
    ratatui_terminal: ratatui_core::terminal::Terminal<T>,
    poll_cursor: u8,
}

impl<B, E, T> App<B, E, T>
where
    B: Backend,
    E: Stream<Item = Event> + Unpin,
    T: ratatui_core::backend::Backend,
{
    pub fn new(backend: B, events: E, terminal: ratatui_core::terminal::Terminal<T>) -> Self {
        let server_url = backend
            .server_url()
            .unwrap_or(DEFAULT_SERVER_URL)
            .to_string();
        Self {
            state: AppState::new_with_server_url(server_url),
            backend,
            events,
            live_events: None,
            ratatui_terminal: terminal,
            poll_cursor: 0,
        }
    }

    pub fn state(&self) -> &AppState {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut AppState {
        &mut self.state
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    pub fn terminal(&self) -> &ratatui_core::terminal::Terminal<T> {
        &self.ratatui_terminal
    }

    pub fn terminal_mut(&mut self) -> &mut ratatui_core::terminal::Terminal<T> {
        &mut self.ratatui_terminal
    }

    pub fn set_session_directory_override(&mut self, session_directory: String) {
        self.state.set_session_directory_override(session_directory);
    }

    #[cfg(test)]
    async fn drain_backend_ops_for_test(&mut self) {
        while let Some(op) = self.state.pending_ops.pop_front() {
            let result = backend_op_future(&mut self.backend, op).await;
            self.apply_backend_op_result(result);
        }
    }

    #[cfg(test)]
    fn apply_backend_op_result(&mut self, result: BackendOpResult<B>) {
        Self::apply_backend_op_result_to(&mut self.state, &mut self.live_events, result);
    }

    fn apply_backend_op_result_to(
        state: &mut AppState,
        live_events: &mut Option<B::EventStream>,
        result: BackendOpResult<B>,
    ) {
        match result {
            BackendOpResult::Agents(result) => state.apply_agent_result(result),
            BackendOpResult::Subscribe(result) => match result {
                Ok(stream) => *live_events = Some(stream),
                Err(e) => state.error = Some(alloc::format!("{}", e)),
            },
            BackendOpResult::SyncHistory(result) => state.handle_sync_history_result(result),
            BackendOpResult::CreateSession { purpose, result } => {
                state.handle_create_session_result(purpose, result)
            }
            BackendOpResult::Submit { submission, result } => {
                state.handle_submit_result::<B>(submission, result);
            }
            BackendOpResult::ListSessions(result) => state.handle_list_sessions(result),
            BackendOpResult::LoadSession { result } => state.handle_load_session(result),
            BackendOpResult::DeleteSession(result) => state.handle_delete_session(result),
            BackendOpResult::OpenModelPicker { result } => state.handle_open_model_picker(result),
            BackendOpResult::TestServerUrl { result } => state.handle_test_server_url(result),
            BackendOpResult::ApplyServerUrl { url, result } => match result {
                Ok(_) => {
                    *live_events = None;
                    state.clear_active_modal();
                    state.reset_for_server_switch(url);
                }
                Err(error) => state.handle_apply_server_url_error(error),
            },
            BackendOpResult::SelectModel { requested, result } => {
                state.handle_select_model(requested, result)
            }
            BackendOpResult::PermissionReply { message, result } => {
                state.handle_permission_reply_result(message, result)
            }
            BackendOpResult::QuestionReply(result) => state.handle_question_reply_result(result),
            BackendOpResult::QuestionReject(result) => state.handle_question_reject_result(result),
            BackendOpResult::Abort(result)
            | BackendOpResult::Dispose(result)
            | BackendOpResult::Upgrade(result) => state.handle_simple_result(result),
            BackendOpResult::RenameSession(result) => state.handle_rename_session(result),
        }
    }

    pub async fn run(&mut self) {
        let Self {
            state,
            backend,
            events,
            live_events,
            ratatui_terminal,
            poll_cursor,
        } = self;

        let backend_ptr: *mut B = backend;
        state.queue_startup();
        let _ = ratatui_terminal.draw(|frame| state.render(frame));

        let mut active_op: Option<BackendOpFuture<'_, B>> = None;
        loop {
            if active_op.is_none() {
                if let Some(op) = state.pending_ops.pop_front() {
                    active_op = Some(backend_op_future_from_ptr(backend_ptr, op));
                }
            }

            let mut completed_op = None;
            let event = futures::future::poll_fn(|cx| {
                for offset in 0..3 {
                    match offset {
                        0 => {
                            if let Some(op) = active_op.as_mut() {
                                if let Poll::Ready(result) = op.as_mut().poll(cx) {
                                    completed_op = Some(result);
                                    return Poll::Ready(DriverEvent::Operation);
                                }
                            }
                        }
                        1 => {
                            if let Some(stream) = live_events {
                                if let Poll::Ready(event) = Pin::new(stream).poll_next(cx) {
                                    return Poll::Ready(DriverEvent::Live(event));
                                }
                            }
                        }
                        _ => {
                            if let Poll::Ready(event) = Pin::new(&mut *events).poll_next(cx) {
                                if let Some(event) = event {
                                    return Poll::Ready(DriverEvent::Platform(event));
                                }
                                return Poll::Ready(DriverEvent::PlatformClosed);
                            }
                        }
                    }
                }
                Poll::Pending
            })
            .await;
            *poll_cursor = (*poll_cursor + 1) % 3;

            if let Some(result) = completed_op {
                active_op = None;
                Self::apply_backend_op_result_to(state, live_events, result);
            }

            let running = match event {
                DriverEvent::Platform(event) => state.handle_event(event),
                DriverEvent::Live(Some(Ok(envelope))) => state.apply_live_envelope(envelope),
                DriverEvent::Live(Some(Err(error))) => {
                    *live_events = None;
                    state.schedule_live_reconnect();
                    state.handle_event(Event::Backend(BackendEvent::Error {
                        message: alloc::format!("{error}"),
                    }))
                }
                DriverEvent::Live(None) => {
                    *live_events = None;
                    state.schedule_live_reconnect();
                    true
                }
                DriverEvent::Operation => true,
                DriverEvent::PlatformClosed => false,
            };

            let _ = ratatui_terminal.draw(|frame| state.render(frame));
            if !running {
                break;
            }
        }
    }
}

fn terminal_start_index(line_count: usize, height: u16, scroll: u16) -> usize {
    line_count
        .saturating_sub(height as usize)
        .saturating_sub(scroll as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{KeyEvent, Modifiers, Scancode};
    use ocpncord_backend::mock::{MockBackend, MockSubmissionCall};
    use ocpncord_backend::{BackendError, Result as BackendResult};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    struct PendingStartupBackend;

    fn pending_backend_error() -> BackendError {
        BackendError::Connection {
            message: "not implemented in test backend".into(),
        }
    }

    impl Backend for PendingStartupBackend {
        type EventStream = futures::stream::Pending<BackendResult<EventEnvelope>>;

        async fn health(&mut self) -> BackendResult<ocpncord_backend::Health> {
            Err(pending_backend_error())
        }

        async fn list_agents(&mut self) -> BackendResult<Vec<ocpncord_backend::Agent>> {
            futures::future::pending().await
        }

        async fn list_sessions(&mut self) -> BackendResult<Vec<ocpncord_backend::Session>> {
            Err(pending_backend_error())
        }

        async fn get_session(
            &mut self,
            _id: &ocpncord_backend::SessionId,
        ) -> BackendResult<ocpncord_backend::Session> {
            Err(pending_backend_error())
        }

        async fn create_session(
            &mut self,
            _title: &str,
            _session_directory: &str,
        ) -> BackendResult<ocpncord_backend::Session> {
            Err(pending_backend_error())
        }

        async fn delete_session(&mut self, _id: &ocpncord_backend::SessionId) -> BackendResult<()> {
            Err(pending_backend_error())
        }

        async fn update_session(
            &mut self,
            _id: &ocpncord_backend::SessionId,
            _title: &str,
        ) -> BackendResult<ocpncord_backend::Session> {
            Err(pending_backend_error())
        }

        async fn children_sessions(
            &mut self,
            _id: &ocpncord_backend::SessionId,
        ) -> BackendResult<Vec<ocpncord_backend::Session>> {
            Err(pending_backend_error())
        }

        async fn abort_session(&mut self, _id: &ocpncord_backend::SessionId) -> BackendResult<()> {
            Err(pending_backend_error())
        }

        async fn list_messages(
            &mut self,
            _id: &ocpncord_backend::SessionId,
        ) -> BackendResult<Vec<ocpncord_backend::MessageSummary>> {
            Err(pending_backend_error())
        }

        async fn get_message(
            &mut self,
            _session_id: &ocpncord_backend::SessionId,
            _message_id: &ocpncord_backend::MessageId,
        ) -> BackendResult<ocpncord_backend::MessageDetail> {
            Err(pending_backend_error())
        }

        async fn submit_prompt(
            &mut self,
            _id: &ocpncord_backend::SessionId,
            _text: &str,
            _agent: Option<&str>,
        ) -> BackendResult<ocpncord_backend::SubmissionReceipt> {
            Err(pending_backend_error())
        }

        async fn submit_command(
            &mut self,
            _id: &ocpncord_backend::SessionId,
            _text: &str,
            _agent: Option<&str>,
        ) -> BackendResult<ocpncord_backend::SubmissionReceipt> {
            Err(pending_backend_error())
        }

        async fn reply_permission(
            &mut self,
            _reply: &ocpncord_backend::PermissionReply,
        ) -> BackendResult<()> {
            Err(pending_backend_error())
        }

        async fn reply_question(
            &mut self,
            _reply: &ocpncord_backend::QuestionReply,
        ) -> BackendResult<()> {
            Err(pending_backend_error())
        }

        async fn reject_question(&mut self, _request_id: &str) -> BackendResult<()> {
            Err(pending_backend_error())
        }

        async fn find_text(
            &mut self,
            _pattern: &str,
        ) -> BackendResult<Vec<ocpncord_backend::TextMatch>> {
            Err(pending_backend_error())
        }

        async fn subscribe_live(&mut self) -> BackendResult<Self::EventStream> {
            Err(pending_backend_error())
        }

        async fn sync_history(
            &mut self,
            _request: &ocpncord_backend::SyncHistoryRequest,
        ) -> BackendResult<ocpncord_backend::SyncHistoryBatch> {
            Err(pending_backend_error())
        }

        async fn get_config(&mut self) -> BackendResult<ocpncord_backend::Config> {
            Err(pending_backend_error())
        }

        async fn list_models(&mut self) -> BackendResult<Vec<ocpncord_backend::ModelSummary>> {
            Err(pending_backend_error())
        }

        async fn set_auth(&mut self, _provider: &str, _api_key: &str) -> BackendResult<()> {
            Err(pending_backend_error())
        }

        async fn set_config(
            &mut self,
            _config: &ocpncord_backend::Config,
        ) -> BackendResult<ocpncord_backend::Config> {
            Err(pending_backend_error())
        }

        async fn dispose(&mut self) -> BackendResult<()> {
            Err(pending_backend_error())
        }

        async fn upgrade(&mut self) -> BackendResult<()> {
            Err(pending_backend_error())
        }

        async fn log(&mut self, _level: &str, _message: &str) -> BackendResult<()> {
            Err(pending_backend_error())
        }

        async fn remove_auth(&mut self, _provider: &str) -> BackendResult<()> {
            Err(pending_backend_error())
        }
    }

    fn ctrl(key: char) -> Event {
        Event::Key(KeyEvent {
            scancode: Scancode::Char(key),
            modifiers: Modifiers {
                ctrl: true,
                shift: false,
                alt: false,
                meta: false,
            },
        })
    }

    fn char_key(ch: char) -> Event {
        Event::Key(KeyEvent {
            scancode: Scancode::Char(ch),
            modifiers: Modifiers::default(),
        })
    }

    type TestApp<B> = App<B, futures::stream::Empty<Event>, TestBackend>;

    fn new_app<B: Backend>(backend: B) -> TestApp<B> {
        App::new(
            backend,
            futures::stream::empty(),
            Terminal::new(TestBackend::new(80, 24)).unwrap(),
        )
    }

    fn new_app_with_events<B: Backend, E: futures_core::Stream<Item = Event> + Unpin>(
        backend: B,
        events: E,
    ) -> App<B, E, TestBackend> {
        App::new(
            backend,
            events,
            Terminal::new(TestBackend::new(80, 24)).unwrap(),
        )
    }

    fn live_event(event: BackendEvent) -> BackendResult<EventEnvelope> {
        Ok(EventEnvelope::new(event))
    }

    fn sync_envelope(event: BackendEvent, aggregate_id: &str, seq: u64) -> EventEnvelope {
        EventEnvelope {
            event,
            scope: EventScope::default(),
            cursor: Some(ocpncord_backend::EventCursor {
                event_id: Some(format!("evt-{seq}")),
                aggregate_id: Some(aggregate_id.into()),
                seq: Some(seq),
            }),
        }
    }

    fn assistant_message_updated(session_id: &str, message_id: &str) -> BackendEvent {
        BackendEvent::MessageUpdated {
            session_id: session_id.into(),
            message: ocpncord_backend::Message::Assistant(ocpncord_backend::AssistantMessage {
                id: message_id.into(),
                session_id: session_id.into(),
                role: ocpncord_backend::MessageRole::Assistant,
                time: ocpncord_backend::MessageTime {
                    created: 0,
                    completed: Some(1),
                },
                error: None,
                parent_id: None,
                model_id: "mock/model".into(),
                provider_id: "mock".into(),
                mode: "default".into(),
                agent: "build".into(),
                path: None,
                summary: None,
                cost: 0.0,
                tokens: None,
                structured: None,
                variant: None,
                finish: None,
            }),
        }
    }

    fn message_part_updated(
        session_id: &str,
        message_id: &str,
        part_id: &str,
        part: ocpncord_backend::Part,
    ) -> BackendEvent {
        BackendEvent::MessagePartUpdated {
            session_id: session_id.into(),
            message_id: message_id.into(),
            part_id: part_id.into(),
            part,
        }
    }

    fn run<B: Backend>(app: &mut TestApp<B>, event: Event) -> bool {
        let running = app.state.handle_event(event);
        futures::executor::block_on(app.drain_backend_ops_for_test());
        running
    }

    fn init<B: Backend>(app: &mut TestApp<B>) {
        app.state.queue_startup();
        futures::executor::block_on(app.drain_backend_ops_for_test());
    }

    fn next_live_event<B: Backend>(
        app: &mut TestApp<B>,
    ) -> Option<Result<BackendEvent, ocpncord_backend::BackendError>> {
        futures::executor::block_on(async {
            use futures::StreamExt;

            let stream = app.live_events.as_mut()?;
            let event = stream.next().await;
            if event.is_none() {
                app.live_events = None;
            }
            event.map(|result| result.map(|envelope| envelope.event))
        })
    }

    fn rendered_screen(terminal: &Terminal<TestBackend>) -> String {
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    fn push_terminal_line(terminal: &mut TerminalPane, line: TermLine) {
        if terminal.lines.len() >= 2000 {
            terminal.lines.pop_front();
        }
        terminal.lines.push_back(line);
    }

    #[test]
    fn ctrl_c_quits() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);
        assert!(!run(&mut app, ctrl('c')));
    }

    #[test]
    fn ctrl_x_q_quits() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);
        run(&mut app, ctrl('x'));
        assert!(!run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::Char('q'),
                modifiers: Modifiers::default(),
            })
        ));
    }

    #[test]
    fn starts_on_start_page() {
        let backend = MockBackend::default();
        let app = new_app(backend);
        assert_eq!(app.state.active_mode(), AppMode::StartPage);
    }

    #[test]
    fn non_quit_events_keep_running() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);
        assert!(run(&mut app, char_key('a')));
        assert!(run(&mut app, char_key('b')));
    }

    #[test]
    fn leader_times_out_after_ticks() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);
        run(&mut app, ctrl('x'));
        for _ in 0..40 {
            run(&mut app, Event::Tick);
        }
        assert!(
            run(&mut app, char_key('q')),
            "q after timeout should not quit"
        );
    }

    #[test]
    fn init_fallback_to_default_agents_when_backend_returns_empty() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);
        init(&mut app);
        assert_eq!(app.state.active_agent_name(), "build");
    }

    #[test]
    fn init_loads_primary_agents_from_backend() {
        let mut backend = MockBackend::default();
        backend.agents = vec![ocpncord_backend::Agent {
            name: "coder".into(),
            mode: ocpncord_backend::AgentMode::Primary,
            description: None,
            native: None,
            hidden: None,
            model: None,
            color: None,
            variant: None,
            prompt: None,
            steps: None,
        }];
        let mut app = new_app(backend);
        init(&mut app);
        assert_eq!(app.state.active_agent_name(), "coder");
    }

    #[test]
    fn shift_tab_cycles_backward_through_agents() {
        let mut backend = MockBackend::default();
        backend.agents = vec![
            ocpncord_backend::Agent {
                name: "build".into(),
                mode: ocpncord_backend::AgentMode::Primary,
                description: None,
                native: None,
                hidden: None,
                model: None,
                color: None,
                variant: None,
                prompt: None,
                steps: None,
            },
            ocpncord_backend::Agent {
                name: "plan".into(),
                mode: ocpncord_backend::AgentMode::Primary,
                description: None,
                native: None,
                hidden: None,
                model: None,
                color: None,
                variant: None,
                prompt: None,
                steps: None,
            },
            ocpncord_backend::Agent {
                name: "coder".into(),
                mode: ocpncord_backend::AgentMode::Primary,
                description: None,
                native: None,
                hidden: None,
                model: None,
                color: None,
                variant: None,
                prompt: None,
                steps: None,
            },
        ];
        let mut app = new_app(backend);
        init(&mut app);

        // Move to index 1 (plan)
        run(&mut app, tab_key());
        assert_eq!(app.state.active_agent_name(), "plan");

        // Shift+Tab should go back to build
        run(&mut app, shift_tab_key());
        assert_eq!(app.state.active_agent_name(), "build");
    }

    fn tab_key() -> Event {
        Event::Key(KeyEvent {
            scancode: Scancode::Tab,
            modifiers: Modifiers::default(),
        })
    }

    fn shift_tab_key() -> Event {
        Event::Key(KeyEvent {
            scancode: Scancode::Tab,
            modifiers: Modifiers {
                ctrl: false,
                shift: true,
                alt: false,
                meta: false,
            },
        })
    }

    #[test]
    fn tab_wraps_to_first_agent_at_end_of_list() {
        let mut backend = MockBackend::default();
        backend.agents = vec![
            ocpncord_backend::Agent {
                name: "build".into(),
                mode: ocpncord_backend::AgentMode::Primary,
                description: None,
                native: None,
                hidden: None,
                model: None,
                color: None,
                variant: None,
                prompt: None,
                steps: None,
            },
            ocpncord_backend::Agent {
                name: "plan".into(),
                mode: ocpncord_backend::AgentMode::Primary,
                description: None,
                native: None,
                hidden: None,
                model: None,
                color: None,
                variant: None,
                prompt: None,
                steps: None,
            },
        ];
        let mut app = new_app(backend);
        init(&mut app);

        // Tab at index 0 → index 1
        run(&mut app, tab_key());
        assert_eq!(app.state.active_agent_name(), "plan");
        // Tab at index 1 → wraps to index 0
        run(&mut app, tab_key());
        assert_eq!(app.state.active_agent_name(), "build");
    }

    #[test]
    fn shift_tab_wraps_to_last_agent_at_start_of_list() {
        let mut backend = MockBackend::default();
        backend.agents = vec![
            ocpncord_backend::Agent {
                name: "build".into(),
                mode: ocpncord_backend::AgentMode::Primary,
                description: None,
                native: None,
                hidden: None,
                model: None,
                color: None,
                variant: None,
                prompt: None,
                steps: None,
            },
            ocpncord_backend::Agent {
                name: "plan".into(),
                mode: ocpncord_backend::AgentMode::Primary,
                description: None,
                native: None,
                hidden: None,
                model: None,
                color: None,
                variant: None,
                prompt: None,
                steps: None,
            },
        ];
        let mut app = new_app(backend);
        init(&mut app);

        // Shift+Tab at index 0 → wraps to last index
        run(&mut app, shift_tab_key());
        assert_eq!(app.state.active_agent_name(), "plan");
    }

    #[test]
    fn tab_cycles_forward_through_agents() {
        let mut backend = MockBackend::default();
        backend.agents = vec![
            ocpncord_backend::Agent {
                name: "build".into(),
                mode: ocpncord_backend::AgentMode::Primary,
                description: None,
                native: None,
                hidden: None,
                model: None,
                color: None,
                variant: None,
                prompt: None,
                steps: None,
            },
            ocpncord_backend::Agent {
                name: "plan".into(),
                mode: ocpncord_backend::AgentMode::Primary,
                description: None,
                native: None,
                hidden: None,
                model: None,
                color: None,
                variant: None,
                prompt: None,
                steps: None,
            },
        ];
        let mut app = new_app(backend);
        init(&mut app);
        assert_eq!(app.state.active_agent_name(), "build");

        run(&mut app, tab_key());
        assert_eq!(app.state.active_agent_name(), "plan");
    }

    #[test]
    fn agent_name_passed_to_prompt_on_send() {
        let mut backend = MockBackend::default();
        backend.agents = vec![
            ocpncord_backend::Agent {
                name: "build".into(),
                mode: ocpncord_backend::AgentMode::Primary,
                description: None,
                native: None,
                hidden: None,
                model: None,
                color: None,
                variant: None,
                prompt: None,
                steps: None,
            },
            ocpncord_backend::Agent {
                name: "plan".into(),
                mode: ocpncord_backend::AgentMode::Primary,
                description: None,
                native: None,
                hidden: None,
                model: None,
                color: None,
                variant: None,
                prompt: None,
                steps: None,
            },
        ];
        let mut app = new_app(backend);

        init(&mut app);
        // Tab to "plan"
        run(&mut app, tab_key());
        assert_eq!(app.state.active_agent_name(), "plan");

        // Send a message
        run(&mut app, char_key('h'));
        let still_running = run(&mut app, enter_key());
        assert!(still_running, "app should keep running after send");

        // Verify session was created
        assert_eq!(app.backend().sessions.len(), 1, "session should be created");
        assert_eq!(
            app.backend().prompt_calls,
            vec![MockSubmissionCall {
                session_id: "mock-session-id".into(),
                text: "h".into(),
                agent: Some("plan".into())
            }],
            "prompt should be sent with the selected agent"
        );
        assert_eq!(
            app.state.active_submission(),
            Some(&Submission {
                kind: SubmissionKind::Prompt,
                session_id: "mock-session-id".into(),
                text: "h".into(),
                execution_text: "h".into(),
                agent: "plan".into(),
            })
        );
    }

    #[test]
    fn explicit_quit_event_quits() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);
        assert!(!run(&mut app, Event::Quit));
    }

    #[test]
    fn run_exits_on_quit_event_and_redraws() {
        let backend = MockBackend::default();
        let events = futures::stream::iter(vec![Event::Quit]);
        let mut app = new_app_with_events(backend, events);

        futures::executor::block_on(app.run());

        let screen = rendered_screen(app.terminal());
        assert!(screen.contains(">"), "screen: {screen}");
    }

    #[test]
    fn run_exits_on_quit_while_startup_backend_op_is_pending() {
        let events = futures::stream::iter(vec![Event::Quit]);
        let mut app = App::new(
            PendingStartupBackend,
            events,
            Terminal::new(TestBackend::new(80, 24)).unwrap(),
        );

        futures::executor::block_on(app.run());

        let screen = rendered_screen(app.terminal());
        assert!(screen.contains(">"), "screen: {screen}");
    }

    #[test]
    fn run_handles_live_events_from_subscribe() {
        let mut backend = MockBackend::default();
        backend.live_events = vec![live_event(ocpncord_backend::BackendEvent::SessionCreated {
            session: make_session("session-from-background", "Background"),
        })];
        let events = futures::stream::iter(vec![Event::Tick, Event::Quit]);
        let mut app = new_app_with_events(backend, events);

        futures::executor::block_on(app.run());

        assert_eq!(
            app.state
                .active_session()
                .map(|session| session.id.as_str()),
            Some("session-from-background")
        );
    }

    #[test]
    fn run_keeps_platform_events_responsive_while_live_stream_is_active() {
        let mut backend = MockBackend::default();
        backend.live_event_stream_pending_polls = 2;
        backend.live_events = vec![live_event(message_part_updated(
            "mock-session-id",
            "msg-assistant-1",
            "prt-assistant-1",
            ocpncord_backend::Part::Text(ocpncord_backend::TextPart {
                identity: Default::default(),
                text: "assistant".into(),
            }),
        ))];
        let events =
            futures::stream::iter(vec![char_key('h'), enter_key(), char_key('n'), Event::Quit]);
        let mut app = new_app_with_events(backend, events);

        futures::executor::block_on(app.run());

        assert_eq!(app.state.prompt_text(), "n");
        assert_eq!(app.state.partial_parts().len(), 1);
    }

    #[test]
    fn run_clears_exhausted_streams_without_exiting() {
        let backend = MockBackend::default();
        let events =
            futures::stream::iter(vec![char_key('h'), enter_key(), char_key('n'), Event::Quit]);
        let mut app = new_app_with_events(backend, events);

        futures::executor::block_on(app.run());

        assert_eq!(app.state.prompt_text(), "n");
        assert!(app.live_events.is_none());
    }

    fn enter_key() -> Event {
        Event::Key(KeyEvent {
            scancode: Scancode::Enter,
            modifiers: Modifiers::default(),
        })
    }

    fn escape_key() -> Event {
        Event::Key(KeyEvent {
            scancode: Scancode::Escape,
            modifiers: Modifiers::default(),
        })
    }

    #[test]
    fn send_message_starts_stream_and_accumulates_parts() {
        let mut backend = MockBackend::default();
        backend.live_events = vec![
            live_event(message_part_updated(
                "mock-session-id",
                "msg-assistant-1",
                "prt-assistant-1",
                ocpncord_backend::Part::Text(ocpncord_backend::TextPart {
                    identity: Default::default(),
                    text: "Hello".into(),
                }),
            )),
            live_event(assistant_message_updated(
                "mock-session-id",
                "msg-assistant-1",
            )),
        ];
        let mut app = new_app(backend);
        init(&mut app);

        run(&mut app, char_key('h'));
        run(&mut app, char_key('i'));
        let running = run(&mut app, enter_key());
        assert!(running);
        assert!(app.state.is_streaming());

        let event = next_live_event(&mut app)
            .expect("prompt stream should yield a part")
            .expect("part event should not error");
        run(&mut app, Event::Backend(event));
        assert_eq!(app.state.partial_parts().len(), 1);

        let event = next_live_event(&mut app)
            .expect("prompt stream should yield assistant MessageUpdated")
            .expect("MessageUpdated event should not error");
        run(&mut app, Event::Backend(event));
        assert!(!app.state.is_streaming());
        assert_eq!(
            app.state.messages().len(),
            2,
            "user and assistant messages should be visible after finalization"
        );
        assert_eq!(
            app.state.partial_parts().len(),
            0,
            "finalized content moves out of partial_parts"
        );
    }

    #[test]
    fn sync_history_replay_finalizes_transcript_and_tracks_cursor() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);
        app.state.active_session = Some(make_session("mock-session-id", "Mock"));
        app.state.is_streaming = true;

        let batch = ocpncord_backend::SyncHistoryBatch {
            envelopes: vec![
                sync_envelope(
                    message_part_updated(
                        "mock-session-id",
                        "msg-assistant-1",
                        "prt-assistant-1",
                        ocpncord_backend::Part::Text(ocpncord_backend::TextPart {
                            identity: Default::default(),
                            text: "from catch-up".into(),
                        }),
                    ),
                    "mock-session-id",
                    1,
                ),
                sync_envelope(
                    assistant_message_updated("mock-session-id", "msg-assistant-1"),
                    "mock-session-id",
                    2,
                ),
            ],
            known_sequences: BTreeMap::new(),
        };

        app.state.handle_sync_history_result(Ok(batch));

        assert!(!app.state.is_streaming());
        assert_eq!(app.state.messages().len(), 1);
        assert_eq!(
            app.state.sync_known_sequences.get("mock-session-id"),
            Some(&2)
        );
    }

    #[test]
    fn message_removed_deletes_loaded_transcript_entry() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);
        app.state.active_session = Some(make_session("mock-session-id", "Mock"));
        app.state.chat.push_message(LoadedMessage {
            id: Some("msg-1".into()),
            session_id: Some("mock-session-id".into()),
            role: ocpncord_backend::MessageRole::Assistant,
            parts: vec![ocpncord_backend::Part::Text(ocpncord_backend::TextPart {
                identity: Default::default(),
                text: "remove me".into(),
            })],
        });

        run(
            &mut app,
            Event::Backend(ocpncord_backend::BackendEvent::MessageRemoved {
                session_id: "mock-session-id".into(),
                message_id: "msg-1".into(),
            }),
        );

        assert!(app.state.messages().is_empty());
    }

    #[test]
    fn message_part_removed_deletes_partial_delta_part() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);
        app.state.active_session = Some(make_session("mock-session-id", "Mock"));

        run(
            &mut app,
            Event::Backend(ocpncord_backend::BackendEvent::MessagePartDelta {
                session_id: "mock-session-id".into(),
                message_id: "msg-1".into(),
                part_id: "part-1".into(),
                field: "text".into(),
                delta: "temporary".into(),
            }),
        );
        assert_eq!(app.state.partial_parts().len(), 1);

        run(
            &mut app,
            Event::Backend(ocpncord_backend::BackendEvent::MessagePartRemoved {
                session_id: "mock-session-id".into(),
                message_id: "msg-1".into(),
                part_id: "part-1".into(),
            }),
        );

        assert!(app.state.partial_parts().is_empty());
    }

    #[test]
    fn live_stream_close_schedules_sync_history_reconnect() {
        let backend = MockBackend::default();
        let events = futures::stream::iter(vec![
            Event::Tick,
            Event::Tick,
            Event::Tick,
            Event::Tick,
            Event::Tick,
            Event::Quit,
        ]);
        let mut app = new_app_with_events(backend, events);

        futures::executor::block_on(app.run());

        assert!(
            app.backend().sync_history_requests.len() >= 2,
            "startup catch-up plus reconnect catch-up should be requested"
        );
    }

    #[test]
    fn streaming_keeps_prompt_editable_and_status_on_bottom_line() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);

        run(&mut app, char_key('h'));
        run(&mut app, char_key('i'));
        run(&mut app, enter_key());
        assert!(app.state.is_streaming());
        assert_eq!(app.state.prompt_text(), "");

        run(&mut app, char_key('n'));
        run(&mut app, char_key('e'));
        run(&mut app, char_key('x'));
        run(&mut app, char_key('t'));
        assert_eq!(app.state.prompt_text(), "next");

        run(&mut app, enter_key());
        assert_eq!(
            app.backend().prompt_calls.len(),
            1,
            "the active prompt remains the only dispatched prompt"
        );
        assert_eq!(
            app.state.queued_submissions().len(),
            1,
            "Enter should queue another prompt while streaming"
        );
        assert_eq!(app.state.prompt_text(), "");

        let test_backend = TestBackend::new(80, 8);
        let mut terminal = Terminal::new(test_backend).unwrap();
        terminal.draw(|frame| app.state.render(frame)).unwrap();
        let screen = rendered_screen(&terminal);

        assert!(screen.contains(">"), "screen: {screen}");
        assert!(screen.contains("Agent is Responding"), "screen: {screen}");
        assert!(screen.contains("mode: primary"), "screen: {screen}");
        assert!(screen.contains("model: default"), "screen: {screen}");
    }

    #[test]
    fn start_page_status_line_matches_prompt_width_without_prompt_background() {
        let backend = MockBackend::default();
        let app = new_app(backend);

        let test_backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(test_backend).unwrap();
        terminal.draw(|frame| app.state.render(frame)).unwrap();
        let buf = terminal.backend().buffer();

        let status_y = 17;
        let prompt_x = 15;
        let prompt_width = 50;
        let row: String = (0..80)
            .map(|x| buf.cell((x, status_y)).map_or(" ", |c| c.symbol()))
            .collect();
        assert!(row.contains("mode: primary"), "row: {row}");

        for x in 0..80 {
            let cell = buf.cell((x, status_y)).unwrap();
            if x < prompt_x || x >= prompt_x + prompt_width {
                assert_eq!(
                    cell.symbol(),
                    " ",
                    "status leaked outside prompt width at x={x}"
                );
            }
            if cell.symbol() != " " {
                assert_ne!(
                    cell.style().bg,
                    app.state.theme.input.bg,
                    "status line should not use prompt input background at x={x}"
                );
            }
        }
    }

    #[test]
    fn status_line_uses_distinct_mode_colours() {
        fn mode_style(
            mode: ocpncord_backend::AgentMode,
            label: &str,
        ) -> Option<ratatui::style::Color> {
            let backend = MockBackend::default();
            let mut app = new_app(backend);
            app.state.agents = vec![ocpncord_backend::Agent {
                name: "agent".into(),
                description: None,
                mode,
                native: None,
                hidden: None,
                model: None,
                color: None,
                variant: None,
                prompt: None,
                steps: None,
            }];

            let test_backend = TestBackend::new(80, 1);
            let mut terminal = Terminal::new(test_backend).unwrap();
            terminal
                .draw(|frame| app.state.render_status_line(frame, Rect::new(0, 0, 80, 1)))
                .unwrap();
            let buf = terminal.backend().buffer();
            let row: String = (0..80)
                .map(|x| buf.cell((x, 0)).map_or(" ", |c| c.symbol()))
                .collect();
            let start = row.find(label).expect("mode label should render") as u16;
            let cell = buf.cell((start, 0)).unwrap();
            assert_ne!(
                cell.style().bg,
                app.state.theme.input.bg,
                "mode label should not use prompt input background"
            );
            cell.style().fg
        }

        let primary = mode_style(ocpncord_backend::AgentMode::Primary, "primary");
        let subagent = mode_style(ocpncord_backend::AgentMode::Subagent, "subagent");
        let all = mode_style(ocpncord_backend::AgentMode::All, "all");

        assert_ne!(primary, subagent);
        assert_ne!(primary, all);
        assert_ne!(subagent, all);
    }

    #[test]
    fn queued_messages_render_after_active_assistant_and_dispatch_in_order() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);

        for ch in "first".chars() {
            run(&mut app, char_key(ch));
        }
        run(&mut app, enter_key());
        assert_eq!(app.backend().prompt_calls[0].text, "first");

        run(
            &mut app,
            Event::Backend(message_part_updated(
                "mock-session-id",
                "msg-assistant-1",
                "prt-assistant-1",
                ocpncord_backend::Part::Text(ocpncord_backend::TextPart {
                    identity: Default::default(),
                    text: "assistant one".into(),
                }),
            )),
        );

        for ch in "second".chars() {
            run(&mut app, char_key(ch));
        }
        run(&mut app, enter_key());
        for ch in "third".chars() {
            run(&mut app, char_key(ch));
        }
        run(&mut app, enter_key());

        assert_eq!(
            app.state
                .queued_submissions()
                .iter()
                .map(|submission| submission.text.as_str())
                .collect::<Vec<_>>(),
            vec!["second", "third"]
        );

        let test_backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(test_backend).unwrap();
        terminal.draw(|frame| app.state.render(frame)).unwrap();
        let screen = rendered_screen(&terminal);
        let first = screen.find("first").expect(&screen);
        let assistant = screen.find("assistant one").expect(&screen);
        let second = screen.find("second").expect(&screen);
        let third = screen.find("third").expect(&screen);
        assert!(first < assistant, "screen: {screen}");
        assert!(assistant < second, "screen: {screen}");
        assert!(second < third, "screen: {screen}");

        run(
            &mut app,
            Event::Backend(assistant_message_updated(
                "mock-session-id",
                "msg-assistant-1",
            )),
        );

        assert_eq!(app.backend().prompt_calls.len(), 2);
        assert_eq!(app.backend().prompt_calls[1].text, "second");
        assert_eq!(app.state.queued_submissions()[0].text, "third");
    }

    #[test]
    fn response_indicator_survives_while_waiting_for_assistant_finalization() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);

        run(&mut app, char_key('h'));
        run(&mut app, char_key('i'));
        run(&mut app, enter_key());
        assert!(app.state.is_streaming());

        assert!(app.state.is_streaming());

        let test_backend = TestBackend::new(80, 8);
        let mut terminal = Terminal::new(test_backend).unwrap();
        terminal.draw(|frame| app.state.render(frame)).unwrap();
        let screen = rendered_screen(&terminal);

        assert!(screen.contains("Agent is Responding"), "screen: {screen}");
    }

    #[test]
    fn queued_prompt_waits_for_assistant_activity_before_dispatching() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);

        for ch in "first".chars() {
            run(&mut app, char_key(ch));
        }
        run(&mut app, enter_key());
        assert_eq!(app.backend().prompt_calls[0].text, "first");

        for ch in "second".chars() {
            run(&mut app, char_key(ch));
        }
        run(&mut app, enter_key());
        assert_eq!(app.state.queued_submissions().len(), 1);

        run(
            &mut app,
            Event::Backend(assistant_message_updated(
                "mock-session-id",
                "msg-assistant-1",
            )),
        );

        assert!(
            app.backend().prompt_calls.len() == 2,
            "assistant finalization should dispatch queued prompt"
        );
        assert_eq!(app.backend().prompt_calls[1].text, "second");
    }

    #[test]
    fn bang_command_submits_via_command_backend() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);

        run(&mut app, char_key('!'));
        run(&mut app, char_key('p'));
        run(&mut app, char_key('w'));
        run(&mut app, char_key('d'));
        let running = run(&mut app, enter_key());

        assert!(running);
        assert_eq!(app.backend().command_calls.len(), 1);
        assert_eq!(app.backend().command_calls[0].text, "pwd");
        assert_eq!(app.backend().prompt_calls.len(), 0);
        assert_eq!(
            app.state.active_submission(),
            Some(&Submission {
                kind: SubmissionKind::Command,
                session_id: "mock-session-id".into(),
                text: "!pwd".into(),
                execution_text: "pwd".into(),
                agent: "build".into(),
            })
        );
    }

    /// Test that the real SSE event path (MessagePartUpdated + MessageUpdated) works.
    /// The SSE background task emits MessagePartUpdated (not Part), which
    /// requires is_streaming to be true.
    #[test]
    fn sse_message_part_updated_accumulates_when_streaming() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);

        // Type and send a message to enter streaming mode
        run(&mut app, char_key('h'));
        run(&mut app, char_key('i'));
        run(&mut app, enter_key());
        assert!(app.state.is_streaming(), "should be streaming after send");

        // Simulate SSE: MessagePartUpdated with a text part
        run(
            &mut app,
            Event::Backend(message_part_updated(
                "mock-session-id",
                "msg-assistant-1",
                "prt-assistant-1",
                ocpncord_backend::Part::Text(ocpncord_backend::TextPart {
                    identity: Default::default(),
                    text: "Hello from assistant".into(),
                }),
            )),
        );
        assert_eq!(
            app.state.partial_parts().len(),
            1,
            "MessagePartUpdated should push to partial_parts"
        );

        // Simulate SSE: assistant message finalized
        run(
            &mut app,
            Event::Backend(assistant_message_updated(
                "mock-session-id",
                "msg-assistant-1",
            )),
        );
        assert!(!app.state.is_streaming());
        assert_eq!(
            app.state.messages().len(),
            2,
            "user and assistant messages should be visible after finalization"
        );
        assert_eq!(
            app.state.partial_parts().len(),
            0,
            "finalized content moves out of partial_parts"
        );
    }

    #[test]
    fn duplicate_sse_part_events_do_not_duplicate_visible_assistant_text() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);

        for ch in "hello from regression test".chars() {
            run(&mut app, char_key(ch));
        }
        run(&mut app, enter_key());

        let session_id = "mock-session-id".to_string();
        let user_echo = ocpncord_backend::Part::Text(ocpncord_backend::TextPart {
            identity: Default::default(),
            text: "hello from regression test".into(),
        });
        for _ in 0..2 {
            run(
                &mut app,
                Event::Backend(message_part_updated(
                    session_id.as_str(),
                    "msg-user-echo",
                    "prt-user-echo",
                    user_echo.clone(),
                )),
            );
        }
        assert!(
            app.state.partial_parts().is_empty(),
            "echoed user text should not become assistant partials"
        );

        let step_start = ocpncord_backend::Part::StepStart(ocpncord_backend::StepStartPart {
            identity: Default::default(),
            snapshot: None,
            session_id: Some(session_id.clone()),
        });
        for _ in 0..2 {
            run(
                &mut app,
                Event::Backend(message_part_updated(
                    session_id.as_str(),
                    "msg-step-start",
                    "prt-step-start",
                    step_start.clone(),
                )),
            );
        }

        for delta in ["Hello", "! 👋", " How can I help you today?"] {
            run(
                &mut app,
                Event::Backend(ocpncord_backend::BackendEvent::MessagePartDelta {
                    session_id: session_id.clone(),
                    message_id: "msg1".into(),
                    part_id: "prt1".into(),
                    field: "text".into(),
                    delta: delta.into(),
                }),
            );
        }

        let final_text = ocpncord_backend::Part::Text(ocpncord_backend::TextPart {
            identity: Default::default(),
            text: "Hello! 👋 How can I help you today?".into(),
        });
        for _ in 0..2 {
            run(
                &mut app,
                Event::Backend(message_part_updated(
                    session_id.as_str(),
                    "msg1",
                    "prt1",
                    final_text.clone(),
                )),
            );
        }

        let tool = ocpncord_backend::Part::Tool(ocpncord_backend::ToolPart {
            identity: Default::default(),
            tool: "bash".into(),
            state: ocpncord_backend::ToolState::Pending {
                input: Default::default(),
                raw: String::new(),
            },
        });
        for _ in 0..2 {
            run(
                &mut app,
                Event::Backend(message_part_updated(
                    session_id.as_str(),
                    "msg-tool-1",
                    "prt-tool-1",
                    tool.clone(),
                )),
            );
        }

        let assistant_text_parts: Vec<&str> = app
            .state
            .partial_parts()
            .iter()
            .filter_map(|part| match part {
                ocpncord_backend::Part::Text(text) if !text.text.is_empty() => {
                    Some(text.text.as_str())
                }
                _ => None,
            })
            .collect();

        assert_eq!(
            assistant_text_parts,
            vec!["Hello! 👋 How can I help you today?"]
        );
        assert_eq!(
            app.state
                .partial_parts()
                .iter()
                .filter(|part| matches!(part, ocpncord_backend::Part::Tool(_)))
                .count(),
            1,
            "duplicate tool updates should be collapsed"
        );
        assert_eq!(
            app.state.messages().len(),
            1,
            "only the user message is committed"
        );
    }

    /// Test that MessagePartDelta accumulates text during streaming.
    #[test]
    fn sse_message_part_delta_accumulates_text_when_streaming() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);

        // Send message to enter streaming mode
        run(&mut app, char_key('h'));
        run(&mut app, char_key('i'));
        run(&mut app, enter_key());
        assert!(app.state.is_streaming());

        // Simulate SSE: MessagePartDelta with text chunks (no initial MessagePartUpdated)
        run(
            &mut app,
            Event::Backend(ocpncord_backend::BackendEvent::MessagePartDelta {
                session_id: "mock-session-id".into(),
                message_id: "msg1".into(),
                part_id: "prt1".into(),
                field: "text".into(),
                delta: "Hello ".into(),
            }),
        );
        assert_eq!(
            app.state.partial_parts().len(),
            1,
            "first delta should create a text part"
        );

        run(
            &mut app,
            Event::Backend(ocpncord_backend::BackendEvent::MessagePartDelta {
                session_id: "mock-session-id".into(),
                message_id: "msg1".into(),
                part_id: "prt1".into(),
                field: "text".into(),
                delta: "world".into(),
            }),
        );
        assert_eq!(
            app.state.partial_parts().len(),
            1,
            "second delta should update existing text part, not add a new one"
        );

        // Verify the accumulated text
        match &app.state.partial_parts()[0] {
            ocpncord_backend::Part::Text(tp) => {
                assert_eq!(tp.text, "Hello world");
            }
            _ => panic!("expected text part"),
        }

        // Finalize
        run(
            &mut app,
            Event::Backend(assistant_message_updated(
                "mock-session-id",
                "msg-assistant-1",
            )),
        );
        assert!(!app.state.is_streaming());
        assert_eq!(
            app.state.messages().len(),
            2,
            "user and assistant messages should be visible after finalization"
        );
        assert_eq!(
            app.state.partial_parts().len(),
            0,
            "finalized content moves out of partial_parts"
        );
    }

    #[test]
    fn reasoning_delta_updates_reasoning_part_in_place() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);

        run(&mut app, char_key('h'));
        run(&mut app, char_key('i'));
        run(&mut app, enter_key());

        run(
            &mut app,
            Event::Backend(message_part_updated(
                "mock-session-id",
                "msg1",
                "prt_reasoning",
                ocpncord_backend::Part::Reasoning(ocpncord_backend::ReasoningPart {
                    identity: Default::default(),
                    text: String::new(),
                }),
            )),
        );
        run(
            &mut app,
            Event::Backend(ocpncord_backend::BackendEvent::MessagePartDelta {
                session_id: "mock-session-id".into(),
                message_id: "msg1".into(),
                part_id: "prt_reasoning".into(),
                field: "text".into(),
                delta: "thinking".into(),
            }),
        );

        assert_eq!(app.state.partial_parts().len(), 1);
        match &app.state.partial_parts()[0] {
            ocpncord_backend::Part::Reasoning(reasoning) => {
                assert_eq!(reasoning.text, "thinking");
            }
            other => panic!("expected reasoning part, got {other:?}"),
        }

        let final_reasoning = ocpncord_backend::Part::Reasoning(ocpncord_backend::ReasoningPart {
            identity: Default::default(),
            text: "thinking done".into(),
        });
        run(
            &mut app,
            Event::Backend(message_part_updated(
                "mock-session-id",
                "msg1",
                "prt_reasoning",
                final_reasoning,
            )),
        );

        assert_eq!(
            app.state.partial_parts().len(),
            1,
            "final reasoning update should replace the streaming placeholder"
        );
        match &app.state.partial_parts()[0] {
            ocpncord_backend::Part::Reasoning(reasoning) => {
                assert_eq!(reasoning.text, "thinking done");
            }
            other => panic!("expected reasoning part, got {other:?}"),
        }
    }

    /// Test that MessagePartUpdated is IGNORED when NOT streaming.
    #[test]
    fn sse_message_part_updated_ignored_when_not_streaming() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);

        // NOT streaming — just sitting on the start page
        assert!(!app.state.is_streaming());

        run(
            &mut app,
            Event::Backend(message_part_updated(
                "ses1",
                "msg-stale",
                "prt-stale",
                ocpncord_backend::Part::Text(ocpncord_backend::TextPart {
                    identity: Default::default(),
                    text: "Stale event".into(),
                }),
            )),
        );
        assert_eq!(
            app.state.partial_parts().len(),
            0,
            "parts should NOT accumulate when not streaming"
        );
    }

    #[test]
    fn backend_error_during_stream_shows_error_and_clears_stream() {
        let mut backend = MockBackend::default();
        backend.live_events = vec![live_event(ocpncord_backend::BackendEvent::Error {
            message: "connection lost".into(),
        })];
        let mut app = new_app(backend);
        init(&mut app);

        run(&mut app, char_key('h'));
        run(&mut app, enter_key());
        assert!(app.state.is_streaming());

        run(
            &mut app,
            Event::Backend(ocpncord_backend::BackendEvent::Error {
                message: "connection lost".into(),
            }),
        );
        assert!(!app.state.is_streaming());
        assert!(app.state.error().unwrap_or("").contains("connection lost"));
    }

    #[test]
    fn session_creation_error_shows_error_and_stays_on_start_page() {
        let mut backend = MockBackend::default();
        backend.fail_create_session = Some(ocpncord_backend::BackendError::Api {
            status: 500,
            message: "server error".into(),
        });
        let mut app = new_app(backend);

        run(&mut app, char_key('h'));
        let running = run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::Enter,
                modifiers: Modifiers::default(),
            }),
        );
        assert!(running);
        assert_eq!(
            app.state.active_mode(),
            AppMode::StartPage,
            "should stay on StartPage on error"
        );
        assert!(
            app.state.error().unwrap_or("").contains("server error"),
            "error should contain failure message"
        );
    }

    #[test]
    fn typing_enter_creates_session_and_switches_to_chat() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);
        assert_eq!(app.state.active_mode(), AppMode::StartPage);

        run(&mut app, char_key('h'));
        run(&mut app, char_key('i'));
        let running = run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::Enter,
                modifiers: Modifiers::default(),
            }),
        );
        assert!(running);
        assert_eq!(
            app.backend().sessions.len(),
            1,
            "a session should have been created"
        );
        assert_eq!(app.state.active_mode(), AppMode::Chat);
        assert_eq!(app.state.draft(), Some("hi"));
    }

    #[test]
    fn enter_on_empty_input_does_nothing() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);

        let running = run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::Enter,
                modifiers: Modifiers::default(),
            }),
        );
        assert!(running);

        assert_eq!(app.state.active_mode(), AppMode::StartPage);
        assert_eq!(app.backend().sessions.len(), 0);
    }

    fn make_session(id: &str, title: &str) -> ocpncord_backend::Session {
        ocpncord_backend::Session {
            id: id.into(),
            title: title.into(),
            project_id: "p1".into(),
            directory: "/".into(),
            path: None,
            parent_id: None,
            time: ocpncord_backend::SessionTime {
                created: 0,
                updated: 0,
                compacting: None,
                archived: None,
            },
            slug: String::new(),
            version: String::new(),
            workspace_id: None,
            summary: None,
            cost: None,
            tokens: None,
            share: None,
            agent: None,
            model: None,
            permission: None,
            revert: None,
        }
    }

    #[test]
    fn session_list_shows_sessions_from_backend() {
        let mut backend = MockBackend::default();
        backend.sessions = vec![
            make_session("s1", "First session"),
            make_session("s2", "Second session"),
        ];
        let mut app = new_app(backend);

        run(&mut app, ctrl('x'));
        run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::Char('l'),
                modifiers: Modifiers::default(),
            }),
        );

        let test_backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(test_backend).unwrap();
        terminal.draw(|frame| app.state.render(frame)).unwrap();
        let buf = terminal.backend().buffer();
        let has_session_1 = buf.content().iter().any(|c| c.symbol() == "F");
        assert!(
            has_session_1,
            "First session title should appear in rendered output"
        );
    }

    #[test]
    fn session_list_shows_empty_state() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);

        run(&mut app, ctrl('x'));
        run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::Char('l'),
                modifiers: Modifiers::default(),
            }),
        );

        let test_backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(test_backend).unwrap();
        terminal.draw(|frame| app.state.render(frame)).unwrap();
        let buf = terminal.backend().buffer();
        let has_empty_msg = buf.content().iter().any(|c| c.symbol() == "N");
        assert!(has_empty_msg, "Empty state should show 'No sessions yet'");
    }

    #[test]
    fn ctrl_x_l_opens_session_list_modal() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);

        run(&mut app, ctrl('x'));
        run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::Char('l'),
                modifiers: Modifiers::default(),
            }),
        );

        assert!(
            app.state.active_modal().is_some(),
            "Ctrl+X L should open session list modal"
        );
    }

    #[test]
    fn escape_closes_active_modal() {
        use crate::modal::Modal;
        use ratatui::Frame;

        struct TestCloseModal;

        impl Modal for TestCloseModal {
            fn render(&self, _frame: &mut Frame, _theme: &Theme, _area: Rect) {}
            fn handle_event(&mut self, _event: Event) -> Action {
                Action::None
            }
            fn title(&self) -> &str {
                "Test"
            }
        }

        let mut app = new_app(MockBackend::default());
        app.state.set_active_modal(Box::new(TestCloseModal));
        assert!(
            app.state.active_modal().is_some(),
            "modal should be active after set"
        );

        run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::Escape,
                modifiers: Modifiers::default(),
            }),
        );
        assert!(
            app.state.active_modal().is_none(),
            "Escape should close the modal"
        );
    }

    #[test]
    fn slash_sessions_opens_modal() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);

        run(&mut app, char_key('/'));
        run(&mut app, char_key('s'));
        run(&mut app, char_key('e'));
        run(&mut app, char_key('s'));
        run(&mut app, char_key('s'));
        run(&mut app, char_key('i'));
        run(&mut app, char_key('o'));
        run(&mut app, char_key('n'));
        run(&mut app, char_key('s'));
        run(&mut app, enter_key());

        assert!(
            app.state.active_modal().is_some(),
            "/sessions should open the session list modal"
        );
    }

    #[test]
    fn slash_server_opens_server_config_modal() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);

        for ch in "/server".chars() {
            run(&mut app, char_key(ch));
        }
        run(&mut app, enter_key());

        assert_eq!(
            app.state.active_modal().map(|modal| modal.title()),
            Some("Server Connection")
        );
    }

    #[test]
    fn slash_display_opens_display_config_modal() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);

        for ch in "/display".chars() {
            run(&mut app, char_key(ch));
        }
        run(&mut app, enter_key());

        assert_eq!(
            app.state.active_modal().map(|modal| modal.title()),
            Some("Display")
        );
    }

    #[test]
    fn display_policy_changes_current_rendering() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);
        app.state.active_mode = AppMode::Chat;
        app.state.chat.push_message(LoadedMessage {
            id: Some("msg-file".into()),
            session_id: Some("session".into()),
            role: ocpncord_backend::MessageRole::Assistant,
            parts: vec![ocpncord_backend::Part::File(ocpncord_backend::FilePart {
                identity: Default::default(),
                mime: "text/plain".into(),
                url: "file:///tmp/report.txt".into(),
                filename: Some("report.txt".into()),
            })],
        });

        app.state.apply_action(Some(Action::SetDisplayMode(
            PartKind::File,
            PartDisplayMode::Hidden,
        )));
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| app.state.render(frame)).unwrap();
        let screen = rendered_screen(&terminal);

        assert!(!screen.contains("report.txt"), "screen: {screen}");
    }

    #[test]
    fn applying_server_url_resets_state_and_reconnects() {
        let mut backend = MockBackend::default();
        backend.server_url = "http://old:4096".into();
        let mut app = new_app(backend);
        app.state.active_session = Some(make_session("old-session", "Old"));
        app.state
            .sync_known_sequences
            .insert("old-session".into(), 10);
        app.state
            .set_active_modal(Box::new(ServerConfigModal::new("http://new:4096".into())));

        app.state
            .apply_action(Some(Action::ApplyServerUrl("http://new:4096/".into())));
        futures::executor::block_on(app.drain_backend_ops_for_test());

        assert_eq!(app.backend().server_url, "http://new:4096");
        assert_eq!(
            app.backend().set_server_url_calls,
            vec!["http://new:4096".to_string()]
        );
        assert!(app.state.active_modal().is_none());
        assert!(app.state.active_session.is_none());
        assert!(app.state.sync_known_sequences.is_empty());
        assert!(
            !app.backend().sync_history_requests.is_empty(),
            "server switch should queue startup reconnect work"
        );
    }

    #[test]
    fn applying_invalid_server_url_keeps_modal_and_existing_backend() {
        let mut backend = MockBackend::default();
        backend.server_url = "http://old:4096".into();
        backend.health_status = None;
        let mut app = new_app(backend);
        app.state
            .set_active_modal(Box::new(ServerConfigModal::new("http://bad:4096".into())));

        app.state
            .apply_action(Some(Action::ApplyServerUrl("http://bad:4096".into())));
        futures::executor::block_on(app.drain_backend_ops_for_test());

        assert_eq!(app.backend().server_url, "http://old:4096");
        assert!(app.backend().set_server_url_calls.is_empty());
        assert_eq!(
            app.state.active_modal().map(|modal| modal.title()),
            Some("Server Connection")
        );
    }

    #[test]
    fn unknown_slash_command_submits_as_message() {
        let mut backend = MockBackend::default();
        backend.live_events = vec![live_event(assistant_message_updated(
            "mock-session-id",
            "msg-assistant-1",
        ))];
        let mut app = new_app(backend);
        init(&mut app);

        run(&mut app, char_key('/'));
        run(&mut app, char_key('u'));
        run(&mut app, char_key('n'));
        run(&mut app, char_key('k'));
        run(&mut app, char_key('n'));
        run(&mut app, char_key('o'));
        run(&mut app, char_key('w'));
        run(&mut app, char_key('n'));
        run(&mut app, enter_key());

        assert_eq!(
            app.state.active_mode(),
            AppMode::Chat,
            "unknown command should transition to chat"
        );
        assert!(
            app.state.messages().len() > 0,
            "unknown command should add a message"
        );
    }

    #[test]
    fn new_command_creates_session_and_stays_on_chat() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);

        run(&mut app, char_key('h'));
        run(&mut app, char_key('i'));
        run(&mut app, enter_key());
        assert_eq!(app.state.active_mode(), AppMode::Chat);

        // Complete the stream so input is accepted again
        run(
            &mut app,
            Event::Backend(assistant_message_updated(
                "mock-session-id",
                "msg-assistant-1",
            )),
        );

        let session_count_before = app.backend().sessions.len();

        run(&mut app, char_key('/'));
        run(&mut app, char_key('n'));
        run(&mut app, char_key('e'));
        run(&mut app, char_key('w'));
        run(&mut app, enter_key());

        assert_eq!(app.state.active_mode(), AppMode::Chat);
        assert_eq!(
            app.backend().sessions.len(),
            session_count_before + 1,
            "/new should create a new session"
        );
    }

    #[test]
    fn slash_models_opens_modal() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);

        run(&mut app, char_key('/'));
        run(&mut app, char_key('m'));
        run(&mut app, char_key('o'));
        run(&mut app, char_key('d'));
        run(&mut app, char_key('e'));
        run(&mut app, char_key('l'));
        run(&mut app, char_key('s'));
        run(&mut app, enter_key());

        assert!(
            app.state.active_modal().is_some(),
            "/models should open the model picker modal"
        );
    }

    #[test]
    fn slash_help_opens_modal() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);

        run(&mut app, char_key('/'));
        run(&mut app, char_key('h'));
        run(&mut app, char_key('e'));
        run(&mut app, char_key('l'));
        run(&mut app, char_key('p'));
        run(&mut app, enter_key());

        assert!(
            app.state.active_modal().is_some(),
            "/help should open the help modal"
        );
    }

    #[test]
    fn slash_todos_selects_todos_panel_and_clears_prompt() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);

        for ch in "/todos".chars() {
            run(&mut app, char_key(ch));
        }
        run(&mut app, enter_key());

        assert!(
            app.state.side_panel_visible,
            "/todos should open the side panel"
        );
        assert_eq!(app.state.side_panel_tab, Tab::Todos);
        assert_eq!(
            app.state.prompt_text(),
            "",
            "/todos should clear the prompt"
        );
    }

    #[test]
    fn ctrl_x_o_toggles_todos_panel() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);

        run(&mut app, ctrl('x'));
        run(&mut app, char_key('o'));
        assert!(app.state.side_panel_visible);
        assert_eq!(app.state.side_panel_tab, Tab::Todos);

        run(&mut app, ctrl('x'));
        run(&mut app, char_key('o'));
        assert!(!app.state.side_panel_visible);
        assert_eq!(app.state.side_panel_tab, Tab::Todos);
    }

    #[test]
    fn ctrl_x_d_selects_diagnostics_when_todos_panel_is_open() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);

        run(&mut app, ctrl('x'));
        run(&mut app, char_key('o'));
        assert_eq!(app.state.side_panel_tab, Tab::Todos);

        run(&mut app, ctrl('x'));
        run(&mut app, char_key('d'));
        assert!(app.state.side_panel_visible);
        assert_eq!(app.state.side_panel_tab, Tab::Diagnostics);
    }

    #[test]
    fn ctrl_x_m_opens_model_picker_modal() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);

        run(&mut app, ctrl('x'));
        run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::Char('m'),
                modifiers: Modifiers::default(),
            }),
        );

        assert!(
            app.state.active_modal().is_some(),
            "Ctrl+X M should open the model picker modal"
        );
    }

    #[test]
    fn model_picker_reuses_cached_model_list() {
        let mut backend = MockBackend::default();
        backend.models = Some(vec![ocpncord_backend::ModelSummary {
            id: "claude-sonnet".into(),
            provider_id: "anthropic".into(),
            name: Some("Claude Sonnet".into()),
            ..Default::default()
        }]);
        let mut app = new_app(backend);

        run(&mut app, ctrl('x'));
        run(&mut app, char_key('m'));
        assert_eq!(app.backend().list_models_calls, 1);

        run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::Escape,
                modifiers: Modifiers::default(),
            }),
        );
        run(&mut app, ctrl('x'));
        run(&mut app, char_key('m'));

        assert_eq!(app.backend().list_models_calls, 1);
    }

    #[test]
    fn model_picker_reports_model_catalog_errors() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);

        run(&mut app, ctrl('x'));
        run(&mut app, char_key('m'));

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| app.state.render(frame)).unwrap();
        let screen = rendered_screen(&terminal);

        assert!(
            screen.contains("no model catalog stub"),
            "model catalog errors should be visible. Screen: {}",
            screen
        );
        assert!(
            !screen.contains("No models found in server config"),
            "model catalog errors should not be hidden behind the empty state. Screen: {}",
            screen
        );
    }

    #[test]
    fn select_model_updates_backend_config() {
        let mut backend = MockBackend::default();
        backend.config_info = Some(ocpncord_backend::Config {
            model: Some("openrouter/old".into()),
            username: Some("mock-user".into()),
            provider: alloc::collections::BTreeMap::from([(
                "openrouter".into(),
                ocpncord_backend::ProviderConfig {
                    name: Some("OpenRouter".into()),
                    models: alloc::collections::BTreeMap::from([(
                        "new".into(),
                        ocpncord_backend::ModelConfig {
                            name: Some("New Model".into()),
                            ..Default::default()
                        },
                    )]),
                    ..Default::default()
                },
            )]),
            agent: Default::default(),
        });
        let mut app = new_app(backend);

        run(&mut app, ctrl('x'));
        run(&mut app, char_key('m'));
        run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::Enter,
                modifiers: Modifiers::default(),
            }),
        );

        assert_eq!(
            app.backend()
                .config_info
                .as_ref()
                .and_then(|cfg| cfg.model.as_deref()),
            Some("openrouter/new")
        );
    }

    #[test]
    fn selecting_model_keeps_populated_picker_while_request_is_pending() {
        let mut backend = MockBackend::default();
        backend.models = Some(vec![ocpncord_backend::ModelSummary {
            id: "claude-sonnet".into(),
            provider_id: "anthropic".into(),
            name: Some("Claude Sonnet".into()),
            ..Default::default()
        }]);
        let mut app = new_app(backend);

        run(&mut app, ctrl('x'));
        run(&mut app, char_key('m'));

        let running = app.state.handle_event(enter_key());
        assert!(running);

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| app.state.render(frame)).unwrap();
        let screen = rendered_screen(&terminal);

        assert!(
            screen.contains("Claude Sonnet"),
            "pending selection should leave the populated picker mounted. Screen: {screen}"
        );
        assert!(
            !screen.contains("No models found in server config"),
            "pending selection should not flash an empty picker. Screen: {screen}"
        );
    }

    #[test]
    fn selecting_model_preserves_picker_search_after_config_update() {
        let mut backend = MockBackend::default();
        backend.models = Some(vec![
            ocpncord_backend::ModelSummary {
                id: "claude-sonnet".into(),
                provider_id: "anthropic".into(),
                name: Some("Claude Sonnet".into()),
                ..Default::default()
            },
            ocpncord_backend::ModelSummary {
                id: "qwen3.6".into(),
                provider_id: "qwen".into(),
                name: Some("Qwen3.6".into()),
                ..Default::default()
            },
        ]);
        let mut app = new_app(backend);

        run(&mut app, ctrl('x'));
        run(&mut app, char_key('m'));
        for ch in ['q', 'w', 'e', 'n'] {
            run(&mut app, char_key(ch));
        }

        app.state.handle_event(enter_key());
        futures::executor::block_on(app.drain_backend_ops_for_test());

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| app.state.render(frame)).unwrap();
        let screen = rendered_screen(&terminal);

        assert!(
            screen.contains("Search: qwen"),
            "selection should preserve the picker search instead of rebuilding it. Screen: {screen}"
        );
        assert!(
            screen.contains("* Qwen3.6"),
            "selection should update the current marker in the existing filtered list. Screen: {screen}"
        );
        assert!(
            !screen.contains("Claude Sonnet"),
            "selection should not reset the filtered list. Screen: {screen}"
        );
    }

    #[test]
    fn ctrl_x_h_opens_help_modal() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);

        run(&mut app, ctrl('x'));
        run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::Char('h'),
                modifiers: Modifiers::default(),
            }),
        );

        assert!(
            app.state.active_modal().is_some(),
            "Ctrl+X H should open the help modal"
        );
    }

    #[test]
    fn escape_closes_help_modal() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);

        run(&mut app, ctrl('x'));
        run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::Char('h'),
                modifiers: Modifiers::default(),
            }),
        );
        assert!(
            app.state.active_modal().is_some(),
            "help modal should be open"
        );

        run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::Escape,
                modifiers: Modifiers::default(),
            }),
        );
        assert!(
            app.state.active_modal().is_none(),
            "Escape should close the help modal"
        );
    }

    #[test]
    fn modal_overlay_preserves_background_outside_modal() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);

        // Open help modal on start page so logo gets drawn under the overlay
        run(&mut app, ctrl('x'));
        run(&mut app, char_key('h'));
        assert!(
            app.state.active_modal().is_some(),
            "help modal should be open"
        );

        let test_backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(test_backend).unwrap();
        terminal.draw(|frame| app.state.render(frame)).unwrap();
        let buf = terminal.backend().buffer();

        let frame_area = Rect::new(0, 0, 80, 24);
        let modal = app
            .state
            .active_modal()
            .expect("help modal should still be active");
        let (modal_width, modal_height) = modal.preferred_size(frame_area);
        let modal_area = Rect::new(
            frame_area.x + (frame_area.width.saturating_sub(modal_width)) / 2,
            frame_area.y + (frame_area.height.saturating_sub(modal_height)) / 2,
            modal_width,
            modal_height,
        );
        let content_area = Block::bordered()
            .border_type(BorderType::Rounded)
            .inner(modal_area);
        let mut has_logo_block_inside_modal = false;
        let mut has_logo_block_outside_modal = false;
        let mut has_theme_bg_inside_modal = false;
        for y in frame_area.top()..frame_area.bottom() {
            for x in frame_area.left()..frame_area.right() {
                let cell = buf.cell((x, y));
                let symbol = cell.map(|c| c.symbol());
                if symbol == Some("█") {
                    if x >= modal_area.left()
                        && x < modal_area.right()
                        && y >= modal_area.top()
                        && y < modal_area.bottom()
                    {
                        has_logo_block_inside_modal = true;
                    } else {
                        has_logo_block_outside_modal = true;
                    }
                }
                if x >= content_area.left()
                    && x < content_area.right()
                    && y >= content_area.top()
                    && y < content_area.bottom()
                    && cell.is_some_and(|c| c.style().bg == app.state.theme.bg.bg)
                {
                    has_theme_bg_inside_modal = true;
                }
            }
        }
        assert!(
            !has_logo_block_inside_modal,
            "modal area should not contain logo block characters"
        );
        assert!(
            has_logo_block_outside_modal,
            "background screen should remain visible outside the modal"
        );
        assert!(
            !has_theme_bg_inside_modal,
            "modal body should use the default app background, not theme.bg"
        );

        // Modal text should still render correctly
        let has_slash = buf.content().iter().any(|c| c.symbol() == "/");
        assert!(has_slash, "modal should render slash commands");
    }

    #[test]
    fn ctrl_x_leader_does_not_leak_to_prompt_bar() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);

        // Type "hello" into the prompt bar
        run(&mut app, char_key('h'));
        run(&mut app, char_key('e'));
        run(&mut app, char_key('l'));
        run(&mut app, char_key('l'));
        run(&mut app, char_key('o'));
        assert_eq!(app.state.prompt_text(), "hello", "should have typed hello");

        // Press Ctrl+X (leader key) — this must NOT leak 'x' to the prompt bar
        run(&mut app, ctrl('x'));
        assert_eq!(
            app.state.prompt_text(),
            "hello",
            "ctrl+x should not leak 'x' to prompt bar"
        );

        // Complete the leader chord with 'h' for help
        run(&mut app, char_key('h'));
        assert!(
            app.state.active_modal().is_some(),
            "help modal should open after ctrl+x h"
        );

        // Close the modal
        run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::Escape,
                modifiers: Modifiers::default(),
            }),
        );

        // Prompt bar should still show "hello" — unchanged by modal lifecycle
        assert_eq!(
            app.state.prompt_text(),
            "hello",
            "prompt bar should preserve input through modal lifecycle"
        );
    }

    #[test]
    fn ctrl_x_t_toggles_terminal_screen() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);
        assert_eq!(app.state.active_mode(), AppMode::StartPage);

        // Ctrl+X T on StartPage → Terminal
        run(&mut app, ctrl('x'));
        run(&mut app, char_key('t'));
        assert_eq!(
            app.state.active_mode(),
            AppMode::Terminal,
            "ctrl+x t should switch to terminal"
        );

        // Ctrl+X T again → back to StartPage
        run(&mut app, ctrl('x'));
        run(&mut app, char_key('t'));
        assert_eq!(
            app.state.active_mode(),
            AppMode::StartPage,
            "ctrl+x t again should return to start page"
        );
    }

    #[test]
    fn terminal_screen_plain_keys_do_not_mutate_prompt() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);

        run(&mut app, ctrl('x'));
        run(&mut app, char_key('t'));
        assert_eq!(app.state.active_mode(), AppMode::Terminal);

        run(&mut app, char_key('a'));
        run(&mut app, char_key('b'));
        assert_eq!(app.state.prompt_text(), "");
    }

    #[test]
    fn terminal_screen_scrolls_from_bottom_with_arrow_and_page_keys() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);
        app.state.terminal.pty_id = Some("pty-1".into());
        app.state.terminal.command = "sh".into();
        for idx in 0..12 {
            push_terminal_line(
                &mut app.state.terminal,
                TermLine {
                    content: alloc::format!("line-{idx}"),
                    is_error: false,
                },
            );
        }

        run(&mut app, ctrl('x'));
        run(&mut app, char_key('t'));
        assert_eq!(app.state.active_mode(), AppMode::Terminal);

        let test_backend = TestBackend::new(40, 6);
        let mut terminal = Terminal::new(test_backend).unwrap();
        terminal.draw(|frame| app.state.render(frame)).unwrap();
        let screen: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(screen.contains("line-11"), "screen: {screen}");
        assert!(!screen.contains("line-0"), "screen: {screen}");

        run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::Up,
                modifiers: Modifiers::default(),
            }),
        );
        assert_eq!(app.state.terminal.scroll, 1);

        run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::PageUp,
                modifiers: Modifiers::default(),
            }),
        );
        assert_eq!(app.state.terminal.scroll, 5);

        run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::PageDown,
                modifiers: Modifiers::default(),
            }),
        );
        assert_eq!(app.state.terminal.scroll, 1);
    }

    #[test]
    fn terminal_screen_scroll_offset_changes_visible_output() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);
        app.state.terminal.pty_id = Some("pty-1".into());
        app.state.terminal.command = "sh".into();
        for idx in 0..12 {
            push_terminal_line(
                &mut app.state.terminal,
                TermLine {
                    content: alloc::format!("line-{idx}"),
                    is_error: false,
                },
            );
        }
        app.state.terminal.scroll = 3;

        run(&mut app, ctrl('x'));
        run(&mut app, char_key('t'));

        let test_backend = TestBackend::new(40, 6);
        let mut terminal = Terminal::new(test_backend).unwrap();
        terminal.draw(|frame| app.state.render(frame)).unwrap();
        let screen = rendered_screen(&terminal);

        assert!(screen.contains("line-5"), "screen: {screen}");
        assert!(screen.contains("line-8"), "screen: {screen}");
        assert!(!screen.contains("line-11"), "screen: {screen}");
    }

    #[test]
    fn diagnostics_panel_renders_table_columns() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);
        app.state.side_panel_visible = true;
        app.state.side_panel_tab = Tab::Diagnostics;
        app.state.lsp_diagnostics.insert(
            "src/main.rs".into(),
            vec![LspDiagnostic {
                message: "missing semicolon".into(),
                severity: Some("error".into()),
                line: 12,
                character: 4,
            }],
        );

        let test_backend = TestBackend::new(140, 12);
        let mut terminal = Terminal::new(test_backend).unwrap();
        terminal.draw(|frame| app.state.render(frame)).unwrap();
        let screen = rendered_screen(&terminal);

        assert!(screen.contains("src/main.rs"), "screen: {screen}");
        assert!(screen.contains("error"), "screen: {screen}");
        assert!(screen.contains("12:4"), "screen: {screen}");
        assert!(screen.contains("missing semicolon"), "screen: {screen}");
    }

    #[test]
    fn todos_panel_renders_list_items_with_status_styles() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);
        app.state.side_panel_visible = true;
        app.state.side_panel_tab = Tab::Todos;
        app.state.todos = vec![
            ocpncord_backend::Todo {
                content: "done task".into(),
                status: "completed".into(),
                priority: "normal".into(),
            },
            ocpncord_backend::Todo {
                content: "active task".into(),
                status: "pending".into(),
                priority: "normal".into(),
            },
        ];

        let test_backend = TestBackend::new(80, 8);
        let mut terminal = Terminal::new(test_backend).unwrap();
        terminal.draw(|frame| app.state.render(frame)).unwrap();
        let screen = rendered_screen(&terminal);

        assert!(screen.contains("[x] done task"), "screen: {screen}");
        assert!(screen.contains("[ ] active task"), "screen: {screen}");

        let buf = terminal.backend().buffer();
        let panel_x = 57;
        assert_eq!(
            buf[(panel_x, 2)].style().fg,
            app.state.theme.text_dim.fg,
            "completed todo should use dim style"
        );
        assert_eq!(
            buf[(panel_x, 3)].style().fg,
            app.state.theme.text.fg,
            "active todo should use normal text style"
        );
    }

    #[test]
    fn terminal_side_panel_uses_terminal_scroll_offset() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);
        app.state.side_panel_visible = true;
        app.state.side_panel_tab = Tab::Pane;
        app.state.terminal.pty_id = Some("pty-1".into());
        app.state.terminal.command = "sh".into();
        for idx in 0..10 {
            push_terminal_line(
                &mut app.state.terminal,
                TermLine {
                    content: alloc::format!("line-{idx}"),
                    is_error: false,
                },
            );
        }
        app.state.terminal.scroll = 2;

        let test_backend = TestBackend::new(80, 6);
        let mut terminal = Terminal::new(test_backend).unwrap();
        terminal.draw(|frame| app.state.render(frame)).unwrap();
        let screen = rendered_screen(&terminal);

        assert!(screen.contains("line-5"), "screen: {screen}");
        assert!(screen.contains("line-7"), "screen: {screen}");
        assert!(!screen.contains("line-2"), "screen: {screen}");
        assert!(!screen.contains("line-9"), "screen: {screen}");
    }

    #[test]
    fn prompt_row_stays_visible_at_narrow_widths() {
        let backend = MockBackend::default();
        let app = new_app(backend);

        let test_backend = TestBackend::new(24, 6);
        let mut terminal = Terminal::new(test_backend).unwrap();
        terminal.draw(|frame| app.state.render(frame)).unwrap();
        let screen = rendered_screen(&terminal);

        assert!(screen.contains(">"), "screen: {screen}");
    }

    #[test]
    fn escape_clears_prompt_when_no_modal_is_open() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);

        run(&mut app, char_key('h'));
        run(&mut app, char_key('i'));
        assert_eq!(app.state.prompt_text(), "hi");

        run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::Escape,
                modifiers: Modifiers::default(),
            }),
        );
        assert_eq!(app.state.prompt_text(), "");
    }

    #[test]
    fn command_palette_enter_closes_palette_before_applying_action() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);

        run(&mut app, ctrl('p'));
        assert_eq!(
            app.state.active_modal().map(|modal| modal.title()),
            Some("Command Palette")
        );

        run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::Enter,
                modifiers: Modifiers::default(),
            }),
        );
        assert_eq!(
            app.state.active_modal().map(|modal| modal.title()),
            Some("Help")
        );
    }

    #[test]
    fn terminal_screen_shows_message_when_no_pty() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);

        // Switch to terminal
        run(&mut app, ctrl('x'));
        run(&mut app, char_key('t'));
        assert_eq!(app.state.active_mode(), AppMode::Terminal);

        // Render and verify helpful message
        let test_backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(test_backend).unwrap();
        terminal.draw(|frame| app.state.render(frame)).unwrap();
        let screen = rendered_screen(&terminal);
        let has_help = screen.contains("No active terminal");
        assert!(
            has_help,
            "terminal should show a helpful message when no PTY"
        );
        assert!(
            !screen.contains("send a prompt"),
            "terminal empty state should not imply every prompt creates a PTY: {screen}"
        );
    }

    #[test]
    fn side_panel_clears_background_chat_symbols() {
        let backend = MockBackend::default();
        let mut app = new_app(backend);

        // Toggle side panel on (while on start page, which has the logo)
        run(&mut app, ctrl('x'));
        run(&mut app, char_key('d'));
        assert!(app.state.side_panel_visible, "side panel should be visible");

        // Render and check panel area
        let test_backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(test_backend).unwrap();
        terminal.draw(|frame| app.state.render(frame)).unwrap();
        let buf = terminal.backend().buffer();

        // The side panel occupies the right 30%. For 80-wide: x=56..79.
        let panel_x = 56u16;
        let has_block_in_panel = buf.content().iter().enumerate().any(|(i, c)| {
            if c.symbol() == "█" {
                let pos = buf.pos_of(i);
                pos.0 >= panel_x
            } else {
                false
            }
        });
        assert!(
            !has_block_in_panel,
            "side panel area should not contain logo block characters"
        );

        // Left side (chat area) should have content
        let has_prompt = buf.content().iter().any(|c| c.symbol() == ">");
        assert!(has_prompt, "start page prompt should be visible");
    }

    #[test]
    fn ctrl_c_during_streaming_interrupts_not_quits() {
        let mut backend = MockBackend::default();
        backend.live_events = vec![
            live_event(message_part_updated(
                "mock-session-id",
                "msg-assistant-1",
                "prt-assistant-1",
                ocpncord_backend::Part::Text(ocpncord_backend::TextPart {
                    identity: Default::default(),
                    text: "streaming".into(),
                }),
            )),
            live_event(assistant_message_updated(
                "mock-session-id",
                "msg-assistant-1",
            )),
        ];
        let mut app = new_app(backend);
        init(&mut app);

        run(&mut app, char_key('h'));
        run(&mut app, enter_key());
        assert!(app.state.is_streaming(), "should be streaming after send");

        let running = run(&mut app, ctrl('c'));
        assert!(
            running,
            "ctrl+c during streaming should interrupt, not quit"
        );
        assert!(!app.state.is_streaming(), "streaming should be stopped");
    }

    fn permission_request(id: &str) -> ocpncord_backend::PermissionRequest {
        ocpncord_backend::PermissionRequest {
            id: id.into(),
            session_id: "session-1".into(),
            permission: "bash".into(),
            patterns: vec!["/tmp/**".into()],
            metadata: Default::default(),
            always: Vec::new(),
            tool: None,
        }
    }

    fn question_request(id: &str) -> ocpncord_backend::QuestionRequest {
        ocpncord_backend::QuestionRequest {
            id: id.into(),
            session_id: "session-1".into(),
            questions: vec![ocpncord_backend::QuestionInfo {
                header: "Confirm".into(),
                question: "Continue?".into(),
                options: vec![
                    ocpncord_backend::QuestionOption {
                        label: "Yes".into(),
                        description: "continue".into(),
                    },
                    ocpncord_backend::QuestionOption {
                        label: "No".into(),
                        description: "stop".into(),
                    },
                ],
                multiple: false,
                custom: false,
            }],
            tool: None,
        }
    }

    #[test]
    fn toast_renderer_shows_newest_first_and_caps_visible_count() {
        let mut app = new_app(MockBackend::default());
        for idx in 0..6 {
            app.state.push_toast(Toast {
                title: Some(alloc::format!("Toast{idx}")),
                message: alloc::format!("message-{idx}"),
                variant: ToastVariant::Info,
                created_at: 0,
                duration: 10,
            });
        }

        let test_backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(test_backend).unwrap();
        terminal
            .draw(|frame| app.state.render_toasts(frame, Rect::new(0, 0, 80, 10)))
            .unwrap();
        let screen = rendered_screen(&terminal);

        assert!(screen.contains("Toast5"), "screen: {screen}");
        assert!(screen.contains("Toast1"), "screen: {screen}");
        assert!(!screen.contains("Toast0"), "screen: {screen}");
        assert!(
            screen.find("Toast5").unwrap() < screen.find("Toast4").unwrap(),
            "newest toast should render first: {screen}"
        );
    }

    #[test]
    fn toast_renderer_handles_narrow_terminals_and_storage_cap() {
        let mut app = new_app(MockBackend::default());
        for idx in 0..20 {
            app.state.push_toast(Toast {
                title: Some(alloc::format!("T{idx}")),
                message: "very long message that should be clipped".into(),
                variant: ToastVariant::Info,
                created_at: 0,
                duration: 10,
            });
        }

        assert_eq!(app.state.toasts.len(), MAX_STORED_TOASTS);
        assert_eq!(
            app.state
                .toasts
                .front()
                .and_then(|toast| toast.title.as_deref()),
            Some("T4")
        );

        let test_backend = TestBackend::new(2, 4);
        let mut terminal = Terminal::new(test_backend).unwrap();
        terminal
            .draw(|frame| app.state.render_toasts(frame, Rect::new(0, 0, 2, 4)))
            .unwrap();
    }

    #[test]
    fn tick_expires_toasts_and_modal_suppresses_rendering() {
        let mut app = new_app(MockBackend::default());
        app.state.push_toast(Toast {
            title: Some("HiddenToast".into()),
            message: "toast message".into(),
            variant: ToastVariant::Info,
            created_at: 0,
            duration: 0,
        });
        app.state.set_active_modal(Box::new(HelpModal::new()));

        let test_backend = TestBackend::new(80, 8);
        let mut terminal = Terminal::new(test_backend).unwrap();
        terminal.draw(|frame| app.state.render(frame)).unwrap();
        let screen = rendered_screen(&terminal);
        assert!(!screen.contains("HiddenToast"), "screen: {screen}");

        app.state.clear_active_modal();
        run(&mut app, Event::Tick);
        assert!(
            app.state.toasts.is_empty(),
            "expired toast should be pruned"
        );
    }

    #[test]
    fn permission_request_opens_modal_and_enter_replies_once() {
        let mut app = new_app(MockBackend::default());

        run(
            &mut app,
            Event::Backend(ocpncord_backend::BackendEvent::PermissionAsked {
                request: permission_request("perm-1"),
            }),
        );

        assert_eq!(
            app.state.active_modal().map(|modal| modal.title()),
            Some("Permission Request")
        );
        assert!(matches!(
            app.state.active_blocking_prompt.as_ref(),
            Some(ActiveBlockingPrompt::Permission(id)) if id == "perm-1"
        ));

        run(&mut app, enter_key());

        assert!(
            app.state.active_modal().is_none(),
            "modal should close after reply"
        );
        assert_eq!(app.backend().permission_replies.len(), 1);
        assert_eq!(app.backend().permission_replies[0].request_id, "perm-1");
        assert_eq!(app.backend().permission_replies[0].reply, "once");
    }

    #[test]
    fn permission_escape_rejects_and_opens_next_queued_request() {
        let mut app = new_app(MockBackend::default());

        run(
            &mut app,
            Event::Backend(ocpncord_backend::BackendEvent::PermissionAsked {
                request: permission_request("perm-1"),
            }),
        );
        run(
            &mut app,
            Event::Backend(ocpncord_backend::BackendEvent::PermissionAsked {
                request: permission_request("perm-2"),
            }),
        );

        run(&mut app, escape_key());

        assert_eq!(app.backend().permission_replies.len(), 1);
        assert_eq!(app.backend().permission_replies[0].request_id, "perm-1");
        assert_eq!(app.backend().permission_replies[0].reply, "reject");
        assert_eq!(app.state.pending_permissions.len(), 1);
        assert_eq!(
            app.state
                .pending_permissions
                .front()
                .map(|request| request.id.as_str()),
            Some("perm-2")
        );
        assert!(matches!(
            app.state.active_blocking_prompt.as_ref(),
            Some(ActiveBlockingPrompt::Permission(id)) if id == "perm-2"
        ));
    }

    #[test]
    fn external_permission_reply_removes_matching_id_without_popping_front() {
        let mut app = new_app(MockBackend::default());

        run(
            &mut app,
            Event::Backend(ocpncord_backend::BackendEvent::PermissionAsked {
                request: permission_request("perm-1"),
            }),
        );
        run(
            &mut app,
            Event::Backend(ocpncord_backend::BackendEvent::PermissionAsked {
                request: permission_request("perm-2"),
            }),
        );

        run(
            &mut app,
            Event::Backend(ocpncord_backend::BackendEvent::PermissionReplied {
                session_id: "session-1".into(),
                request_id: "perm-2".into(),
                reply: "always".into(),
            }),
        );

        assert_eq!(app.state.pending_permissions.len(), 1);
        assert_eq!(
            app.state
                .pending_permissions
                .front()
                .map(|request| request.id.as_str()),
            Some("perm-1")
        );
        assert!(matches!(
            app.state.active_blocking_prompt.as_ref(),
            Some(ActiveBlockingPrompt::Permission(id)) if id == "perm-1"
        ));
    }

    #[test]
    fn question_request_submits_nested_answers_and_escape_rejects() {
        let mut app = new_app(MockBackend::default());

        run(
            &mut app,
            Event::Backend(ocpncord_backend::BackendEvent::QuestionAsked {
                request: question_request("question-1"),
            }),
        );
        run(&mut app, enter_key());

        assert_eq!(app.backend().question_replies.len(), 1);
        assert_eq!(
            app.backend().question_replies[0].answers,
            vec![vec!["Yes".to_string()]]
        );

        run(
            &mut app,
            Event::Backend(ocpncord_backend::BackendEvent::QuestionAsked {
                request: question_request("question-2"),
            }),
        );
        run(&mut app, escape_key());

        assert_eq!(
            app.backend().rejected_questions,
            vec!["question-2".to_string()]
        );
        assert!(
            app.state.active_modal().is_none(),
            "question modal should close after reject"
        );
    }
}
