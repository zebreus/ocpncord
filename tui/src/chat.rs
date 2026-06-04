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
pub enum PartDisplayMode {
    Full,
    Summary,
    Hidden,
}

impl PartDisplayMode {
    pub(crate) fn next(self) -> Self {
        match self {
            Self::Full => Self::Summary,
            Self::Summary => Self::Hidden,
            Self::Hidden => Self::Full,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Full => "Full",
            Self::Summary => "Summary",
            Self::Hidden => "Hidden",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartKind {
    Text,
    Reasoning,
    Tool,
    Step,
    File,
    Snapshot,
    Patch,
    Agent,
    Subtask,
    Retry,
    Compaction,
}

impl PartKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Text => "Text",
            Self::Reasoning => "Reasoning",
            Self::Tool => "Tools",
            Self::Step => "Steps",
            Self::File => "Files",
            Self::Snapshot => "Snapshots",
            Self::Patch => "Patches",
            Self::Agent => "Agents",
            Self::Subtask => "Subtasks",
            Self::Retry => "Retries",
            Self::Compaction => "Compactions",
        }
    }
}

pub(crate) const PART_KIND_ORDER: [PartKind; 11] = [
    PartKind::Text,
    PartKind::Reasoning,
    PartKind::Tool,
    PartKind::Step,
    PartKind::File,
    PartKind::Snapshot,
    PartKind::Patch,
    PartKind::Agent,
    PartKind::Subtask,
    PartKind::Retry,
    PartKind::Compaction,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ChatDisplayPolicy {
    text: PartDisplayMode,
    reasoning: PartDisplayMode,
    tool: PartDisplayMode,
    step: PartDisplayMode,
    file: PartDisplayMode,
    snapshot: PartDisplayMode,
    patch: PartDisplayMode,
    agent: PartDisplayMode,
    subtask: PartDisplayMode,
    retry: PartDisplayMode,
    compaction: PartDisplayMode,
}

impl ChatDisplayPolicy {
    pub(crate) fn mode_for_kind(&self, kind: PartKind) -> PartDisplayMode {
        match kind {
            PartKind::Text => self.text,
            PartKind::Reasoning => self.reasoning,
            PartKind::Tool => self.tool,
            PartKind::Step => self.step,
            PartKind::File => self.file,
            PartKind::Snapshot => self.snapshot,
            PartKind::Patch => self.patch,
            PartKind::Agent => self.agent,
            PartKind::Subtask => self.subtask,
            PartKind::Retry => self.retry,
            PartKind::Compaction => self.compaction,
        }
    }

    pub(crate) fn set_mode(&mut self, kind: PartKind, mode: PartDisplayMode) {
        match kind {
            PartKind::Text => self.text = mode,
            PartKind::Reasoning => self.reasoning = mode,
            PartKind::Tool => self.tool = mode,
            PartKind::Step => self.step = mode,
            PartKind::File => self.file = mode,
            PartKind::Snapshot => self.snapshot = mode,
            PartKind::Patch => self.patch = mode,
            PartKind::Agent => self.agent = mode,
            PartKind::Subtask => self.subtask = mode,
            PartKind::Retry => self.retry = mode,
            PartKind::Compaction => self.compaction = mode,
        }
    }

    pub(crate) fn mode_for_part(&self, part: &ocpncord_backend::Part) -> PartDisplayMode {
        self.mode_for_kind(part_kind(part))
    }
}

impl Default for ChatDisplayPolicy {
    fn default() -> Self {
        Self {
            text: PartDisplayMode::Full,
            reasoning: PartDisplayMode::Full,
            tool: PartDisplayMode::Full,
            step: PartDisplayMode::Hidden,
            file: PartDisplayMode::Full,
            snapshot: PartDisplayMode::Full,
            patch: PartDisplayMode::Full,
            agent: PartDisplayMode::Full,
            subtask: PartDisplayMode::Full,
            retry: PartDisplayMode::Full,
            compaction: PartDisplayMode::Full,
        }
    }
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
        self.complete_running_tools_with_output();
        self.clear_partial_stream();
    }

    pub(crate) fn complete_running_tools_with_output(&mut self) {
        for part in self.partial_parts.iter_mut() {
            complete_running_tool_with_output(part);
        }
        for message in self.messages.iter_mut() {
            for part in message.parts.iter_mut() {
                complete_running_tool_with_output(part);
            }
        }
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
                let completed = message_completed(&message);
                self.finish_streaming_response(message_id, message_session_id);
                completed
            }
        }
    }

    pub(crate) fn remove_message(&mut self, message_id: &str) {
        self.messages
            .retain(|message| message.id.as_deref() != Some(message_id));
    }

    pub(crate) fn merge_stream_part(
        &mut self,
        part_id: Option<String>,
        part: ocpncord_backend::Part,
    ) -> Option<usize> {
        if self.is_echoed_user_text(&part) {
            return None;
        }

        let keyed_part_id = part_id.or_else(|| part_identity_id(&part).map(ToOwned::to_owned));
        if let Some(part_id) = keyed_part_id.as_ref() {
            if let Some(index) = self
                .partial_part_indices
                .get(part_id)
                .copied()
                .filter(|index| *index < self.partial_parts.len())
            {
                if let Some(kind) = stream_text_kind(&part) {
                    let incoming_text = stream_text(&part).unwrap_or_default();
                    if !incoming_text.is_empty() {
                        self.partial_texts
                            .insert(part_id.clone(), incoming_text.clone());
                        set_stream_text(&mut self.partial_parts[index], incoming_text, kind);
                    }
                    self.latest_text_part_index = Some(index);
                } else {
                    self.partial_parts[index] = part;
                }
                return Some(index);
            }
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

        if let Some(index) = self
            .partial_parts
            .iter()
            .rposition(|existing| parts_describe_same_entity(existing, &part))
        {
            self.partial_parts[index] = part;
            if let Some(part_id) = keyed_part_id {
                self.partial_part_indices.insert(part_id, index);
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
        if let Some(part_id) = keyed_part_id {
            self.partial_part_indices.insert(part_id, index);
        }
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

fn message_completed(message: &ocpncord_backend::Message) -> bool {
    match message {
        ocpncord_backend::Message::User(message) => message.time.completed.is_some(),
        ocpncord_backend::Message::Assistant(message) => message.time.completed.is_some(),
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

fn parts_describe_same_entity(
    left: &ocpncord_backend::Part,
    right: &ocpncord_backend::Part,
) -> bool {
    match (left, right) {
        (ocpncord_backend::Part::Tool(left), ocpncord_backend::Part::Tool(right)) => {
            if let (Some(left_id), Some(right_id)) =
                (left.identity.id.as_deref(), right.identity.id.as_deref())
            {
                left_id == right_id
            } else {
                left.tool == right.tool && left.identity.id.is_none() && right.identity.id.is_none()
            }
        }
        (ocpncord_backend::Part::StepStart(left), ocpncord_backend::Part::StepStart(right)) => {
            left.session_id == right.session_id && left.snapshot == right.snapshot
        }
        (ocpncord_backend::Part::StepFinish(left), ocpncord_backend::Part::StepFinish(right)) => {
            left.session_id == right.session_id
                && left.snapshot == right.snapshot
                && left.reason == right.reason
        }
        (ocpncord_backend::Part::File(left), ocpncord_backend::Part::File(right)) => {
            left.url == right.url
        }
        (ocpncord_backend::Part::Snapshot(left), ocpncord_backend::Part::Snapshot(right)) => {
            left.snapshot == right.snapshot
        }
        (ocpncord_backend::Part::Patch(left), ocpncord_backend::Part::Patch(right)) => {
            left.hash == right.hash
        }
        (ocpncord_backend::Part::Agent(left), ocpncord_backend::Part::Agent(right)) => {
            left.name == right.name
        }
        (ocpncord_backend::Part::Subtask(left), ocpncord_backend::Part::Subtask(right)) => {
            left.agent == right.agent && left.description == right.description
        }
        (ocpncord_backend::Part::Retry(left), ocpncord_backend::Part::Retry(right)) => {
            left.attempt == right.attempt
        }
        (ocpncord_backend::Part::Compaction(left), ocpncord_backend::Part::Compaction(right)) => {
            left.auto == right.auto && left.overflow == right.overflow
        }
        _ => false,
    }
}

fn part_identity_id(part: &ocpncord_backend::Part) -> Option<&str> {
    match part {
        ocpncord_backend::Part::Text(part) => part.identity.id.as_deref(),
        ocpncord_backend::Part::Reasoning(part) => part.identity.id.as_deref(),
        ocpncord_backend::Part::Tool(part) => part.identity.id.as_deref(),
        ocpncord_backend::Part::StepStart(part) => part.identity.id.as_deref(),
        ocpncord_backend::Part::StepFinish(part) => part.identity.id.as_deref(),
        ocpncord_backend::Part::File(part) => part.identity.id.as_deref(),
        ocpncord_backend::Part::Snapshot(part) => part.identity.id.as_deref(),
        ocpncord_backend::Part::Patch(part) => part.identity.id.as_deref(),
        ocpncord_backend::Part::Agent(part) => part.identity.id.as_deref(),
        ocpncord_backend::Part::Subtask(part) => part.identity.id.as_deref(),
        ocpncord_backend::Part::Retry(part) => part.identity.id.as_deref(),
        ocpncord_backend::Part::Compaction(part) => part.identity.id.as_deref(),
    }
}

fn part_kind(part: &ocpncord_backend::Part) -> PartKind {
    match part {
        ocpncord_backend::Part::Text(_) => PartKind::Text,
        ocpncord_backend::Part::Reasoning(_) => PartKind::Reasoning,
        ocpncord_backend::Part::Tool(_) => PartKind::Tool,
        ocpncord_backend::Part::StepStart(_) | ocpncord_backend::Part::StepFinish(_) => {
            PartKind::Step
        }
        ocpncord_backend::Part::File(_) => PartKind::File,
        ocpncord_backend::Part::Snapshot(_) => PartKind::Snapshot,
        ocpncord_backend::Part::Patch(_) => PartKind::Patch,
        ocpncord_backend::Part::Agent(_) => PartKind::Agent,
        ocpncord_backend::Part::Subtask(_) => PartKind::Subtask,
        ocpncord_backend::Part::Retry(_) => PartKind::Retry,
        ocpncord_backend::Part::Compaction(_) => PartKind::Compaction,
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

fn complete_running_tool_with_output(part: &mut ocpncord_backend::Part) {
    let ocpncord_backend::Part::Tool(tool) = part else {
        return;
    };
    let ocpncord_backend::ToolState::Running {
        input,
        title,
        metadata,
        time,
    } = &tool.state
    else {
        return;
    };
    let Some(metadata) = metadata.as_ref() else {
        return;
    };
    let Some(output) = metadata.get("output").cloned() else {
        return;
    };
    let start = time.as_ref().map(|time| time.start).unwrap_or_default();
    tool.state = ocpncord_backend::ToolState::Completed {
        input: input.clone(),
        output,
        title: title.clone().unwrap_or_else(|| tool.tool.clone()),
        metadata: metadata.clone(),
        time: ocpncord_backend::ToolTimeCompleted { start, end: start },
        attachments: Vec::new(),
    };
}

fn stream_text_kind(part: &ocpncord_backend::Part) -> Option<StreamTextKind> {
    match part {
        ocpncord_backend::Part::Text(_) => Some(StreamTextKind::Text),
        ocpncord_backend::Part::Reasoning(_) => Some(StreamTextKind::Reasoning),
        _ => None,
    }
}

fn stream_text(part: &ocpncord_backend::Part) -> Option<String> {
    match part {
        ocpncord_backend::Part::Text(text) => Some(text.text.clone()),
        ocpncord_backend::Part::Reasoning(reasoning) => Some(reasoning.text.clone()),
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
    pub display_policy: &'a ChatDisplayPolicy,
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
        render_message(&mut lines, msg, theme, transcript.display_policy);
    }

    for part in transcript.active_parts {
        lines.extend(render_part_with_policy(
            part,
            theme,
            transcript.display_policy,
        ));
    }

    for msg in transcript.queued_messages {
        render_queued_message(&mut lines, msg, theme, transcript.display_policy);
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
        let mut state =
            ScrollbarState::new((max_scroll as usize).max(1)).position(scroll_y as usize);
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .thumb_style(theme.scrollbar)
            .track_style(theme.text_dim)
            .render(msg_area, frame.buffer_mut(), &mut state);
    }
}

fn render_message<'a>(
    lines: &mut Vec<Line<'a>>,
    msg: &'a LoadedMessage,
    theme: &'a Theme,
    policy: &'a ChatDisplayPolicy,
) {
    match msg.role {
        ocpncord_backend::MessageRole::User => {
            render_user_message(lines, msg, theme, policy, false)
        }
        ocpncord_backend::MessageRole::Assistant => {
            for part in &msg.parts {
                lines.extend(render_part_with_policy(part, theme, policy));
            }
        }
    }
}

fn render_queued_message<'a>(
    lines: &mut Vec<Line<'a>>,
    msg: &'a LoadedMessage,
    theme: &'a Theme,
    policy: &'a ChatDisplayPolicy,
) {
    render_user_message(lines, msg, theme, policy, true);
}

fn render_user_message<'a>(
    lines: &mut Vec<Line<'a>>,
    msg: &'a LoadedMessage,
    theme: &'a Theme,
    policy: &'a ChatDisplayPolicy,
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
            _ => lines.extend(render_part_with_policy(part, theme, policy)),
        }
    }
}

#[cfg(test)]
fn render_part<'a>(
    part: &'a ocpncord_backend::Part,
    theme: &'a Theme,
    show_details: bool,
) -> Vec<Line<'a>> {
    let mode = if show_details {
        ChatDisplayPolicy::default().mode_for_part(part)
    } else {
        PartDisplayMode::Summary
    };
    render_part_with_mode(part, theme, mode)
}

fn render_part_with_policy<'a>(
    part: &'a ocpncord_backend::Part,
    theme: &'a Theme,
    policy: &'a ChatDisplayPolicy,
) -> Vec<Line<'a>> {
    render_part_with_mode(part, theme, policy.mode_for_part(part))
}

