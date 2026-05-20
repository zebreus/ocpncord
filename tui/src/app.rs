use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::Cell;

use ocpncord_backend::{Backend, BackendEvent};

use crate::chat::{render_chat, Chat, ChatTranscript};
use crate::command_palette::CommandPaletteModal;
use crate::event::{Event, Scancode};
use crate::key_chord::KeyChord;
use crate::modal::{HelpModal, Modal, ModelPickerModal, SessionListModal};
use crate::prompt_bar::PromptBar;
use crate::screen::{Action, ModalId, ScreenId, Tab};
use crate::start_page::StartPage;
use crate::theme::Theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Text};
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

/// A message held in memory, built from streaming Parts.
#[derive(Debug, Clone)]
pub struct LoadedMessage {
    pub role: ocpncord_backend::MessageRole,
    pub parts: Vec<ocpncord_backend::Part>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingPrompt {
    pub session_id: String,
    pub text: String,
    pub agent: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamTextKind {
    Text,
    Reasoning,
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

fn parts_equivalent(left: &ocpncord_backend::Part, right: &ocpncord_backend::Part) -> bool {
    match (left, right) {
        (ocpncord_backend::Part::Text(left), ocpncord_backend::Part::Text(right)) => {
            left.text == right.text
        }
        (ocpncord_backend::Part::Reasoning(left), ocpncord_backend::Part::Reasoning(right)) => {
            left.text == right.text
        }
        (ocpncord_backend::Part::Tool(left), ocpncord_backend::Part::Tool(right)) => {
            left.tool == right.tool && tool_states_equivalent(&left.state, &right.state)
        }
        (ocpncord_backend::Part::StepStart(_), ocpncord_backend::Part::StepStart(_)) => true,
        (ocpncord_backend::Part::StepFinish(left), ocpncord_backend::Part::StepFinish(right)) => {
            left.reason == right.reason
        }
        _ => false,
    }
}

fn tool_states_equivalent(
    left: &ocpncord_backend::ToolState,
    right: &ocpncord_backend::ToolState,
) -> bool {
    match (left, right) {
        (
            ocpncord_backend::ToolState::Pending {
                input: left_input,
                raw: left_raw,
            },
            ocpncord_backend::ToolState::Pending {
                input: right_input,
                raw: right_raw,
            },
        ) => left_input == right_input && left_raw == right_raw,
        (
            ocpncord_backend::ToolState::Running { .. },
            ocpncord_backend::ToolState::Running { .. },
        ) => true,
        (
            ocpncord_backend::ToolState::Completed {
                output: left_output,
                title: left_title,
                ..
            },
            ocpncord_backend::ToolState::Completed {
                output: right_output,
                title: right_title,
                ..
            },
        ) => left_output == right_output && left_title == right_title,
        (
            ocpncord_backend::ToolState::Error {
                error: left_error, ..
            },
            ocpncord_backend::ToolState::Error {
                error: right_error, ..
            },
        ) => left_error == right_error,
        _ => false,
    }
}

fn clamp_tail(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let len = text.chars().count();
    text.chars().skip(len.saturating_sub(width)).collect()
}

fn user_loaded_message(text: &str) -> LoadedMessage {
    LoadedMessage {
        role: ocpncord_backend::MessageRole::User,
        parts: vec![ocpncord_backend::Part::Text(ocpncord_backend::TextPart {
            text: text.into(),
        })],
    }
}

fn stream_text_kind(part: &ocpncord_backend::Part) -> Option<StreamTextKind> {
    match part {
        ocpncord_backend::Part::Text(_) => Some(StreamTextKind::Text),
        ocpncord_backend::Part::Reasoning(_) => Some(StreamTextKind::Reasoning),
        _ => None,
    }
}

fn set_stream_text(part: &mut ocpncord_backend::Part, text: String, kind: StreamTextKind) {
    *part = match kind {
        StreamTextKind::Text => ocpncord_backend::Part::Text(ocpncord_backend::TextPart { text }),
        StreamTextKind::Reasoning => {
            ocpncord_backend::Part::Reasoning(ocpncord_backend::ReasoningPart { text })
        }
    };
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

    pub fn push_line(&mut self, line: TermLine) {
        if self.lines.len() >= 2000 {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
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

/// Top-level app state.
pub struct App<B: Backend> {
    backend: B,
    active_screen: ScreenId,
    theme: Theme,
    key_chord: KeyChord,
    tick: u64,
    prompt_bar: PromptBar,
    chat: Chat,
    active_session: Option<ocpncord_backend::Session>,
    draft: Option<String>,
    error: Option<String>,
    is_streaming: bool,
    response_indicator_until_tick: u64,
    partial_parts: Vec<ocpncord_backend::Part>,
    /// Accumulated delta text per part_id for real-time streaming.
    partial_texts: alloc::collections::BTreeMap<String, String>,
    partial_part_indices: alloc::collections::BTreeMap<String, usize>,
    latest_text_part_index: Option<usize>,
    messages: Vec<LoadedMessage>,
    pending_prompts: Vec<PendingPrompt>,
    queued_prompts: Vec<PendingPrompt>,
    queued_messages: Vec<LoadedMessage>,
    ignore_done_until_tick: u64,
    response_seen_assistant_activity: bool,
    stream: Option<B::PromptStream>,
    sync_stream: Option<B::EventStream>,
    active_modal: Option<Box<dyn Modal>>,
    agents: Vec<ocpncord_backend::Agent>,
    active_agent: usize,
    // --- New fields for full API integration ---
    terminal: TerminalPane,
    // Session cache for modals
    cached_sessions: Vec<ocpncord_backend::Session>,
    // Permission & Question pending queues
    pending_permissions:
        alloc::collections::VecDeque<(ocpncord_backend::PermissionRequest, String)>,
    pending_questions: alloc::collections::VecDeque<(ocpncord_backend::QuestionRequest, String)>,
    // Toast notifications
    toasts: alloc::collections::VecDeque<Toast>,
    // Side panel state
    side_panel_visible: bool,
    side_panel_tab: crate::screen::Tab,
    side_panel_scroll: u16,
    // Track the screen before switching to Terminal for toggle back
    screen_before_terminal: ScreenId,
    terminal_view_height: Cell<u16>,
    // LSP diagnostics cache: file_path -> diagnostics
    lsp_diagnostics: alloc::collections::BTreeMap<String, Vec<LspDiagnostic>>,
    // Todo cache
    todos: Vec<ocpncord_backend::Todo>,
    // Workspace state
    current_workspace: Option<String>,
    current_branch: Option<String>,
}

impl<B: Backend> App<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            active_screen: ScreenId::StartPage,
            theme: Theme::default(),
            key_chord: KeyChord::new(),
            tick: 0,
            prompt_bar: PromptBar::new(),
            chat: Chat::new(),
            active_session: None,
            draft: None,
            error: None,
            is_streaming: false,
            response_indicator_until_tick: 0,
            partial_parts: Vec::new(),
            partial_texts: alloc::collections::BTreeMap::new(),
            partial_part_indices: alloc::collections::BTreeMap::new(),
            latest_text_part_index: None,
            messages: Vec::new(),
            pending_prompts: Vec::new(),
            queued_prompts: Vec::new(),
            queued_messages: Vec::new(),
            ignore_done_until_tick: 0,
            response_seen_assistant_activity: false,
            stream: None,
            active_modal: None,
            agents: Vec::new(),
            active_agent: 0,
            terminal: TerminalPane::new(),
            sync_stream: None,
            cached_sessions: Vec::new(),
            pending_permissions: alloc::collections::VecDeque::new(),
            pending_questions: alloc::collections::VecDeque::new(),
            toasts: alloc::collections::VecDeque::new(),
            side_panel_visible: false,
            side_panel_tab: crate::screen::Tab::Diagnostics,
            side_panel_scroll: 0,
            screen_before_terminal: ScreenId::StartPage,
            terminal_view_height: Cell::new(10),
            lsp_diagnostics: alloc::collections::BTreeMap::new(),
            todos: Vec::new(),
            current_workspace: None,
            current_branch: None,
        }
    }

    pub fn set_active_modal(&mut self, modal: Box<dyn Modal>) {
        self.active_modal = Some(modal);
    }

    pub fn active_modal(&self) -> Option<&dyn Modal> {
        self.active_modal.as_deref()
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    pub fn active_screen(&self) -> ScreenId {
        self.active_screen
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

    pub fn set_cwd(&mut self, cwd: String) {
        self.current_workspace = Some(cwd);
    }

    /// Returns true if there is an active event stream (prompt or sync) to poll.
    pub fn has_event_stream(&self) -> bool {
        self.stream.is_some() || self.sync_stream.is_some()
    }

    /// Poll the next event from whichever stream is active.
    /// Prompt stream takes priority (it's the active conversation).
    /// Sync stream provides background session/message updates.
    /// Returns None when the stream is exhausted.
    pub async fn poll_next_event(
        &mut self,
    ) -> Option<Result<BackendEvent, ocpncord_backend::BackendError>> {
        use futures::StreamExt;

        if let Some(stream) = &mut self.stream {
            match stream.next().await {
                Some(result) => return Some(result),
                None => {
                    self.stream = None;
                    self.is_streaming = false;
                }
            }
        }
        if let Some(stream) = &mut self.sync_stream {
            return stream.next().await;
        }
        None
    }

    /// Replace the current prompt stream (e.g., after sending a message).
    pub fn set_stream(&mut self, stream: B::PromptStream) {
        self.stream = Some(stream);
    }

    /// Open the sync event stream if not already active.
    pub async fn initiate_sync_stream(&mut self) {
        if self.sync_stream.is_none() {
            if let Ok(stream) = self.backend.sync_events().await {
                self.sync_stream = Some(stream);
            }
        }
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

    pub async fn init(&mut self) {
        match self.backend.list_agents().await {
            Ok(agents) => {
                self.agents = agents
                    .into_iter()
                    .filter(|a| matches!(a.mode, ocpncord_backend::AgentMode::Primary))
                    .collect();
            }
            Err(_) => {}
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
        &self.partial_parts
    }

    pub fn messages(&self) -> &[LoadedMessage] {
        &self.messages
    }

    pub fn pending_prompts(&self) -> &[PendingPrompt] {
        &self.pending_prompts
    }

    pub fn queued_prompts(&self) -> &[PendingPrompt] {
        &self.queued_prompts
    }

    pub fn take_pending_prompts(&mut self) -> Vec<PendingPrompt> {
        core::mem::take(&mut self.pending_prompts)
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

    fn queue_or_dispatch_prompt(&mut self, prompt: PendingPrompt, message: LoadedMessage) {
        if self.is_streaming || !self.pending_prompts.is_empty() {
            self.queued_prompts.push(prompt);
            self.queued_messages.push(message);
            return;
        }

        self.dispatch_prompt(prompt, message);
    }

    fn dispatch_prompt(&mut self, prompt: PendingPrompt, message: LoadedMessage) {
        self.messages.push(message);
        self.draft = Some(prompt.text.clone());
        self.active_screen = ScreenId::Chat;
        self.is_streaming = true;
        self.mark_response_active();
        self.partial_parts.clear();
        self.partial_texts.clear();
        self.partial_part_indices.clear();
        self.latest_text_part_index = None;
        self.response_seen_assistant_activity = false;
        self.pending_prompts.push(prompt);
    }

    fn dispatch_next_queued_prompt(&mut self) {
        if self.queued_prompts.is_empty() || self.queued_messages.is_empty() {
            return;
        }

        if !self.partial_parts.is_empty() {
            let parts = core::mem::take(&mut self.partial_parts);
            self.messages.push(LoadedMessage {
                role: ocpncord_backend::MessageRole::Assistant,
                parts,
            });
            self.partial_texts.clear();
            self.partial_part_indices.clear();
            self.latest_text_part_index = None;
        }

        let prompt = self.queued_prompts.remove(0);
        let message = self.queued_messages.remove(0);
        self.ignore_done_until_tick = self.tick.saturating_add(2);
        self.dispatch_prompt(prompt, message);
    }

    /// Returns `false` when the application should quit.
    pub async fn handle_event(&mut self, event: Event) -> bool {
        match event {
            Event::Key(ref key) => {
                self.error = None;

                if let Some(ref mut modal) = self.active_modal {
                    if key.scancode == Scancode::Escape {
                        self.active_modal = None;
                        return true;
                    }
                    let action = modal.handle_event(Event::Key(key.clone()));
                    match action {
                        Action::CloseModal => self.active_modal = None,
                        Action::None => {}
                        other => {
                            self.active_modal = None;
                            return self.apply_action(Some(other)).await;
                        }
                    }
                    return true;
                }

                if self.is_streaming {
                    if key.scancode == Scancode::Escape {
                        return self.handle_interrupt().await;
                    }
                    if key.scancode == Scancode::Char('c') && key.modifiers.ctrl {
                        return self.handle_interrupt().await;
                    }
                }

                if let Some(action) = self.key_chord.handle(key, self.tick) {
                    return self.apply_action(Some(action)).await;
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

                if self.active_screen == ScreenId::Terminal {
                    let action = match key.scancode {
                        Scancode::Up => Some(Action::ScrollUp),
                        Scancode::Down => Some(Action::ScrollDown),
                        Scancode::PageUp => Some(Action::ScrollPageUp),
                        Scancode::PageDown => Some(Action::ScrollPageDown),
                        _ => None,
                    };
                    if action.is_some() {
                        return self.apply_action(action).await;
                    }
                    return true;
                }

                if self.active_screen == ScreenId::Chat
                    || (self.side_panel_visible && self.side_panel_tab == crate::screen::Tab::Pane)
                {
                    let action = match key.scancode {
                        Scancode::Up => Some(Action::ScrollUp),
                        Scancode::Down => Some(Action::ScrollDown),
                        Scancode::PageUp => Some(Action::ScrollPageUp),
                        Scancode::PageDown => Some(Action::ScrollPageDown),
                        _ => None,
                    };
                    if action.is_some() {
                        return self.apply_action(action).await;
                    }
                }

                if let Some(action) = self.prompt_bar.handle_key(key) {
                    match action {
                        Action::SendMessage => {
                            return self.handle_send_message().await;
                        }
                        _ => {}
                    }
                }
            }
            Event::Backend(event) => {
                #[allow(unreachable_patterns)]
                match event {
                    ocpncord_backend::BackendEvent::Part { part, delta: _ } => {
                        self.mark_response_active();
                        self.response_seen_assistant_activity = true;
                        self.merge_stream_part(part);
                    }
                    ocpncord_backend::BackendEvent::Done => {
                        if self.tick >= self.ignore_done_until_tick
                            && self.response_seen_assistant_activity
                        {
                            let was_streaming = self.is_streaming;
                            self.is_streaming = false;
                            self.stream = None;
                            self.partial_texts.clear();
                            self.partial_part_indices.clear();
                            self.latest_text_part_index = None;

                            // REST API fallback: fetch all messages to fill in any
                            // that SSE events might have missed.
                            if let Some(ref session) = self.active_session.clone() {
                                let session_id = session.id.clone();
                                if let Ok(summaries) = self.backend.list_messages(&session_id).await
                                {
                                    let mut api_messages: Vec<LoadedMessage> = Vec::new();
                                    for summary in &summaries {
                                        if let Ok(detail) =
                                            self.backend.get_message(&session_id, &summary.id).await
                                        {
                                            api_messages.push(LoadedMessage {
                                                role: detail.info.role,
                                                parts: detail.parts,
                                            });
                                        }
                                    }
                                    if !api_messages.is_empty()
                                        && api_messages.len() >= self.messages.len()
                                    {
                                        self.messages = api_messages;
                                        self.partial_parts.clear();
                                    }
                                }
                            }

                            if was_streaming {
                                self.dispatch_next_queued_prompt();
                            }
                        }
                    }
                    ocpncord_backend::BackendEvent::Error { message } => {
                        self.error = Some(message);
                        self.partial_parts.clear();
                        self.is_streaming = false;
                        self.response_indicator_until_tick = 0;
                        self.stream = None;
                        self.partial_texts.clear();
                        self.partial_part_indices.clear();
                        self.latest_text_part_index = None;
                        self.response_seen_assistant_activity = false;
                        self.dispatch_next_queued_prompt();
                    }
                    ocpncord_backend::BackendEvent::SessionCreated { session } => {
                        let is_new = self
                            .active_session
                            .as_ref()
                            .map(|s| s.id != session.id)
                            .unwrap_or(true);
                        self.active_session = Some(session);
                        self.active_screen = ScreenId::Chat;
                        if is_new {
                            self.messages.clear();
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
                        self.messages.clear();
                        self.active_screen = ScreenId::StartPage;
                    }
                    ocpncord_backend::BackendEvent::SessionIdle { .. } => {}
                    ocpncord_backend::BackendEvent::SessionError { error, .. } => {
                        self.error = Some(alloc::format!("Session error: {:?}", error));
                    }
                    ocpncord_backend::BackendEvent::SessionDiff { .. } => {}
                    ocpncord_backend::BackendEvent::SessionCompacted { .. } => {}
                    ocpncord_backend::BackendEvent::MessageUpdated { .. } => {
                        // MessageUpdated is a persistence/status notification.
                        // The visible stream is built from part events, and
                        // Done performs the REST fallback for committed
                        // messages. Flushing here duplicates replayed parts.
                    }
                    ocpncord_backend::BackendEvent::MessageRemoved { .. } => {}
                    ocpncord_backend::BackendEvent::MessagePartUpdated {
                        session_id,
                        ref part,
                    } => {
                        if self.active_session.as_ref().map(|s| s.id.as_str())
                            == Some(session_id.as_str())
                        {
                            self.mark_response_active();
                            if !self.is_echoed_user_text(part) {
                                self.response_seen_assistant_activity = true;
                            }
                            self.merge_stream_part(part.clone());
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
                        self.response_seen_assistant_activity = true;
                        self.merge_stream_delta(part_id, delta);
                    }
                    ocpncord_backend::BackendEvent::MessagePartDelta { .. } => {}
                    ocpncord_backend::BackendEvent::MessagePartRemoved { .. } => {}
                    ocpncord_backend::BackendEvent::PermissionAsked { request } => {
                        let sid = self
                            .active_session
                            .as_ref()
                            .map(|s| s.id.clone())
                            .unwrap_or_default();
                        self.pending_permissions.push_back((request.clone(), sid));
                    }
                    ocpncord_backend::BackendEvent::PermissionReplied { .. } => {
                        self.pending_permissions.pop_front();
                    }
                    ocpncord_backend::BackendEvent::QuestionAsked { request } => {
                        let sid = self
                            .active_session
                            .as_ref()
                            .map(|s| s.id.clone())
                            .unwrap_or_default();
                        self.pending_questions.push_back((request.clone(), sid));
                    }
                    ocpncord_backend::BackendEvent::QuestionRejected { .. } => {
                        self.pending_questions.pop_front();
                    }
                    ocpncord_backend::BackendEvent::QuestionReplied { .. } => {
                        self.pending_questions.pop_front();
                    }
                    ocpncord_backend::BackendEvent::CommandExecuted {
                        name, arguments, ..
                    } => {
                        self.toasts.push_back(Toast {
                            title: Some("Command".into()),
                            message: alloc::format!("{name} {arguments}"),
                            variant: ToastVariant::Info,
                            created_at: self.tick,
                            duration: 12,
                        });
                    }
                    ocpncord_backend::BackendEvent::FileEdited { file } => {
                        self.toasts.push_back(Toast {
                            title: Some("File Edited".into()),
                            message: file,
                            variant: ToastVariant::Info,
                            created_at: self.tick,
                            duration: 8,
                        });
                    }
                    ocpncord_backend::BackendEvent::FileWatcherUpdated { file, event } => {
                        self.toasts.push_back(Toast {
                            title: Some("File Watcher".into()),
                            message: alloc::format!("{file}: {event}"),
                            variant: ToastVariant::Info,
                            created_at: self.tick,
                            duration: 6,
                        });
                    }
                    ocpncord_backend::BackendEvent::PtyCreated { info } => {
                        self.terminal.set_from_pty(&info);
                        self.side_panel_tab = crate::screen::Tab::Pane;
                        self.active_screen = ScreenId::Terminal;
                    }
                    ocpncord_backend::BackendEvent::PtyUpdated { .. } => {}
                    ocpncord_backend::BackendEvent::PtyDeleted { .. } => {
                        self.active_screen = ScreenId::Chat;
                    }
                    ocpncord_backend::BackendEvent::PtyExited { exit_code, .. } => {
                        self.terminal.status = ocpncord_backend::PtyStatus::Exited;
                        self.terminal.exit_code = Some(exit_code);
                        self.toasts.push_back(Toast {
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
                        self.side_panel_tab = crate::screen::Tab::Diagnostics;
                        self.side_panel_visible = true;
                    }
                    ocpncord_backend::BackendEvent::LspUpdated => {}
                    ocpncord_backend::BackendEvent::McpBrowserOpenFailed { mcp_name, url } => {
                        self.toasts.push_back(Toast {
                            title: Some("MCP Error".into()),
                            message: alloc::format!("{mcp_name}: {url}"),
                            variant: ToastVariant::Error,
                            created_at: self.tick,
                            duration: 10,
                        });
                    }
                    ocpncord_backend::BackendEvent::McpToolsChanged { server } => {
                        self.toasts.push_back(Toast {
                            title: Some("MCP Tools".into()),
                            message: alloc::format!("Updated from {server}"),
                            variant: ToastVariant::Info,
                            created_at: self.tick,
                            duration: 6,
                        });
                    }
                    ocpncord_backend::BackendEvent::InstallationUpdateAvailable { version } => {
                        self.toasts.push_back(Toast {
                            title: Some("Update Available".into()),
                            message: alloc::format!("version {version}"),
                            variant: ToastVariant::Info,
                            created_at: self.tick,
                            duration: 12,
                        });
                    }
                    ocpncord_backend::BackendEvent::InstallationUpdated { version } => {
                        self.toasts.push_back(Toast {
                            title: Some("Updated".into()),
                            message: alloc::format!("now on {version}"),
                            variant: ToastVariant::Success,
                            created_at: self.tick,
                            duration: 6,
                        });
                    }
                    ocpncord_backend::BackendEvent::WorkspaceReady { name } => {
                        self.current_workspace = Some(name);
                    }
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
                        self.handle_slash_command_inner(command).await;
                    }
                    ocpncord_backend::BackendEvent::TuiToastShow {
                        message,
                        variant,
                        title,
                        duration,
                    } => {
                        self.toasts.push_back(Toast {
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
                        self.handle_select_session(session_id).await;
                    }
                    ocpncord_backend::BackendEvent::ServerConnected => {
                        self.toasts.push_back(Toast {
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
                        self.toasts.push_back(Toast {
                            title: Some("Instance Disposed".into()),
                            message: directory,
                            variant: ToastVariant::Warning,
                            created_at: self.tick,
                            duration: 8,
                        });
                    }
                    ocpncord_backend::BackendEvent::ProjectUpdated(project) => {
                        self.current_workspace = project.name.or(self.current_workspace.clone());
                    }
                    _ => {}
                }
            }
            Event::Tick => {
                self.tick = self.tick.wrapping_add(1);
                let tick = self.tick;
                self.toasts
                    .retain(|toast| tick.saturating_sub(toast.created_at) <= toast.duration);
                if let Some(action) = self.key_chord.tick(self.tick) {
                    return self.apply_action(Some(action)).await;
                }
            }
            Event::Quit => return false,
        }
        true
    }

    async fn handle_send_message(&mut self) -> bool {
        let text = self.prompt_bar.text().to_string();

        // Slash command routing
        if text.starts_with('/') {
            return self.handle_slash_command(&text).await;
        }

        if self.active_session.is_none() {
            match self
                .backend
                .create_session("Chat", self.current_workspace.as_deref().unwrap_or(""))
                .await
            {
                Ok(session) => {
                    self.active_session = Some(session);
                }
                Err(e) => {
                    self.error = Some(alloc::format!("{}", e));
                    return true;
                }
            }
        }

        let session_id = self
            .active_session
            .as_ref()
            .map(|s| s.id.clone())
            .unwrap_or_default();

        self.prompt_bar.clear();
        let agent = self.active_agent_name().to_string();
        let message = user_loaded_message(&text);
        let prompt = PendingPrompt {
            session_id,
            text: text.clone(),
            agent,
        };
        self.queue_or_dispatch_prompt(prompt, message);

        true
    }

    async fn handle_slash_command(&mut self, text: &str) -> bool {
        match text {
            "/models" | "/settings" | "/config" => {
                self.prompt_bar.clear();
                let mut modal = ModelPickerModal::new();
                match self.backend.get_config().await {
                    Ok(config) => modal.set_config(config),
                    Err(e) => modal.set_error(alloc::format!("{}", e)),
                }
                self.active_modal = Some(Box::new(modal));
                true
            }
            "/new" => {
                match self
                    .backend
                    .create_session("Chat", self.current_workspace.as_deref().unwrap_or(""))
                    .await
                {
                    Ok(session) => {
                        self.active_session = Some(session);
                    }
                    Err(e) => {
                        self.error = Some(alloc::format!("{}", e));
                        return true;
                    }
                }
                self.prompt_bar.clear();
                self.draft = None;
                self.messages.clear();
                self.active_modal = None;
                self.active_screen = ScreenId::Chat;
                true
            }
            "/sessions" => {
                self.prompt_bar.clear();
                let mut modal = SessionListModal::new();
                match self.backend.list_sessions().await {
                    Ok(sessions) => modal.set_sessions(sessions),
                    Err(e) => modal.set_error(alloc::format!("{}", e)),
                }
                self.active_modal = Some(Box::new(modal));
                true
            }
            "/help" => {
                self.prompt_bar.clear();
                self.active_modal = Some(Box::new(HelpModal::new()));
                true
            }
            "/todos" => {
                self.prompt_bar.clear();
                self.side_panel_visible = true;
                self.side_panel_tab = Tab::Todos;
                true
            }
            "/diagnostics" => {
                self.prompt_bar.clear();
                self.side_panel_visible = true;
                self.side_panel_tab = Tab::Diagnostics;
                true
            }
            "/pty" => {
                self.prompt_bar.clear();
                self.side_panel_visible = true;
                self.side_panel_tab = Tab::Pane;
                true
            }
            "/abort" => {
                if let Some(session) = &self.active_session {
                    let _ = self.backend.abort_session(&session.id).await;
                }
                true
            }
            "/dispose" => {
                let _ = self.backend.dispose().await;
                true
            }
            "/upgrade" => {
                let _ = self.backend.upgrade().await;
                true
            }
            "/exit" => false,
            _ => self.handle_unknown_slash_command(text).await,
        }
    }

    async fn handle_unknown_slash_command(&mut self, text: &str) -> bool {
        if self.active_session.is_none() {
            match self.backend.create_session("Chat", "").await {
                Ok(session) => {
                    self.active_session = Some(session);
                }
                Err(e) => {
                    self.error = Some(alloc::format!("{}", e));
                    return true;
                }
            }
        }

        let session_id = self
            .active_session
            .as_ref()
            .map(|s| s.id.clone())
            .unwrap_or_default();

        self.prompt_bar.clear();
        let agent = self.active_agent_name().to_string();
        let message = user_loaded_message(text);
        let prompt = PendingPrompt {
            session_id,
            text: text.into(),
            agent,
        };
        self.queue_or_dispatch_prompt(prompt, message);

        true
    }

    async fn handle_interrupt(&mut self) -> bool {
        if let Some(session) = &self.active_session {
            let _ = self.backend.abort_session(&session.id).await;
        }
        self.stream = None;
        self.is_streaming = false;
        self.response_indicator_until_tick = 0;
        self.partial_parts.clear();
        self.partial_texts.clear();
        self.partial_part_indices.clear();
        self.latest_text_part_index = None;
        self.response_seen_assistant_activity = false;
        self.queued_prompts.clear();
        self.queued_messages.clear();
        true
    }

    async fn handle_slash_command_inner(&mut self, text: &str) {
        match text {
            "/sessions" => {
                self.prompt_bar.clear();
                let mut modal = SessionListModal::new();
                match self.backend.list_sessions().await {
                    Ok(sessions) => modal.set_sessions(sessions),
                    Err(e) => modal.set_error(alloc::format!("{}", e)),
                }
                self.active_modal = Some(Box::new(modal));
            }
            "/models" | "/settings" | "/config" => {
                self.prompt_bar.clear();
                let mut modal = ModelPickerModal::new();
                match self.backend.get_config().await {
                    Ok(config) => modal.set_config(config),
                    Err(e) => modal.set_error(alloc::format!("{}", e)),
                }
                self.active_modal = Some(Box::new(modal));
            }
            "/new" => {
                match self.backend.create_session("Chat", "").await {
                    Ok(session) => {
                        self.active_session = Some(session);
                    }
                    Err(e) => {
                        self.error = Some(alloc::format!("{}", e));
                        return;
                    }
                }
                self.prompt_bar.clear();
                self.draft = None;
                self.messages.clear();
                self.active_modal = None;
                self.active_screen = ScreenId::Chat;
            }
            "/help" => {
                self.prompt_bar.clear();
                self.active_modal = Some(Box::new(HelpModal::new()));
            }
            _ => {}
        }
    }

    async fn handle_select_session(&mut self, id: &str) {
        let session_id = id.to_string();
        match self.backend.get_session(&session_id).await {
            Ok(session) => {
                self.active_session = Some(session);
                self.active_screen = ScreenId::Chat;
                self.messages.clear();
                match self.backend.list_messages(&session_id).await {
                    Ok(summaries) => {
                        let mut messages = Vec::new();
                        for summary in summaries {
                            if let Ok(detail) =
                                self.backend.get_message(&session_id, &summary.id).await
                            {
                                messages.push(LoadedMessage {
                                    role: detail.info.role,
                                    parts: detail.parts,
                                });
                            }
                        }
                        self.messages = messages;
                    }
                    Err(e) => self.error = Some(alloc::format!("{}", e)),
                }
            }
            Err(e) => self.error = Some(alloc::format!("{}", e)),
        }
    }

    async fn apply_action(&mut self, action: Option<Action>) -> bool {
        match action {
            Some(Action::Quit) => return false,
            Some(Action::SwitchScreen(id)) => self.active_screen = id,
            Some(Action::CloseModal) => self.active_modal = None,
            Some(Action::OpenPalette) => {
                let modal = CommandPaletteModal::new(crate::command_palette::default_commands());
                self.active_modal = Some(Box::new(modal));
            }
            Some(Action::OpenModal(ModalId::SessionList)) => {
                let mut modal = SessionListModal::new();
                match self.backend.list_sessions().await {
                    Ok(sessions) => modal.set_sessions(sessions),
                    Err(e) => modal.set_error(alloc::format!("{}", e)),
                }
                self.active_modal = Some(Box::new(modal));
            }
            Some(Action::OpenModal(ModalId::ModelPicker)) => {
                let mut modal = ModelPickerModal::new();
                match self.backend.get_config().await {
                    Ok(config) => modal.set_config(config),
                    Err(e) => modal.set_error(alloc::format!("{}", e)),
                }
                self.active_modal = Some(Box::new(modal));
            }
            Some(Action::OpenModal(ModalId::Help)) => {
                self.active_modal = Some(Box::new(HelpModal::new()));
            }
            Some(Action::OpenModal(ModalId::Settings)) => {
                let mut modal = ModelPickerModal::new();
                match self.backend.get_config().await {
                    Ok(config) => modal.set_config(config),
                    Err(e) => modal.set_error(alloc::format!("{}", e)),
                }
                self.active_modal = Some(Box::new(modal));
            }
            Some(Action::OpenModal(ModalId::PermissionApproval)) => {}
            Some(Action::OpenModal(ModalId::QuestionApproval)) => {}
            Some(Action::OpenModal(_)) => {}
            Some(Action::LoadSession(ref id)) => {
                let session_id = id.clone();
                match self.backend.list_messages(&session_id).await {
                    Ok(summaries) => {
                        // Load full messages
                        let mut messages = Vec::new();
                        for summary in summaries {
                            if let Ok(detail) =
                                self.backend.get_message(&session_id, &summary.id).await
                            {
                                messages.push(LoadedMessage {
                                    role: detail.info.role,
                                    parts: detail.parts,
                                });
                            }
                        }
                        self.messages = messages;
                    }
                    Err(e) => {
                        self.error = Some(alloc::format!("{}", e));
                    }
                }
                self.active_session = self.backend.get_session(&session_id).await.ok();
                self.active_modal = None;
                self.active_screen = ScreenId::Chat;
            }
            Some(Action::DeleteSession(ref id)) => {
                let _ = self.backend.delete_session(&id).await;
                // Re-fetch sessions and re-open modal
                let mut modal = SessionListModal::new();
                match self.backend.list_sessions().await {
                    Ok(sessions) => modal.set_sessions(sessions),
                    Err(e) => modal.set_error(alloc::format!("{}", e)),
                }
                self.active_modal = Some(Box::new(modal));
            }
            Some(Action::Interrupt) => {
                return self.handle_interrupt().await;
            }
            Some(Action::ScrollUp) => {
                if self.scroll_targets_terminal() {
                    self.scroll_terminal_up(1);
                } else {
                    self.chat.scroll = self.chat.scroll.saturating_add(1);
                }
            }
            Some(Action::ScrollDown) => {
                if self.scroll_targets_terminal() {
                    self.scroll_terminal_down(1);
                } else {
                    self.chat.scroll = self.chat.scroll.saturating_sub(1);
                }
            }
            Some(Action::ScrollPageUp) => {
                let amount = self.page_scroll_amount();
                if self.scroll_targets_terminal() {
                    self.scroll_terminal_up(amount);
                } else {
                    self.chat.scroll = self.chat.scroll.saturating_add(amount);
                }
            }
            Some(Action::ScrollPageDown) => {
                let amount = self.page_scroll_amount();
                if self.scroll_targets_terminal() {
                    self.scroll_terminal_down(amount);
                } else {
                    self.chat.scroll = self.chat.scroll.saturating_sub(amount);
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
            Some(Action::OpenTerminal(_pty_id)) => {
                if self.active_screen == ScreenId::Terminal {
                    self.active_screen = self.screen_before_terminal;
                } else {
                    self.screen_before_terminal = self.active_screen;
                    self.active_screen = ScreenId::Terminal;
                }
            }
            Some(Action::CloseTerminal) => {
                self.active_screen = ScreenId::Chat;
            }
            Some(Action::OpenSettings) => {
                let mut modal = ModelPickerModal::new();
                match self.backend.get_config().await {
                    Ok(config) => modal.set_config(config),
                    Err(e) => modal.set_error(alloc::format!("{}", e)),
                }
                self.active_modal = Some(Box::new(modal));
            }
            Some(Action::SelectModel(ref model)) => {
                let mut modal = ModelPickerModal::new();
                match self.backend.get_config().await {
                    Ok(mut config) => {
                        config.model = Some(model.clone());
                        match self.backend.set_config(&config).await {
                            Ok(updated) => {
                                if updated.provider.is_empty() && !config.provider.is_empty() {
                                    modal.set_config(config);
                                } else {
                                    modal.set_config(updated);
                                }
                            }
                            Err(e) => modal.set_error(alloc::format!("{}", e)),
                        }
                    }
                    Err(e) => modal.set_error(alloc::format!("{}", e)),
                }
                self.active_modal = Some(Box::new(modal));
            }
            Some(Action::AbortSession(ref id)) => {
                let _ = self.backend.abort_session(id).await;
            }
            Some(Action::RenameSession(ref id, ref title)) => {
                match self.backend.update_session(id, title).await {
                    Ok(session) => {
                        self.active_session = Some(session);
                    }
                    Err(e) => self.error = Some(alloc::format!("{}", e)),
                }
            }
            Some(Action::SwitchToChat(ref id)) => {
                self.handle_select_session(id).await;
            }
            _ => {}
        }
        true
    }

    fn page_scroll_amount(&self) -> u16 {
        self.terminal_view_height.get().saturating_sub(1).max(1)
    }

    fn scroll_targets_terminal(&self) -> bool {
        self.active_screen == ScreenId::Terminal
            || (self.side_panel_visible && self.side_panel_tab == crate::screen::Tab::Pane)
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
        let mut status = alloc::format!("[{agent}]  mode: {mode}  model: {model}");
        if self.should_show_response_indicator() {
            let spinner =
                ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"][(self.tick as usize / 3) % 10];
            status.push_str("  ");
            status.push_str(spinner);
            status.push_str(" Agent is Responding...");
        }
        Text::from(clamp_tail(status.as_str(), area.width as usize))
            .style(self.theme.agent_indicator)
            .render(area, frame.buffer_mut());
    }

    fn merge_stream_part(&mut self, part: ocpncord_backend::Part) -> Option<usize> {
        if self.is_echoed_user_text(&part) {
            return None;
        }

        if let Some(index) = self
            .partial_parts
            .iter()
            .rposition(|existing| parts_equivalent(existing, &part))
        {
            if stream_text_kind(&part).is_some() {
                self.latest_text_part_index = Some(index);
            }
            return Some(index);
        }

        if let Some(kind) = stream_text_kind(&part) {
            let incoming_text = match &part {
                ocpncord_backend::Part::Text(text) => text.text.clone(),
                ocpncord_backend::Part::Reasoning(reasoning) => reasoning.text.clone(),
                _ => String::new(),
            };

            if let Some(index) = self
                .partial_parts
                .iter()
                .rposition(|existing| stream_text_kind(existing) == Some(kind))
            {
                if incoming_text.is_empty() {
                    self.latest_text_part_index = Some(index);
                    return Some(index);
                }
                set_stream_text(&mut self.partial_parts[index], incoming_text, kind);
                self.latest_text_part_index = Some(index);
                return Some(index);
            }
        }

        self.partial_parts.push(part);
        let index = self.partial_parts.len() - 1;
        if stream_text_kind(&self.partial_parts[index]).is_some() {
            self.latest_text_part_index = Some(index);
        }
        Some(index)
    }

    fn merge_stream_delta(&mut self, part_id: String, delta: String) {
        let text = {
            let acc = self.partial_texts.entry(part_id.clone()).or_default();
            acc.push_str(&delta);
            acc.clone()
        };

        let index = self
            .partial_part_indices
            .get(&part_id)
            .copied()
            .filter(|index| *index < self.partial_parts.len())
            .or(self.latest_text_part_index)
            .filter(|index| *index < self.partial_parts.len());

        let (index, kind) = match index {
            Some(index) => {
                let kind =
                    stream_text_kind(&self.partial_parts[index]).unwrap_or(StreamTextKind::Text);
                (index, kind)
            }
            None => {
                self.partial_parts
                    .push(ocpncord_backend::Part::Text(ocpncord_backend::TextPart {
                        text: String::new(),
                    }));
                (self.partial_parts.len() - 1, StreamTextKind::Text)
            }
        };

        set_stream_text(&mut self.partial_parts[index], text, kind);
        self.partial_part_indices.insert(part_id, index);
        self.latest_text_part_index = Some(index);
    }

    fn is_echoed_user_text(&self, part: &ocpncord_backend::Part) -> bool {
        let ocpncord_backend::Part::Text(incoming) = part else {
            return false;
        };

        self.messages.last().is_some_and(|message| {
            matches!(message.role, ocpncord_backend::MessageRole::User)
                && message.parts.len() == 1
                && matches!(
                    &message.parts[0],
                    ocpncord_backend::Part::Text(existing) if existing.text == incoming.text
                )
        })
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

        match self.active_screen {
            ScreenId::StartPage => {
                StartPage.render_in(frame, &self.theme, main_area);
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
                self.render_status_line(frame, status_row);
            }
            ScreenId::Chat => {
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
                        messages: &self.messages,
                        active_parts: &self.partial_parts,
                        queued_messages: &self.queued_messages,
                        is_streaming: self.is_streaming,
                    },
                    self.chat.scroll,
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
            ScreenId::Terminal => {
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

        // Render toasts (top-right corner)
        {
            let area = frame.area();
            let mut toast_y = 2u16;
            for toast in self.toasts.iter().rev().take(5) {
                if toast_y >= area.height.saturating_sub(2) {
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
                let max_width = area.width.saturating_sub(4);
                let display: String = display.chars().take(max_width as usize).collect();
                let display_width = display.len() as u16;
                let x = area.width.saturating_sub(display_width + 2);
                Text::from(display)
                    .style(style)
                    .render(Rect::new(x, toast_y, display_width, 1), frame.buffer_mut());
                toast_y += 1;
            }
        }

        // Render side panel (right 30% when visible)
        if self.side_panel_visible {
            let panel_area = if chunks.len() > 1 { chunks[1] } else { area };
            self.render_side_panel(frame, panel_area);
        }

        if let Some(ref modal) = self.active_modal {
            let area = frame.area();
            Clear.render(area, frame.buffer_mut());
            Block::new()
                .style(self.theme.bg)
                .render(area, frame.buffer_mut());
            let (modal_width, modal_height) = modal.preferred_size(area);
            let modal_x = area.x + (area.width.saturating_sub(modal_width)) / 2;
            let modal_y = area.y + (area.height.saturating_sub(modal_height)) / 2;
            let modal_area = Rect::new(modal_x, modal_y, modal_width, modal_height);
            Clear.render(modal_area, frame.buffer_mut());
            let block = Block::bordered()
                .style(self.theme.bg)
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

fn terminal_start_index(line_count: usize, height: u16, scroll: u16) -> usize {
    line_count
        .saturating_sub(height as usize)
        .saturating_sub(scroll as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{KeyEvent, Modifiers, Scancode};
    use ocpncord_backend::mock::MockBackend;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

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

    fn run<B: Backend>(app: &mut App<B>, event: Event) -> bool {
        futures::executor::block_on(app.handle_event(event))
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

    #[test]
    fn ctrl_c_quits() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);
        assert!(!run(&mut app, ctrl('c')));
    }

    #[test]
    fn ctrl_x_q_quits() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);
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
        let app = App::new(backend);
        assert_eq!(app.active_screen(), ScreenId::StartPage);
    }

    #[test]
    fn non_quit_events_keep_running() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);
        assert!(run(&mut app, char_key('a')));
        assert!(run(&mut app, char_key('b')));
    }

    #[test]
    fn leader_times_out_after_ticks() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);
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
        let mut app = App::new(backend);
        futures::executor::block_on(app.init());
        assert_eq!(app.active_agent_name(), "build");
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
        let mut app = App::new(backend);
        futures::executor::block_on(app.init());
        assert_eq!(app.active_agent_name(), "coder");
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
        let mut app = App::new(backend);
        futures::executor::block_on(app.init());

        // Move to index 1 (plan)
        run(&mut app, tab_key());
        assert_eq!(app.active_agent_name(), "plan");

        // Shift+Tab should go back to build
        run(&mut app, shift_tab_key());
        assert_eq!(app.active_agent_name(), "build");
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
        let mut app = App::new(backend);
        futures::executor::block_on(app.init());

        // Tab at index 0 → index 1
        run(&mut app, tab_key());
        assert_eq!(app.active_agent_name(), "plan");
        // Tab at index 1 → wraps to index 0
        run(&mut app, tab_key());
        assert_eq!(app.active_agent_name(), "build");
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
        let mut app = App::new(backend);
        futures::executor::block_on(app.init());

        // Shift+Tab at index 0 → wraps to last index
        run(&mut app, shift_tab_key());
        assert_eq!(app.active_agent_name(), "plan");
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
        let mut app = App::new(backend);
        futures::executor::block_on(app.init());
        assert_eq!(app.active_agent_name(), "build");

        run(&mut app, tab_key());
        assert_eq!(app.active_agent_name(), "plan");
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
        backend.prompt_events = vec![Ok(ocpncord_backend::BackendEvent::Done)];
        let mut app = App::new(backend);

        futures::executor::block_on(app.init());
        // Tab to "plan"
        run(&mut app, tab_key());
        assert_eq!(app.active_agent_name(), "plan");

        // Send a message
        run(&mut app, char_key('h'));
        let still_running = run(&mut app, enter_key());
        assert!(still_running, "app should keep running after send");

        // Verify session was created
        assert_eq!(app.backend().sessions.len(), 1, "session should be created");
        assert_eq!(
            app.pending_prompts(),
            &[PendingPrompt {
                session_id: "mock-session-id".into(),
                text: "h".into(),
                agent: "plan".into()
            }],
            "prompt should be queued with the selected agent"
        );
    }

    #[test]
    fn explicit_quit_event_quits() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);
        assert!(!run(&mut app, Event::Quit));
    }

    fn enter_key() -> Event {
        Event::Key(KeyEvent {
            scancode: Scancode::Enter,
            modifiers: Modifiers::default(),
        })
    }

    #[test]
    fn send_message_starts_stream_and_accumulates_parts() {
        let mut backend = MockBackend::default();
        backend.prompt_events = vec![
            Ok(ocpncord_backend::BackendEvent::Part {
                part: ocpncord_backend::Part::Text(ocpncord_backend::TextPart {
                    text: "Hello".into(),
                }),
                delta: None,
            }),
            Ok(ocpncord_backend::BackendEvent::Done),
        ];
        let mut app = App::new(backend);

        run(&mut app, char_key('h'));
        run(&mut app, char_key('i'));
        let running = run(&mut app, enter_key());
        assert!(running);
        assert!(app.is_streaming());

        run(
            &mut app,
            Event::Backend(ocpncord_backend::BackendEvent::Part {
                part: ocpncord_backend::Part::Text(ocpncord_backend::TextPart {
                    text: "Hello".into(),
                }),
                delta: None,
            }),
        );
        assert_eq!(app.partial_parts().len(), 1);

        run(
            &mut app,
            Event::Backend(ocpncord_backend::BackendEvent::Done),
        );
        assert!(!app.is_streaming());
        // Done no longer flushes partial_parts to messages; the REST API
        // fallback populates messages when the server has persisted them.
        assert_eq!(app.messages().len(), 1, "only user msg (no flush on Done)");
        assert_eq!(
            app.partial_parts().len(),
            1,
            "content stays in partial_parts for rendering"
        );
    }

    #[test]
    fn streaming_keeps_prompt_editable_and_status_on_bottom_line() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);

        run(&mut app, char_key('h'));
        run(&mut app, char_key('i'));
        run(&mut app, enter_key());
        assert!(app.is_streaming());
        assert_eq!(app.prompt_text(), "");

        run(&mut app, char_key('n'));
        run(&mut app, char_key('e'));
        run(&mut app, char_key('x'));
        run(&mut app, char_key('t'));
        assert_eq!(app.prompt_text(), "next");

        run(&mut app, enter_key());
        assert_eq!(
            app.pending_prompts().len(),
            1,
            "the active prompt remains the only dispatched prompt"
        );
        assert_eq!(
            app.queued_prompts().len(),
            1,
            "Enter should queue another prompt while streaming"
        );
        assert_eq!(app.prompt_text(), "");

        let test_backend = TestBackend::new(80, 8);
        let mut terminal = Terminal::new(test_backend).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
        let screen = rendered_screen(&terminal);

        assert!(screen.contains(">"), "screen: {screen}");
        assert!(screen.contains("Agent is Responding"), "screen: {screen}");
        assert!(screen.contains("mode: primary"), "screen: {screen}");
        assert!(screen.contains("model: default"), "screen: {screen}");
    }

    #[test]
    fn queued_messages_render_after_active_assistant_and_dispatch_in_order() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);

        for ch in "first".chars() {
            run(&mut app, char_key(ch));
        }
        run(&mut app, enter_key());
        assert_eq!(app.take_pending_prompts()[0].text, "first");

        run(
            &mut app,
            Event::Backend(ocpncord_backend::BackendEvent::MessagePartUpdated {
                session_id: "mock-session-id".into(),
                part: ocpncord_backend::Part::Text(ocpncord_backend::TextPart {
                    text: "assistant one".into(),
                }),
            }),
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
            app.queued_prompts()
                .iter()
                .map(|prompt| prompt.text.as_str())
                .collect::<Vec<_>>(),
            vec!["second", "third"]
        );

        let test_backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(test_backend).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
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
            Event::Backend(ocpncord_backend::BackendEvent::Done),
        );

        let dispatched = app.take_pending_prompts();
        assert_eq!(dispatched.len(), 1);
        assert_eq!(dispatched[0].text, "second");
        assert_eq!(app.queued_prompts()[0].text, "third");
    }

    #[test]
    fn response_indicator_survives_early_done_for_late_sse_parts() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);

        run(&mut app, char_key('h'));
        run(&mut app, char_key('i'));
        run(&mut app, enter_key());
        assert!(app.is_streaming());

        run(
            &mut app,
            Event::Backend(ocpncord_backend::BackendEvent::Done),
        );
        assert!(app.is_streaming());

        let test_backend = TestBackend::new(80, 8);
        let mut terminal = Terminal::new(test_backend).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
        let screen = rendered_screen(&terminal);

        assert!(screen.contains("Agent is Responding"), "screen: {screen}");
    }

    #[test]
    fn queued_prompt_waits_for_assistant_activity_before_dispatching() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);

        for ch in "first".chars() {
            run(&mut app, char_key(ch));
        }
        run(&mut app, enter_key());
        assert_eq!(app.take_pending_prompts()[0].text, "first");

        for ch in "second".chars() {
            run(&mut app, char_key(ch));
        }
        run(&mut app, enter_key());
        assert_eq!(app.queued_prompts().len(), 1);

        run(
            &mut app,
            Event::Backend(ocpncord_backend::BackendEvent::Done),
        );

        assert!(
            app.take_pending_prompts().is_empty(),
            "early Done without assistant activity must not dispatch queued prompt"
        );
        assert_eq!(app.queued_prompts()[0].text, "second");
        assert!(app.is_streaming());
    }

    /// Test that the real SSE event path (MessagePartUpdated + Done) works.
    /// The SSE background task emits MessagePartUpdated (not Part), which
    /// requires is_streaming to be true.
    #[test]
    fn sse_message_part_updated_accumulates_when_streaming() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);

        // Type and send a message to enter streaming mode
        run(&mut app, char_key('h'));
        run(&mut app, char_key('i'));
        run(&mut app, enter_key());
        assert!(app.is_streaming(), "should be streaming after send");

        // Simulate SSE: MessagePartUpdated with a text part
        run(
            &mut app,
            Event::Backend(ocpncord_backend::BackendEvent::MessagePartUpdated {
                session_id: "mock-session-id".into(),
                part: ocpncord_backend::Part::Text(ocpncord_backend::TextPart {
                    text: "Hello from assistant".into(),
                }),
            }),
        );
        assert_eq!(
            app.partial_parts().len(),
            1,
            "MessagePartUpdated should push to partial_parts"
        );

        // Simulate SSE: Done
        run(
            &mut app,
            Event::Backend(ocpncord_backend::BackendEvent::Done),
        );
        assert!(!app.is_streaming());
        assert_eq!(app.messages().len(), 1, "only user msg (no flush on Done)");
        assert_eq!(
            app.partial_parts().len(),
            1,
            "content stays in partial_parts for rendering"
        );
    }

    #[test]
    fn duplicate_sse_part_events_do_not_duplicate_visible_assistant_text() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);

        for ch in "hello from regression test".chars() {
            run(&mut app, char_key(ch));
        }
        run(&mut app, enter_key());

        let session_id = "mock-session-id".to_string();
        let user_echo = ocpncord_backend::Part::Text(ocpncord_backend::TextPart {
            text: "hello from regression test".into(),
        });
        for _ in 0..2 {
            run(
                &mut app,
                Event::Backend(ocpncord_backend::BackendEvent::MessagePartUpdated {
                    session_id: session_id.clone(),
                    part: user_echo.clone(),
                }),
            );
        }
        assert!(
            app.partial_parts().is_empty(),
            "echoed user text should not become assistant partials"
        );

        let step_start = ocpncord_backend::Part::StepStart(ocpncord_backend::StepStartPart {
            snapshot: None,
            session_id: Some(session_id.clone()),
        });
        for _ in 0..2 {
            run(
                &mut app,
                Event::Backend(ocpncord_backend::BackendEvent::MessagePartUpdated {
                    session_id: session_id.clone(),
                    part: step_start.clone(),
                }),
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
            text: "Hello! 👋 How can I help you today?".into(),
        });
        for _ in 0..2 {
            run(
                &mut app,
                Event::Backend(ocpncord_backend::BackendEvent::MessagePartUpdated {
                    session_id: session_id.clone(),
                    part: final_text.clone(),
                }),
            );
        }

        let tool = ocpncord_backend::Part::Tool(ocpncord_backend::ToolPart {
            tool: "bash".into(),
            state: ocpncord_backend::ToolState::Pending {
                input: Default::default(),
                raw: String::new(),
            },
        });
        for _ in 0..2 {
            run(
                &mut app,
                Event::Backend(ocpncord_backend::BackendEvent::MessagePartUpdated {
                    session_id: session_id.clone(),
                    part: tool.clone(),
                }),
            );
        }

        let assistant_text_parts: Vec<&str> = app
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
            app.partial_parts()
                .iter()
                .filter(|part| matches!(part, ocpncord_backend::Part::Tool(_)))
                .count(),
            1,
            "duplicate tool updates should be collapsed"
        );
        assert_eq!(
            app.messages().len(),
            1,
            "only the user message is committed"
        );
    }

    /// Test that MessagePartDelta accumulates text during streaming.
    #[test]
    fn sse_message_part_delta_accumulates_text_when_streaming() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);

        // Send message to enter streaming mode
        run(&mut app, char_key('h'));
        run(&mut app, char_key('i'));
        run(&mut app, enter_key());
        assert!(app.is_streaming());

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
            app.partial_parts().len(),
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
            app.partial_parts().len(),
            1,
            "second delta should update existing text part, not add a new one"
        );

