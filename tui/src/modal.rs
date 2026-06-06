use ratatui::layout::Rect;
use ratatui::Frame;

use crate::app::{Action, ConnectionStatusDisplay, ConnectionTier, PermissionReplyAction};
use crate::chat::{ChatDisplayPolicy, PART_KIND_ORDER};
use crate::event::{Event, Scancode};
use crate::theme::Theme;
use alloc::collections::BTreeMap;
use core::cell::Cell;
use ocpncord_backend::ServerConnection;

/// An overlay dialog drawn on top of a full-screen view.
pub trait Modal {
    fn render(&self, frame: &mut Frame, theme: &Theme, area: Rect);
    fn handle_event(&mut self, event: Event) -> Action;
    fn title(&self) -> &str;
    fn as_model_picker(&self) -> Option<&ModelPickerModal> {
        None
    }
    fn as_model_picker_mut(&mut self) -> Option<&mut ModelPickerModal> {
        None
    }
    fn as_server_config_mut(&mut self) -> Option<&mut ServerConfigModal> {
        None
    }
    fn as_display_config_mut(&mut self) -> Option<&mut DisplayConfigModal> {
        None
    }
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
use alloc::vec;
use alloc::vec::Vec;
use ocpncord_backend::{Config, ModelSummary, Session};
use ratatui::text::{Line, Span, Text};
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

fn fit_with_ellipsis(text: String, width: usize) -> String {
    let len = text.chars().count();
    if len <= width {
        return text;
    }
    if width == 0 {
        return String::new();
    }
    if width <= 3 {
        return ".".repeat(width);
    }
    let mut out: String = text.chars().take(width.saturating_sub(3)).collect();
    out.push_str("...");
    out
}

fn short_id(id: &str) -> String {
    let len = id.chars().count();
    if len <= 12 {
        return id.into();
    }
    let head: String = id.chars().take(6).collect();
    let tail: String = id
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{head}...{tail}")
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

    #[cfg(test)]
    fn selected_index(&self) -> usize {
        self.selected
    }

    #[cfg(test)]
    fn scroll_offset(&self) -> u16 {
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
                        let row = alloc::format!("{}  [{}]", session.title, short_id(&session.id));
                        ListItem::new(Line::from(fit_with_ellipsis(
                            row,
                            list_content_area.width as usize,
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
                    let max_scroll = self.sessions.len().saturating_sub(visible_height as usize);
                    let mut scrollbar_state =
                        ScrollbarState::new(max_scroll.max(1)).position(scroll as usize);
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

// --- Server config modal ---

#[derive(Debug, Clone, PartialEq, Eq)]
enum ServerConfigStatus {
    Idle,
    Applying,
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServerConfigFocus {
    Url,
    Username,
    Password,
    Apply,
}

pub struct ServerConfigModal {
    url: String,
    username: String,
    password: String,
    status: ServerConfigStatus,
    focus: ServerConfigFocus,
    active_connection: ConnectionStatusDisplay,
}

impl ServerConfigModal {
    pub fn new(url: String) -> Self {
        Self::new_with_connection(ServerConnection::unauthenticated(url))
    }

    pub fn new_with_connection(connection: ServerConnection) -> Self {
        Self {
            url: connection.url,
            username: connection.username.unwrap_or_else(|| "opencode".into()),
            password: connection.password.unwrap_or_default(),
            status: ServerConfigStatus::Idle,
            focus: ServerConfigFocus::Url,
            active_connection: ConnectionStatusDisplay {
                tier: ConnectionTier::Yellow,
                summary: "connecting".into(),
                detail: "Waiting for the first live event from the active server".into(),
            },
        }
    }

    pub fn set_connection_status(&mut self, status: ConnectionStatusDisplay) {
        self.active_connection = status;
    }

    pub fn set_applying(&mut self) {
        self.status = ServerConfigStatus::Applying;
    }

    pub fn set_apply_error(&mut self, error: String) {
        self.status = ServerConfigStatus::Error(error);
    }

    #[cfg(test)]
    pub fn url(&self) -> &str {
        &self.url
    }

    #[cfg(test)]
    pub fn username(&self) -> &str {
        &self.username
    }

    #[cfg(test)]
    pub fn password(&self) -> &str {
        &self.password
    }

    #[cfg(test)]
    pub fn active_connection_summary(&self) -> &str {
        &self.active_connection.summary
    }

    fn connection(&self) -> ServerConnection {
        let password = if self.password.is_empty() {
            None
        } else {
            Some(self.password.clone())
        };
        let username = if password.is_none() || self.username.is_empty() {
            None
        } else {
            Some(self.username.clone())
        };
        ServerConnection::new(self.url.clone(), username, password)
    }

    fn edit_focused_field(&mut self, ch: char) {
        match self.focus {
            ServerConfigFocus::Url => self.url.push(ch),
            ServerConfigFocus::Username => self.username.push(ch),
            ServerConfigFocus::Password => self.password.push(ch),
            ServerConfigFocus::Apply => {}
        }
        self.status = ServerConfigStatus::Idle;
    }

    fn backspace_focused_field(&mut self) {
        match self.focus {
            ServerConfigFocus::Url => {
                self.url.pop();
            }
            ServerConfigFocus::Username => {
                self.username.pop();
            }
            ServerConfigFocus::Password => {
                self.password.pop();
            }
            ServerConfigFocus::Apply => {}
        }
        self.status = ServerConfigStatus::Idle;
    }

    fn cycle_focus_forward(&mut self) {
        self.focus = match self.focus {
            ServerConfigFocus::Url => ServerConfigFocus::Username,
            ServerConfigFocus::Username => ServerConfigFocus::Password,
            ServerConfigFocus::Password => ServerConfigFocus::Apply,
            ServerConfigFocus::Apply => ServerConfigFocus::Url,
        };
    }

    fn cycle_focus_back(&mut self) {
        self.focus = match self.focus {
            ServerConfigFocus::Url => ServerConfigFocus::Apply,
            ServerConfigFocus::Username => ServerConfigFocus::Url,
            ServerConfigFocus::Password => ServerConfigFocus::Username,
            ServerConfigFocus::Apply => ServerConfigFocus::Password,
        };
    }

    fn status_line(&self) -> Line<'_> {
        match &self.status {
            ServerConfigStatus::Idle => Line::from("Edit the server connection, then apply."),
            ServerConfigStatus::Applying => Line::from("Applying connection..."),
            ServerConfigStatus::Error(message) => Line::from(message.as_str()),
        }
    }

    fn active_connection_line(&self) -> Line<'_> {
        Line::from(alloc::format!(
            "Active server: {}",
            self.active_connection.summary
        ))
    }
}

impl Modal for ServerConfigModal {
    fn render(&self, frame: &mut Frame, theme: &Theme, area: Rect) {
        let mut lines = Vec::new();
        lines.push(Line::from("Server URL").style(theme.text_dim));
        let url_cursor = if self.focus == ServerConfigFocus::Url {
            " "
        } else {
            ""
        };
        lines.push(
            Line::from(alloc::format!("{}{}", self.url, url_cursor)).style(
                if self.focus == ServerConfigFocus::Url {
                    theme.input
                } else {
                    theme.text
                },
            ),
        );
        lines.push(Line::from("Username").style(theme.text_dim));
        let username_cursor = if self.focus == ServerConfigFocus::Username {
            " "
        } else {
            ""
        };
        lines.push(
            Line::from(alloc::format!("{}{}", self.username, username_cursor)).style(
                if self.focus == ServerConfigFocus::Username {
                    theme.input
                } else {
                    theme.text
                },
            ),
        );
        lines.push(Line::from("Password").style(theme.text_dim));
        let password_cursor = if self.focus == ServerConfigFocus::Password {
            " "
        } else {
            ""
        };
        let masked_password = "*".repeat(self.password.chars().count());
        lines.push(
            Line::from(alloc::format!("{}{}", masked_password, password_cursor)).style(
                if self.focus == ServerConfigFocus::Password {
                    theme.input
                } else {
                    theme.text
                },
            ),
        );
        lines.push(Line::from(""));
        let apply_style = if self.focus == ServerConfigFocus::Apply {
            theme.dialog_button_focused
        } else {
            theme.dialog_button
        };
        lines.push(Line::from(vec![Span::styled(" Apply ", apply_style)]));
        lines.push(Line::from(""));
        let connection_style = match self.active_connection.tier {
            ConnectionTier::Green => theme.toast_success,
            ConnectionTier::Yellow => theme.toast_warning,
            ConnectionTier::Red => theme.text_error,
        };
        lines.push(self.active_connection_line().style(connection_style));
        let status_style = match self.status {
            ServerConfigStatus::Error(_) => theme.text_error,
            _ => theme.text_dim,
        };
        lines.push(self.status_line().style(status_style));
        lines.push(Line::from(""));
        lines.push(
            Line::from("Tab/Arrows move  Enter confirm/apply  Esc cancel").style(theme.text_dim),
        );

        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .render(area, frame.buffer_mut());
    }

    fn handle_event(&mut self, event: Event) -> Action {
        match event {
            Event::Key(ref ke) => match ke.scancode {
                Scancode::Escape => Action::CloseModal,
                Scancode::Tab | Scancode::Right => {
                    self.cycle_focus_forward();
                    Action::None
                }
                Scancode::Left => {
                    self.cycle_focus_back();
                    Action::None
                }
                Scancode::Backspace
                    if matches!(
                        self.focus,
                        ServerConfigFocus::Url
                            | ServerConfigFocus::Username
                            | ServerConfigFocus::Password
                    ) =>
                {
                    self.backspace_focused_field();
                    Action::None
                }
                Scancode::Char(ch)
                    if matches!(
                        self.focus,
                        ServerConfigFocus::Url
                            | ServerConfigFocus::Username
                            | ServerConfigFocus::Password
                    ) =>
                {
                    if !ke.modifiers.ctrl && !ke.modifiers.alt && !ke.modifiers.meta {
                        self.edit_focused_field(ch);
                    }
                    Action::None
                }
                Scancode::Enter => match self.focus {
                    // Pressing Enter while editing a field must not switch the
                    // connection mid-typing; move focus to Apply so the switch
                    // only happens on a deliberate confirmation.
                    ServerConfigFocus::Url
                    | ServerConfigFocus::Username
                    | ServerConfigFocus::Password => {
                        self.focus = ServerConfigFocus::Apply;
                        Action::None
                    }
                    ServerConfigFocus::Apply => {
                        self.set_applying();
                        Action::ApplyServerConnection(self.connection())
                    }
                },
                _ => Action::None,
            },
            _ => Action::None,
        }
    }

    fn title(&self) -> &str {
        "Server Connection"
    }

    fn as_server_config_mut(&mut self) -> Option<&mut ServerConfigModal> {
        Some(self)
    }

    fn preferred_size(&self, area: Rect) -> (u16, u16) {
        let width = if area.width < 50 {
            area.width
        } else {
            ((area.width as u32 * 3) / 5).clamp(56, area.width as u32) as u16
        };
        (width, 11_u16.min(area.height))
    }
}

// --- Display config modal ---

pub struct DisplayConfigModal {
    policy: ChatDisplayPolicy,
    selected: usize,
}

impl DisplayConfigModal {
    pub(crate) fn new(policy: ChatDisplayPolicy) -> Self {
        Self {
            policy,
            selected: 0,
        }
    }

    pub(crate) fn set_policy(&mut self, policy: ChatDisplayPolicy) {
        self.policy = policy;
    }

    fn selected_kind(&self) -> crate::chat::PartKind {
        PART_KIND_ORDER[self.selected.min(PART_KIND_ORDER.len().saturating_sub(1))]
    }

    fn selected_next_action(&self) -> Action {
        let kind = self.selected_kind();
        Action::SetDisplayMode(kind, self.policy.mode_for_kind(kind).next())
    }
}

impl Modal for DisplayConfigModal {
    fn render(&self, frame: &mut Frame, theme: &Theme, area: Rect) {
        let list_height = area.height.saturating_sub(1);
        let list_area = Rect::new(area.x, area.y, area.width, list_height);
        let rows: Vec<ListItem<'_>> = PART_KIND_ORDER
            .iter()
            .map(|kind| {
                let mode = self.policy.mode_for_kind(*kind);
                ListItem::new(Line::from(alloc::format!(
                    "{:<12} {}",
                    kind.label(),
                    mode.label()
                )))
            })
            .collect();
        let mut state = ListState::default().with_selected(Some(self.selected));
        StatefulWidget::render(
            List::new(rows)
                .style(theme.text)
                .highlight_style(theme.selection)
                .highlight_symbol("> "),
            list_area,
            frame.buffer_mut(),
            &mut state,
        );

        if area.height > 0 {
            let help_y = area.y + area.height.saturating_sub(1);
            let help = fit_with_ellipsis(
                "Up/Down move  Enter cycle  Esc close".into(),
                area.width as usize,
            );
            Line::from(help)
                .style(theme.text_dim)
                .render(Rect::new(area.x, help_y, area.width, 1), frame.buffer_mut());
        }
    }

    fn handle_event(&mut self, event: Event) -> Action {
        match event {
            Event::Key(ref ke) => match ke.scancode {
                Scancode::Escape => Action::CloseModal,
                Scancode::Up => {
                    self.selected = self.selected.saturating_sub(1);
                    Action::None
                }
                Scancode::Down => {
                    self.selected = self
                        .selected
                        .saturating_add(1)
                        .min(PART_KIND_ORDER.len().saturating_sub(1));
                    Action::None
                }
                Scancode::Enter | Scancode::Char(' ') => self.selected_next_action(),
                _ => Action::None,
            },
            _ => Action::None,
        }
    }

    fn title(&self) -> &str {
        "Display"
    }

    fn as_display_config_mut(&mut self) -> Option<&mut DisplayConfigModal> {
        Some(self)
    }

    fn preferred_size(&self, area: Rect) -> (u16, u16) {
        let width = if area.width < 36 {
            area.width
        } else {
            ((area.width as u32 * 2) / 5).clamp(44, area.width as u32) as u16
        };
        (width, 15.min(area.height))
    }
}

// --- Model picker modal ---

pub struct ModelPickerModal {
    current_model: Option<String>,
    agent_model: Option<String>,
    models: Vec<ModelChoice>,
    filtered_indices: Vec<usize>,
    search: String,
    selected: usize,
    scroll: u16,
    visible_rows: Cell<u16>,
    error: Option<String>,
}

#[derive(Debug, Clone)]
struct ModelChoice {
    id: String,
    label: String,
    provider: String,
    family: Option<String>,
}

impl ModelPickerModal {
    pub fn new() -> Self {
        Self {
            current_model: None,
            agent_model: None,
            models: Vec::new(),
            filtered_indices: Vec::new(),
            search: String::new(),
            selected: 0,
            scroll: 0,
            visible_rows: Cell::new(10),
            error: None,
        }
    }

    pub fn set_config(&mut self, config: Config) {
        self.update_current_from_config(&config);

        let mut models = Vec::new();
        for (provider_id, provider) in config.provider {
            for (model_id, model) in provider.models {
                let full_id = format!("{provider_id}/{model_id}");
                let model_label = model
                    .name
                    .clone()
                    .unwrap_or_else(|| model.id.clone().unwrap_or_else(|| model_id.clone()));
                models.push(ModelChoice {
                    id: full_id,
                    label: model_label,
                    provider: provider_id.clone(),
                    family: model.family.clone(),
                });
            }
        }
        self.set_models(models);
    }

    pub fn set_models_from_config(&mut self, config: Config, models: &[ModelSummary]) {
        self.update_current_from_config(&config);

        let mut choices: Vec<ModelChoice> = models
            .iter()
            .map(|model| ModelChoice {
                id: format!("{}/{}", model.provider_id, model.id),
                label: model.name.clone().unwrap_or_else(|| model.id.clone()),
                provider: model.provider_id.clone(),
                family: model.family.clone(),
            })
            .collect();

        for (provider_id, provider) in config.provider {
            for (model_id, model) in provider.models {
                let full_id = format!("{provider_id}/{model_id}");
                let model_label = model
                    .name
                    .clone()
                    .unwrap_or_else(|| model.id.clone().unwrap_or_else(|| model_id.clone()));
                choices.push(ModelChoice {
                    id: full_id,
                    label: model_label,
                    provider: provider_id.clone(),
                    family: model.family.clone(),
                });
            }
        }
        self.set_models(choices);
    }

    pub fn update_current_from_config(&mut self, config: &Config) {
        self.current_model = config.model.clone();
        self.agent_model = config
            .agent
            .get("build")
            .and_then(|agent| agent.model.clone())
            .or_else(|| config.agent.values().find_map(|agent| agent.model.clone()));
        self.error = None;
    }

    fn set_models(&mut self, mut models: Vec<ModelChoice>) {
        let mut unique = BTreeMap::new();
        for model in models.drain(..) {
            if !unique.contains_key(&model.id) {
                unique.insert(model.id.clone(), model);
            }
        }
        self.models = unique.into_values().collect();
        self.refilter();
        self.selected = self
            .current_model
            .as_ref()
            .and_then(|current| {
                self.filtered_indices
                    .iter()
                    .position(|index| self.models[*index].id == *current)
            })
            .unwrap_or(0);
        self.ensure_selected_visible(self.visible_rows.get());
    }

    fn refilter(&mut self) {
        let query = self.search.to_lowercase();
        self.filtered_indices = self
            .models
            .iter()
            .enumerate()
            .filter_map(|(idx, choice)| {
                if query.is_empty()
                    || choice.id.to_lowercase().contains(&query)
                    || choice.label.to_lowercase().contains(&query)
                    || choice.provider.to_lowercase().contains(&query)
                    || choice
                        .family
                        .as_deref()
                        .map(|family| family.to_lowercase().contains(&query))
                        .unwrap_or(false)
                {
                    Some(idx)
                } else {
                    None
                }
            })
            .collect();
        if self.filtered_indices.is_empty() {
            self.selected = 0;
            self.scroll = 0;
        } else {
            self.selected = self.selected.min(self.filtered_indices.len() - 1);
            self.ensure_selected_visible(self.visible_rows.get());
        }
    }

    #[cfg(test)]
    fn filtered_len(&self) -> usize {
        self.filtered_indices.len()
    }

    fn set_search(&mut self, search: String) {
        self.search = search;
        self.selected = 0;
        self.scroll = 0;
        self.refilter();
    }

    #[cfg(test)]
    fn search(&self) -> &str {
        &self.search
    }

    #[cfg(test)]
    fn visible_model_count(&self) -> usize {
        self.filtered_len()
    }

    #[cfg(test)]
    fn scroll_offset(&self) -> u16 {
        self.scroll
    }

    #[cfg(test)]
    fn search_matches(&self, model: &str) -> bool {
        self.filtered_indices
            .iter()
            .any(|index| self.models[*index].id == model)
    }

    #[cfg(test)]
    fn set_visible_rows_for_test(&self, rows: u16) {
        self.visible_rows.set(rows);
    }

    fn current_marker(&self, choice: &ModelChoice) -> &'static str {
        if Some(choice.id.as_str()) == self.current_model.as_deref() {
            "*"
        } else {
            " "
        }
    }

    fn current_display(&self) -> String {
        let Some(current) = self
            .current_model
            .as_deref()
            .or(self.agent_model.as_deref())
        else {
            return "No model configured".into();
        };
        self.models
            .iter()
            .find(|choice| choice.id == current)
            .map(|choice| format!("{}  [{}]", choice.label, choice.provider))
            .unwrap_or_else(|| current.into())
    }

    fn move_selected(&mut self, delta: isize) {
        if self.filtered_indices.is_empty() {
            self.selected = 0;
            self.scroll = 0;
            return;
        }
        if delta < 0 {
            self.selected = self.selected.saturating_sub(delta.unsigned_abs());
        } else {
            self.selected = (self.selected + delta as usize).min(self.filtered_indices.len() - 1);
        }
        self.ensure_selected_visible(self.visible_rows.get());
    }

    fn page_amount(&self) -> usize {
        self.visible_rows.get().max(1) as usize
    }

    fn search_input(&mut self, scancode: Scancode) -> bool {
        match scancode {
            Scancode::Char(c) => {
                self.search.push(c);
                self.set_search(self.search.clone());
                true
            }
            Scancode::Backspace => {
                self.search.pop();
                self.set_search(self.search.clone());
                true
            }
            _ => false,
        }
    }

    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }

    pub fn selected_model(&self) -> Option<&str> {
        self.models
            .get(*self.filtered_indices.get(self.selected)?)
            .map(|choice| choice.id.as_str())
    }

    #[cfg(test)]
    fn selected_index(&self) -> usize {
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

        let current_label = fit_with_ellipsis(
            format!("Current: {}", self.current_display()),
            area.width as usize,
        );
        Text::from(current_label)
            .style(theme.text_dim)
            .render(Rect::new(area.x, area.y, area.width, 1), frame.buffer_mut());

        let search_label =
            fit_with_ellipsis(format!("Search: {}", self.search), area.width as usize);
        Text::from(search_label).style(theme.text).render(
            Rect::new(area.x, area.y + 1, area.width, 1),
            frame.buffer_mut(),
        );

        if self.models.is_empty() {
            Text::from("No models found in server config")
                .style(theme.text_dim)
                .render(
                    Rect::new(area.x, area.y + 3, area.width, 1.min(area.height)),
                    frame.buffer_mut(),
                );
            return;
        }

        if self.filtered_indices.is_empty() {
            Text::from("No models match search")
                .style(theme.text_dim)
                .render(
                    Rect::new(area.x, area.y + 3, area.width, 1.min(area.height)),
                    frame.buffer_mut(),
                );
            return;
        }

        let list_y = area.y.saturating_add(3);
        if list_y >= area.bottom() {
            return;
        }
        let list_area = Rect::new(area.x, list_y, area.width, area.bottom() - list_y);
        let visible_rows = list_area.height.max(1);
        self.visible_rows.set(visible_rows);
        let needs_scrollbar = self.filtered_indices.len() > visible_rows as usize;
        let list_content_area = if needs_scrollbar && list_area.width > 1 {
            Rect::new(
                list_area.x,
                list_area.y,
                list_area.width - 1,
                list_area.height,
            )
        } else {
            list_area
        };
        let rows: Vec<ListItem<'_>> = self
            .filtered_indices
            .iter()
            .skip(self.scroll as usize)
            .take(visible_rows as usize)
            .filter_map(|model_index| self.models.get(*model_index))
            .map(|choice| {
                let marker = self.current_marker(choice);
                let display = format!("{marker} {}  [{}]", choice.label, choice.provider);
                ListItem::new(Line::from(fit_with_ellipsis(
                    display,
                    list_content_area.width as usize,
                )))
            })
            .collect();
        let mut state = ListState::default();
        state.select(Some(self.selected.saturating_sub(self.scroll as usize)));
        StatefulWidget::render(
            List::new(rows)
                .style(theme.text)
                .highlight_style(theme.selection),
            list_content_area,
            frame.buffer_mut(),
            &mut state,
        );

        if needs_scrollbar {
            let max_scroll = self
                .filtered_indices
                .len()
                .saturating_sub(visible_rows as usize);
            let mut scroll_state =
                ScrollbarState::new(max_scroll.max(1)).position(self.scroll as usize);
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
                    self.move_selected(-1);
                    Action::None
                }
                Scancode::Down => {
                    self.move_selected(1);
                    Action::None
                }
                Scancode::PageUp => {
                    self.move_selected(-(self.page_amount() as isize));
                    Action::None
                }
                Scancode::PageDown => {
                    self.move_selected(self.page_amount() as isize);
                    Action::None
                }
                Scancode::Enter => self
                    .selected_model()
                    .map(|model| Action::SelectModel(model.to_string()))
                    .unwrap_or(Action::None),
                scancode if self.search_input(scancode) => Action::None,
                _ => Action::None,
            },
            _ => Action::None,
        }
    }

    fn title(&self) -> &str {
        "Model Picker"
    }

    fn as_model_picker(&self) -> Option<&ModelPickerModal> {
        Some(self)
    }

    fn as_model_picker_mut(&mut self) -> Option<&mut ModelPickerModal> {
        Some(self)
    }
}

// --- Help modal ---

const HELP_LINE_COUNT: u16 = 32;

pub struct HelpModal {
    scroll: u16,
    visible_height: Cell<u16>,
}

impl HelpModal {
    pub fn new() -> Self {
        Self {
            scroll: 0,
            visible_height: Cell::new(1),
        }
    }

