use ratatui_core::layout::Rect;
use ratatui_core::terminal::Frame;

use crate::event::{Event, Scancode};
use crate::screen::Action;
use crate::theme::Theme;

/// An overlay dialog drawn on top of a full-screen view.
pub trait Modal {
    fn render(&self, frame: &mut Frame, theme: &Theme, area: Rect);
    fn handle_event(&mut self, event: Event) -> Action;
    fn title(&self) -> &str;
}

// --- Session list modal ---

use alloc::string::String;
use alloc::vec::Vec;
use ocpncord_backend::{Config, Session};
use ratatui_core::text::Text;
use ratatui_core::widgets::Widget;

enum SessionListState {
    Loading,
    Loaded,
    Empty,
    Error(String),
}

pub struct SessionListModal {
    state: SessionListState,
    sessions: Vec<Session>,
    selected: usize,
    confirm_delete: Option<usize>,
}

impl SessionListModal {
    pub fn new() -> Self {
        Self {
            state: SessionListState::Loading,
            sessions: Vec::new(),
            selected: 0,
            confirm_delete: None,
        }
    }

    pub fn set_sessions(&mut self, sessions: Vec<Session>) {
        if sessions.is_empty() {
            self.state = SessionListState::Empty;
        } else {
            self.selected = self.selected.min(sessions.len().saturating_sub(1));
            self.sessions = sessions;
            self.state = SessionListState::Loaded;
        }
    }

    pub fn set_error(&mut self, error: String) {
        self.state = SessionListState::Error(error);
    }

    pub fn selected_index(&self) -> usize {
        self.selected
    }
}

impl Modal for SessionListModal {
    fn render(&self, frame: &mut Frame, theme: &Theme, area: Rect) {
        match self.state {
            SessionListState::Loading => {
                Text::from("Loading sessions...")
                    .style(theme.text_dim)
                    .render(area, frame.buffer_mut());
            }
            SessionListState::Empty => {
                Text::from("No sessions yet")
                    .style(theme.text_dim)
                    .render(area, frame.buffer_mut());
            }
            SessionListState::Loaded => {
                // Title
                Text::from("Sessions")
                    .style(theme.text_accent)
                    .render(Rect::new(area.x, area.y, area.width, 1), frame.buffer_mut());

                // Confirmation bar
                if self.confirm_delete == Some(self.selected) {
                    let confirm_msg = "Press Delete again to confirm, Escape to cancel";
                    Text::from(confirm_msg).style(theme.text_error).render(
                        Rect::new(area.x, area.y + 1, area.width, 1),
                        frame.buffer_mut(),
                    );
                }

                // Session list
                for (i, session) in self.sessions.iter().enumerate() {
                    let y = area.y + 2 + i as u16;
                    if y >= area.bottom() {
                        break;
                    }
                    let style = if i == self.selected {
                        theme.selection
                    } else {
                        theme.text
                    };
                    let display = if i == self.selected {
                        alloc::format!("> {}  [{}]", session.title, session.id)
                    } else {
                        alloc::format!("  {}  [{}]", session.title, session.id)
                    };
                    Text::from(display)
                        .style(style)
                        .render(Rect::new(area.x, y, area.width, 1), frame.buffer_mut());
                }
            }
            SessionListState::Error(ref e) => {
                Text::from(e.as_str())
                    .style(theme.text_error)
                    .render(area, frame.buffer_mut());
            }
        }
    }

