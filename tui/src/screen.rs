use crate::{Event, Theme};
use ratatui_core::terminal::Frame;

/// One screen or widget tree in the TUI.
pub trait Screen {
    fn render(&self, frame: &mut Frame, theme: &Theme);
    fn handle_event(&mut self, event: Event) -> Action;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    Quit,
    SwitchScreen(ScreenId),
    OpenModal(ModalId),
    CloseModal,
    CycleAgent,
    Interrupt,
    SendMessage,
    OpenPalette,
    ScrollUp,
    ScrollDown,
    ToggleDetails,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenId {
    StartPage,
    Chat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalId {
    SessionList,
    Help,
    ModelPicker,
    CommandPalette,
    Settings,
}