    fn lines(&self, theme: &Theme) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'_>> = Vec::new();
        for (text, heading) in [
            ("Slash Commands", true),
            ("  /help         Show this help", false),
            ("  /sessions     List sessions", false),
            ("  /new          New session", false),
            ("  /models       Model picker", false),
            ("  /todos        Toggle todos panel", false),
            ("  /diagnostics  Toggle diagnostics panel", false),
            ("  /pty          Toggle terminal panel", false),
            ("  /server       Configure server connection", false),
            ("  /display      Configure transcript detail", false),
            ("  /abort        Abort current session", false),
            ("  /exit         Quit", false),
            ("", false),
            ("Keybindings", true),
            ("  Ctrl+X H      Help", false),
            ("  Ctrl+X Q      Quit", false),
            ("  Ctrl+X N      New session", false),
            ("  Ctrl+X L      Sessions", false),
            ("  Ctrl+X M      Models", false),
            ("  Ctrl+X T      Terminal", false),
            ("  Ctrl+X D      Diagnostics", false),
            ("  Ctrl+X O      Todos", false),
            ("  Ctrl+P        Command palette", false),
            ("  Tab           Cycle agent forward", false),
            ("  Shift+Tab     Cycle agent backward", false),
            ("  Escape        Close modal / interrupt", false),
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
        lines
    }