    fn handle_event(&mut self, event: Event) -> Action {
        match self.state {
            SessionListState::Loaded => match event {
                Event::Key(ref ke) => match ke.scancode {
                    Scancode::Up => {
                        self.selected = self.selected.saturating_sub(1);
                        Action::None
                    }
                    Scancode::Down => {
                        let max = self.sessions.len().saturating_sub(1);
                        self.selected = self.selected.saturating_add(1).min(max);
                        Action::None
                    }
                    Scancode::Enter => {
                        if self.confirm_delete.is_some() {
                            Action::None
                        } else if let Some(session) = self.sessions.get(self.selected) {
                            Action::LoadSession(session.id.clone())
                        } else {
                            Action::None
                        }
                    }
                    Scancode::Delete => {
                        if self.confirm_delete == Some(self.selected) {
                            // Already confirming, execute delete
                            if let Some(session) = self.sessions.get(self.selected) {
                                let id = session.id.clone();
                                self.confirm_delete = None;
                                Action::DeleteSession(id)
                            } else {
                                Action::None
                            }
                        } else {
                            // Start confirmation
                            self.confirm_delete = Some(self.selected);
                            Action::None
                        }
                    }
                    Scancode::Escape => {
                        self.confirm_delete = None;
                        Action::CloseModal
                    }
                    _ => Action::None,
                },
                _ => Action::None,
            },
            _ => Action::None,
        }
    }

    fn title(&self) -> &str {
        "Sessions"
    }
}

// --- Model picker modal ---

pub struct ModelPickerModal {
    model: Option<String>,
    error: Option<String>,
}

impl ModelPickerModal {
    pub fn new() -> Self {
        Self {
            model: None,
            error: None,
        }
    }

    pub fn set_config(&mut self, config: Config) {
        self.model = config.model;
    }

    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }
}

impl Modal for ModelPickerModal {
    fn render(&self, frame: &mut Frame, theme: &Theme, area: Rect) {
        Text::from("Model")
            .style(theme.text_accent)
            .render(Rect::new(area.x, area.y, area.width, 1), frame.buffer_mut());

        let content_y = area.y + 2;

        if let Some(err) = &self.error {
            Text::from(err.as_str()).style(theme.text_error).render(
                Rect::new(area.x, content_y, area.width, 1),
                frame.buffer_mut(),
            );
        } else if let Some(model) = &self.model {
            Text::from(alloc::format!("Current model: {}", model))
                .style(theme.text)
                .render(
                    Rect::new(area.x, content_y, area.width, 1),
                    frame.buffer_mut(),
                );
        } else {
            Text::from("No model configured")
                .style(theme.text_dim)
                .render(
                    Rect::new(area.x, content_y, area.width, 1),
                    frame.buffer_mut(),
                );
        }

        if self.error.is_none() {
            let notice = "Read-only: configure model via server config";
            Text::from(notice).style(theme.text_dim).render(
                Rect::new(area.x, area.y + 4, area.width, 1),
                frame.buffer_mut(),
            );
        }
    }

    fn handle_event(&mut self, _event: Event) -> Action {
        Action::None
    }

    fn title(&self) -> &str {
        "Model Picker"
    }
}

// --- Help modal ---

pub struct HelpModal;

impl HelpModal {
    pub fn new() -> Self {
        Self
    }
}

impl Modal for HelpModal {
    fn render(&self, frame: &mut Frame, theme: &Theme, area: Rect) {
        let mut lines: Vec<alloc::string::String> = Vec::new();

        lines.push("Slash Commands".into());
        lines.push("  /help         Show this help".into());
        lines.push("  /sessions     List sessions".into());
        lines.push("  /new          New session".into());
        lines.push("  /settings     Settings / model picker".into());
        lines.push("  /todos        Toggle todos panel".into());
        lines.push("  /diagnostics  Toggle diagnostics panel".into());
        lines.push("  /pty          Toggle terminal panel".into());
        lines.push("  /abort        Abort current session".into());
        lines.push("  /exit         Quit".into());
        lines.push("  /details      Toggle tool details".into());
        lines.push("".into());
        lines.push("Keybindings".into());
        lines.push("  Ctrl+X H      Help".into());
        lines.push("  Ctrl+X Q      Quit".into());
        lines.push("  Ctrl+X N      New session".into());
        lines.push("  Ctrl+X L      Sessions".into());
        lines.push("  Ctrl+X M      Settings".into());
        lines.push("  Ctrl+X T      Terminal".into());
        lines.push("  Ctrl+X D      Diagnostics".into());
        lines.push("  Ctrl+X O      Todos".into());
        lines.push("  Ctrl+P        Command palette".into());
        lines.push("  Tab           Cycle agent forward".into());
        lines.push("  Shift+Tab     Cycle agent backward".into());
        lines.push("  Escape        Close modal / interrupt".into());
        lines.push("".into());
        lines.push("Input Prefixes".into());
        lines.push("  /             Command mode".into());
        lines.push("  !             Shell mode".into());
        lines.push("  @             File reference".into());
        lines.push("  #             Tool reference".into());

        for (i, line) in lines.iter().enumerate() {
            let y = area.y + i as u16;
            if y >= area.bottom() {
                break;
            }
            let style = if line.ends_with("Commands")
                || line == "Keybindings"
                || line == "Input Prefixes"
            {
                theme.text_accent
            } else {
                theme.text
            };
            Text::from(line.as_str())
                .style(style)
                .render(Rect::new(area.x, y, area.width, 1), frame.buffer_mut());
        }
    }

