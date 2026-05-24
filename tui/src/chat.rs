use alloc::borrow::ToOwned;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Text};
use ratatui::widgets::{
    Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget, Widget, Wrap,
};

use crate::theme::Theme;

/// A message held in memory, built from streaming Parts.
#[derive(Debug, Clone)]
pub(crate) struct LoadedMessage {
    pub(crate) id: Option<String>,
    pub(crate) session_id: Option<String>,
    pub(crate) role: ocpncord_backend::MessageRole,
    pub(crate) parts: Vec<ocpncord_backend::Part>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamTextKind {
    Text,
    Reasoning,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ChatState {
    partial_parts: Vec<ocpncord_backend::Part>,
    partial_texts: BTreeMap<String, String>,
    partial_part_indices: BTreeMap<String, usize>,
    latest_text_part_index: Option<usize>,
    messages: Vec<LoadedMessage>,
    queued_messages: Vec<LoadedMessage>,
}

impl ChatState {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn partial_parts(&self) -> &[ocpncord_backend::Part] {
        &self.partial_parts
    }

    pub(crate) fn messages(&self) -> &[LoadedMessage] {
        &self.messages
    }

    pub(crate) fn queued_messages(&self) -> &[LoadedMessage] {
        &self.queued_messages
    }

    pub(crate) fn clear_partial_stream(&mut self) {
        self.partial_parts.clear();
        self.partial_texts.clear();
        self.partial_part_indices.clear();
        self.latest_text_part_index = None;
    }

    pub(crate) fn clear_messages(&mut self) {
        self.messages.clear();
    }

    pub(crate) fn replace_messages(&mut self, messages: Vec<LoadedMessage>) {
        self.messages = messages;
    }

    pub(crate) fn push_message(&mut self, message: LoadedMessage) {
        self.messages.push(message);
    }

    pub(crate) fn queue_message(&mut self, message: LoadedMessage) {
        self.queued_messages.push(message);
    }

    pub(crate) fn pop_queued_message(&mut self) -> Option<LoadedMessage> {
        if self.queued_messages.is_empty() {
            None
        } else {
            Some(self.queued_messages.remove(0))
        }
    }

    pub(crate) fn clear_queued_messages(&mut self) {
        self.queued_messages.clear();
    }

    pub(crate) fn has_partial_response(&self) -> bool {
        !self.partial_parts.is_empty()
    }

    pub(crate) fn flush_partial_response(&mut self, active_session_id: Option<String>) {
        if self.partial_parts.is_empty() {
            self.clear_partial_stream();
            return;
        }

        let parts = core::mem::take(&mut self.partial_parts);
        self.messages.push(LoadedMessage {
            id: None,
            session_id: active_session_id,
            role: ocpncord_backend::MessageRole::Assistant,
            parts,
        });
        self.clear_partial_stream();
    }

    pub(crate) fn finish_streaming_response(&mut self, message_id: &str, session_id: &str) {
        self.upsert_assistant_message_from_partial(message_id, session_id);
        self.clear_partial_stream();
    }

    pub(crate) fn apply_message_updated(&mut self, message: ocpncord_backend::Message) -> bool {
        let (message_id, message_session_id, role) = message_identity(&message);

        match role {
            ocpncord_backend::MessageRole::User => {
                if let Some(index) = self.find_message_index(message_id) {
                    self.messages[index].session_id = Some(message_session_id.to_owned());
                    self.messages[index].role = ocpncord_backend::MessageRole::User;
                } else {
                    self.attach_id_to_optimistic_user_message(message_id, message_session_id);
                }
                false
            }
            ocpncord_backend::MessageRole::Assistant => {
                self.finish_streaming_response(message_id, message_session_id);
                true
            }
        }
    }

    pub(crate) fn remove_message(&mut self, message_id: &str) {
        self.messages
            .retain(|message| message.id.as_deref() != Some(message_id));
    }

    pub(crate) fn merge_stream_part(&mut self, part: ocpncord_backend::Part) -> Option<usize> {
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

    pub(crate) fn merge_stream_delta(&mut self, part_id: String, delta: String) {
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
                        identity: Default::default(),
                        text: String::new(),
                    }));
                (self.partial_parts.len() - 1, StreamTextKind::Text)
            }
        };

        set_stream_text(&mut self.partial_parts[index], text, kind);
        self.partial_part_indices.insert(part_id, index);
        self.latest_text_part_index = Some(index);
    }

    pub(crate) fn remove_stream_part(&mut self, part_id: &str) {
        self.partial_texts.remove(part_id);
        let Some(index) = self.partial_part_indices.remove(part_id) else {
            return;
        };
        if index >= self.partial_parts.len() {
            return;
        }

        self.partial_parts.remove(index);
        for value in self.partial_part_indices.values_mut() {
            if *value > index {
                *value -= 1;
            }
        }
        self.latest_text_part_index = self.latest_text_part_index.and_then(|latest| {
            if latest == index {
                None
            } else if latest > index {
                Some(latest - 1)
            } else {
                Some(latest)
            }
        });
    }

    fn find_message_index(&self, message_id: &str) -> Option<usize> {
        self.messages
            .iter()
            .position(|message| message.id.as_deref() == Some(message_id))
    }

    fn attach_id_to_optimistic_user_message(&mut self, message_id: &str, session_id: &str) -> bool {
        let Some(index) = self.messages.iter().position(|message| {
            message.id.is_none() && matches!(message.role, ocpncord_backend::MessageRole::User)
        }) else {
            return false;
        };

        self.messages[index].id = Some(message_id.to_owned());
        self.messages[index].session_id = Some(session_id.to_owned());
        true
    }

    fn upsert_assistant_message_from_partial(&mut self, message_id: &str, session_id: &str) {
        let parts = core::mem::take(&mut self.partial_parts);
        if let Some(index) = self.find_message_index(message_id) {
            self.messages[index].session_id = Some(session_id.to_owned());
            self.messages[index].role = ocpncord_backend::MessageRole::Assistant;
            if !parts.is_empty() {
                self.messages[index].parts = parts;
            }
            return;
        }

        self.messages.push(LoadedMessage {
            id: Some(message_id.to_owned()),
            session_id: Some(session_id.to_owned()),
            role: ocpncord_backend::MessageRole::Assistant,
            parts,
        });
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
}

