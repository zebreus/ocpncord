use alloc::vec::Vec;

use ratatui::layout::{Alignment, Rect};
use ratatui::text::{Line, Text};
use ratatui::widgets::{
    Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget, Widget, Wrap,
};

use crate::app::LoadedMessage;
use crate::event::Event;
use crate::part_renderer::render_part;
use crate::screen::{Action, Screen};
use crate::theme::Theme;

const PROMPT_BAR_LINES: u16 = 1;

/// Renders the Chat message area using the provided data.
pub fn render_chat(
    frame: &mut ratatui::Frame,
    theme: &Theme,
    area: Rect,
    messages: &[LoadedMessage],
    partial_parts: &[ocpncord_backend::Part],
    is_streaming: bool,
    scroll: u16,
) {
    let msg_area = Rect::new(
        area.x,
        area.y,
        area.width,
        area.height.saturating_sub(PROMPT_BAR_LINES),
    );

    if messages.is_empty() && !is_streaming {
        Text::from("No messages yet")
            .style(theme.text_dim)
            .alignment(Alignment::Center)
            .render(msg_area, frame.buffer_mut());
        return;
    }

    let mut lines: Vec<Line<'_>> = Vec::new();
    for msg in messages {
        for part in &msg.parts {
            lines.extend(render_part(part, theme, true));
        }
    }

    for part in partial_parts {
        lines.extend(render_part(part, theme, true));
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

pub struct Chat {
    pub(crate) scroll: u16,
}

impl Chat {
    pub fn new() -> Self {
        Self { scroll: 0 }
    }
}

impl Default for Chat {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen for Chat {
    fn render(&self, _frame: &mut ratatui::Frame, _theme: &Theme) {
        // Chat is rendered via `render_chat` from App::render
    }

    fn handle_event(&mut self, event: Event) -> Action {
        match event {
            Event::Key(key) => match key.scancode {
                crate::event::Scancode::Up => Action::ScrollUp,
                crate::event::Scancode::Down => Action::ScrollDown,
                crate::event::Scancode::PageUp => Action::ScrollPageUp,
                crate::event::Scancode::PageDown => Action::ScrollPageDown,
                _ => Action::None,
            },
            _ => Action::None,
        }
    }
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
                render_chat(frame, &theme, frame.area(), &[], &[], false, 0);
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
                render_chat(frame, &theme, frame.area(), &msgs, &[], false, 0);
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
                render_chat(frame, &theme, frame.area(), &[], &partial, true, 0);
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        let has_text = buf.content().iter().any(|c| c.symbol() == "s");
        assert!(has_text);
    }

    #[test]
    fn handle_event_up_down_returns_scroll_actions() {
        let mut chat = Chat::new();
        assert_eq!(
            chat.handle_event(Event::Key(crate::event::KeyEvent {
                scancode: crate::event::Scancode::Up,
                modifiers: Default::default(),
            })),
            Action::ScrollUp
        );
        assert_eq!(
            chat.handle_event(Event::Key(crate::event::KeyEvent {
                scancode: crate::event::Scancode::Down,
                modifiers: Default::default(),
            })),
            Action::ScrollDown
        );
    }

    #[test]
    fn handle_event_page_keys_return_page_scroll_actions() {
        let mut chat = Chat::new();
        assert_eq!(
            chat.handle_event(Event::Key(crate::event::KeyEvent {
                scancode: crate::event::Scancode::PageUp,
                modifiers: Default::default(),
            })),
            Action::ScrollPageUp
        );
        assert_eq!(
            chat.handle_event(Event::Key(crate::event::KeyEvent {
                scancode: crate::event::Scancode::PageDown,
                modifiers: Default::default(),
            })),
            Action::ScrollPageDown
        );
    }

    #[test]
    fn wrapped_height_counts_width_after_scrollbar_reservation() {
        let line = Line::from("1234567890");
        assert_eq!(wrapped_height(&[line.clone()], 10), 1);
        assert_eq!(wrapped_height(&[line], 9), 2);
    }
}