fn render_part_with_mode<'a>(
    part: &'a ocpncord_backend::Part,
    theme: &'a Theme,
    mode: PartDisplayMode,
) -> Vec<Line<'a>> {
    if mode == PartDisplayMode::Hidden {
        return Vec::new();
    }

    match part {
        ocpncord_backend::Part::Text(tp) => styled_lines(tp.text.as_str(), theme.part_text),
        ocpncord_backend::Part::Reasoning(rp) => {
            if mode == PartDisplayMode::Full {
                reasoning_lines(rp.text.as_str(), theme.part_reasoning)
            } else {
                vec![Line::from("reasoning hidden").style(theme.text_dim)]
            }
        }
        ocpncord_backend::Part::Tool(tp) => tool_lines(tp, theme, mode),
        ocpncord_backend::Part::StepStart(sp) => {
            if mode == PartDisplayMode::Summary {
                vec![Line::from("[step] started").style(theme.part_step_divider)]
            } else {
                let mut lines = vec![Line::from("[step] started").style(theme.part_step_divider)];
                push_optional_line(
                    &mut lines,
                    "snapshot",
                    sp.snapshot.as_deref(),
                    theme.text_dim,
                );
                push_optional_line(
                    &mut lines,
                    "session",
                    sp.session_id.as_deref(),
                    theme.text_dim,
                );
                lines
            }
        }
        ocpncord_backend::Part::StepFinish(sp) => {
            if mode == PartDisplayMode::Summary {
                vec![Line::from("[step] finished").style(theme.part_step_divider)]
            } else {
                let mut lines = vec![Line::from("[step] finished").style(theme.part_step_divider)];
                push_optional_line(&mut lines, "reason", sp.reason.as_deref(), theme.text_dim);
                push_optional_line(
                    &mut lines,
                    "snapshot",
                    sp.snapshot.as_deref(),
                    theme.text_dim,
                );
                push_optional_line(
                    &mut lines,
                    "session",
                    sp.session_id.as_deref(),
                    theme.text_dim,
                );
                lines
            }
        }
        ocpncord_backend::Part::File(fp) => {
            let label = fp.filename.as_deref().unwrap_or(&fp.url);
            if mode == PartDisplayMode::Summary {
                vec![Line::from(alloc::format!("[file] {label}")).style(theme.part_file)]
            } else {
                vec![
                    Line::from(alloc::format!("[file] {label}")).style(theme.part_file),
                    Line::from(alloc::format!("  url: {}", fp.url)).style(theme.text_dim),
                    Line::from(alloc::format!("  mime: {}", fp.mime)).style(theme.text_dim),
                ]
            }
        }
        ocpncord_backend::Part::Snapshot(sp) => {
            vec![Line::from(alloc::format!("[snapshot] {}", sp.snapshot)).style(theme.part_snapshot)]
        }
        ocpncord_backend::Part::Patch(pp) => {
            let mut lines = vec![Line::from(alloc::format!(
                "[patch] {} files {}",
                pp.files.len(),
                pp.hash
            ))
            .style(theme.part_patch)];
            if mode == PartDisplayMode::Full {
                for file in &pp.files {
                    lines.push(Line::from(alloc::format!("  {file}")).style(theme.text_dim));
                }
            }
            lines
        }
        ocpncord_backend::Part::Agent(ap) => {
            vec![Line::from(alloc::format!("[agent] {}", ap.name)).style(theme.part_agent)]
        }
        ocpncord_backend::Part::Subtask(st) => {
            if mode == PartDisplayMode::Summary {
                vec![Line::from(alloc::format!("[subtask] {}", st.description))
                    .style(theme.part_subtask)]
            } else {
                let mut lines = vec![Line::from(alloc::format!(
                    "[subtask] {} ({})",
                    st.description,
                    st.agent
                ))
                .style(theme.part_subtask)];
                push_text_block(&mut lines, "prompt", st.prompt.as_str(), theme.text_dim);
                lines
            }
        }
        ocpncord_backend::Part::Retry(rp) => {
            vec![Line::from(alloc::format!("[retry #{}]", rp.attempt)).style(theme.part_retry)]
        }
        ocpncord_backend::Part::Compaction(cp) => {
            let label = if cp.overflow == Some(true) {
                "compaction (overflow)"
            } else {
                "compaction"
            };
            if mode == PartDisplayMode::Summary {
                vec![Line::from(label).style(theme.part_compaction)]
            } else {
                vec![Line::from(alloc::format!("{label}: auto={}", cp.auto))
                    .style(theme.part_compaction)]
            }
        }
    }
}