    fn move_scroll(&mut self, delta: i16) {
        let visible_height = self.visible_height.get().max(1);
        let max_scroll = HELP_LINE_COUNT.saturating_sub(visible_height);
        let scroll = self.scroll.min(max_scroll);
        if delta < 0 {
            self.scroll = scroll.saturating_sub(delta.unsigned_abs());
        } else {
            self.scroll = scroll.saturating_add(delta as u16).min(max_scroll);
        }
    }

    #[cfg(test)]
    fn set_visible_height_for_test(&self, visible_height: u16) {
        self.visible_height.set(visible_height);
    }

    #[cfg(test)]
    fn scroll_offset(&self) -> u16 {
        self.scroll
    }
}

impl Modal for HelpModal {
    fn render(&self, frame: &mut Frame, theme: &Theme, area: Rect) {
        let lines = self.lines(theme);
        let content_len = lines.len();
        let visible_height = area.height.max(1);
        self.visible_height.set(visible_height);
        let scroll = self
            .scroll
            .min((content_len as u16).saturating_sub(visible_height));
        let content_area = if content_len > visible_height as usize && area.width > 1 {
            Rect::new(area.x, area.y, area.width - 1, area.height)
        } else {
            area
        };
        let visible_lines: Vec<Line<'_>> = lines
            .into_iter()
            .skip(scroll as usize)
            .take(visible_height as usize)
            .collect();

        Paragraph::new(Text::from(visible_lines))
            .wrap(Wrap { trim: false })
            .render(content_area, frame.buffer_mut());

        if content_len > visible_height as usize {
            let max_scroll = (content_len as u16).saturating_sub(visible_height);
            let mut state =
                ScrollbarState::new(max_scroll.max(1) as usize).position(scroll as usize);
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .thumb_style(theme.scrollbar)
                .track_style(theme.text_dim)
                .render(area, frame.buffer_mut(), &mut state);
        }
    }