    fn handle_event(&mut self, event: Event) -> Action {
        match event {
            Event::Key(ref ke) if ke.scancode == Scancode::Escape => Action::CloseModal,
            _ => Action::None,
        }
    }

    fn title(&self) -> &str {
        "Help"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Scancode;
    use ratatui_core::backend::TestBackend;
    use ratatui_core::terminal::Terminal;

    struct TestModal;

    impl Modal for TestModal {
        fn render(&self, _frame: &mut Frame, _theme: &Theme, _area: Rect) {}
        fn handle_event(&mut self, event: Event) -> Action {
            match event {
                Event::Key(ke) if ke.scancode == Scancode::Escape => Action::CloseModal,
                _ => Action::None,
            }
        }
        fn title(&self) -> &str {
            "Test Modal"
        }
    }

    #[test]
    fn modal_trait_title_works() {
        let modal = TestModal;
        assert_eq!(modal.title(), "Test Modal");
    }

    #[test]
    fn session_list_starts_in_loading_state() {
        let modal = SessionListModal::new();
        let theme = Theme::default();
        let backend = TestBackend::new(60, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                Modal::render(&modal, frame, &theme, Rect::new(10, 5, 40, 10));
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let has_loading = buf.content().iter().any(|c| c.symbol() == "L");
        assert!(
            has_loading,
            "Loading state should show text starting with L"
        );
    }

    #[test]
    fn model_picker_shows_model_from_config() {
        use ocpncord_backend::Config;
        let mut modal = ModelPickerModal::new();
        modal.set_config(Config {
            model: Some("gpt-4".into()),
            username: None,
        });

        let theme = Theme::default();
        let backend = TestBackend::new(60, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                Modal::render(&modal, frame, &theme, Rect::new(10, 5, 40, 10));
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let screen: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(
            screen.contains("gpt-4"),
            "Model name 'gpt-4' should appear. Screen: {}",
            screen
        );
    }

    #[test]
    fn model_picker_shows_readonly_notice() {
        use ocpncord_backend::Config;
        let mut modal = ModelPickerModal::new();
        modal.set_config(Config {
            model: Some("gpt-4".into()),
            username: None,
        });

        let theme = Theme::default();
        let backend = TestBackend::new(60, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                Modal::render(&modal, frame, &theme, Rect::new(10, 5, 40, 10));
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let screen: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(
            screen.contains("Read-only:"),
            "Read-only notice should appear. Screen: {}",
            screen
        );
    }

    #[test]
    fn help_modal_renders_title() {
        let modal = HelpModal::new();
        let theme = Theme::default();
        let backend = TestBackend::new(60, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                modal.render(frame, &theme, Rect::new(0, 0, 60, 30));
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let screen: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(
            screen.contains("Slash Commands"),
            "Should show Slash Commands section. Screen: {}",
            screen
        );
    }

    #[test]
    fn help_modal_shows_all_sections() {
        let modal = HelpModal::new();
        let theme = Theme::default();
        let backend = TestBackend::new(60, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                modal.render(frame, &theme, Rect::new(0, 0, 60, 30));
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let screen: String = buf.content().iter().map(|c| c.symbol()).collect();

        // Slash commands
        assert!(
            screen.contains("/help"),
            "Should show /help. Screen: {}",
            screen
        );
        assert!(
            screen.contains("/sessions"),
            "Should show /sessions. Screen: {}",
            screen
        );
        assert!(
            screen.contains("/new"),
            "Should show /new. Screen: {}",
            screen
        );
        assert!(
            screen.contains("/settings"),
            "Should show /settings. Screen: {}",
            screen
        );
        assert!(
            screen.contains("/exit"),
            "Should show /exit. Screen: {}",
            screen
        );
        assert!(
            screen.contains("/details"),
            "Should show /details. Screen: {}",
            screen
        );

        // Keybindings
        assert!(
            screen.contains("Ctrl+X H"),
            "Should show Ctrl+X H. Screen: {}",
            screen
        );
        assert!(
            screen.contains("Ctrl+X Q"),
            "Should show Ctrl+X Q. Screen: {}",
            screen
        );
        assert!(
            screen.contains("Ctrl+X N"),
            "Should show Ctrl+X N. Screen: {}",
            screen
        );
        assert!(
            screen.contains("Ctrl+X L"),
            "Should show Ctrl+X L. Screen: {}",
            screen
        );
        assert!(
            screen.contains("Ctrl+X M"),
            "Should show Ctrl+X M. Screen: {}",
            screen
        );
        assert!(
            screen.contains("Ctrl+P"),
            "Should show Ctrl+P. Screen: {}",
            screen
        );
        assert!(
            screen.contains("Tab"),
            "Should show Tab. Screen: {}",
            screen
        );
        assert!(
            screen.contains("Escape"),
            "Should show Escape. Screen: {}",
            screen
        );

        // Input prefixes
        assert!(
            screen.contains("Command mode"),
            "Should show Command mode. Screen: {}",
            screen
        );
        assert!(
            screen.contains("Shell mode"),
            "Should show Shell mode. Screen: {}",
            screen
        );
        assert!(
            screen.contains("File reference"),
            "Should show File reference. Screen: {}",
            screen
        );
    }

    #[test]
    fn model_picker_shows_error_state() {
        let mut modal = ModelPickerModal::new();
        modal.set_error("Failed to load model config".into());

        let theme = Theme::default();
        let backend = TestBackend::new(60, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                Modal::render(&modal, frame, &theme, Rect::new(10, 5, 40, 10));
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let screen: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(
            screen.contains("Failed to load model config"),
            "Error message should appear. Screen: {}",
            screen
        );
    }
}

// --- Permission approval modal ---

pub struct PermissionModal {
    request: ocpncord_backend::PermissionRequest,
    selected: usize,
}

impl PermissionModal {
    pub fn new(request: ocpncord_backend::PermissionRequest) -> Self {
        Self {
            request,
            selected: 0,
        }
    }
}

impl Modal for PermissionModal {
    fn render(&self, frame: &mut Frame, theme: &Theme, area: Rect) {
        Text::from("Permission Request")
            .style(theme.dialog_title)
            .render(Rect::new(area.x, area.y, area.width, 1), frame.buffer_mut());
        let display = alloc::format!("Permission: {}", self.request.permission);
        Text::from(display.as_str()).style(theme.text).render(
            Rect::new(area.x, area.y + 2, area.width, 1),
            frame.buffer_mut(),
        );
        if !self.request.patterns.is_empty() {
            Text::from("Patterns:").style(theme.text_dim).render(
                Rect::new(area.x, area.y + 3, area.width, 1),
                frame.buffer_mut(),
            );
            for (i, pattern) in self.request.patterns.iter().enumerate() {
                let y = area.y + 4 + i as u16;
                if y >= area.bottom() {
                    break;
                }
                let display = alloc::format!("  {pattern}");
                Text::from(display.as_str())
                    .style(theme.text)
                    .render(Rect::new(area.x, y, area.width, 1), frame.buffer_mut());
            }
        }
        let buttons = ["Allow Once", "Allow Always", "Deny"];
        let btn_y = area.y + area.height - 2;
        for (i, label) in buttons.iter().enumerate() {
            let style = if i == self.selected {
                theme.dialog_button_focused
            } else {
                theme.dialog_button
            };
            let display = alloc::format!("  {label}  ");
            Text::from(display.as_str()).style(style).render(
                Rect::new(area.x + (i as u16 * 16), btn_y, display.len() as u16, 1),
                frame.buffer_mut(),
            );
        }
    }

    fn handle_event(&mut self, event: Event) -> Action {
        match event {
            Event::Key(ref ke) => match ke.scancode {
                Scancode::Left => {
                    self.selected = self.selected.saturating_sub(1);
                    Action::None
                }
                Scancode::Right => {
                    self.selected = (self.selected + 1).min(2);
                    Action::None
                }
                Scancode::Enter => {
                    let reply = match self.selected {
                        0 => crate::screen::PermissionReplyAction::Once,
                        1 => crate::screen::PermissionReplyAction::Always,
                        _ => crate::screen::PermissionReplyAction::Reject,
                    };
                    Action::ReplyPermission(
                        self.request.session_id.clone(),
                        self.request.id.clone(),
                        reply,
                    )
                }
                Scancode::Escape => Action::CloseModal,
                _ => Action::None,
            },
            _ => Action::None,
        }
    }

    fn title(&self) -> &str {
        "Permission Request"
    }
}

// --- Question modal ---

pub struct QuestionModal {
    request: ocpncord_backend::QuestionRequest,
    current_q: usize,
    selected: usize,
    custom_input: String,
}

impl QuestionModal {
    pub fn new(request: ocpncord_backend::QuestionRequest) -> Self {
        Self {
            request,
            current_q: 0,
            selected: 0,
            custom_input: String::new(),
        }
    }
}

impl Modal for QuestionModal {
    fn render(&self, frame: &mut Frame, theme: &Theme, area: Rect) {
        Text::from("Question")
            .style(theme.dialog_title)
            .render(Rect::new(area.x, area.y, area.width, 1), frame.buffer_mut());
        if let Some(qinfo) = self.request.questions.get(self.current_q) {
            Text::from(qinfo.header.as_str())
                .style(theme.text_accent)
                .render(
                    Rect::new(area.x, area.y + 2, area.width, 1),
                    frame.buffer_mut(),
                );
            Text::from(qinfo.question.as_str())
                .style(theme.text)
                .render(
                    Rect::new(area.x, area.y + 3, area.width, 1),
                    frame.buffer_mut(),
                );
            for (i, opt) in qinfo.options.iter().enumerate() {
                let y = area.y + 5 + i as u16;
                if y >= area.bottom() {
                    break;
                }
                let style = if i == self.selected {
                    theme.dialog_button_focused
                } else {
                    theme.dialog_button
                };
                let display = alloc::format!("  {} - {}", opt.label, opt.description);
                Text::from(display.as_str())
                    .style(style)
                    .render(Rect::new(area.x, y, area.width, 1), frame.buffer_mut());
            }
        }
    }

    fn handle_event(&mut self, event: Event) -> Action {
        match event {
            Event::Key(ref ke) => match ke.scancode {
                Scancode::Up => {
                    self.selected = self.selected.saturating_sub(1);
                    Action::None
                }
                Scancode::Down => {
                    let max = self
                        .request
                        .questions
                        .get(self.current_q)
                        .map(|q| q.options.len().saturating_sub(1))
                        .unwrap_or(0);
                    self.selected = (self.selected + 1).min(max);
                    Action::None
                }
                Scancode::Enter => {
                    let answer = if let Some(qinfo) = self.request.questions.get(self.current_q) {
                        if let Some(opt) = qinfo.options.get(self.selected) {
                            opt.label.clone()
                        } else {
                            String::new()
                        }
                    } else {
                        String::new()
                    };
                    Action::ReplyQuestion(
                        self.request.session_id.clone(),
                        self.request.id.clone(),
                        alloc::vec::Vec::from([answer]),
                    )
                }
                Scancode::Escape => Action::CloseModal,
                _ => Action::None,
            },
            _ => Action::None,
        }
    }

    fn title(&self) -> &str {
        "Question"
    }
}
