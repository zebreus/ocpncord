use ratatui_core::layout::{Alignment, Rect};
use ratatui_core::text::Text;
use ratatui_core::widgets::Widget;

use crate::event::Event;
use crate::screen::{Action, Screen};
use crate::theme::Theme;

pub struct Chat;

impl Screen for Chat {
    fn render(&self, frame: &mut ratatui_core::terminal::Frame, theme: &Theme) {
        let area = frame.area();
        let msg_area = Rect::new(area.x, area.y, area.width, area.height.saturating_sub(3));

        Text::from("No messages yet")
            .style(theme.text_dim)
            .alignment(Alignment::Center)
            .render(msg_area, frame.buffer_mut());
    }

    fn handle_event(&mut self, _event: Event) -> Action {
        Action::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui_core::backend::TestBackend;
    use ratatui_core::terminal::Terminal;

    #[test]
    fn renders_placeholder_message() {
        let chat = Chat;
        let theme = Theme::default();

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                chat.render(frame, &theme);
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        let has_placeholder = buf.content().iter().any(|c| c.symbol() == "N");
        assert!(
            has_placeholder,
            "Chat should render 'No messages yet' placeholder"
        );
    }

    #[test]
    fn handle_event_returns_none() {
        let mut chat = Chat;
        let event = Event::Tick;
        assert_eq!(chat.handle_event(event), Action::None);
    }
}
