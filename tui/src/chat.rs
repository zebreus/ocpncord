use alloc::vec::Vec;

use ratatui::layout::{Alignment, Rect};
use ratatui::text::{Line, Text};
use ratatui::widgets::{
    Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget, Widget, Wrap,
};

use crate::app::LoadedMessage;
use crate::part_renderer::render_part;
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
}
