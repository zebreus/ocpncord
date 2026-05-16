use alloc::string::String;

use ratatui_core::layout::Rect;
use ratatui_core::text::Text;
use ratatui_core::widgets::Widget;

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

    pub fn append_text(&mut self, text: &str) {
        for ch in text.chars() {
            self.input.insert(self.cursor, ch);
            self.cursor += 1;
        }
    }

    pub fn render(
        &self,
        area: Rect,
        frame: &mut ratatui_core::terminal::Frame,
        theme: &Theme,
        is_streaming: bool,
        agent_name: &str,
    ) {
        let agent_display = alloc::format!("[{}]", agent_name);
        let agent_area = Rect::new(
            area.right().saturating_sub(agent_display.len() as u16 + 1),
            area.y,
            agent_display.len() as u16,
            1,
        );
        Text::from(agent_display.as_str())
            .style(theme.agent_indicator)
            .render(agent_area, frame.buffer_mut());

        if is_streaming {
            Text::from("Agent is responding... (Esc to interrupt)")
                .style(theme.text_dim)
                .render(area, frame.buffer_mut());
            return;
        }

        let prefix = match self.input_mode() {
            InputMode::Command => "/",
            InputMode::Shell => "!",
            InputMode::FileRef => "@",
            InputMode::ToolRef => "#",
            InputMode::Normal => "> ",
        };

        let display = alloc::format!("{}{}", prefix, self.input);
        Text::from(display)
            .style(theme.input)
            .render(area, frame.buffer_mut());
    }
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

    fn enter() -> KeyEvent {
        KeyEvent {
            scancode: Scancode::Enter,
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
}