    fn handle_event(&mut self, event: Event) -> Action {
        match event {
            Event::Key(ref ke) if ke.scancode == Scancode::Escape => Action::CloseModal,
            Event::Key(ref ke) if ke.scancode == Scancode::Up => {
                self.move_scroll(-1);
                Action::None
            }
            Event::Key(ref ke) if ke.scancode == Scancode::Down => {
                self.move_scroll(1);
                Action::None
            }
            Event::Key(ref ke) if ke.scancode == Scancode::PageUp => {
                self.move_scroll(-8);
                Action::None
            }
            Event::Key(ref ke) if ke.scancode == Scancode::PageDown => {
                self.move_scroll(8);
                Action::None
            }
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

    #[test]
    fn server_config_modal_edits_url_and_applies() {
        let mut modal = ServerConfigModal::new("http://localhost:4096".into());
        modal.handle_event(key(Scancode::Char('/')));
        modal.handle_event(key(Scancode::Char('v')));
        assert_eq!(modal.url(), "http://localhost:4096/v");
        modal.handle_event(key(Scancode::Backspace));
        assert_eq!(modal.url(), "http://localhost:4096/");

        // Enter while editing the URL must not apply; it only moves focus to
        // Apply so a switch requires a deliberate confirmation.
        let action = modal.handle_event(key(Scancode::Enter));
        assert_eq!(action, Action::None);

        // The follow-up Enter on the now-focused Apply button applies.
        let action = modal.handle_event(key(Scancode::Enter));
        assert_eq!(
            action,
            Action::ApplyServerConnection(ServerConnection::new(
                "http://localhost:4096/",
                None::<String>,
                None::<String>,
            ))
        );
    }

    #[test]
    fn server_config_modal_enter_while_typing_does_not_apply() {
        let mut modal = ServerConfigModal::new("http://localhost:4096".into());
        // Type a partial change, then hit Enter as if confirming the field.
        modal.handle_event(key(Scancode::Char('/')));
        let action = modal.handle_event(key(Scancode::Enter));
        assert_eq!(
            action,
            Action::None,
            "Enter mid-edit must not switch the connection"
        );
        // Editing remains possible afterwards (focus moved to Apply, no switch).
        let action = modal.handle_event(key(Scancode::Enter));
        assert_eq!(
            action,
            Action::ApplyServerConnection(ServerConnection::new(
                "http://localhost:4096/",
                None::<String>,
                None::<String>,
            ))
        );
    }

    #[test]
    fn server_config_modal_apply_button_returns_apply_action() {
        let mut modal = ServerConfigModal::new("http://localhost:4096".into());
        // Url -> Username -> Password -> Apply.
        modal.handle_event(key(Scancode::Tab));
        modal.handle_event(key(Scancode::Tab));
        modal.handle_event(key(Scancode::Tab));
        let action = modal.handle_event(key(Scancode::Enter));
        assert_eq!(
            action,
            Action::ApplyServerConnection(ServerConnection::new(
                "http://localhost:4096",
                None::<String>,
                None::<String>,
            ))
        );
    }

    #[test]
    fn server_config_modal_masks_password_and_returns_auth() {
        let mut modal = ServerConfigModal::new_with_connection(ServerConnection::new(
            "http://localhost:4096",
            Some("user"),
            Some("secret"),
        ));
        assert_eq!(modal.username(), "user");
        assert_eq!(modal.password(), "secret");

        let backend = TestBackend::new(80, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| modal.render(frame, &Theme::default(), frame.area()))
            .unwrap();
        let screen = rendered_screen_for_test(&terminal);
        assert!(screen.contains("******"), "screen: {screen}");
        assert!(!screen.contains("secret"), "screen: {screen}");

        modal.handle_event(key(Scancode::Tab));
        modal.handle_event(key(Scancode::Tab));
        modal.handle_event(key(Scancode::Tab));
        let action = modal.handle_event(key(Scancode::Enter));
        assert_eq!(
            action,
            Action::ApplyServerConnection(ServerConnection::new(
                "http://localhost:4096",
                Some("user"),
                Some("secret"),
            ))
        );
    }

    #[test]
    fn display_config_modal_cycles_selected_part_mode() {
        let mut modal = DisplayConfigModal::new(ChatDisplayPolicy::default());
        let action = modal.handle_event(key(Scancode::Enter));
        assert_eq!(
            action,
            Action::SetDisplayMode(
                crate::chat::PartKind::Text,
                crate::chat::PartDisplayMode::Summary
            )
        );
    }

    fn make_session(id: &str, title: &str) -> Session {
        Session {
            id: id.into(),
            title: title.into(),
            project_id: "p1".into(),
            directory: "/".into(),
            path: None,
            parent_id: None,
            time: SessionTime {
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

    fn key(scancode: Scancode) -> Event {
        Event::Key(crate::event::KeyEvent {
            scancode,
            modifiers: Default::default(),
        })
    }

    fn rendered_screen_for_test(terminal: &Terminal<TestBackend>) -> String {
        let buffer = terminal.backend().buffer();
        let area = buffer.area;
        let mut out = String::new();
        for y in area.y..area.y + area.height {
            for x in area.x..area.x + area.width {
                out.push_str(buffer.cell((x, y)).map_or(" ", |cell| cell.symbol()));
            }
            out.push('\n');
        }
        out
    }

    fn permission_request(id: &str) -> PermissionRequest {
        PermissionRequest {
            id: id.into(),
            session_id: "session-1".into(),
            permission: "bash".into(),
            patterns: vec!["/tmp/**".into()],
            metadata: BTreeMap::new(),
            always: Vec::new(),
            tool: Some(PermissionToolInfo {
                message_id: "m1".into(),
                call_id: "c1".into(),
            }),
        }
    }

    fn single_question_request(id: &str) -> QuestionRequest {
        QuestionRequest {
            id: id.into(),
            session_id: "session-1".into(),
            questions: vec![QuestionInfo {
                question: "Proceed?".into(),
                header: "Confirm".into(),
                options: vec![
                    QuestionOption {
                        label: "A".into(),
                        description: "first".into(),
                    },
                    QuestionOption {
                        label: "B".into(),
                        description: "second".into(),
                    },
                ],
                multiple: false,
                custom: false,
            }],
            tool: None,
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
    fn model_picker_shows_models_from_catalog() {
        use ocpncord_backend::{Config, ModelSummary};
        let models = vec![ModelSummary {
            id: "anthropic/claude-sonnet-4".into(),
            provider_id: "openrouter".into(),
            name: Some("Claude Sonnet 4".into()),
            ..Default::default()
        }];
        let mut modal = ModelPickerModal::new();
        modal.set_models_from_config(
            Config {
                model: Some("openrouter/anthropic/claude-sonnet-4".into()),
                username: None,
                provider: Default::default(),
                agent: Default::default(),
            },
            &models,
        );

        let theme = Theme::default();
        let backend = TestBackend::new(70, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                Modal::render(&modal, frame, &theme, Rect::new(5, 5, 60, 10));
            })
            .unwrap();
        let screen: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(
            screen.contains("Claude Sonnet 4"),
            "Catalog model should appear. Screen: {}",
            screen
        );
    }

    #[test]
    fn model_picker_search_filters_by_name_provider_id_and_family() {
        use ocpncord_backend::{Config, ModelSummary};
        let mut modal = ModelPickerModal::new();
        modal.set_models_from_config(
            Config::default(),
            &[
                ModelSummary {
                    id: "claude-sonnet".into(),
                    provider_id: "anthropic".into(),
                    name: Some("Claude Sonnet".into()),
                    family: Some("claude".into()),
                    ..Default::default()
                },
                ModelSummary {
                    id: "gpt-4".into(),
                    provider_id: "openai".into(),
                    name: Some("GPT-4".into()),
                    family: Some("gpt".into()),
                    ..Default::default()
                },
            ],
        );

        modal.handle_event(Event::Key(crate::event::KeyEvent {
            scancode: Scancode::Char('c'),
            modifiers: Default::default(),
        }));
        modal.handle_event(Event::Key(crate::event::KeyEvent {
            scancode: Scancode::Char('l'),
            modifiers: Default::default(),
        }));

        assert_eq!(modal.search(), "cl");
        assert_eq!(modal.visible_model_count(), 1);
        assert!(modal.search_matches("anthropic/claude-sonnet"));
        assert!(!modal.search_matches("openai/gpt-4"));
    }

    #[test]
    fn model_picker_search_backspace_restores_matches() {
        use ocpncord_backend::{Config, ModelSummary};
        let mut modal = ModelPickerModal::new();
        modal.set_models_from_config(
            Config::default(),
            &[
                ModelSummary {
                    id: "claude-sonnet".into(),
                    provider_id: "anthropic".into(),
                    name: Some("Claude Sonnet".into()),
                    ..Default::default()
                },
                ModelSummary {
                    id: "gpt-4".into(),
                    provider_id: "openai".into(),
                    name: Some("GPT-4".into()),
                    ..Default::default()
                },
            ],
        );

        modal.handle_event(Event::Key(crate::event::KeyEvent {
            scancode: Scancode::Char('z'),
            modifiers: Default::default(),
        }));
        assert_eq!(modal.visible_model_count(), 0);

        modal.handle_event(Event::Key(crate::event::KeyEvent {
            scancode: Scancode::Backspace,
            modifiers: Default::default(),
        }));
        assert_eq!(modal.search(), "");
        assert_eq!(modal.visible_model_count(), 2);
    }

    #[test]
    fn model_picker_includes_custom_config_models_when_api_models_are_available() {
        use ocpncord_backend::{Config, ModelConfig, ModelSummary, ProviderConfig};
        let mut modal = ModelPickerModal::new();
        modal.set_models_from_config(
            Config {
                model: None,
                username: None,
                provider: BTreeMap::from([(
                    "openrouter-shortcut".into(),
                    ProviderConfig {
                        name: Some("OpenRouter Shortcut".into()),
                        models: BTreeMap::from([(
                            "big-pickle".into(),
                            ModelConfig {
                                name: Some("Big Pickle".into()),
                                family: Some("pickle".into()),
                                ..Default::default()
                            },
                        )]),
                        ..Default::default()
                    },
                )]),
                agent: Default::default(),
            },
            &[ModelSummary {
                id: "claude-sonnet".into(),
                provider_id: "anthropic".into(),
                name: Some("Claude Sonnet".into()),
                ..Default::default()
            }],
        );

        modal.handle_event(Event::Key(crate::event::KeyEvent {
            scancode: Scancode::Char('p'),
            modifiers: Default::default(),
        }));
        modal.handle_event(Event::Key(crate::event::KeyEvent {
            scancode: Scancode::Char('i'),
            modifiers: Default::default(),
        }));
        modal.handle_event(Event::Key(crate::event::KeyEvent {
            scancode: Scancode::Char('c'),
            modifiers: Default::default(),
        }));
        modal.handle_event(Event::Key(crate::event::KeyEvent {
            scancode: Scancode::Char('k'),
            modifiers: Default::default(),
        }));
        modal.handle_event(Event::Key(crate::event::KeyEvent {
            scancode: Scancode::Char('l'),
            modifiers: Default::default(),
        }));
        modal.handle_event(Event::Key(crate::event::KeyEvent {
            scancode: Scancode::Char('e'),
            modifiers: Default::default(),
        }));

        assert_eq!(modal.visible_model_count(), 1);
        assert!(modal.search_matches("openrouter-shortcut/big-pickle"));
    }

    #[test]
    fn model_picker_page_down_uses_visible_row_count() {
        use ocpncord_backend::{Config, ModelSummary};
        let models: Vec<ModelSummary> = (0..20)
            .map(|idx| ModelSummary {
                id: alloc::format!("model-{idx:02}"),
                provider_id: "provider".into(),
                name: Some(alloc::format!("Model {idx:02}")),
                ..Default::default()
            })
            .collect();
        let mut modal = ModelPickerModal::new();
        modal.set_models_from_config(Config::default(), &models);
        modal.set_visible_rows_for_test(5);

        modal.handle_event(Event::Key(crate::event::KeyEvent {
            scancode: Scancode::PageDown,
            modifiers: Default::default(),
        }));

        assert_eq!(modal.selected_index(), 5);
        assert_eq!(modal.scroll_offset(), 1);
    }

    #[test]
    fn model_picker_truncates_current_model_with_ellipsis() {
        use ocpncord_backend::{Config, ModelConfig, ProviderConfig};
        let mut provider = BTreeMap::new();
        provider.insert(
            "very-long-provider-name".into(),
            ProviderConfig {
                name: Some("Very Long Provider".into()),
                models: BTreeMap::from([(
                    "very-long-model-name-that-does-not-fit".into(),
                    ModelConfig {
                        name: Some("Long Model".into()),
                        ..Default::default()
                    },
                )]),
                ..Default::default()
            },
        );
        let mut modal = ModelPickerModal::new();
        modal.set_config(Config {
            model: Some("very-long-provider-name/very-long-model-name-that-does-not-fit".into()),
            username: None,
            provider,
            agent: Default::default(),
        });

        let theme = Theme::default();
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                Modal::render(&modal, frame, &theme, Rect::new(0, 0, 24, 6));
            })
            .unwrap();
        let screen: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(screen.contains("..."), "screen: {screen}");
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
            screen.contains("/models"),
            "Should show /models. Screen: {}",
            screen
        );
        assert!(
            screen.contains("/exit"),
            "Should show /exit. Screen: {}",
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
    fn help_modal_clamps_scroll_at_bottom() {
        let mut modal = HelpModal::new();
        modal.set_visible_height_for_test(20);

        for _ in 0..40 {
            modal.handle_event(key(Scancode::Down));
        }
        assert_eq!(modal.scroll_offset(), HELP_LINE_COUNT - 20);

        modal.handle_event(key(Scancode::Up));
        assert_eq!(modal.scroll_offset(), HELP_LINE_COUNT - 21);
    }

    #[test]
    fn help_modal_renders_scrollbar_when_scrollable() {
        let modal = HelpModal::new();
        let theme = Theme::default();
        let backend = TestBackend::new(60, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                modal.render(frame, &theme, Rect::new(0, 0, 60, 20));
            })
            .unwrap();
        let screen: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(screen.contains("█"), "screen: {screen}");
        assert!(screen.contains("▼"), "screen: {screen}");
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

    #[test]
    fn permission_modal_escape_returns_reject_reply() {
        let mut modal = PermissionModal::new(permission_request("permission-1"));

        let action = modal.handle_event(key(Scancode::Escape));

        assert!(matches!(
            action,
            Action::ReplyPermission(session_id, request_id, PermissionReplyAction::Reject)
                if session_id == "session-1" && request_id == "permission-1"
        ));
    }

    #[test]
    fn question_modal_single_select_returns_nested_answer() {
        let mut modal = QuestionModal::new(single_question_request("question-1"));

        let action = modal.handle_event(key(Scancode::Enter));

        assert!(matches!(
            action,
            Action::ReplyQuestion(session_id, request_id, answers)
                if session_id == "session-1"
                    && request_id == "question-1"
                    && answers == vec![vec!["A".to_string()]]
        ));
    }

    #[test]
    fn question_modal_multi_select_collects_toggled_answers() {
        let request = QuestionRequest {
            id: "question-1".into(),
            session_id: "session-1".into(),
            questions: vec![QuestionInfo {
                question: "Choose several".into(),
                header: "Multi".into(),
                options: vec![
                    QuestionOption {
                        label: "A".into(),
                        description: "first".into(),
                    },
                    QuestionOption {
                        label: "B".into(),
                        description: "second".into(),
                    },
                ],
                multiple: true,
                custom: false,
            }],
            tool: None,
        };
        let mut modal = QuestionModal::new(request);

        assert!(matches!(
            modal.handle_event(key(Scancode::Char(' '))),
            Action::None
        ));
        assert!(matches!(
            modal.handle_event(key(Scancode::Down)),
            Action::None
        ));
        assert!(matches!(
            modal.handle_event(key(Scancode::Char(' '))),
            Action::None
        ));

        let action = modal.handle_event(key(Scancode::Enter));

        assert!(matches!(
            action,
            Action::ReplyQuestion(_, _, answers) if answers == vec![vec!["A".to_string(), "B".to_string()]]
        ));
    }

    #[test]
    fn question_modal_custom_input_returns_custom_answer() {
        let request = QuestionRequest {
            id: "question-1".into(),
            session_id: "session-1".into(),
            questions: vec![QuestionInfo {
                question: "Type your answer".into(),
                header: "Custom".into(),
                options: Vec::new(),
                multiple: false,
                custom: true,
            }],
            tool: None,
        };
        let mut modal = QuestionModal::new(request);

        assert!(matches!(
            modal.handle_event(key(Scancode::Char('o'))),
            Action::None
        ));
        assert!(matches!(
            modal.handle_event(key(Scancode::Char('k'))),
            Action::None
        ));

        let action = modal.handle_event(key(Scancode::Enter));

        assert!(matches!(
            action,
            Action::ReplyQuestion(_, _, answers) if answers == vec![vec!["ok".to_string()]]
        ));
    }

    #[test]
    fn question_modal_supports_multiple_questions_and_back_navigation() {
        let request = QuestionRequest {
            id: "question-1".into(),
            session_id: "session-1".into(),
            questions: vec![
                QuestionInfo {
                    question: "First".into(),
                    header: "One".into(),
                    options: vec![
                        QuestionOption {
                            label: "A".into(),
                            description: "first".into(),
                        },
                        QuestionOption {
                            label: "B".into(),
                            description: "second".into(),
                        },
                    ],
                    multiple: false,
                    custom: false,
                },
                QuestionInfo {
                    question: "Second".into(),
                    header: "Two".into(),
                    options: vec![
                        QuestionOption {
                            label: "C".into(),
                            description: "third".into(),
                        },
                        QuestionOption {
                            label: "D".into(),
                            description: "fourth".into(),
                        },
                    ],
                    multiple: false,
                    custom: false,
                },
            ],
            tool: None,
        };
        let mut modal = QuestionModal::new(request);

        assert!(matches!(
            modal.handle_event(key(Scancode::Enter)),
            Action::None
        ));
        assert_eq!(modal.current_q, 1);
        assert!(matches!(
            modal.handle_event(key(Scancode::Left)),
            Action::None
        ));
        assert_eq!(modal.current_q, 0);
        assert!(matches!(
            modal.handle_event(key(Scancode::Right)),
            Action::None
        ));
        assert_eq!(modal.current_q, 1);
        assert!(matches!(
            modal.handle_event(key(Scancode::Down)),
            Action::None
        ));

        let action = modal.handle_event(key(Scancode::Enter));

        assert!(matches!(
            action,
            Action::ReplyQuestion(_, _, answers)
                if answers == vec![vec!["A".to_string()], vec!["D".to_string()]]
        ));
    }

    #[test]
    fn question_modal_escape_returns_reject_action() {
        let mut modal = QuestionModal::new(single_question_request("question-escape"));

        let action = modal.handle_event(key(Scancode::Escape));

        assert!(matches!(
            action,
            Action::RejectQuestion(request_id) if request_id == "question-escape"
        ));
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
        if !self.request.always.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from("Always Rules:").style(theme.text_dim));
            for rule in self.request.always.iter() {
                lines.push(Line::from(alloc::format!("  {rule}")).style(theme.text));
            }
        }
        if !self.request.metadata.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from("Metadata:").style(theme.text_dim));
            for (key, value) in self.request.metadata.iter() {
                lines.push(Line::from(alloc::format!("  {key}: {value}")).style(theme.text));
            }
        }
        if let Some(tool) = self.request.tool.as_ref() {
            lines.push(Line::from(""));
            lines.push(Line::from("Tool Call:").style(theme.text_dim));
            lines.push(
                Line::from(alloc::format!("  Message ID: {}", tool.message_id)).style(theme.text),
            );
            lines.push(Line::from(alloc::format!("  Call ID: {}", tool.call_id)).style(theme.text));
        }
        lines.push(Line::from(""));
        lines
            .push(Line::from("Left/Right: choose  Enter: submit  Esc: deny").style(theme.text_dim));
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
                        0 => PermissionReplyAction::Once,
                        1 => PermissionReplyAction::Always,
                        _ => PermissionReplyAction::Reject,
                    };
                    Action::ReplyPermission(
                        self.request.session_id.clone(),
                        self.request.id.clone(),
                        reply,
                    )
                }
                Scancode::Escape => Action::ReplyPermission(
                    self.request.session_id.clone(),
                    self.request.id.clone(),
                    PermissionReplyAction::Reject,
                ),
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

#[derive(Debug, Clone)]
struct QuestionDraft {
    cursor: usize,
    selected_option: Option<usize>,
    selected_options: Vec<bool>,
    custom_input: String,
    custom_selected: bool,
}

impl QuestionDraft {
    fn new(question: &ocpncord_backend::QuestionInfo) -> Self {
        Self {
            cursor: 0,
            selected_option: None,
            selected_options: alloc::vec![false; question.options.len()],
            custom_input: String::new(),
            custom_selected: false,
        }
    }
}

pub struct QuestionModal {
    request: ocpncord_backend::QuestionRequest,
    current_q: usize,
    drafts: Vec<QuestionDraft>,
}

impl QuestionModal {
    pub fn new(request: ocpncord_backend::QuestionRequest) -> Self {
        let drafts = request.questions.iter().map(QuestionDraft::new).collect();
        Self {
            request,
            current_q: 0,
            drafts,
        }
    }

