use ratatui_core::layout::{Alignment, Rect};
use ratatui_core::text::Text;
use ratatui_core::widgets::Widget;

use crate::app::LoadedMessage;
use crate::event::Event;
use crate::part_renderer::render_part;
use crate::screen::{Action, Screen};
use crate::theme::Theme;

const PROMPT_BAR_LINES: u16 = 1;

/// Renders the Chat message area using the provided data.
pub fn render_chat(
    frame: &mut ratatui_core::terminal::Frame,
    theme: &Theme,
    messages: &[LoadedMessage],
    partial_parts: &[opencode_backend::Part],
    is_streaming: bool,
    _scroll: u16,
) {
    let area = frame.area();
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

    let mut y = msg_area.y;
    let max_y = msg_area.y + msg_area.height;

    for msg in messages {
        for part in &msg.parts {
            if y >= max_y {
                break;
            }
            for line in render_part(part, theme, true) {
                if y >= max_y {
                    break;
                }
                let line_area = Rect::new(msg_area.x, y, msg_area.width, 1);
                let text = Text::from(line);
                text.render(line_area, frame.buffer_mut());
                y += 1;
            }
        }
        if y >= max_y {
            break;
        }
    }

    if is_streaming {
        for part in partial_parts {
            if y >= max_y {
                break;
            }
            for line in render_part(part, theme, true) {
                if y >= max_y {
                    break;
                }
                let line_area = Rect::new(msg_area.x, y, msg_area.width, 1);
                let text = Text::from(line);
                text.render(line_area, frame.buffer_mut());
                y += 1;
            }
        }
    }
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
    fn render(&self, _frame: &mut ratatui_core::terminal::Frame, _theme: &Theme) {
        // Chat is rendered via `render_chat` from App::render
    }

    fn handle_event(&mut self, event: Event) -> Action {
        match event {
            Event::Key(key) => match key.scancode {
                crate::event::Scancode::Up => Action::ScrollUp,
                crate::event::Scancode::Down => Action::ScrollDown,
                crate::event::Scancode::PageUp => Action::ScrollUp,
                crate::event::Scancode::PageDown => Action::ScrollDown,
                _ => Action::None,
            },
            _ => Action::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opencode_backend::*;
    use ratatui_core::backend::TestBackend;
    use ratatui_core::terminal::Terminal;

    #[test]
    fn renders_placeholder_when_empty() {
        let theme = Theme::default();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_chat(frame, &theme, &[], &[], false, 0);
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
                render_chat(frame, &theme, &msgs, &[], false, 0);
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
                render_chat(frame, &theme, &[], &partial, true, 0);
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
}
