use alloc::string::String;

use ratatui::layout::Rect;
use ratatui::text::Text;
use ratatui::widgets::Widget;

use crate::event::{KeyEvent, Scancode};
use crate::screen::Action;
use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Command,
    Shell,
    FileRef,
    ToolRef,
}

pub struct PromptBar {
    input: String,
    cursor: usize,
}

impl PromptBar {
    pub fn new() -> Self {
        Self {
            input: String::new(),
            cursor: 0,
        }
    }

    pub fn text(&self) -> &str {
        &self.input
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn input_mode(&self) -> InputMode {
        let trimmed = self.input.trim_start();
        match trimmed.chars().next() {
            Some('/') => InputMode::Command,
            Some('!') => InputMode::Shell,
            Some('@') => InputMode::FileRef,
            Some('#') => InputMode::ToolRef,
            _ => InputMode::Normal,
        }
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> Option<Action> {
        match key.scancode {
            Scancode::Char(c) => {
                self.input.insert(self.cursor, c);
                self.cursor += 1;
                None
            }
            Scancode::Backspace => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.input.remove(self.cursor);
                }
                None
            }
            Scancode::Enter => {
                if !self.input.is_empty() {
                    Some(Action::SendMessage)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub fn clear(&mut self) {
        self.input.clear();
        self.cursor = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.input.is_empty()
    }

    pub fn append_text(&mut self, text: &str) {
        for ch in text.chars() {
            self.input.insert(self.cursor, ch);
            self.cursor += 1;
        }
    }

    pub fn render(
        &self,
        area: Rect,
        frame: &mut ratatui::Frame,
        theme: &Theme,
        _is_streaming: bool,
        _agent_name: &str,
        _tick: u64,
    ) {
        let (prefix, body) = match self.input_mode() {
            InputMode::Command => ("/ ", self.input.trim_start_matches('/')),
            InputMode::Shell => ("! ", self.input.trim_start_matches('!')),
            InputMode::FileRef => ("@ ", self.input.trim_start_matches('@')),
            InputMode::ToolRef => ("# ", self.input.trim_start_matches('#')),
            InputMode::Normal => ("> ", self.input.as_str()),
        };

        let display = alloc::format!("{}{}", prefix, body);
        Text::from(clamp_tail(display.as_str(), area.width as usize))
            .style(theme.input)
            .render(area, frame.buffer_mut());
    }
}

fn clamp_tail(text: &str, width: usize) -> alloc::string::String {
    if width == 0 {
        return alloc::string::String::new();
    }
    let len = text.chars().count();
    text.chars().skip(len.saturating_sub(width)).collect()
}

impl Default for PromptBar {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{KeyEvent, Modifiers, Scancode};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn key_event(ch: char) -> KeyEvent {
        KeyEvent {
            scancode: Scancode::Char(ch),
            modifiers: Modifiers::default(),
        }
    }

    fn backspace() -> KeyEvent {
        KeyEvent {
            scancode: Scancode::Backspace,
            modifiers: Modifiers::default(),
        }
    }

    // Tracer bullet: character insertion
    #[test]
    fn insert_char_appends_text() {
        let mut bar = PromptBar::new();
        bar.handle_key(&key_event('h'));
        assert_eq!(bar.text(), "h");
    }

    #[test]
    fn backspace_removes_last_char() {
        let mut bar = PromptBar::new();
        bar.handle_key(&key_event('a'));
        bar.handle_key(&key_event('b'));
        bar.handle_key(&key_event('c'));
        assert_eq!(bar.text(), "abc");
        bar.handle_key(&backspace());
        assert_eq!(bar.text(), "ab");
    }

    #[test]
    fn leading_slash_is_command_mode() {
        let mut bar = PromptBar::new();
        bar.handle_key(&key_event('/'));
        assert_eq!(bar.input_mode(), InputMode::Command);
    }

    #[test]
    fn leading_bang_is_shell_mode() {
        let mut bar = PromptBar::new();
        bar.handle_key(&key_event('!'));
        assert_eq!(bar.input_mode(), InputMode::Shell);
    }

    #[test]
    fn leading_at_is_fileref_mode() {
        let mut bar = PromptBar::new();
        bar.handle_key(&key_event('@'));
        assert_eq!(bar.input_mode(), InputMode::FileRef);
    }

    #[test]
    fn empty_input_is_normal_mode() {
        let bar = PromptBar::new();
        assert_eq!(bar.input_mode(), InputMode::Normal);
    }

    #[test]
    fn normal_text_is_normal_mode() {
        let mut bar = PromptBar::new();
        bar.handle_key(&key_event('h'));
        bar.handle_key(&key_event('i'));
        assert_eq!(bar.input_mode(), InputMode::Normal);
    }

    #[test]
    fn leading_hash_is_toolref_mode() {
        let mut bar = PromptBar::new();
        bar.handle_key(&key_event('#'));
        assert_eq!(bar.input_mode(), InputMode::ToolRef);
    }

    #[test]
    fn command_prefix_is_not_rendered_twice() {
        let mut bar = PromptBar::new();
        bar.append_text("/help");
        let theme = Theme::default();
        let backend = TestBackend::new(40, 1);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| {
                bar.render(frame.area(), frame, &theme, false, "build", 0);
            })
            .unwrap();

        let screen: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(screen.contains("/ help"));
        assert!(!screen.contains("//help"));
    }

    #[test]
    fn long_input_is_clamped_to_prompt_area() {
        let mut bar = PromptBar::new();
        bar.append_text("this input is intentionally longer than the prompt area");
        let theme = Theme::default();
        let backend = TestBackend::new(24, 1);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| {
                bar.render(frame.area(), frame, &theme, false, "build", 0);
            })
            .unwrap();

        let screen: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert_eq!(screen.chars().count(), 24, "screen: {screen}");
    }
}