pub(crate) fn user_loaded_message(text: &str) -> LoadedMessage {
    LoadedMessage {
        id: None,
        session_id: None,
        role: ocpncord_backend::MessageRole::User,
        parts: vec![ocpncord_backend::Part::Text(ocpncord_backend::TextPart {
            identity: Default::default(),
            text: text.into(),
        })],
    }
}

pub(crate) fn loaded_messages_from_details(
    details: Vec<ocpncord_backend::MessageDetail>,
) -> Vec<LoadedMessage> {
    details
        .into_iter()
        .map(|detail| LoadedMessage {
            id: Some(detail.info.id),
            session_id: Some(detail.info.session_id),
            role: detail.info.role,
            parts: detail.parts,
        })
        .collect()
}

fn message_identity(
    message: &ocpncord_backend::Message,
) -> (&str, &str, ocpncord_backend::MessageRole) {
    match message {
        ocpncord_backend::Message::User(message) => {
            (&message.id, &message.session_id, message.role.clone())
        }
        ocpncord_backend::Message::Assistant(message) => {
            (&message.id, &message.session_id, message.role.clone())
        }
    }
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

fn stream_text_kind(part: &ocpncord_backend::Part) -> Option<StreamTextKind> {
    match part {
        ocpncord_backend::Part::Text(_) => Some(StreamTextKind::Text),
        ocpncord_backend::Part::Reasoning(_) => Some(StreamTextKind::Reasoning),
        _ => None,
    }
}

fn set_stream_text(part: &mut ocpncord_backend::Part, text: String, kind: StreamTextKind) {
    *part = match kind {
        StreamTextKind::Text => ocpncord_backend::Part::Text(ocpncord_backend::TextPart {
            identity: Default::default(),
            text,
        }),
        StreamTextKind::Reasoning => {
            ocpncord_backend::Part::Reasoning(ocpncord_backend::ReasoningPart {
                identity: Default::default(),
                text,
            })
        }
    };
}

pub(crate) struct ChatTranscript<'a> {
    pub messages: &'a [LoadedMessage],
    pub active_parts: &'a [ocpncord_backend::Part],
    pub queued_messages: &'a [LoadedMessage],
    pub is_streaming: bool,
}

