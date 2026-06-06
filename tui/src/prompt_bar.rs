use alloc::string::String;

use ratatui::layout::Rect;
use ratatui::text::Text;
use ratatui::widgets::Widget;

use crate::app::Action;
use crate::event::{KeyEvent, Scancode};
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
        // `cursor` is a byte offset into `input` and is always kept on a UTF-8
        // char boundary, so every mutation below is valid for any Unicode text.
        match key.scancode {
            Scancode::Char(c) => {
                self.input.insert(self.cursor, c);
                self.cursor += c.len_utf8();
                None
            }
            Scancode::Backspace => {
                if self.cursor > 0 {
                    self.cursor = self.prev_boundary();
                    self.input.remove(self.cursor);
                }
                None
            }
            Scancode::Delete => {
                if self.cursor < self.input.len() {
                    self.input.remove(self.cursor);
                }
                None
            }
            Scancode::Left => {
                self.cursor = self.prev_boundary();
                None
            }
            Scancode::Right => {
                self.cursor = self.next_boundary();
                None
            }
            Scancode::Home => {
                self.cursor = 0;
                None
            }
            Scancode::End => {
                self.cursor = self.input.len();
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

    /// Byte index of the char boundary immediately before the cursor.
    fn prev_boundary(&self) -> usize {
        self.input[..self.cursor]
            .chars()
            .next_back()
            .map_or(self.cursor, |c| self.cursor - c.len_utf8())
    }

    /// Byte index of the char boundary immediately after the cursor.
    fn next_boundary(&self) -> usize {
        self.input[self.cursor..]
            .chars()
            .next()
            .map_or(self.cursor, |c| self.cursor + c.len_utf8())
    }

    pub fn clear(&mut self) {
        self.input.clear();
        self.cursor = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.input.is_empty()
    }

    pub fn append_text(&mut self, text: &str) {
        self.input.insert_str(self.cursor, text);
        self.cursor += text.len();
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

        let width = area.width as usize;
        if width == 0 {
            return;
        }

        // Caret position, in characters, within the full display string. The
        // synthetic mode prefix (e.g. "/ ") replaces the leading marker chars
        // trimmed from `input`, so map the input cursor into display space.
        let prefix_chars = prefix.chars().count();
        let input_caret_chars = self.input[..self.cursor].chars().count();
        let trimmed = self
            .input
            .chars()
            .count()
            .saturating_sub(body.chars().count());
        let caret = prefix_chars + input_caret_chars.saturating_sub(trimmed);

        // Horizontal scroll: keep the caret in view, pinned to the last column
        // once the text overflows the prompt width (this preserves the previous
        // tail-clamp behaviour when the cursor sits at the end of the input).
        let start = caret.saturating_sub(width.saturating_sub(1));
        let visible: alloc::string::String = display.chars().skip(start).take(width).collect();

        Text::from(visible.as_str())
            .style(theme.input)
            .render(area, frame.buffer_mut());

        let caret_col = (caret - start) as u16;
        frame.set_cursor_position((area.x + caret_col, area.y));
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

    fn key(scancode: Scancode) -> KeyEvent {
        KeyEvent {
            scancode,
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

    // Regression: a byte-vs-char cursor mismatch used to panic on the first
    // multi-byte character. The cursor is a byte offset now, so this is safe.
    #[test]
    fn insert_multibyte_does_not_panic() {
        let mut bar = PromptBar::new();
        bar.handle_key(&key_event('é'));
        bar.handle_key(&key_event('x'));
        bar.handle_key(&key_event('😀'));
        bar.handle_key(&key_event('y'));
        assert_eq!(bar.text(), "éx😀y");
    }

    #[test]
    fn backspace_removes_whole_multibyte_char() {
        let mut bar = PromptBar::new();
        bar.append_text("a😀");
        bar.handle_key(&backspace());
        assert_eq!(bar.text(), "a");
        bar.handle_key(&backspace());
        assert_eq!(bar.text(), "");
    }

    #[test]
    fn cursor_left_then_insert_inserts_midstring() {
        let mut bar = PromptBar::new();
        bar.append_text("abc");
        bar.handle_key(&key(Scancode::Left));
        bar.handle_key(&key(Scancode::Left));
        bar.handle_key(&key_event('X'));
        assert_eq!(bar.text(), "aXbc");
    }

    #[test]
    fn cursor_movement_is_char_safe_across_multibyte() {
        let mut bar = PromptBar::new();
        bar.append_text("é😀");
        // End, then Left over the emoji, then insert before it.
        bar.handle_key(&key(Scancode::End));
        bar.handle_key(&key(Scancode::Left));
        bar.handle_key(&key_event('z'));
        assert_eq!(bar.text(), "éz😀");
    }

    #[test]
    fn delete_at_cursor_removes_following_char() {
        let mut bar = PromptBar::new();
        bar.append_text("abc");
        bar.handle_key(&key(Scancode::Home));
        bar.handle_key(&key(Scancode::Delete));
        assert_eq!(bar.text(), "bc");
    }

    #[test]
    fn home_end_move_to_bounds() {
        let mut bar = PromptBar::new();
        bar.append_text("hello");
        bar.handle_key(&key(Scancode::Home));
        bar.handle_key(&key_event('>'));
        assert_eq!(bar.text(), ">hello");
        bar.handle_key(&key(Scancode::End));
        bar.handle_key(&key_event('<'));
        assert_eq!(bar.text(), ">hello<");
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
