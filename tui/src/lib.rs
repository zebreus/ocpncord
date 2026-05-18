#![cfg_attr(not(test), no_std)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

//! Platform-agnostic TUI for the ocpncord client.
//!
//! Built on `ratatui` widgets. Renders via Crossterm on desktop
//! and via mousefood on embedded hardware.

extern crate alloc;

mod app;
mod chat;
mod command_palette;
mod event;
mod key_chord;
mod modal;
mod part_renderer;
mod prompt_bar;
mod screen;
mod start_page;
mod theme;

pub use app::App;
pub use command_palette::{default_commands, CommandPaletteModal};
pub use event::{Event, KeyEvent, Modifiers, Scancode};
pub use key_chord::KeyChord;
pub use modal::Modal;
pub use screen::{Action, ModalId, Screen, ScreenId, Tab};
pub use theme::Theme;