/// Renders the Chat message area using the provided data.
pub(crate) fn render_chat(
    frame: &mut ratatui::Frame,
    theme: &Theme,
    area: Rect,
    transcript: ChatTranscript<'_>,
    scroll: u16,
) {
    let msg_area = area;

    if transcript.messages.is_empty()
        && transcript.active_parts.is_empty()
        && transcript.queued_messages.is_empty()
        && !transcript.is_streaming
    {
        Text::from("No messages yet")
            .style(theme.text_dim)
            .alignment(Alignment::Center)
            .render(msg_area, frame.buffer_mut());
        return;
    }

    let mut lines: Vec<Line<'_>> = Vec::new();
    for msg in transcript.messages {
        render_message(&mut lines, msg, theme);
    }

    for part in transcript.active_parts {
        lines.extend(render_part(part, theme, true));
    }

    for msg in transcript.queued_messages {
        render_queued_message(&mut lines, msg, theme);
    }

    let full_width_height = wrapped_height(&lines, msg_area.width);
    let show_scrollbar = full_width_height > msg_area.height as usize
        || (msg_area.width > 1
            && wrapped_height(&lines, msg_area.width - 1) > msg_area.height as usize);
    let text_area = if show_scrollbar && msg_area.width > 1 {
        Rect::new(msg_area.x, msg_area.y, msg_area.width - 1, msg_area.height)
    } else {
        msg_area
    };

    let content_height = wrapped_height(&lines, text_area.width);
    let max_scroll = content_height.saturating_sub(text_area.height as usize) as u16;
    let scroll_y = max_scroll.saturating_sub(scroll.min(max_scroll));

    Paragraph::new(Text::from(lines))
        .wrap(Wrap { trim: false })
        .scroll((scroll_y, 0))
        .render(text_area, frame.buffer_mut());

    if show_scrollbar {
        let mut state = ScrollbarState::new(content_height).position(scroll_y as usize);
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .thumb_style(theme.scrollbar)
            .track_style(theme.text_dim)
            .render(msg_area, frame.buffer_mut(), &mut state);
    }
}

fn render_message<'a>(lines: &mut Vec<Line<'a>>, msg: &'a LoadedMessage, theme: &'a Theme) {
    match msg.role {
        ocpncord_backend::MessageRole::User => render_user_message(lines, msg, theme, false),
        ocpncord_backend::MessageRole::Assistant => {
            for part in &msg.parts {
                lines.extend(render_part(part, theme, true));
            }
        }
    }
}

fn render_queued_message<'a>(lines: &mut Vec<Line<'a>>, msg: &'a LoadedMessage, theme: &'a Theme) {
    render_user_message(lines, msg, theme, true);
}

fn render_user_message<'a>(
    lines: &mut Vec<Line<'a>>,
    msg: &'a LoadedMessage,
    theme: &'a Theme,
    queued: bool,
) {
    for part in &msg.parts {
        match part {
            ocpncord_backend::Part::Text(text) => {
                let suffix = if queued { " [queued]" } else { "" };
                for (line_index, line) in text.text.split('\n').enumerate() {
                    let prefix = if line_index == 0 { "> " } else { "  " };
                    lines.push(
                        Line::from(alloc::format!("{prefix}{line}{suffix}"))
                            .style(theme.message_user),
                    );
                }
            }
            _ => lines.extend(render_part(part, theme, true)),
        }
    }
}

