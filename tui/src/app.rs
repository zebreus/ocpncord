use alloc::boxed::Box;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

use opencode_backend::Backend;

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

/// A message held in memory, built from streaming Parts.
#[derive(Debug, Clone)]
pub struct LoadedMessage {
    pub role: opencode_backend::MessageRole,
    pub parts: Vec<opencode_backend::Part>,
}

/// Top-level application state.
pub struct App<B: Backend> {
    backend: B,
    active_screen: ScreenId,
    theme: Theme,
    key_chord: KeyChord,
    tick: u64,
    prompt_bar: PromptBar,
    chat: Chat,
    active_session: Option<opencode_backend::Session>,
    draft: Option<String>,
    error: Option<String>,
    is_streaming: bool,
    partial_parts: Vec<opencode_backend::Part>,
    messages: Vec<LoadedMessage>,
    stream: Option<B::PromptStream>,
    active_modal: Option<Box<dyn Modal>>,
    agents: Vec<opencode_backend::Agent>,
    active_agent: usize,
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
            messages: Vec::new(),
            stream: None,
            active_modal: None,
            agents: Vec::new(),
            active_agent: 0,
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

    pub fn active_session(&self) -> Option<&opencode_backend::Session> {
        self.active_session.as_ref()
    }

    pub fn take_stream(&mut self) -> Option<B::PromptStream> {
        self.stream.take()
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
                    .filter(|a| matches!(a.mode, opencode_backend::AgentMode::Primary))
                    .collect();
            }
            Err(_) => {}
        }
        if self.agents.is_empty() {
            self.agents = vec![
                opencode_backend::Agent {
                    name: "build".into(),
                    mode: opencode_backend::AgentMode::Primary,
                    description: None,
                    native: None,
                    hidden: None,
                    model: None,
                    color: None,
                    variant: None,
                    prompt: None,
                    steps: None,
                },
                opencode_backend::Agent {
                    name: "plan".into(),
                    mode: opencode_backend::AgentMode::Primary,
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

    pub fn partial_parts(&self) -> &[opencode_backend::Part] {
        &self.partial_parts
    }

    pub fn messages(&self) -> &[LoadedMessage] {
        &self.messages
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
                    return true;
                }

                if let Some(action) = self.key_chord.handle(key, self.tick) {
                    return self.apply_action(Some(action)).await;
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
                match event {
                    opencode_backend::BackendEvent::Part { part, .. } => {
                        self.partial_parts.push(part);
                    }
                    opencode_backend::BackendEvent::Done => {
                        let parts = core::mem::take(&mut self.partial_parts);
                        if !parts.is_empty() {
                            self.messages.push(LoadedMessage {
                                role: opencode_backend::MessageRole::Assistant,
                                parts,
                            });
                        }
                        self.is_streaming = false;
                        self.stream = None;
                    }
                    opencode_backend::BackendEvent::Error { message } => {
                        self.error = Some(message);
                        self.partial_parts.clear();
                        self.is_streaming = false;
                        self.stream = None;
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
            role: opencode_backend::MessageRole::User,
            parts: vec![opencode_backend::Part::Text(opencode_backend::TextPart {
                text: text.clone(),
            })],
        });

        self.draft = Some(text.clone());
        self.prompt_bar.clear();
        self.active_screen = ScreenId::Chat;

        let agent = self.active_agent_name().to_string();
        match self.backend.prompt(&session_id, &text, Some(&agent)).await {
            Ok(stream) => {
                self.stream = Some(stream);
                self.is_streaming = true;
                self.partial_parts = Vec::new();
            }
            Err(e) => {
                self.error = Some(alloc::format!("{}", e));
            }
        }

        true
    }

    async fn handle_slash_command(&mut self, text: &str) -> bool {
        match text {
            "/models" => {
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
                match self.backend.create_session("Chat", "").await {
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
            "/exit" => false,
            _ => {
                // Unknown command — submit as message
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
            role: opencode_backend::MessageRole::User,
            parts: vec![opencode_backend::Part::Text(opencode_backend::TextPart {
                text: text.into(),
            })],
        });

        self.draft = Some(text.into());
        self.prompt_bar.clear();
        self.active_screen = ScreenId::Chat;

        let agent = self.active_agent_name().to_string();
        match self.backend.prompt(&session_id, text, Some(&agent)).await {
            Ok(stream) => {
                self.stream = Some(stream);
                self.is_streaming = true;
                self.partial_parts = Vec::new();
            }
            Err(e) => {
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
        true
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
            Some(Action::OpenModal(_)) => {}
            Some(Action::LoadSession(ref id)) => {
                match self.backend.list_messages(id).await {
                    Ok(summaries) => {
                        // Load full messages
                        let mut messages = Vec::new();
                        for summary in summaries {
                            if let Ok(detail) =
                                self.backend.get_message(id, &summary.id).await
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
                self.active_session = self.backend.get_session(id).await.ok();
                self.active_modal = None;
                self.active_screen = ScreenId::Chat;
            }
            Some(Action::DeleteSession(ref id)) => {
                let _ = self.backend.delete_session(id).await;
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
                );
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

        if let Some(ref modal) = self.active_modal {
            let area = frame.area();
            frame
                .buffer_mut()
                .set_style(area, Style::new().bg(Color::Rgb(0, 0, 0)).fg(Color::Rgb(0, 0, 0)));

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{KeyEvent, Modifiers, Scancode};
    use opencode_backend::mock::MockBackend;
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
        backend.agents = vec![opencode_backend::Agent {
            name: "coder".into(),
            mode: opencode_backend::AgentMode::Primary,
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
            opencode_backend::Agent {
                name: "build".into(),
                mode: opencode_backend::AgentMode::Primary,
                description: None,
                native: None,
                hidden: None,
                model: None,
                color: None,
                variant: None,
                prompt: None,
                steps: None,
            },
            opencode_backend::Agent {
                name: "plan".into(),
                mode: opencode_backend::AgentMode::Primary,
                description: None,
                native: None,
                hidden: None,
                model: None,
                color: None,
                variant: None,
                prompt: None,
                steps: None,
            },
            opencode_backend::Agent {
                name: "coder".into(),
                mode: opencode_backend::AgentMode::Primary,
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
            opencode_backend::Agent {
                name: "build".into(),
                mode: opencode_backend::AgentMode::Primary,
                description: None,
                native: None,
                hidden: None,
                model: None,
                color: None,
                variant: None,
                prompt: None,
                steps: None,
            },
            opencode_backend::Agent {
                name: "plan".into(),
                mode: opencode_backend::AgentMode::Primary,
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
            opencode_backend::Agent {
                name: "build".into(),
                mode: opencode_backend::AgentMode::Primary,
                description: None,
                native: None,
                hidden: None,
                model: None,
                color: None,
                variant: None,
                prompt: None,
                steps: None,
            },
            opencode_backend::Agent {
                name: "plan".into(),
                mode: opencode_backend::AgentMode::Primary,
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
            opencode_backend::Agent {
                name: "build".into(),
                mode: opencode_backend::AgentMode::Primary,
                description: None,
                native: None,
                hidden: None,
                model: None,
                color: None,
                variant: None,
                prompt: None,
                steps: None,
            },
            opencode_backend::Agent {
                name: "plan".into(),
                mode: opencode_backend::AgentMode::Primary,
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
            opencode_backend::Agent {
                name: "build".into(),
                mode: opencode_backend::AgentMode::Primary,
                description: None,
                native: None,
                hidden: None,
                model: None,
                color: None,
                variant: None,
                prompt: None,
                steps: None,
            },
            opencode_backend::Agent {
                name: "plan".into(),
                mode: opencode_backend::AgentMode::Primary,
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
        backend.prompt_events = vec![Ok(opencode_backend::BackendEvent::Done)];
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
            Ok(opencode_backend::BackendEvent::Part {
                part: opencode_backend::Part::Text(opencode_backend::TextPart {
                    text: "Hello".into(),
                }),
                delta: None,
            }),
            Ok(opencode_backend::BackendEvent::Done),
        ];
        let mut app = App::new(backend);

        run(&mut app, char_key('h'));
        run(&mut app, char_key('i'));
        let running = run(&mut app, enter_key());
        assert!(running);
        assert!(app.is_streaming());

        run(
            &mut app,
            Event::Backend(opencode_backend::BackendEvent::Part {
                part: opencode_backend::Part::Text(opencode_backend::TextPart {
                    text: "Hello".into(),
                }),
                delta: None,
            }),
        );
        assert_eq!(app.partial_parts().len(), 1);

        run(&mut app, Event::Backend(opencode_backend::BackendEvent::Done));
        assert!(!app.is_streaming());
        assert_eq!(app.messages().len(), 2, "user msg + assistant msg");
    }

    #[test]
    fn backend_error_during_stream_shows_error_and_clears_stream() {
        let mut backend = MockBackend::default();
        backend.prompt_events = vec![Ok(opencode_backend::BackendEvent::Error {
            message: "connection lost".into(),
        })];
        let mut app = App::new(backend);

        run(&mut app, char_key('h'));
        run(&mut app, enter_key());
        assert!(app.is_streaming());

        run(
            &mut app,
            Event::Backend(opencode_backend::BackendEvent::Error {
                message: "connection lost".into(),
            }),
        );
        assert!(!app.is_streaming());
        assert!(app.error().unwrap_or("").contains("connection lost"));
    }

    #[test]
    fn session_creation_error_shows_error_and_stays_on_start_page() {
        let mut backend = MockBackend::default();
        backend.fail_create_session = Some(opencode_backend::BackendError::Api {
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

    fn make_session(id: &str, title: &str) -> opencode_backend::Session {
        opencode_backend::Session {
            id: id.into(),
            title: title.into(),
            project_id: "p1".into(),
            directory: "/".into(),
            parent_id: None,
            time: opencode_backend::SessionTime {
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
        backend.prompt_events = vec![Ok(opencode_backend::BackendEvent::Done)];
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
        run(&mut app, Event::Backend(opencode_backend::BackendEvent::Done));

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
}
