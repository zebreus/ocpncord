use ratatui::layout::Rect;
use ratatui::Frame;

use crate::event::{Event, Scancode};
use crate::screen::Action;
use crate::theme::Theme;

/// An overlay dialog drawn on top of a full-screen view.
pub trait Modal {
    fn render(&self, frame: &mut Frame, theme: &Theme, area: Rect);
    fn handle_event(&mut self, event: Event) -> Action;
    fn title(&self) -> &str;
    fn preferred_size(&self, area: Rect) -> (u16, u16) {
        (
            ((area.width as u32 * 3) / 5).clamp(40, area.width as u32) as u16,
            ((area.height as u32 * 7) / 10).clamp(8, area.height as u32) as u16,
        )
    }
}

// --- Session list modal ---

use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use ocpncord_backend::{Config, Session};
use ratatui::text::{Line, Text};
use ratatui::widgets::{
    List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
    StatefulWidget, Widget, Wrap,
};

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
    scroll: u16,
    confirm_delete: Option<usize>,
}

impl SessionListModal {
    pub fn new() -> Self {
        Self {
            state: SessionListState::Loading,
            sessions: Vec::new(),
            selected: 0,
            scroll: 0,
            confirm_delete: None,
        }
    }

    pub fn set_sessions(&mut self, sessions: Vec<Session>) {
        if sessions.is_empty() {
            self.state = SessionListState::Empty;
        } else {
            self.selected = self.selected.min(sessions.len().saturating_sub(1));
            self.ensure_selected_visible(0);
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

    pub fn scroll_offset(&self) -> u16 {
        self.scroll
    }

    fn ensure_selected_visible(&mut self, visible_height: u16) {
        if visible_height == 0 {
            return;
        }
        let selected = self.selected as u16;
        if selected < self.scroll {
            self.scroll = selected;
        } else {
            let bottom = self.scroll.saturating_add(visible_height.saturating_sub(1));
            if selected > bottom {
                self.scroll = selected.saturating_sub(visible_height.saturating_sub(1));
            }
        }
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
                let mut list_area = area;
                if self.confirm_delete == Some(self.selected) {
                    let confirm_msg = "Press Delete again to confirm, Escape to cancel";
                    Text::from(confirm_msg)
                        .style(theme.text_error)
                        .render(Rect::new(area.x, area.y, area.width, 1), frame.buffer_mut());
                    list_area = Rect::new(
                        area.x,
                        area.y + 1,
                        area.width,
                        area.height.saturating_sub(1),
                    );
                }

                let visible_height = list_area.height;
                let scroll = self
                    .scroll
                    .min(self.sessions.len().saturating_sub(visible_height as usize) as u16);
                let list_content_area =
                    if self.sessions.len() > visible_height as usize && list_area.width > 1 {
                        Rect::new(
                            list_area.x,
                            list_area.y,
                            list_area.width - 1,
                            list_area.height,
                        )
                    } else {
                        list_area
                    };
                let items: Vec<ListItem<'_>> = self
                    .sessions
                    .iter()
                    .map(|session| {
                        ListItem::new(Line::from(alloc::format!(
                            "{}  [{}]",
                            session.title,
                            session.id
                        )))
                    })
                    .collect();
                let mut state = ListState::default()
                    .with_selected(Some(self.selected))
                    .with_offset(scroll as usize);
                StatefulWidget::render(
                    List::new(items)
                        .style(theme.text)
                        .highlight_style(theme.selection)
                        .highlight_symbol("> "),
                    list_content_area,
                    frame.buffer_mut(),
                    &mut state,
                );
                if self.sessions.len() > visible_height as usize {
                    let mut scrollbar_state =
                        ScrollbarState::new(self.sessions.len()).position(scroll as usize);
                    Scrollbar::new(ScrollbarOrientation::VerticalRight)
                        .thumb_style(theme.scrollbar)
                        .track_style(theme.text_dim)
                        .render(list_area, frame.buffer_mut(), &mut scrollbar_state);
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
                        self.ensure_selected_visible(1);
                        Action::None
                    }
                    Scancode::Down => {
                        let max = self.sessions.len().saturating_sub(1);
                        self.selected = self.selected.saturating_add(1).min(max);
                        self.ensure_selected_visible(1);
                        if self.selected as u16 >= self.scroll.saturating_add(1) {
                            self.scroll = self.selected as u16;
                        }
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
    current_model: Option<String>,
    agent_model: Option<String>,
    models: Vec<ModelChoice>,
    selected: usize,
    scroll: u16,
    error: Option<String>,
}

#[derive(Debug, Clone)]
struct ModelChoice {
    id: String,
    label: String,
    details: String,
}

impl ModelPickerModal {
    pub fn new() -> Self {
        Self {
            current_model: None,
            agent_model: None,
            models: Vec::new(),
            selected: 0,
            scroll: 0,
            error: None,
        }
    }

    pub fn set_config(&mut self, config: Config) {
        self.current_model = config.model.clone();
        self.agent_model = config
            .agent
            .get("build")
            .and_then(|agent| agent.model.clone())
            .or_else(|| config.agent.values().find_map(|agent| agent.model.clone()));

        let mut models = Vec::new();
        for (provider_id, provider) in config.provider {
            let provider_label = provider
                .name
                .unwrap_or_else(|| provider.id.clone().unwrap_or_else(|| provider_id.clone()));
            for (model_id, model) in provider.models {
                let full_id = format!("{provider_id}/{model_id}");
                let model_label = model
                    .name
                    .unwrap_or_else(|| model.id.clone().unwrap_or_else(|| model_id.clone()));
                let mut detail = provider_label.clone();
                if let Some(family) = model.family {
                    detail.push_str(" - ");
                    detail.push_str(&family);
                }
                if let Some(status) = model.status {
                    detail.push_str(" - ");
                    detail.push_str(&status);
                }
                if model.reasoning == Some(true) {
                    detail.push_str(" - reasoning");
                }
                if model.tool_call == Some(true) {
                    detail.push_str(" - tools");
                }
                models.push(ModelChoice {
                    id: full_id,
                    label: model_label,
                    details: detail,
                });
            }
        }
        models.sort_by(|a, b| a.id.cmp(&b.id));
        self.selected = self
            .current_model
            .as_ref()
            .and_then(|current| models.iter().position(|choice| &choice.id == current))
            .unwrap_or(0);
        self.scroll = self.selected.saturating_sub(4) as u16;
        self.models = models;
    }

    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }

    pub fn selected_model(&self) -> Option<&str> {
        self.models
            .get(self.selected)
            .map(|choice| choice.id.as_str())
    }

    pub fn selected_index(&self) -> usize {
        self.selected
    }

    fn ensure_selected_visible(&mut self, visible_rows: u16) {
        if visible_rows == 0 {
            return;
        }
        let selected = self.selected as u16;
        if selected < self.scroll {
            self.scroll = selected;
        } else if selected >= self.scroll + visible_rows {
            self.scroll = selected.saturating_sub(visible_rows.saturating_sub(1));
        }
    }
}

impl Modal for ModelPickerModal {
    fn render(&self, frame: &mut Frame, theme: &Theme, area: Rect) {
        if let Some(err) = &self.error {
            Paragraph::new(err.as_str())
                .style(theme.text_error)
                .wrap(Wrap { trim: false })
                .render(area, frame.buffer_mut());
            return;
        }

        if area.height == 0 {
            return;
        }

        let current = self
            .current_model
            .as_deref()
            .or(self.agent_model.as_deref())
            .unwrap_or("No model configured");
        Text::from(format!("Current: {current}"))
            .style(theme.text_dim)
            .render(Rect::new(area.x, area.y, area.width, 1), frame.buffer_mut());

        if self.models.is_empty() {
            Text::from("No models found in server config")
                .style(theme.text_dim)
                .render(
                    Rect::new(area.x, area.y + 2, area.width, 1.min(area.height)),
                    frame.buffer_mut(),
                );
            return;
        }

        let list_y = area.y.saturating_add(2);
        if list_y >= area.bottom() {
            return;
        }
        let list_area = Rect::new(area.x, list_y, area.width, area.bottom() - list_y);
        let visible_rows = list_area.height.max(1);
        let rows: Vec<ListItem<'_>> = self
            .models
            .iter()
            .skip(self.scroll as usize)
            .take(visible_rows as usize)
            .map(|choice| {
                let marker = if Some(choice.id.as_str()) == self.current_model.as_deref() {
                    "*"
                } else {
                    " "
                };
                ListItem::new(Line::from(format!(
                    "{marker} {}  [{}]",
                    choice.label, choice.details
                )))
            })
            .collect();
        let mut state = ListState::default();
        state.select(Some(self.selected.saturating_sub(self.scroll as usize)));
        StatefulWidget::render(
            List::new(rows)
                .style(theme.text)
                .highlight_style(theme.selection),
            list_area,
            frame.buffer_mut(),
            &mut state,
        );

        if self.models.len() > visible_rows as usize {
            let mut scroll_state =
                ScrollbarState::new(self.models.len()).position(self.scroll as usize);
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .thumb_style(theme.scrollbar)
                .render(list_area, frame.buffer_mut(), &mut scroll_state);
        }
    }

    fn handle_event(&mut self, event: Event) -> Action {
        match event {
            Event::Key(key) => match key.scancode {
                Scancode::Escape => Action::CloseModal,
                Scancode::Up => {
                    self.selected = self.selected.saturating_sub(1);
                    self.ensure_selected_visible(1);
                    Action::None
                }
                Scancode::Down => {
                    if !self.models.is_empty() {
                        self.selected = (self.selected + 1).min(self.models.len() - 1);
                    }
                    self.ensure_selected_visible(1);
                    Action::None
                }
                Scancode::PageUp => {
                    self.selected = self.selected.saturating_sub(10);
                    self.ensure_selected_visible(1);
                    Action::None
                }
                Scancode::PageDown => {
                    if !self.models.is_empty() {
                        self.selected = (self.selected + 10).min(self.models.len() - 1);
                    }
                    self.ensure_selected_visible(1);
                    Action::None
                }
                Scancode::Enter => self
                    .selected_model()
                    .map(|model| Action::SelectModel(model.to_string()))
                    .unwrap_or(Action::None),
                _ => Action::None,
            },
            _ => Action::None,
        }
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
        let mut lines: Vec<Line<'_>> = Vec::new();
        for (text, heading) in [
            ("Slash Commands", true),
            ("  /help         Show this help", false),
            ("  /sessions     List sessions", false),
            ("  /new          New session", false),
            ("  /settings     Settings / model picker", false),
            ("  /todos        Toggle todos panel", false),
            ("  /diagnostics  Toggle diagnostics panel", false),
            ("  /pty          Toggle terminal panel", false),
            ("  /abort        Abort current session", false),
            ("  /exit         Quit", false),
            ("  /details      Toggle tool details", false),
            ("", false),
            ("Keybindings", true),
            ("  Ctrl+X H      Help", false),
            ("  Ctrl+X Q      Quit", false),
            ("  Ctrl+X N      New session", false),
            ("  Ctrl+X L      Sessions", false),
            ("  Ctrl+X M      Settings", false),
            ("  Ctrl+X T      Terminal", false),
            ("  Ctrl+X D      Diagnostics", false),
            ("  Ctrl+X O      Todos", false),
            ("  Ctrl+P        Command palette", false),
            ("  Tab           Cycle agent forward", false),
            ("  Shift+Tab     Cycle agent backward", false),
            ("  Escape        Close modal / interrupt", false),
            ("", false),
            ("Input Prefixes", true),
            ("  /             Command mode", false),
            ("  !             Shell mode", false),
            ("  @             File reference", false),
            ("  #             Tool reference", false),
        ] {
            lines.push(Line::from(text).style(if heading {
                theme.text_accent
            } else {
                theme.text
            }));
        }

        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .render(area, frame.buffer_mut());
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

    fn preferred_size(&self, area: Rect) -> (u16, u16) {
        (
            ((area.width as u32 * 3) / 5).clamp(50, area.width as u32) as u16,
            34.min(area.height),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Scancode;
    use alloc::collections::BTreeMap;
    use ocpncord_backend::{
        PermissionRequest, PermissionToolInfo, QuestionInfo, QuestionOption, QuestionRequest,
        SessionTime,
    };
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

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

    fn make_session(id: &str, title: &str) -> Session {
        Session {
            id: id.into(),
            title: title.into(),
            project_id: "p1".into(),
            directory: "/".into(),
            parent_id: None,
            time: SessionTime {
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
        use ocpncord_backend::{Config, ModelConfig, ProviderConfig};
        let mut provider = BTreeMap::new();
        provider.insert(
            "openrouter".into(),
            ProviderConfig {
                name: Some("OpenRouter".into()),
                models: BTreeMap::from([(
                    "gpt-4".into(),
                    ModelConfig {
                        name: Some("GPT-4".into()),
                        ..Default::default()
                    },
                )]),
                ..Default::default()
            },
        );
        let mut modal = ModelPickerModal::new();
        modal.set_config(Config {
            model: Some("openrouter/gpt-4".into()),
            username: None,
            provider,
            agent: Default::default(),
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
            screen.contains("GPT-4"),
            "Model name 'GPT-4' should appear. Screen: {}",
            screen
        );
    }

    #[test]
    fn model_picker_enter_returns_selected_model() {
        use ocpncord_backend::{Config, ModelConfig, ProviderConfig};
        let mut provider = BTreeMap::new();
        provider.insert(
            "anthropic".into(),
            ProviderConfig {
                name: Some("Anthropic".into()),
                models: BTreeMap::from([
                    ("claude-haiku".into(), ModelConfig::default()),
                    ("claude-sonnet".into(), ModelConfig::default()),
                ]),
                ..Default::default()
            },
        );
        let mut modal = ModelPickerModal::new();
        modal.set_config(Config {
            model: Some("anthropic/claude-haiku".into()),
            username: None,
            provider,
            agent: Default::default(),
        });

        modal.handle_event(Event::Key(crate::event::KeyEvent {
            scancode: Scancode::Down,
            modifiers: Default::default(),
        }));
        assert_eq!(modal.selected_index(), 1);
        let action = modal.handle_event(Event::Key(crate::event::KeyEvent {
            scancode: Scancode::Enter,
            modifiers: Default::default(),
        }));
        assert_eq!(
            action,
            Action::SelectModel("anthropic/claude-sonnet".into())
        );
    }

    #[test]
    fn session_list_scrolls_long_lists_to_selected_item() {
        let mut modal = SessionListModal::new();
        let sessions = (0..12)
            .map(|idx| make_session(&alloc::format!("s{idx}"), &alloc::format!("Session {idx}")))
            .collect();
        modal.set_sessions(sessions);

        for _ in 0..9 {
            modal.handle_event(Event::Key(crate::event::KeyEvent {
                scancode: Scancode::Down,
                modifiers: Default::default(),
            }));
        }

        assert_eq!(modal.selected_index(), 9);
        assert!(modal.scroll_offset() > 0);

        let theme = Theme::default();
        let backend = TestBackend::new(40, 6);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                Modal::render(&modal, frame, &theme, Rect::new(0, 0, 40, 4));
            })
            .unwrap();
        let screen: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(screen.contains("Session 9"), "screen: {screen}");
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

    #[test]
    fn permission_modal_wraps_content_without_internal_title() {
        let modal = PermissionModal::new(PermissionRequest {
            id: "permission-1".into(),
            session_id: "session-1".into(),
            permission: "very-long-permission-name-that-needs-wrapping".into(),
            patterns: vec!["/tmp/some/really/long/path/that/needs/wrapping".into()],
            metadata: BTreeMap::new(),
            always: Vec::new(),
            tool: Some(PermissionToolInfo {
                message_id: "m1".into(),
                call_id: "c1".into(),
            }),
        });
        let theme = Theme::default();
        let backend = TestBackend::new(32, 8);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| {
                Modal::render(&modal, frame, &theme, Rect::new(0, 0, 32, 8));
            })
            .unwrap();

        let screen: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(!screen.contains("Permission Request"));
        assert!(screen.contains("Permission:"));
        assert!(screen.contains("Allow Once"));
    }

    #[test]
    fn question_modal_wraps_content_without_internal_title() {
        let modal = QuestionModal::new(QuestionRequest {
            id: "question-1".into(),
            session_id: "session-1".into(),
            questions: vec![QuestionInfo {
                question: "Choose the option that should be used for this very long prompt".into(),
                header: "Decision".into(),
                options: vec![
                    QuestionOption {
                        label: "A".into(),
                        description: "First long option description".into(),
                    },
                    QuestionOption {
                        label: "B".into(),
                        description: "Second long option description".into(),
                    },
                ],
                multiple: false,
                custom: false,
            }],
            tool: None,
        });
        let theme = Theme::default();
        let backend = TestBackend::new(36, 8);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| {
                Modal::render(&modal, frame, &theme, Rect::new(0, 0, 36, 8));
            })
            .unwrap();

        let screen: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(!screen.contains("QuestionQuestion"));
        assert!(screen.contains("Decision"));
        assert!(screen.contains("A -"));
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
        let button_y = area.y + area.height.saturating_sub(1);
        let content_height = button_y.saturating_sub(area.y);
        let content_area = Rect::new(area.x, area.y, area.width, content_height);
        let mut lines: Vec<Line<'_>> = Vec::new();
        lines.push(
            Line::from(alloc::format!("Permission: {}", self.request.permission)).style(theme.text),
        );
        if !self.request.patterns.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from("Patterns:").style(theme.text_dim));
            for pattern in self.request.patterns.iter() {
                lines.push(Line::from(alloc::format!("  {pattern}")).style(theme.text));
            }
        }
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .render(content_area, frame.buffer_mut());

        let buttons = ["Allow Once", "Allow Always", "Deny"];
        let mut x = area.x;
        for (i, label) in buttons.iter().enumerate() {
            let style = if i == self.selected {
                theme.dialog_button_focused
            } else {
                theme.dialog_button
            };
            let display = alloc::format!("  {label}  ");
            Text::from(display.as_str()).style(style).render(
                Rect::new(x, button_y, display.len() as u16, 1),
                frame.buffer_mut(),
            );
            x = x.saturating_add(display.len() as u16 + 1);
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
        if let Some(qinfo) = self.request.questions.get(self.current_q) {
            let mut lines: Vec<Line<'_>> = Vec::new();
            lines.push(Line::from(qinfo.header.as_str()).style(theme.text_accent));
            lines.push(Line::from(qinfo.question.as_str()).style(theme.text));
            lines.push(Line::from(""));
            for (i, opt) in qinfo.options.iter().enumerate() {
                let style = if i == self.selected {
                    theme.dialog_button_focused
                } else {
                    theme.dialog_button
                };
                let display = alloc::format!("  {} - {}", opt.label, opt.description);
                lines.push(Line::from(display).style(style));
            }
            Paragraph::new(Text::from(lines))
                .wrap(Wrap { trim: false })
                .render(area, frame.buffer_mut());
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