fn render_part<'a>(
    part: &'a ocpncord_backend::Part,
    theme: &'a Theme,
    show_details: bool,
) -> Vec<Line<'a>> {
    match part {
        ocpncord_backend::Part::Text(tp) => styled_lines(tp.text.as_str(), theme.part_text),
        ocpncord_backend::Part::Reasoning(rp) => {
            if show_details {
                reasoning_lines(rp.text.as_str(), theme.part_reasoning)
            } else {
                vec![Line::from("reasoning hidden").style(theme.text_dim)]
            }
        }
        ocpncord_backend::Part::Tool(tp) => {
            let (icon, style) = match &tp.state {
                ocpncord_backend::ToolState::Pending { .. } => ("...", theme.part_tool_idle),
                ocpncord_backend::ToolState::Running { .. } => (">>>", theme.part_tool_running),
                ocpncord_backend::ToolState::Completed { .. } => ("[ok]", theme.part_tool_done),
                ocpncord_backend::ToolState::Error { .. } => ("[!!]", theme.part_tool_error),
            };
            if show_details {
                let summary = match &tp.state {
                    ocpncord_backend::ToolState::Completed { output, .. } => {
                        alloc::format!("{icon} {} - done: {output}", tp.tool)
                    }
                    ocpncord_backend::ToolState::Error { error, .. } => {
                        alloc::format!("{icon} {} - error: {error}", tp.tool)
                    }
                    _ => alloc::format!("{icon} {}", tp.tool),
                };
                styled_lines_owned(summary, style)
            } else {
                vec![Line::from(alloc::format!("{icon} {}", tp.tool)).style(style)]
            }
        }
        ocpncord_backend::Part::StepStart(_) | ocpncord_backend::Part::StepFinish(_) => Vec::new(),
        ocpncord_backend::Part::File(fp) => {
            let label = fp.filename.as_deref().unwrap_or(&fp.url);
            vec![Line::from(alloc::format!("[file] {label}")).style(theme.part_text)]
        }
        ocpncord_backend::Part::Snapshot(sp) => {
            vec![Line::from(alloc::format!("[snapshot] {}", sp.snapshot)).style(theme.text_dim)]
        }
        ocpncord_backend::Part::Patch(pp) => {
            vec![
                Line::from(alloc::format!("[patch] {} files", pp.files.len()))
                    .style(theme.text_dim),
            ]
        }
        ocpncord_backend::Part::Agent(ap) => {
            vec![Line::from(alloc::format!("[agent] {}", ap.name)).style(theme.text_dim)]
        }
        ocpncord_backend::Part::Subtask(st) => {
            vec![Line::from(alloc::format!("[subtask] {}", st.description)).style(theme.text_dim)]
        }
        ocpncord_backend::Part::Retry(rp) => {
            vec![Line::from(alloc::format!("[retry #{}]", rp.attempt)).style(theme.text_dim)]
        }
        ocpncord_backend::Part::Compaction(cp) => {
            let label = if cp.overflow == Some(true) {
                "compaction (overflow)"
            } else {
                "compaction"
            };
            vec![Line::from(label).style(theme.text_dim)]
        }
    }
}

fn styled_lines(text: &str, style: Style) -> Vec<Line<'_>> {
    let mut lines = Vec::new();
    for line in text.split('\n') {
        lines.push(Line::from(line).style(style));
    }
    if lines.is_empty() {
        lines.push(Line::from("").style(style));
    }
    lines
}

