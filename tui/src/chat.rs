use alloc::vec;
use alloc::vec::Vec;

use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Text};
use ratatui::widgets::{
    Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget, Widget, Wrap,
};

use crate::app::LoadedMessage;
use crate::theme::Theme;

pub struct ChatTranscript<'a> {
    pub messages: &'a [LoadedMessage],
    pub active_parts: &'a [ocpncord_backend::Part],
    pub queued_messages: &'a [LoadedMessage],
    pub is_streaming: bool,
}

/// Renders the Chat message area using the provided data.
pub fn render_chat(
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
            text: "thinking...".into(),
        });
        let lines = render_part(&part, &theme, false);
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn tool_pending_shows_idle_style() {
        let theme = Theme::default();
        let part = Part::Tool(ToolPart {
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
            reason: None,
            snapshot: None,
            session_id: None,
        });
        let lines = render_part(&part, &theme, true);
        assert!(lines.is_empty());
    }
}
