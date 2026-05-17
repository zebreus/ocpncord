use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

use ocpncord_backend::{Backend, BackendEvent};

use crate::chat::{render_chat, Chat};
use crate::command_palette::CommandPaletteModal;
use crate::event::{Event, Scancode};
use crate::key_chord::KeyChord;
use crate::modal::{HelpModal, Modal, ModelPickerModal, SessionListModal};
use crate::prompt_bar::PromptBar;
use crate::screen::{Action, ModalId, Screen, ScreenId};
use crate::start_page::StartPage;
use crate::theme::Theme;
use ratatui_core::layout::{Position, Rect};
use ratatui_core::style::{Color, Style};
use ratatui_core::text::Text;
use ratatui_core::widgets::Widget;

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
    partial_parts: Vec<ocpncord_backend::Part>,
    /// Accumulated delta text per part_id for real-time streaming.
    partial_texts: alloc::collections::BTreeMap<String, String>,
    messages: Vec<LoadedMessage>,
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
    pending_permissions: alloc::collections::VecDeque<(ocpncord_backend::PermissionRequest, String)>,
    pending_questions: alloc::collections::VecDeque<(ocpncord_backend::QuestionRequest, String)>,
    // Toast notifications
    toasts: alloc::collections::VecDeque<Toast>,
    // Side panel state
    side_panel_visible: bool,
    side_panel_tab: crate::screen::Tab,
    side_panel_scroll: u16,