fn tool_lines<'a>(
    part: &'a ocpncord_backend::ToolPart,
    theme: &'a Theme,
    mode: PartDisplayMode,
) -> Vec<Line<'a>> {
    let (icon, status, style) = match &part.state {
        ocpncord_backend::ToolState::Pending { .. } => ("...", "pending", theme.part_tool_idle),
        ocpncord_backend::ToolState::Running { .. } => (">>>", "running", theme.part_tool_running),
        ocpncord_backend::ToolState::Completed { .. } => ("[ok]", "done", theme.part_tool_done),
        ocpncord_backend::ToolState::Error { .. } => ("[!!]", "error", theme.part_tool_error),
    };

    if mode == PartDisplayMode::Summary {
        let label = match &part.state {
            ocpncord_backend::ToolState::Completed { title, .. } => {
                alloc::format!("{icon} {} - {status}: {title}", part.tool)
            }
            ocpncord_backend::ToolState::Running { title, .. } => title
                .as_ref()
                .map(|title| alloc::format!("{icon} {} - {status}: {title}", part.tool))
                .unwrap_or_else(|| alloc::format!("{icon} {} - {status}", part.tool)),
            _ => alloc::format!("{icon} {} - {status}", part.tool),
        };
        return vec![Line::from(label).style(style)];
    }

    let mut lines =
        vec![Line::from(alloc::format!("{icon} {} - {status}", part.tool)).style(style)];
    match &part.state {
        ocpncord_backend::ToolState::Pending { input, raw } => {
            push_map_lines(&mut lines, "input", input, theme.text_dim);
            push_optional_line(&mut lines, "raw", nonempty(raw), theme.text_dim);
        }
        ocpncord_backend::ToolState::Running {
            input,
            title,
            metadata,
            ..
        } => {
            push_optional_line(&mut lines, "title", title.as_deref(), theme.text_dim);
            push_map_lines(&mut lines, "input", input, theme.text_dim);
            if let Some(metadata) = metadata {
                push_map_lines(&mut lines, "metadata", metadata, theme.text_dim);
            }
        }
        ocpncord_backend::ToolState::Completed {
            input,
            output,
            title,
            metadata,
            attachments,
            ..
        } => {
            push_optional_line(&mut lines, "title", nonempty(title), theme.text_dim);
            push_map_lines(&mut lines, "input", input, theme.text_dim);
            push_map_lines(&mut lines, "metadata", metadata, theme.text_dim);
            push_text_block(&mut lines, "output", output.as_str(), style);
            for attachment in attachments {
                let label = attachment.filename.as_deref().unwrap_or(&attachment.url);
                lines.push(
                    Line::from(alloc::format!(
                        "  attachment: {label} ({})",
                        attachment.mime
                    ))
                    .style(theme.text_dim),
                );
                lines.push(
                    Line::from(alloc::format!("    {}", attachment.url)).style(theme.text_dim),
                );
            }
        }
        ocpncord_backend::ToolState::Error {
            input,
            error,
            metadata,
            ..
        } => {
            push_map_lines(&mut lines, "input", input, theme.text_dim);
            if let Some(metadata) = metadata {
                push_map_lines(&mut lines, "metadata", metadata, theme.text_dim);
            }
            push_text_block(&mut lines, "error", error.as_str(), style);
        }
    }
    lines
}