    fn current_question(&self) -> Option<&ocpncord_backend::QuestionInfo> {
        self.request.questions.get(self.current_q)
    }

    fn current_draft(&self) -> Option<&QuestionDraft> {
        self.drafts.get(self.current_q)
    }

    fn row_count(question: &ocpncord_backend::QuestionInfo) -> usize {
        question.options.len() + usize::from(question.custom)
    }

    fn custom_row_index(question: &ocpncord_backend::QuestionInfo) -> Option<usize> {
        question.custom.then_some(question.options.len())
    }

    fn move_cursor(&mut self, delta: isize) {
        let Some(question) = self.current_question() else {
            return;
        };
        let row_count = Self::row_count(question);
        if row_count == 0 {
            return;
        }
        let draft = &mut self.drafts[self.current_q];
        if delta < 0 {
            draft.cursor = draft.cursor.saturating_sub(delta.unsigned_abs());
        } else {
            draft.cursor = (draft.cursor + delta as usize).min(row_count.saturating_sub(1));
        }
    }

    fn append_custom_char(&mut self, ch: char) -> bool {
        let current_q = self.current_q;
        let Some(question) = self.request.questions.get(current_q) else {
            return false;
        };
        let Some(custom_index) = Self::custom_row_index(question) else {
            return false;
        };
        let multiple = question.multiple;
        let draft = &mut self.drafts[current_q];
        if draft.cursor != custom_index {
            return false;
        }
        draft.custom_input.push(ch);
        draft.custom_selected = true;
        if !multiple {
            draft.selected_option = None;
        }
        true
    }