// Track the screen before switching to Terminal for toggle back
    screen_before_terminal: ScreenId,
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
            partial_parts: Vec::new(),
            partial_texts: alloc::collections::BTreeMap::new(),
            messages: Vec::new(),
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
    pub async fn poll_next_event(&mut self) -> Option<Result<BackendEvent, ocpncord_backend::BackendError>> {
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

    pub fn tick(&self) -> u64 {
        self.tick
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
                    return true;
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
                        self.partial_parts.push(part);
                    }
                    ocpncord_backend::BackendEvent::Done => {
                        self.is_streaming = false;
                        self.stream = None;
                        self.partial_texts.clear();

                        // REST API fallback: fetch all messages to fill in any
                        // that SSE events might have missed.
                        if let Some(ref session) = self.active_session.clone() {
                            let session_id = session.id.clone();
                            if let Ok(summaries) = self.backend.list_messages(&session_id).await {
                                let mut api_messages: Vec<LoadedMessage> = Vec::new();
                                for summary in &summaries {
                                    if let Ok(detail) = self.backend.get_message(&session_id, &summary.id).await {
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
                    }
                    ocpncord_backend::BackendEvent::Error { message } => {
                        self.error = Some(message);
                        self.partial_parts.clear();
                        self.is_streaming = false;
                        self.stream = None;
                        self.partial_texts.clear();
                    }
                    ocpncord_backend::BackendEvent::SessionCreated { session } => {
                        let is_new = self.active_session.as_ref().map(|s| s.id != session.id).unwrap_or(true);
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
                if self.is_streaming {
                            let parts = core::mem::take(&mut self.partial_parts);
                            if !parts.is_empty() {
                                self.messages.push(LoadedMessage {
                                    role: ocpncord_backend::MessageRole::Assistant,
                                    parts,
                                });
                            }
                            self.is_streaming = false;
                            self.stream = None;
                            self.partial_texts.clear();
                        }
                    }
                    ocpncord_backend::BackendEvent::MessageRemoved { .. } => {}
                    ocpncord_backend::BackendEvent::MessagePartUpdated { session_id, ref part } => {
                        if self.active_session.as_ref().map(|s| s.id.as_str()) == Some(session_id.as_str()) {
                            self.partial_parts.push(part.clone());
                        }
                    }
                    ocpncord_backend::BackendEvent::MessagePartDelta { ref session_id, part_id, field, delta, .. } if field == "text"
                        && self.active_session.as_ref().map(|s| s.id.as_str()) == Some(session_id.as_str()) =>
                    {
                        let acc = self.partial_texts.entry(part_id).or_default();
                        acc.push_str(&delta);
                        let new_part = ocpncord_backend::Part::Text(ocpncord_backend::TextPart {
                            text: acc.clone(),
                        });
                        let replaced = self.partial_parts.iter_mut().rev().find_map(|p| {
                            if matches!(p, ocpncord_backend::Part::Text(_)) {
                                *p = new_part.clone();
                                Some(())
                            } else {
                                None
                            }
                        });
                        if replaced.is_none() {
                            self.partial_parts.push(new_part);
                        }
                    }
                    ocpncord_backend::BackendEvent::MessagePartDelta { .. } => {}
                    ocpncord_backend::BackendEvent::MessagePartRemoved { .. } => {}
                    ocpncord_backend::BackendEvent::PermissionAsked { request } => {
                        let sid = self.active_session.as_ref().map(|s| s.id.clone()).unwrap_or_default();
                        self.pending_permissions.push_back((request.clone(), sid));
                    }
                    ocpncord_backend::BackendEvent::PermissionReplied { .. } => {
                        self.pending_permissions.pop_front();
                    }
                    ocpncord_backend::BackendEvent::QuestionAsked { request } => {
                        let sid = self.active_session.as_ref().map(|s| s.id.clone()).unwrap_or_default();
                        self.pending_questions.push_back((request.clone(), sid));
                    }
                    ocpncord_backend::BackendEvent::QuestionRejected { .. } => {
                        self.pending_questions.pop_front();
                    }
                    ocpncord_backend::BackendEvent::QuestionReplied { .. } => {
                        self.pending_questions.pop_front();
                    }
                    ocpncord_backend::BackendEvent::CommandExecuted { name, arguments, .. } => {
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
            match self.backend.create_session("Chat", self.current_workspace.as_deref().unwrap_or("")).await {
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

        self.messages.push(LoadedMessage {
            role: ocpncord_backend::MessageRole::User,
            parts: vec![ocpncord_backend::Part::Text(ocpncord_backend::TextPart {
                text: text.clone(),
            })],
        });

        self.draft = Some(text.clone());
        self.prompt_bar.clear();
        self.active_screen = ScreenId::Chat;

        let agent = self.active_agent_name().to_string();

        // Set streaming BEFORE the POST so SSE events received during the
        // HTTP round-trip are not dropped (they arrive via the persistent
        // /global/event connection and check is_streaming).
        self.is_streaming = true;
        self.partial_parts = Vec::new();
        self.partial_texts.clear();

        match self.backend.prompt(&session_id, &text, Some(&agent)).await {
            Ok(_stream) => {}
            Err(e) => {
                self.is_streaming = false;
                self.error = Some(alloc::format!("{}", e));
            }
        }

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
                match self.backend.create_session("Chat", self.current_workspace.as_deref().unwrap_or("")).await {
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
                self.side_panel_visible = true;
                self.side_panel_tab = crate::screen::Tab::Todos;
                true
            }
            "/diagnostics" => {
                self.side_panel_visible = true;
                self.side_panel_tab = crate::screen::Tab::Diagnostics;
                true
            }
            "/pty" => {
                self.side_panel_visible = true;
                self.side_panel_tab = crate::screen::Tab::Pane;
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
            _ => {
                self.handle_unknown_slash_command(text).await
            }
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

        self.messages.push(LoadedMessage {
            role: ocpncord_backend::MessageRole::User,
            parts: vec![ocpncord_backend::Part::Text(ocpncord_backend::TextPart {
                text: text.into(),
            })],
        });

        self.draft = Some(text.into());
        self.prompt_bar.clear();
        self.active_screen = ScreenId::Chat;

        let agent = self.active_agent_name().to_string();

        self.is_streaming = true;
        self.partial_parts = Vec::new();
        self.partial_texts.clear();

        match self.backend.prompt(&session_id, text, Some(&agent)).await {
            Ok(_stream) => {}
            Err(e) => {
                self.is_streaming = false;
                self.error = Some(alloc::format!("{}", e));
            }
        }

        true
    }

    async fn handle_interrupt(&mut self) -> bool {
        if let Some(session) = &self.active_session {
            let _ = self.backend.abort_session(&session.id).await;
        }
        self.stream = None;
        self.is_streaming = false;
        self.partial_parts.clear();
        self.partial_texts.clear();
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
                            if let Ok(detail) = self.backend.get_message(&session_id, &summary.id).await {
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
                self.chat.scroll = self.chat.scroll.saturating_add(1);
            }
            Some(Action::ScrollDown) => {
                self.chat.scroll = self.chat.scroll.saturating_sub(1);
            }
            Some(Action::ToggleSidePanel) => {
                self.side_panel_visible = !self.side_panel_visible;
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

    pub fn render(&self, frame: &mut ratatui_core::terminal::Frame) {
        match self.active_screen {
            ScreenId::StartPage => {
                StartPage.render(frame, &self.theme);
                let area = frame.area();
                let prompt_area = Rect::new(
                    area.x + area.width.saturating_sub(50) / 2,
                    area.height.saturating_sub(8),
                    50.min(area.width),
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
            }
ScreenId::Chat => {
                 render_chat(
                     frame,
                     &self.theme,
                     &self.messages,
                     &self.partial_parts,
                     self.is_streaming,
                     self.chat.scroll,
                 );
                 let area = frame.area();
                 let prompt_area = Rect::new(area.x, area.height.saturating_sub(1), area.width, 1);
                 self.prompt_bar.render(
                     prompt_area,
                     frame,
                     &self.theme,
                     self.is_streaming,
                     self.active_agent_name(),
                     self.tick,
                 );
             }
             ScreenId::Terminal => {
                 self.render_terminal(frame);
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
            let area = frame.area();
            let panel_width = (area.width as f32 * 0.3) as u16;
            let panel_x = area.width - panel_width;
            let panel_area = Rect::new(panel_x, area.y, panel_width, area.height);

            // Background — clear stale symbols and set panel colour
            let buf = frame.buffer_mut();
            let panel_style = self.theme.side_panel_bg;
            for y in panel_area.top()..panel_area.bottom() {
                for x in panel_area.left()..panel_area.right() {
                    if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                        cell.set_symbol(" ");
                        cell.set_style(panel_style);
                    }
                }
            }

            // Tab bar
            let _tabs = ["Diagnostics", "Todos", "Terminal"];
            let tab_labels: [&str; 3] = ["Diagnostics", "Todos", "Terminal"];
            let mut tab_x = panel_x + 1;
            for (i, label) in tab_labels.iter().enumerate() {
                let is_active = match (i, self.side_panel_tab) {
                    (0, crate::screen::Tab::Diagnostics) => true,
                    (1, crate::screen::Tab::Todos) => true,
                    (2, crate::screen::Tab::Pane) => true,
                    _ => false,
                };
                let style = if is_active {
                    self.theme.side_panel_tab_active
                } else {
                    self.theme.side_panel_tab_inactive
                };
                let display = alloc::format!(" {} ", label);
                Text::from(display.as_str())
                    .style(style)
                    .render(Rect::new(tab_x, area.y, display.len() as u16, 1), frame.buffer_mut());
                tab_x += display.len() as u16 + 1;
            }

            // Content area
            let content_area = Rect::new(panel_x + 1, area.y + 1, panel_width - 2, area.height - 1);
            match self.side_panel_tab {
                crate::screen::Tab::Diagnostics => {
                    if self.lsp_diagnostics.is_empty() {
                        Text::from("No diagnostics")
                            .style(self.theme.text_dim)
                            .render(content_area, frame.buffer_mut());
                    } else {
                        let mut y = content_area.y;
                        for (file, diags) in self.lsp_diagnostics.iter() {
                            if y >= content_area.bottom() { break; }
                            Text::from(file.as_str())
                                .style(self.theme.side_panel_title)
                                .render(Rect::new(content_area.x, y, content_area.width, 1), frame.buffer_mut());
                            y += 1;
                            for diag in diags.iter() {
                                if y >= content_area.bottom() { break; }
                                let sev = diag.severity.as_deref().unwrap_or("info");
                                let display = alloc::format!("  [{}] {}:{} {}", sev, diag.line, diag.character, diag.message);
                                let display: String = display.chars().take(content_area.width as usize).collect();
                                Text::from(display.as_str())
                                    .style(self.theme.text)
                                    .render(Rect::new(content_area.x, y, content_area.width, 1), frame.buffer_mut());
                                y += 1;
                            }
                        }
                    }
                }
                crate::screen::Tab::Todos => {
                    if self.todos.is_empty() {
                        Text::from("No todos")
                            .style(self.theme.text_dim)
                            .render(content_area, frame.buffer_mut());
                    } else {
                        let mut y = content_area.y;
                        for todo in self.todos.iter() {
                            if y >= content_area.bottom() { break; }
                            let checkbox = if todo.status == "completed" { "[x]" } else { "[ ]" };
                            let display = alloc::format!("{} {}", checkbox, todo.content);
                            let display: String = display.chars().take(content_area.width as usize).collect();
                            let style = if todo.status == "completed" {
                                self.theme.text_dim
                            } else {
                                self.theme.text
                            };
                            Text::from(display.as_str())
                                .style(style)
                                .render(Rect::new(content_area.x, y, content_area.width, 1), frame.buffer_mut());
                            y += 1;
                        }
                    }
                }
                crate::screen::Tab::Pane => {
                    // Show PTY output in side panel
                    if self.terminal.pty_id.is_none() {
                        Text::from("No terminal")
                            .style(self.theme.text_dim)
                            .render(content_area, frame.buffer_mut());
                    } else {
                        let mut y = content_area.y;
                        for line in self.terminal.lines.iter().skip(self.terminal.scroll as usize) {
                            if y >= content_area.bottom() { break; }
                            let style = if line.is_error { self.theme.pty_error } else { self.theme.pty_output };
                            let display: String = line.content.chars().take(content_area.width as usize).collect();
                            Text::from(display.as_str())
                                .style(style)
                                .render(Rect::new(content_area.x, y, content_area.width, 1), frame.buffer_mut());
                            y += 1;
                        }
                    }
                }
            }
        }

        if let Some(ref modal) = self.active_modal {
            let area = frame.area();

            // Clear background symbols and apply dark overlay style.
            // set_style alone is not enough — it changes styles but leaves
            // stale symbols from the underlying screen (e.g. logo block
            // characters, chat text) in the buffer.
            {
                let buf = frame.buffer_mut();
                let dark = Style::new().bg(Color::Rgb(0, 0, 0)).fg(Color::Rgb(0, 0, 0));
                for y in area.top()..area.bottom() {
                    for x in area.left()..area.right() {
                        if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                            cell.set_symbol(" ");
                            cell.set_style(dark);
                        }
                    }
                }
            }

            let modal_width = (area.width as f32 * 0.6) as u16;
            let modal_height = (area.height as f32 * 0.7) as u16;
            let modal_x = area.x + (area.width.saturating_sub(modal_width)) / 2;
            let modal_y = area.y + (area.height.saturating_sub(modal_height)) / 2;

            use ratatui_core::symbols::border::ROUNDED;

            let border_style = self.theme.border;
            let buf = frame.buffer_mut();

            for x in modal_x..modal_x + modal_width {
                if let Some(cell) = buf.cell_mut(Position::new(x, modal_y)) {
                    cell.set_style(border_style).set_symbol(ROUNDED.horizontal_top);
                }
                if let Some(cell) = buf.cell_mut(Position::new(x, modal_y + modal_height - 1)) {
                    cell.set_style(border_style).set_symbol(ROUNDED.horizontal_bottom);
                }
            }
            for y in modal_y..modal_y + modal_height {
                if let Some(cell) = buf.cell_mut(Position::new(modal_x, y)) {
                    cell.set_style(border_style).set_symbol(ROUNDED.vertical_left);
                }
                if let Some(cell) = buf.cell_mut(Position::new(modal_x + modal_width - 1, y)) {
                    cell.set_style(border_style).set_symbol(ROUNDED.vertical_right);
                }
            }

            if let Some(cell) = buf.cell_mut(Position::new(modal_x, modal_y)) {
                cell.set_symbol(ROUNDED.top_left);
            }
            if let Some(cell) = buf.cell_mut(Position::new(modal_x + modal_width - 1, modal_y)) {
                cell.set_symbol(ROUNDED.top_right);
            }
            if let Some(cell) = buf.cell_mut(Position::new(modal_x, modal_y + modal_height - 1)) {
                cell.set_symbol(ROUNDED.bottom_left);
            }
            if let Some(cell) = buf.cell_mut(Position::new(modal_x + modal_width - 1, modal_y + modal_height - 1)) {
                cell.set_symbol(ROUNDED.bottom_right);
            }

            let title = modal.title();
            let title_style = self.theme.text_accent;
            let title_x = modal_x + 2;
            for (i, ch) in title.chars().enumerate() {
                let tx = title_x + i as u16;
                if tx < modal_x + modal_width - 1 {
                    if let Some(cell) = buf.cell_mut(Position::new(tx, modal_y)) {
                        cell.set_char(ch).set_style(title_style);
                    }
                }
            }

            let content_area = Rect::new(
                modal_x + 1,
                modal_y + 1,
                modal_width - 2,
                modal_height - 2,
            );
            modal.render(frame, &self.theme, content_area);
        }
    }

    fn render_terminal(&self, frame: &mut ratatui_core::terminal::Frame) {
        let area = frame.area();

        if self.terminal.pty_id.is_none() {
            let msg = "No active terminal — send a prompt to the agent to create one";
            let msg_width = msg.len() as u16;
            let x = (area.width.saturating_sub(msg_width)) / 2;
            let y = area.height / 2;
            Text::from(msg)
                .style(self.theme.text_dim)
                .render(Rect::new(x, y, msg_width.min(area.width), 1), frame.buffer_mut());
        } else {
            let pty_height = area.height.saturating_sub(1);

            let start_idx = self.terminal.scroll as usize;
            let lines: Vec<_> = self
                .terminal
                .lines
                .iter()
                .skip(start_idx)
                .take(pty_height as usize)
                .collect();

            for (i, line) in lines.iter().enumerate() {
                let y = area.y + i as u16;
                let style = if line.is_error {
                    self.theme.pty_error
                } else {
                    self.theme.pty_output
                };
                Text::from(line.content.as_str())
                    .style(style)
                    .render(Rect::new(area.x, y, area.width, 1), frame.buffer_mut());
            }

            let status_area = Rect::new(
                area.x,
                area.y + area.height - 1,
                area.width,
                1,
            );
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
                self.terminal.exit_code.map(|c| c.to_string()).unwrap_or_default(),
            );
            Text::from(status_str)
                .style(self.theme.pty_status_bar)
                .render(status_area, frame.buffer_mut());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{KeyEvent, Modifiers, Scancode};
    use ocpncord_backend::mock::MockBackend;
    use ratatui_core::backend::TestBackend;
    use ratatui_core::terminal::Terminal;

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
        // Verify prompt_events were consumed (prompt() was called)
        assert!(
            app.backend().prompt_events.is_empty(),
            "prompt_events should be consumed by prompt()"
        );
        assert_eq!(
            app.backend().last_prompt_agent.as_deref(),
            Some("plan"),
            "agent name should be passed to prompt()"
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

        run(&mut app, Event::Backend(ocpncord_backend::BackendEvent::Done));
        assert!(!app.is_streaming());
        // Done no longer flushes partial_parts to messages; the REST API
        // fallback populates messages when the server has persisted them.
        assert_eq!(app.messages().len(), 1, "only user msg (no flush on Done)");
        assert_eq!(app.partial_parts().len(), 1, "content stays in partial_parts for rendering");
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
        run(&mut app, Event::Backend(ocpncord_backend::BackendEvent::Done));
        assert!(!app.is_streaming());
        assert_eq!(app.messages().len(), 1, "only user msg (no flush on Done)");
        assert_eq!(app.partial_parts().len(), 1, "content stays in partial_parts for rendering");
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
        run(&mut app, Event::Backend(ocpncord_backend::BackendEvent::Done));
        assert!(!app.is_streaming());
        assert_eq!(app.messages().len(), 1, "only user msg (no flush on Done)");
        assert_eq!(app.partial_parts().len(), 1, "content stays in partial_parts for rendering");
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
        terminal
            .draw(|frame| app.render(frame))
            .unwrap();
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
        terminal
            .draw(|frame| app.render(frame))
            .unwrap();
        let buf = terminal.backend().buffer();
        let has_empty_msg = buf.content().iter().any(|c| c.symbol() == "N");
        assert!(
            has_empty_msg,
            "Empty state should show 'No sessions yet'"
        );
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
        use ratatui_core::terminal::Frame;

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
        assert!(app.messages().len() > 0, "unknown command should add a message");
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
        run(&mut app, Event::Backend(ocpncord_backend::BackendEvent::Done));

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

        // app.render() draws the start page logo first (█ characters),
        // then the modal dark overlay. The overlay's set_style() changes
        // cell styles but NOT symbols — logo block characters would survive
        // unless the symbol itself is cleared.
        let has_logo_block = buf.content().iter().any(|c| c.symbol() == "█");
        assert!(
            !has_logo_block,
            "modal overlay should not contain logo block characters — symbols must be cleared, not just styles"
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
        let buf = terminal.backend().buffer();

        let has_help = buf.content().iter().any(|c| c.symbol() == "a")
            && buf.content().iter().any(|c| c.symbol() == "g")
            && buf.content().iter().any(|c| c.symbol() == "e")
            && buf.content().iter().any(|c| c.symbol() == "n");
        assert!(has_help, "terminal should show a helpful message when no PTY");
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
        assert!(running, "ctrl+c during streaming should interrupt, not quit");
        assert!(!app.is_streaming(), "streaming should be stopped");
    }
}
