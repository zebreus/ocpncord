use ratatui_core::layout::{Alignment, Rect};
use ratatui_core::text::Text;
use ratatui_core::widgets::Widget;

use crate::event::Event;
use crate::screen::{Action, Screen};
use crate::theme::Theme;

const LOGO: &str = r#"
        ██████   ██████ ██████  ███    ██ ██████  ██████  ██████  ██████
       ██    ██ ██      ██   ██ ████   ██ ██      ██    ██ ██   ██ ██   ██
       ██    ██ ██      ██████  ██ ██  ██ ██      ██    ██ ██████  ██   ██
       ██    ██ ██      ██      ██  ██ ██ ██      ██    ██ ██   ██ ██   ██
        ██████   ██████ ██      ██   ████  ██████  ██████  ██   ██ ██████
"#;

const TIP: &str = "Tip: Ctrl+X H for help, Ctrl+X Q to quit";

pub struct StartPage;

impl Screen for StartPage {
    fn render(&self, frame: &mut ratatui_core::terminal::Frame, theme: &Theme) {
        let area = frame.area();

        let logo_height = LOGO.lines().count() as u16;
        let tip_height = 1u16;
        let total_content_height = logo_height + tip_height;
        let start_y = area.height.saturating_sub(total_content_height) / 2;

        let logo_area = Rect::new(area.x, start_y, area.width, logo_height);
        Text::from(LOGO)
            .style(theme.logo)
            .alignment(Alignment::Center)
            .render(logo_area, frame.buffer_mut());

        let tip_area = Rect::new(area.x, start_y + logo_height, area.width, tip_height);
        Text::from(TIP)
            .style(theme.text_dim)
            .alignment(Alignment::Center)
            .render(tip_area, frame.buffer_mut());
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
    fn renders_logo_and_tip() {
        let start_page = StartPage;
        let theme = Theme::default();

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                start_page.render(frame, &theme);
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        let has_tip =
            (0..80).any(|x| (0..24).any(|y| buf.cell((x, y)).is_some_and(|c| c.symbol() == "T")));
        assert!(
            has_tip,
            "tip line (starting with 'T') should appear on screen"
        );
    }

    #[test]
    fn renders_something_on_screen() {
        let start_page = StartPage;
        let theme = Theme::default();

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                start_page.render(frame, &theme);
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        let has_any_content = buf.content().iter().any(|c| c.symbol() != " ");
        assert!(has_any_content, "StartPage should render visible content");
    }

    #[test]
    fn handle_event_returns_none() {
        let mut start_page = StartPage;
        let event = Event::Tick;
        assert_eq!(start_page.handle_event(event), Action::None);
    }
}