fn styled_lines_owned(text: alloc::string::String, style: Style) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for line in text.split('\n') {
        lines.push(Line::from(alloc::string::String::from(line)).style(style));
    }
    if lines.is_empty() {
        lines.push(Line::from("").style(style));
    }
    lines
}

fn reasoning_lines(text: &str, style: Style) -> Vec<Line<'_>> {
    if text.is_empty() {
        return vec![Line::from("... reasoning").style(style)];
    }

    let mut lines = Vec::new();
    for (index, line) in text.split('\n').enumerate() {
        let prefix = if index == 0 {
            "... reasoning: "
        } else {
            "    "
        };
        lines.push(Line::from(alloc::format!("{prefix}{line}")).style(style));
    }
    lines
}

fn wrapped_height(lines: &[Line<'_>], width: u16) -> usize {
    let width = width.max(1) as usize;
    lines
        .iter()
        .map(|line| {
            let line_width = line.width();
            core::cmp::max(1, line_width.saturating_add(width - 1) / width)
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ocpncord_backend::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn renders_placeholder_when_empty() {
        let theme = Theme::default();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_chat(
                    frame,
                    &theme,
                    frame.area(),
                    ChatTranscript {
                        messages: &[],
                        active_parts: &[],
                        queued_messages: &[],
                        is_streaming: false,
                    },
                    0,
                );
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        let has_placeholder = buf.content().iter().any(|c| c.symbol() == "N");
        assert!(has_placeholder);
    }

    #[test]
    fn renders_user_message() {
        let theme = Theme::default();
        let msgs = vec![LoadedMessage {
            id: None,
            session_id: None,
            role: MessageRole::User,
            parts: vec![Part::Text(TextPart {
                identity: Default::default(),
                text: "hello".into(),
            })],
        }];

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_chat(
                    frame,
                    &theme,
                    frame.area(),
                    ChatTranscript {
                        messages: &msgs,
                        active_parts: &[],
                        queued_messages: &[],
                        is_streaming: false,
                    },
                    0,
                );
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        let has_text = buf.content().iter().any(|c| c.symbol() == "h");
        assert!(has_text);
    }

    #[test]
    fn renders_partial_parts_when_streaming() {
        let theme = Theme::default();
        let partial = vec![Part::Text(TextPart {
            identity: Default::default(),
            text: "streaming...".into(),
        })];

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_chat(
                    frame,
                    &theme,
                    frame.area(),
                    ChatTranscript {
                        messages: &[],
                        active_parts: &partial,
                        queued_messages: &[],
                        is_streaming: true,
                    },
                    0,
                );
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        let has_text = buf.content().iter().any(|c| c.symbol() == "s");
        assert!(has_text);
    }

    #[test]
    fn wrapped_height_counts_width_after_scrollbar_reservation() {
        let line = Line::from("1234567890");
        assert_eq!(wrapped_height(&[line.clone()], 10), 1);
        assert_eq!(wrapped_height(&[line], 9), 2);
    }

    #[test]
    fn text_part_renders_with_part_text_style() {
        let theme = Theme::default();
        let part = Part::Text(TextPart {
            identity: Default::default(),
            text: "hello world".into(),
        });
        let lines = render_part(&part, &theme, true);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].to_string(), "hello world");
    }

    #[test]
    fn text_part_newlines_render_as_distinct_lines() {
        let theme = Theme::default();
        let part = Part::Text(TextPart {
            identity: Default::default(),
            text: "hello\nworld".into(),
        });
        let lines = render_part(&part, &theme, true);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].to_string(), "hello");
        assert_eq!(lines[1].to_string(), "world");
    }

    #[test]
    fn reasoning_part_shows_full_when_details_on() {
        let theme = Theme::default();
        let part = Part::Reasoning(ReasoningPart {
            identity: Default::default(),
            text: "thinking...".into(),
        });
        let lines = render_part(&part, &theme, true);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].to_string(), "... reasoning: thinking...");
    }

    #[test]
    fn empty_reasoning_part_has_stable_placeholder_line() {
        let theme = Theme::default();
        let part = Part::Reasoning(ReasoningPart {
            identity: Default::default(),
            text: String::new(),
        });
        let lines = render_part(&part, &theme, true);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].to_string(), "... reasoning");
    }

    #[test]
    fn reasoning_part_collapsed_when_details_off() {
        let theme = Theme::default();
        let part = Part::Reasoning(ReasoningPart {
            identity: Default::default(),
            text: "thinking...".into(),
        });
        let lines = render_part(&part, &theme, false);
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn tool_pending_shows_idle_style() {
        let theme = Theme::default();
        let part = Part::Tool(ToolPart {
            identity: Default::default(),
            tool: "read".into(),
            state: ToolState::Pending {
                input: alloc::collections::BTreeMap::new(),
                raw: "".into(),
            },
        });
        let lines = render_part(&part, &theme, true);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].to_string().contains("read"));
    }

    #[test]
    fn tool_running_shows_running_style() {
        let theme = Theme::default();
        let part = Part::Tool(ToolPart {
            identity: Default::default(),
            tool: "write".into(),
            state: ToolState::Running {
                input: alloc::collections::BTreeMap::new(),
                title: None,
                metadata: None,
                time: None,
            },
        });
        let lines = render_part(&part, &theme, true);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].to_string().contains("write"));
    }

    #[test]
    fn tool_completed_shows_output() {
        let theme = Theme::default();
        let part = Part::Tool(ToolPart {
            identity: Default::default(),
            tool: "grep".into(),
            state: ToolState::Completed {
                input: alloc::collections::BTreeMap::new(),
                output: "found 3 matches".into(),
                title: "grep".into(),
                metadata: alloc::collections::BTreeMap::new(),
                time: ocpncord_backend::ToolTimeCompleted { start: 0, end: 1 },
                attachments: Vec::new(),
            },
        });
        let lines = render_part(&part, &theme, true);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].to_string().contains("found 3 matches"));
    }

    #[test]
    fn tool_error_shows_error_message() {
        let theme = Theme::default();
        let part = Part::Tool(ToolPart {
            identity: Default::default(),
            tool: "curl".into(),
            state: ToolState::Error {
                input: alloc::collections::BTreeMap::new(),
                error: "timeout".into(),
                metadata: None,
                time: ocpncord_backend::ToolTimeCompleted { start: 0, end: 1 },
            },
        });
        let lines = render_part(&part, &theme, true);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].to_string().contains("timeout"));
    }

    #[test]
    fn step_start_is_hidden_in_chat() {
        let theme = Theme::default();
        let part = Part::StepStart(StepStartPart {
            identity: Default::default(),
            snapshot: None,
            session_id: None,
        });
        let lines = render_part(&part, &theme, true);
        assert!(lines.is_empty());
    }

    #[test]
    fn step_start_with_snapshot_is_hidden_in_chat() {
        let theme = Theme::default();
        let part = Part::StepStart(StepStartPart {
            identity: Default::default(),
            snapshot: Some("planning".into()),
            session_id: None,
        });
        let lines = render_part(&part, &theme, true);
        assert!(lines.is_empty());
    }

    #[test]
    fn step_finish_with_reason_is_hidden_in_chat() {
        let theme = Theme::default();
        let part = Part::StepFinish(StepFinishPart {
            identity: Default::default(),
            reason: Some("done".into()),
            snapshot: None,
            session_id: None,
        });
        let lines = render_part(&part, &theme, true);
        assert!(lines.is_empty());
    }

    #[test]
    fn step_finish_without_reason_is_hidden_in_chat() {
        let theme = Theme::default();
        let part = Part::StepFinish(StepFinishPart {
            identity: Default::default(),
            reason: None,
            snapshot: None,
            session_id: None,
        });
        let lines = render_part(&part, &theme, true);
        assert!(lines.is_empty());
    }
}
