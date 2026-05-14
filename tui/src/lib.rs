#![cfg_attr(not(test), no_std)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

//! Platform-agnostic TUI for the opencode client.
//!
//! Built on `ratatui-core` widgets. Renders via Crossterm on desktop
//! and via mousefood on embedded hardware.

extern crate alloc;

mod app;
mod event;
mod screen;
mod theme;

pub use app::App;
pub use event::Event;
pub use screen::Screen;
pub use theme::Theme;