    fn backspace_custom_input(&mut self) -> bool {
        let current_q = self.current_q;
        let Some(question) = self.request.questions.get(current_q) else {
            return false;
        };
        let Some(custom_index) = Self::custom_row_index(question) else {
            return false;
        };
        let draft = &mut self.drafts[current_q];
        if draft.cursor != custom_index {
            return false;
        }
        draft.custom_input.pop();
        if draft.custom_input.is_empty() {
            draft.custom_selected = false;
        }
        true
    }

    fn toggle_current_multi_selection(&mut self) -> bool {
        let current_q = self.current_q;
        let Some(question) = self.request.questions.get(current_q) else {
            return false;
        };
        if !question.multiple {
            return false;
        }
        let options_len = question.options.len();
        let custom_index = Self::custom_row_index(question);
        let draft = &mut self.drafts[current_q];
        if draft.cursor < options_len {
            if let Some(selected) = draft.selected_options.get_mut(draft.cursor) {
                *selected = !*selected;
                return true;
            }
            return false;
        }
        if custom_index == Some(draft.cursor) {
            draft.custom_selected = !draft.custom_selected;
            return true;
        }
        false
    }

    fn go_back(&mut self) {
        if self.current_q > 0 {
            self.current_q -= 1;
        }
    }

    fn advance_or_submit(&mut self) -> Action {
        if self.current_q + 1 < self.request.questions.len() {
            self.current_q += 1;
            Action::None
        } else {
            Action::ReplyQuestion(
                self.request.session_id.clone(),
                self.request.id.clone(),
                self.collect_answers(),
            )
        }
    }

