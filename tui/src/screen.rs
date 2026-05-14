use crate::{Event, Theme};
use ratatui_core::terminal::Frame;

/// One screen or widget tree in the TUI.
pub trait Screen {
    fn render(&self, frame: &mut Frame, theme: &Theme);
    fn handle_event(&mut self, event: Event) -> Action;
}

pub enum Action {
    None,
    Quit,
    Navigate(ScreenId),
}

pub enum ScreenId {
    SessionList,
    Chat,
}
