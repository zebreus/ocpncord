use alloc::string::String;
use alloc::vec::Vec;

use crate::{Event, Theme};
use ratatui::Frame;

/// One screen or widget tree in the TUI.
pub trait Screen {
    fn render(&self, frame: &mut Frame, theme: &Theme);
    fn handle_event(&mut self, event: Event) -> Action;
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
    ScrollPageUp,
    ScrollPageDown,
    ToggleDetails,
    SelectModel(String),
    LoadSession(String),
    DeleteSession(String),
    NewSession,
    // --- New: session lifecycle ---
    RenameSession(String, String), // session_id, new_title
    AbortSession(String),          // session_id
    SwitchToChat(String),          // session_id (load then switch)
    // --- New: UI navigation ---
    OpenSettings,
    OpenTerminal(String), // pty_id
    CloseTerminal,
    ToggleSidePanel,
    ToggleSidePanelTab(Tab),
    SidePanelSelectTab(Tab),
    // --- New: modal replies ---
    ReplyPermission(String, String, PermissionReplyAction), // session_id, request_id, reply
    ReplyQuestion(String, String, Vec<Vec<String>>),        // session_id, request_id, answers
    RejectQuestion(String),                                 // request_id
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenId {
    StartPage,
    Chat,
    Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalId {
    SessionList,
    Help,
    ModelPicker,
    CommandPalette,
    Settings,
    PermissionApproval,
    QuestionApproval,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Diagnostics,
    Todos,
    Pane,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionReplyAction {
    Once,
    Always,
    Reject,
}