    fn submit_current_question(&mut self) -> Action {
        let current_q = self.current_q;
        let Some(question) = self.request.questions.get(current_q) else {
            return Action::None;
        };

        if question.multiple {
            return self.advance_or_submit();
        }

        let custom_index = Self::custom_row_index(question);
        let options_len = question.options.len();
        let draft = &mut self.drafts[current_q];
        if custom_index == Some(draft.cursor) {
            if draft.custom_input.is_empty() {
                return Action::None;
            }
            draft.custom_selected = true;
            draft.selected_option = None;
        } else if draft.cursor < options_len {
            draft.selected_option = Some(draft.cursor);
            draft.custom_selected = false;
        } else {
            return Action::None;
        }

        self.advance_or_submit()
    }

    fn collect_answers(&self) -> Vec<Vec<String>> {
        let mut answers = Vec::with_capacity(self.request.questions.len());

        for (question, draft) in self.request.questions.iter().zip(self.drafts.iter()) {
            let mut question_answers = Vec::new();
            if question.multiple {
                for (index, option) in question.options.iter().enumerate() {
                    if draft.selected_options.get(index).copied().unwrap_or(false) {
                        question_answers.push(option.label.clone());
                    }
                }
                if question.custom && draft.custom_selected && !draft.custom_input.is_empty() {
                    question_answers.push(draft.custom_input.clone());
                }
            } else if draft.custom_selected {
                if !draft.custom_input.is_empty() {
                    question_answers.push(draft.custom_input.clone());
                }
            } else if let Some(index) = draft.selected_option {
                if let Some(option) = question.options.get(index) {
                    question_answers.push(option.label.clone());
                }
            }
            answers.push(question_answers);
        }

        answers
    }
}

impl Modal for QuestionModal {
    fn render(&self, frame: &mut Frame, theme: &Theme, area: Rect) {
        if let (Some(qinfo), Some(draft)) = (self.current_question(), self.current_draft()) {
            let mut lines: Vec<Line<'_>> = Vec::new();
            lines.push(
                Line::from(alloc::format!(
                    "Question {}/{}",
                    self.current_q + 1,
                    self.request.questions.len()
                ))
                .style(theme.text_dim),
            );
            lines.push(Line::from(qinfo.header.as_str()).style(theme.text_accent));
            lines.push(Line::from(qinfo.question.as_str()).style(theme.text));
            lines.push(Line::from(""));
            for (i, opt) in qinfo.options.iter().enumerate() {
                let style = if i == draft.cursor {
                    theme.dialog_button_focused
                } else {
                    theme.dialog_button
                };
                let marker = if qinfo.multiple {
                    if draft.selected_options.get(i).copied().unwrap_or(false) {
                        "[x]"
                    } else {
                        "[ ]"
                    }
                } else if draft.selected_option == Some(i) {
                    "(x)"
                } else {
                    "( )"
                };
                let display = alloc::format!(" {marker} {} - {}", opt.label, opt.description);
                lines.push(Line::from(display).style(style));
            }
            if let Some(custom_index) = Self::custom_row_index(qinfo) {
                let style = if custom_index == draft.cursor {
                    theme.dialog_button_focused
                } else {
                    theme.dialog_button
                };
                let marker = if qinfo.multiple {
                    if draft.custom_selected {
                        "[x]"
                    } else {
                        "[ ]"
                    }
                } else if draft.custom_selected {
                    "(x)"
                } else {
                    "( )"
                };
                let custom_text = if draft.custom_input.is_empty() {
                    "type a custom answer"
                } else {
                    draft.custom_input.as_str()
                };
                let display = alloc::format!(" {marker} Custom: {custom_text}");
                lines.push(Line::from(display).style(style));
            }
            if let Some(tool) = self.request.tool.as_ref() {
                lines.push(Line::from(""));
                lines.push(Line::from("Tool Call:").style(theme.text_dim));
                lines.push(
                    Line::from(alloc::format!("  Message ID: {}", tool.message_id))
                        .style(theme.text),
                );
                lines.push(
                    Line::from(alloc::format!("  Call ID: {}", tool.call_id)).style(theme.text),
                );
            }
            lines.push(Line::from(""));
            lines.push(
                Line::from(if qinfo.multiple {
                    "Up/Down: move  Space: toggle  Left: back  Enter/Right: next"
                } else {
                    "Up/Down: move  Left: back  Enter/Right: next"
                })
                .style(theme.text_dim),
            );
            if qinfo.custom {
                lines.push(
                    Line::from("Type while Custom is selected. Backspace edits custom input.")
                        .style(theme.text_dim),
                );
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
                    self.move_cursor(-1);
                    Action::None
                }
                Scancode::Down => {
                    self.move_cursor(1);
                    Action::None
                }
                Scancode::Left => {
                    self.go_back();
                    Action::None
                }
                Scancode::Right | Scancode::Enter => self.submit_current_question(),
                Scancode::Backspace => {
                    self.backspace_custom_input();
                    Action::None
                }
                Scancode::Char(' ') => {
                    self.toggle_current_multi_selection();
                    Action::None
                }
                Scancode::Char(c) => {
                    self.append_custom_char(c);
                    Action::None
                }
                Scancode::Escape => Action::RejectQuestion(self.request.id.clone()),
                _ => Action::None,
            },
            _ => Action::None,
        }
    }

    fn title(&self) -> &str {
        "Question"
    }
}