        // Verify the accumulated text
        match &app.partial_parts()[0] {
            ocpncord_backend::Part::Text(tp) => {
                assert_eq!(tp.text, "Hello world");
            }
            _ => panic!("expected text part"),
        }

        // Finalize
        run(
            &mut app,
            Event::Backend(ocpncord_backend::BackendEvent::Done),
        );
        assert!(!app.is_streaming());
        assert_eq!(app.messages().len(), 1, "only user msg (no flush on Done)");
        assert_eq!(
            app.partial_parts().len(),
            1,
            "content stays in partial_parts for rendering"
        );
    }

    #[test]
    fn reasoning_delta_updates_reasoning_part_in_place() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);

        run(&mut app, char_key('h'));
        run(&mut app, char_key('i'));
        run(&mut app, enter_key());

        run(
            &mut app,
            Event::Backend(ocpncord_backend::BackendEvent::MessagePartUpdated {
                session_id: "mock-session-id".into(),
                part: ocpncord_backend::Part::Reasoning(ocpncord_backend::ReasoningPart {
                    text: String::new(),
                }),
            }),
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

        assert_eq!(app.partial_parts().len(), 1);
        match &app.partial_parts()[0] {
            ocpncord_backend::Part::Reasoning(reasoning) => {
                assert_eq!(reasoning.text, "thinking");
            }
            other => panic!("expected reasoning part, got {other:?}"),
        }

        let final_reasoning = ocpncord_backend::Part::Reasoning(ocpncord_backend::ReasoningPart {
            text: "thinking done".into(),
        });
        run(
            &mut app,
            Event::Backend(ocpncord_backend::BackendEvent::MessagePartUpdated {
                session_id: "mock-session-id".into(),
                part: final_reasoning,
            }),
        );

        assert_eq!(
            app.partial_parts().len(),
            1,
            "final reasoning update should replace the streaming placeholder"
        );
        match &app.partial_parts()[0] {
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
        let mut app = App::new(backend);

        // NOT streaming — just sitting on the start page
        assert!(!app.is_streaming());

        run(
            &mut app,
            Event::Backend(ocpncord_backend::BackendEvent::MessagePartUpdated {
                session_id: "ses1".into(),
                part: ocpncord_backend::Part::Text(ocpncord_backend::TextPart {
                    text: "Stale event".into(),
                }),
            }),
        );
        assert_eq!(
            app.partial_parts().len(),
            0,
            "parts should NOT accumulate when not streaming"
        );
    }

    #[test]
    fn backend_error_during_stream_shows_error_and_clears_stream() {
        let mut backend = MockBackend::default();
        backend.prompt_events = vec![Ok(ocpncord_backend::BackendEvent::Error {
            message: "connection lost".into(),
        })];
        let mut app = App::new(backend);

        run(&mut app, char_key('h'));
        run(&mut app, enter_key());
        assert!(app.is_streaming());

        run(
            &mut app,
            Event::Backend(ocpncord_backend::BackendEvent::Error {
                message: "connection lost".into(),
            }),
        );
        assert!(!app.is_streaming());
        assert!(app.error().unwrap_or("").contains("connection lost"));
    }

    #[test]
    fn session_creation_error_shows_error_and_stays_on_start_page() {
        let mut backend = MockBackend::default();
        backend.fail_create_session = Some(ocpncord_backend::BackendError::Api {
            status: 500,
            message: "server error".into(),
        });
        let mut app = App::new(backend);

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
            app.active_screen(),
            ScreenId::StartPage,
            "should stay on StartPage on error"
        );
        assert!(
            app.error().unwrap_or("").contains("server error"),
            "error should contain failure message"
        );
    }

    #[test]
    fn typing_enter_creates_session_and_switches_to_chat() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);
        assert_eq!(app.active_screen(), ScreenId::StartPage);

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
        assert_eq!(app.active_screen(), ScreenId::Chat);
        assert_eq!(app.draft(), Some("hi"));
    }

    #[test]
    fn enter_on_empty_input_does_nothing() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);

        let running = run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::Enter,
                modifiers: Modifiers::default(),
            }),
        );
        assert!(running);

        assert_eq!(app.active_screen(), ScreenId::StartPage);
        assert_eq!(app.backend().sessions.len(), 0);
    }

    fn make_session(id: &str, title: &str) -> ocpncord_backend::Session {
        ocpncord_backend::Session {
            id: id.into(),
            title: title.into(),
            project_id: "p1".into(),
            directory: "/".into(),
            parent_id: None,
            time: ocpncord_backend::SessionTime {
                created: 0,
                updated: 0,
            },
            slug: String::new(),
            version: String::new(),
            workspace_id: None,
            summary: None,
            share: None,
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
        let mut app = App::new(backend);

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
        terminal.draw(|frame| app.render(frame)).unwrap();
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
        let mut app = App::new(backend);

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
        terminal.draw(|frame| app.render(frame)).unwrap();
        let buf = terminal.backend().buffer();
        let has_empty_msg = buf.content().iter().any(|c| c.symbol() == "N");
        assert!(has_empty_msg, "Empty state should show 'No sessions yet'");
    }

    #[test]
    fn ctrl_x_l_opens_session_list_modal() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);

        run(&mut app, ctrl('x'));
        run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::Char('l'),
                modifiers: Modifiers::default(),
            }),
        );

        assert!(
            app.active_modal().is_some(),
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

        let mut app = App::new(MockBackend::default());
        app.set_active_modal(Box::new(TestCloseModal));
        assert!(
            app.active_modal().is_some(),
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
            app.active_modal().is_none(),
            "Escape should close the modal"
        );
    }

    #[test]
    fn slash_sessions_opens_modal() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);

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
            app.active_modal().is_some(),
            "/sessions should open the session list modal"
        );
    }

    #[test]
    fn unknown_slash_command_submits_as_message() {
        let mut backend = MockBackend::default();
        backend.prompt_events = vec![Ok(ocpncord_backend::BackendEvent::Done)];
        let mut app = App::new(backend);

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
            app.active_screen(),
            ScreenId::Chat,
            "unknown command should transition to chat"
        );
        assert!(
            app.messages().len() > 0,
            "unknown command should add a message"
        );
    }

    #[test]
    fn new_command_creates_session_and_stays_on_chat() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);

        run(&mut app, char_key('h'));
        run(&mut app, char_key('i'));
        run(&mut app, enter_key());
        assert_eq!(app.active_screen(), ScreenId::Chat);

        // Complete the stream so input is accepted again
        run(
            &mut app,
            Event::Backend(ocpncord_backend::BackendEvent::Done),
        );

        let session_count_before = app.backend().sessions.len();

        run(&mut app, char_key('/'));
        run(&mut app, char_key('n'));
        run(&mut app, char_key('e'));
        run(&mut app, char_key('w'));
        run(&mut app, enter_key());

        assert_eq!(app.active_screen(), ScreenId::Chat);
        assert_eq!(
            app.backend().sessions.len(),
            session_count_before + 1,
            "/new should create a new session"
        );
    }

    #[test]
    fn slash_models_opens_modal() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);

        run(&mut app, char_key('/'));
        run(&mut app, char_key('m'));
        run(&mut app, char_key('o'));
        run(&mut app, char_key('d'));
        run(&mut app, char_key('e'));
        run(&mut app, char_key('l'));
        run(&mut app, char_key('s'));
        run(&mut app, enter_key());

        assert!(
            app.active_modal().is_some(),
            "/models should open the model picker modal"
        );
    }

    #[test]
    fn slash_help_opens_modal() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);

        run(&mut app, char_key('/'));
        run(&mut app, char_key('h'));
        run(&mut app, char_key('e'));
        run(&mut app, char_key('l'));
        run(&mut app, char_key('p'));
        run(&mut app, enter_key());

        assert!(
            app.active_modal().is_some(),
            "/help should open the help modal"
        );
    }

    #[test]
    fn slash_todos_selects_todos_panel_and_clears_prompt() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);

        for ch in "/todos".chars() {
            run(&mut app, char_key(ch));
        }
        run(&mut app, enter_key());

        assert!(app.side_panel_visible, "/todos should open the side panel");
        assert_eq!(app.side_panel_tab, Tab::Todos);
        assert_eq!(app.prompt_text(), "", "/todos should clear the prompt");
    }

    #[test]
    fn ctrl_x_o_toggles_todos_panel() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);

        run(&mut app, ctrl('x'));
        run(&mut app, char_key('o'));
        assert!(app.side_panel_visible);
        assert_eq!(app.side_panel_tab, Tab::Todos);

        run(&mut app, ctrl('x'));
        run(&mut app, char_key('o'));
        assert!(!app.side_panel_visible);
        assert_eq!(app.side_panel_tab, Tab::Todos);
    }

    #[test]
    fn ctrl_x_d_selects_diagnostics_when_todos_panel_is_open() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);

        run(&mut app, ctrl('x'));
        run(&mut app, char_key('o'));
        assert_eq!(app.side_panel_tab, Tab::Todos);

        run(&mut app, ctrl('x'));
        run(&mut app, char_key('d'));
        assert!(app.side_panel_visible);
        assert_eq!(app.side_panel_tab, Tab::Diagnostics);
    }

    #[test]
    fn ctrl_x_m_opens_model_picker_modal() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);

        run(&mut app, ctrl('x'));
        run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::Char('m'),
                modifiers: Modifiers::default(),
            }),
        );

        assert!(
            app.active_modal().is_some(),
            "Ctrl+X M should open the model picker modal"
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
        let mut app = App::new(backend);

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
    fn ctrl_x_h_opens_help_modal() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);

        run(&mut app, ctrl('x'));
        run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::Char('h'),
                modifiers: Modifiers::default(),
            }),
        );

        assert!(
            app.active_modal().is_some(),
            "Ctrl+X H should open the help modal"
        );
    }

    #[test]
    fn escape_closes_help_modal() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);

        run(&mut app, ctrl('x'));
        run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::Char('h'),
                modifiers: Modifiers::default(),
            }),
        );
        assert!(app.active_modal().is_some(), "help modal should be open");

        run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::Escape,
                modifiers: Modifiers::default(),
            }),
        );
        assert!(
            app.active_modal().is_none(),
            "Escape should close the help modal"
        );
    }

    #[test]
    fn modal_overlay_clears_background_symbols() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);

        // Open help modal on start page so logo gets drawn under the overlay
        run(&mut app, ctrl('x'));
        run(&mut app, char_key('h'));
        assert!(app.active_modal().is_some(), "help modal should be open");

        let test_backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(test_backend).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
        let buf = terminal.backend().buffer();

        // app.render() draws the start page first, then clears only the
        // modal rectangle before drawing the modal block. Background content
        // may remain outside the modal, but must not bleed through inside it.
        let modal_area = Rect::new(15, 0, 50, 24);
        let mut has_logo_block_inside_modal = false;
        for y in modal_area.top()..modal_area.bottom() {
            for x in modal_area.left()..modal_area.right() {
                if buf.cell((x, y)).is_some_and(|c| c.symbol() == "█") {
                    has_logo_block_inside_modal = true;
                }
            }
        }
        assert!(
            !has_logo_block_inside_modal,
            "modal area should not contain logo block characters"
        );
        let has_logo_block_anywhere = buf.content().iter().any(|c| c.symbol() == "█");
        assert!(
            !has_logo_block_anywhere,
            "modal backdrop should hide the underlying screen"
        );

        // Modal text should still render correctly
        let has_slash = buf.content().iter().any(|c| c.symbol() == "/");
        assert!(has_slash, "modal should render slash commands");
    }

    #[test]
    fn ctrl_x_leader_does_not_leak_to_prompt_bar() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);

        // Type "hello" into the prompt bar
        run(&mut app, char_key('h'));
        run(&mut app, char_key('e'));
        run(&mut app, char_key('l'));
        run(&mut app, char_key('l'));
        run(&mut app, char_key('o'));
        assert_eq!(app.prompt_text(), "hello", "should have typed hello");

        // Press Ctrl+X (leader key) — this must NOT leak 'x' to the prompt bar
        run(&mut app, ctrl('x'));
        assert_eq!(
            app.prompt_text(),
            "hello",
            "ctrl+x should not leak 'x' to prompt bar"
        );

        // Complete the leader chord with 'h' for help
        run(&mut app, char_key('h'));
        assert!(
            app.active_modal().is_some(),
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
            app.prompt_text(),
            "hello",
            "prompt bar should preserve input through modal lifecycle"
        );
    }

    #[test]
    fn ctrl_x_t_toggles_terminal_screen() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);
        assert_eq!(app.active_screen(), ScreenId::StartPage);

        // Ctrl+X T on StartPage → Terminal
        run(&mut app, ctrl('x'));
        run(&mut app, char_key('t'));
        assert_eq!(
            app.active_screen(),
            ScreenId::Terminal,
            "ctrl+x t should switch to terminal"
        );

        // Ctrl+X T again → back to StartPage
        run(&mut app, ctrl('x'));
        run(&mut app, char_key('t'));
        assert_eq!(
            app.active_screen(),
            ScreenId::StartPage,
            "ctrl+x t again should return to start page"
        );
    }

    #[test]
    fn terminal_screen_plain_keys_do_not_mutate_prompt() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);

        run(&mut app, ctrl('x'));
        run(&mut app, char_key('t'));
        assert_eq!(app.active_screen(), ScreenId::Terminal);

        run(&mut app, char_key('a'));
        run(&mut app, char_key('b'));
        assert_eq!(app.prompt_text(), "");
    }

    #[test]
    fn terminal_screen_scrolls_from_bottom_with_arrow_and_page_keys() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);
        app.terminal.pty_id = Some("pty-1".into());
        app.terminal.command = "sh".into();
        for idx in 0..12 {
            app.terminal.push_line(TermLine {
                content: alloc::format!("line-{idx}"),
                is_error: false,
            });
        }

        run(&mut app, ctrl('x'));
        run(&mut app, char_key('t'));
        assert_eq!(app.active_screen(), ScreenId::Terminal);

        let test_backend = TestBackend::new(40, 6);
        let mut terminal = Terminal::new(test_backend).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
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
        assert_eq!(app.terminal.scroll, 1);

        run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::PageUp,
                modifiers: Modifiers::default(),
            }),
        );
        assert_eq!(app.terminal.scroll, 5);

        run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::PageDown,
                modifiers: Modifiers::default(),
            }),
        );
        assert_eq!(app.terminal.scroll, 1);
    }

    #[test]
    fn terminal_screen_scroll_offset_changes_visible_output() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);
        app.terminal.pty_id = Some("pty-1".into());
        app.terminal.command = "sh".into();
        for idx in 0..12 {
            app.terminal.push_line(TermLine {
                content: alloc::format!("line-{idx}"),
                is_error: false,
            });
        }
        app.terminal.scroll = 3;

        run(&mut app, ctrl('x'));
        run(&mut app, char_key('t'));

        let test_backend = TestBackend::new(40, 6);
        let mut terminal = Terminal::new(test_backend).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
        let screen = rendered_screen(&terminal);

        assert!(screen.contains("line-5"), "screen: {screen}");
        assert!(screen.contains("line-8"), "screen: {screen}");
        assert!(!screen.contains("line-11"), "screen: {screen}");
    }

    #[test]
    fn diagnostics_panel_renders_table_columns() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);
        app.side_panel_visible = true;
        app.side_panel_tab = crate::screen::Tab::Diagnostics;
        app.lsp_diagnostics.insert(
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
        terminal.draw(|frame| app.render(frame)).unwrap();
        let screen = rendered_screen(&terminal);

        assert!(screen.contains("src/main.rs"), "screen: {screen}");
        assert!(screen.contains("error"), "screen: {screen}");
        assert!(screen.contains("12:4"), "screen: {screen}");
        assert!(screen.contains("missing semicolon"), "screen: {screen}");
    }

    #[test]
    fn todos_panel_renders_list_items_with_status_styles() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);
        app.side_panel_visible = true;
        app.side_panel_tab = crate::screen::Tab::Todos;
        app.todos = vec![
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
        terminal.draw(|frame| app.render(frame)).unwrap();
        let screen = rendered_screen(&terminal);

        assert!(screen.contains("[x] done task"), "screen: {screen}");
        assert!(screen.contains("[ ] active task"), "screen: {screen}");

        let buf = terminal.backend().buffer();
        let panel_x = 57;
        assert_eq!(
            buf[(panel_x, 2)].style().fg,
            app.theme.text_dim.fg,
            "completed todo should use dim style"
        );
        assert_eq!(
            buf[(panel_x, 3)].style().fg,
            app.theme.text.fg,
            "active todo should use normal text style"
        );
    }

    #[test]
    fn terminal_side_panel_uses_terminal_scroll_offset() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);
        app.side_panel_visible = true;
        app.side_panel_tab = crate::screen::Tab::Pane;
        app.terminal.pty_id = Some("pty-1".into());
        app.terminal.command = "sh".into();
        for idx in 0..10 {
            app.terminal.push_line(TermLine {
                content: alloc::format!("line-{idx}"),
                is_error: false,
            });
        }
        app.terminal.scroll = 2;

        let test_backend = TestBackend::new(80, 6);
        let mut terminal = Terminal::new(test_backend).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
        let screen = rendered_screen(&terminal);

        assert!(screen.contains("line-5"), "screen: {screen}");
        assert!(screen.contains("line-7"), "screen: {screen}");
        assert!(!screen.contains("line-2"), "screen: {screen}");
        assert!(!screen.contains("line-9"), "screen: {screen}");
    }

    #[test]
    fn prompt_row_stays_visible_at_narrow_widths() {
        let backend = MockBackend::default();
        let app = App::new(backend);

        let test_backend = TestBackend::new(24, 6);
        let mut terminal = Terminal::new(test_backend).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
        let screen = rendered_screen(&terminal);

        assert!(screen.contains(">"), "screen: {screen}");
    }

    #[test]
    fn escape_clears_prompt_when_no_modal_is_open() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);

        run(&mut app, char_key('h'));
        run(&mut app, char_key('i'));
        assert_eq!(app.prompt_text(), "hi");

        run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::Escape,
                modifiers: Modifiers::default(),
            }),
        );
        assert_eq!(app.prompt_text(), "");
    }

    #[test]
    fn command_palette_enter_closes_palette_before_applying_action() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);

        run(&mut app, ctrl('p'));
        assert_eq!(
            app.active_modal().map(|modal| modal.title()),
            Some("Command Palette")
        );

        run(
            &mut app,
            Event::Key(KeyEvent {
                scancode: Scancode::Enter,
                modifiers: Modifiers::default(),
            }),
        );
        assert_eq!(app.active_modal().map(|modal| modal.title()), Some("Help"));
    }

    #[test]
    fn terminal_screen_shows_message_when_no_pty() {
        let backend = MockBackend::default();
        let mut app = App::new(backend);

        // Switch to terminal
        run(&mut app, ctrl('x'));
        run(&mut app, char_key('t'));
        assert_eq!(app.active_screen(), ScreenId::Terminal);

        // Render and verify helpful message
        let test_backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(test_backend).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
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
        let mut app = App::new(backend);

        // Toggle side panel on (while on start page, which has the logo)
        run(&mut app, ctrl('x'));
        run(&mut app, char_key('d'));
        assert!(app.side_panel_visible, "side panel should be visible");

        // Render and check panel area
        let test_backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(test_backend).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
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
        backend.prompt_events = vec![
            Ok(ocpncord_backend::BackendEvent::Part {
                part: ocpncord_backend::Part::Text(ocpncord_backend::TextPart {
                    text: "streaming".into(),
                }),
                delta: None,
            }),
            Ok(ocpncord_backend::BackendEvent::Done),
        ];
        let mut app = App::new(backend);
        futures::executor::block_on(app.init());

        run(&mut app, char_key('h'));
        run(&mut app, enter_key());
        assert!(app.is_streaming(), "should be streaming after send");

        let running = run(&mut app, ctrl('c'));
        assert!(
            running,
            "ctrl+c during streaming should interrupt, not quit"
        );
        assert!(!app.is_streaming(), "streaming should be stopped");
    }
}