fn nonempty(value: &str) -> Option<&str> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn push_optional_line(
    lines: &mut Vec<Line<'static>>,
    label: &str,
    value: Option<&str>,
    style: Style,
) {
    if let Some(value) = value {
        if !value.is_empty() {
            lines.push(Line::from(alloc::format!("  {label}: {value}")).style(style));
        }
    }
}

fn push_map_lines(
    lines: &mut Vec<Line<'static>>,
    label: &str,
    values: &BTreeMap<String, String>,
    style: Style,
) {
    if values.is_empty() {
        return;
    }
    lines.push(Line::from(alloc::format!("  {label}:")).style(style));
    for (key, value) in values {
        lines.push(Line::from(alloc::format!("    {key}: {value}")).style(style));
    }
}

fn push_text_block(lines: &mut Vec<Line<'static>>, label: &str, text: &str, style: Style) {
    if text.is_empty() {
        return;
    }
    lines.push(Line::from(alloc::format!("  {label}:")).style(style));
    for line in text.split('\n') {
        lines.push(Line::from(alloc::format!("    {line}")).style(style));
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
                        display_policy: &ChatDisplayPolicy::default(),
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
                        display_policy: &ChatDisplayPolicy::default(),
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
                        display_policy: &ChatDisplayPolicy::default(),
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
        let mut input = alloc::collections::BTreeMap::new();
        input.insert("command".into(), "git status --short".into());
        let part = Part::Tool(ToolPart {
            identity: Default::default(),
            tool: "grep".into(),
            state: ToolState::Completed {
                input,
                output: "found 3 matches\nsrc/lib.rs".into(),
                title: "grep".into(),
                metadata: alloc::collections::BTreeMap::new(),
                time: ocpncord_backend::ToolTimeCompleted { start: 0, end: 1 },
                attachments: Vec::new(),
            },
        });
        let lines = render_part(&part, &theme, true);
        let rendered = lines
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("git status --short"), "{rendered}");
        assert!(rendered.contains("found 3 matches"), "{rendered}");
        assert!(rendered.contains("src/lib.rs"), "{rendered}");
    }

    #[test]
    fn running_tool_with_output_metadata_is_completed_on_finalize() {
        let mut input = alloc::collections::BTreeMap::new();
        input.insert("command".into(), "pwd".into());
        let mut metadata = alloc::collections::BTreeMap::new();
        metadata.insert("output".into(), "/tmp/project".into());
        let mut state = ChatState::new();
        state.merge_stream_part(
            Some("tool-1".into()),
            Part::Tool(ToolPart {
                identity: PartIdentity {
                    id: Some("tool-1".into()),
                    message_id: None,
                },
                tool: "bash".into(),
                state: ToolState::Running {
                    input,
                    title: Some("Print working directory".into()),
                    metadata: Some(metadata),
                    time: Some(ocpncord_backend::ToolTime { start: 7 }),
                },
            }),
        );

        state.complete_running_tools_with_output();

        match &state.partial_parts()[0] {
            Part::Tool(tool) => match &tool.state {
                ToolState::Completed { output, title, .. } => {
                    assert_eq!(output, "/tmp/project");
                    assert_eq!(title, "Print working directory");
                }
                other => panic!("expected completed tool, got {other:?}"),
            },
            other => panic!("expected tool part, got {other:?}"),
        }
    }

    #[test]
    fn tool_error_shows_error_message() {
        let theme = Theme::default();
        let mut input = alloc::collections::BTreeMap::new();
        input.insert("file".into(), "src/main.rs".into());
        let part = Part::Tool(ToolPart {
            identity: Default::default(),
            tool: "curl".into(),
            state: ToolState::Error {
                input,
                error: "timeout".into(),
                metadata: None,
                time: ocpncord_backend::ToolTimeCompleted { start: 0, end: 1 },
            },
        });
        let lines = render_part(&part, &theme, true);
        let rendered = lines
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("src/main.rs"), "{rendered}");
        assert!(rendered.contains("timeout"), "{rendered}");
    }

    #[test]
    fn tool_summary_hides_full_output() {
        let theme = Theme::default();
        let part = Part::Tool(ToolPart {
            identity: Default::default(),
            tool: "bash".into(),
            state: ToolState::Completed {
                input: alloc::collections::BTreeMap::new(),
                output: "line one\nline two".into(),
                title: "git diff --stat".into(),
                metadata: alloc::collections::BTreeMap::new(),
                time: ocpncord_backend::ToolTimeCompleted { start: 0, end: 1 },
                attachments: Vec::new(),
            },
        });
        let lines = render_part_with_mode(&part, &theme, PartDisplayMode::Summary);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].to_string().contains("git diff --stat"));
        assert!(!lines[0].to_string().contains("line one"));
    }

    #[test]
    fn display_policy_can_hide_files() {
        let theme = Theme::default();
        let mut policy = ChatDisplayPolicy::default();
        policy.set_mode(PartKind::File, PartDisplayMode::Hidden);
        let part = Part::File(FilePart {
            identity: Default::default(),
            mime: "text/plain".into(),
            url: "file:///tmp/report.txt".into(),
            filename: Some("report.txt".into()),
        });
        assert!(render_part_with_policy(&part, &theme, &policy).is_empty());
    }

    #[test]
    fn stream_tool_updates_with_same_part_id_replace_previous_state() {
        let mut state = ChatState::new();
        let mut input = alloc::collections::BTreeMap::new();
        input.insert("command".into(), "git status --short".into());

        state.merge_stream_part(
            Some("tool-1".into()),
            Part::Tool(ToolPart {
                identity: Default::default(),
                tool: "bash".into(),
                state: ToolState::Pending {
                    input: input.clone(),
                    raw: "git status --short".into(),
                },
            }),
        );
        state.merge_stream_part(
            Some("tool-1".into()),
            Part::Tool(ToolPart {
                identity: Default::default(),
                tool: "bash".into(),
                state: ToolState::Completed {
                    input,
                    output: " M tui/src/chat.rs".into(),
                    title: "git status --short".into(),
                    metadata: alloc::collections::BTreeMap::new(),
                    time: ocpncord_backend::ToolTimeCompleted { start: 0, end: 1 },
                    attachments: Vec::new(),
                },
            }),
        );

        assert_eq!(state.partial_parts().len(), 1);
        match &state.partial_parts()[0] {
            Part::Tool(tool) => match &tool.state {
                ToolState::Completed { output, .. } => {
                    assert!(output.contains("tui/src/chat.rs"));
                }
                other => panic!("expected completed tool, got {other:?}"),
            },
            other => panic!("expected tool part, got {other:?}"),
        }
    }

    #[test]
    fn duplicate_completed_tool_update_does_not_duplicate_output() {
        let mut state = ChatState::new();
        let tool = Part::Tool(ToolPart {
            identity: Default::default(),
            tool: "bash".into(),
            state: ToolState::Completed {
                input: alloc::collections::BTreeMap::new(),
                output: "done".into(),
                title: "echo done".into(),
                metadata: alloc::collections::BTreeMap::new(),
                time: ocpncord_backend::ToolTimeCompleted { start: 0, end: 1 },
                attachments: Vec::new(),
            },
        });

        state.merge_stream_part(Some("tool-1".into()), tool.clone());
        state.merge_stream_part(Some("tool-1".into()), tool);

        assert_eq!(state.partial_parts().len(), 1);
    }

    #[test]
    fn unkeyed_file_updates_with_same_url_replace_previous_state() {
        let mut state = ChatState::new();
        state.merge_stream_part(
            None,
            Part::File(FilePart {
                identity: Default::default(),
                mime: "text/plain".into(),
                url: "file:///tmp/report.txt".into(),
                filename: Some("old.txt".into()),
            }),
        );
        state.merge_stream_part(
            None,
            Part::File(FilePart {
                identity: Default::default(),
                mime: "text/markdown".into(),
                url: "file:///tmp/report.txt".into(),
                filename: Some("report.md".into()),
            }),
        );

        assert_eq!(state.partial_parts().len(), 1);
        match &state.partial_parts()[0] {
            Part::File(file) => {
                assert_eq!(file.filename.as_deref(), Some("report.md"));
                assert_eq!(file.mime, "text/markdown");
            }
            other => panic!("expected file part, got {other:?}"),
        }
    }

    #[test]
    fn unkeyed_patch_updates_with_same_hash_replace_previous_state() {
        let mut state = ChatState::new();
        state.merge_stream_part(
            None,
            Part::Patch(PatchPart {
                identity: Default::default(),
                hash: "abc123".into(),
                files: vec!["src/lib.rs".into()],
            }),
        );
        state.merge_stream_part(
            None,
            Part::Patch(PatchPart {
                identity: Default::default(),
                hash: "abc123".into(),
                files: vec!["src/lib.rs".into(), "src/app.rs".into()],
            }),
        );

        assert_eq!(state.partial_parts().len(), 1);
        match &state.partial_parts()[0] {
            Part::Patch(patch) => assert_eq!(patch.files.len(), 2),
            other => panic!("expected patch part, got {other:?}"),
        }
    }

    #[test]
    fn unkeyed_subtask_updates_with_same_agent_and_description_replace_previous_state() {
        let mut state = ChatState::new();
        state.merge_stream_part(
            None,
            Part::Subtask(SubtaskPart {
                identity: Default::default(),
                prompt: "first prompt".into(),
                description: "review renderer".into(),
                agent: "build".into(),
            }),
        );
        state.merge_stream_part(
            None,
            Part::Subtask(SubtaskPart {
                identity: Default::default(),
                prompt: "updated prompt".into(),
                description: "review renderer".into(),
                agent: "build".into(),
            }),
        );

        assert_eq!(state.partial_parts().len(), 1);
        match &state.partial_parts()[0] {
            Part::Subtask(subtask) => assert_eq!(subtask.prompt, "updated prompt"),
            other => panic!("expected subtask part, got {other:?}"),
        }
    }

    #[test]
    fn empty_keyed_text_update_does_not_clear_accumulated_delta() {
        let mut state = ChatState::new();
        state.merge_stream_delta("text-1".into(), "hello".into());
        state.merge_stream_part(
            Some("text-1".into()),
            Part::Text(TextPart {
                identity: Default::default(),
                text: String::new(),
            }),
        );

        match &state.partial_parts()[0] {
            Part::Text(text) => assert_eq!(text.text, "hello"),
            other => panic!("expected text part, got {other:?}"),
        }
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
