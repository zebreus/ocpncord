pub use ocpncord_backend::BackendEvent;

/// Generic keypress event, SDL2-style scancode abstraction.
///
/// The platform layer translates OS-specific input into this enum.
/// The TUI never reads raw hardware events directly.
#[derive(Debug, Clone)]
pub enum Event {
    Key(KeyEvent),
    Backend(BackendEvent),
    Tick,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEvent {
    pub scancode: Scancode,
    pub modifiers: Modifiers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scancode {
    Char(char),
    Enter,
    Escape,
    Backspace,
    Tab,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Delete,
    F(u8),
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}
