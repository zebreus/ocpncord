use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use ratatui_core::layout::Rect;
use ratatui_core::terminal::Frame;
use ratatui_core::text::Text;
use ratatui_core::widgets::Widget;

use crate::event::{Event, Scancode};
use crate::modal::Modal;
use crate::screen::{Action, ModalId};
use crate::theme::Theme;

/// A single command entry in the palette.
#[derive(Debug, Clone)]
pub struct PaletteCommand {
    pub name: &'static str,
    pub slash_command: &'static str,
    pub keybinding: &'static str,
    pub action: Action,
}

/// Returns the MVP list of palette commands.
pub fn default_commands() -> Vec<PaletteCommand> {
    vec![
        PaletteCommand {
            name: "Help",
            slash_command: "/help",
            keybinding: "Ctrl+X H",
            action: Action::OpenModal(ModalId::Help),
        },
        PaletteCommand {
            name: "New Session",
            slash_command: "/new",
            keybinding: "Ctrl+X N",
            action: Action::NewSession,
        },
        PaletteCommand {
            name: "Sessions",
            slash_command: "/sessions",
            keybinding: "Ctrl+X L",
            action: Action::OpenModal(ModalId::SessionList),
        },
        PaletteCommand {
            name: "Settings",
            slash_command: "/settings",
            keybinding: "Ctrl+X M",
            action: Action::OpenSettings,
        },
        PaletteCommand {
            name: "Todos",
            slash_command: "/todos",
            keybinding: "",
            action: Action::ToggleSidePanel,
        },
        PaletteCommand {
            name: "Diagnostics",
            slash_command: "/diagnostics",
            keybinding: "",
            action: Action::ToggleSidePanel,
        },
        PaletteCommand {
            name: "Terminal",
            slash_command: "/pty",
            keybinding: "Ctrl+X T",
            action: Action::OpenTerminal(String::new()),
        },
        PaletteCommand {
            name: "Exit",
            slash_command: "/exit",
            keybinding: "Ctrl+X Q",
            action: Action::Quit,
        },
        PaletteCommand {
            name: "Toggle Details",
            slash_command: "/details",
            keybinding: "",
            action: Action::ToggleDetails,
        },
        PaletteCommand {
            name: "Cycle Agent",
            slash_command: "",
            keybinding: "Tab",
            action: Action::CycleAgent,
        },
    ]
}

pub struct CommandPaletteModal {
    search: String,
    commands: Vec<PaletteCommand>,
    filtered_indices: Vec<usize>,
    selected: usize,
}

impl CommandPaletteModal {
    pub fn new(commands: Vec<PaletteCommand>) -> Self {
        let filtered_indices = (0..commands.len()).collect();
        Self {
            search: String::new(),
            commands,
            filtered_indices,
            selected: 0,
        }
    }

    pub fn search(&self) -> &str {
        &self.search
    }

    pub fn selected_index(&self) -> usize {
        self.selected
    }

    pub fn filtered_count(&self) -> usize {
        self.filtered_indices.len()
    }

    fn update_filter(&mut self) {
        let query = self.search.to_lowercase();
        self.filtered_indices = self
            .commands
            .iter()
            .enumerate()
            .filter(|(_, cmd)| {
                cmd.name.to_lowercase().contains(&query)
                    || cmd.slash_command.to_lowercase().contains(&query)
            })
            .map(|(i, _)| i)
            .collect();
        self.selected = 0;
    }
}

impl Modal for CommandPaletteModal {
    fn render(&self, frame: &mut Frame, theme: &Theme, area: Rect) {
        // Title
        Text::from("Command Palette")
            .style(theme.text_accent)
            .render(Rect::new(area.x, area.y, area.width, 1), frame.buffer_mut());

        // Search input
        let search_area = Rect::new(area.x, area.y + 1, area.width, 1);
        let search_display = alloc::format!("> {}", self.search);
        Text::from(search_display)
            .style(theme.input)
            .render(search_area, frame.buffer_mut());

        // Command list
        let list_start_y = area.y + 3;
        let max_y = area.bottom();

        if self.filtered_indices.is_empty() {
            Text::from("No matching commands")
                .style(theme.text_dim)
                .render(
                    Rect::new(area.x, list_start_y, area.width, 1),
                    frame.buffer_mut(),
                );
            return;
        }

        for (visible_i, &cmd_idx) in self.filtered_indices.iter().enumerate() {
            let y = list_start_y + visible_i as u16;
            if y >= max_y {
                break;
            }

            let cmd = &self.commands[cmd_idx];
            let is_selected = visible_i == self.selected;

            let name_part = if cmd.slash_command.is_empty() {
                cmd.name.to_string()
            } else {
                alloc::format!("{} ({})", cmd.name, cmd.slash_command)
            };

            let keybinding_part = if cmd.keybinding.is_empty() {
                String::new()
            } else {
                alloc::format!("  [{}]", cmd.keybinding)
            };

            let display = alloc::format!("{}{}", name_part, keybinding_part);

            let prefix = if is_selected { "> " } else { "  " };
            let full_display = alloc::format!("{}{}", prefix, display);

            let style = if is_selected {
                theme.selection
            } else {
                theme.text
            };

            Text::from(full_display)
                .style(style)
                .render(Rect::new(area.x, y, area.width, 1), frame.buffer_mut());
        }
    }

    fn handle_event(&mut self, event: Event) -> Action {
        match event {
            Event::Key(key) => match key.scancode {
                Scancode::Escape => Action::CloseModal,
                Scancode::Enter => {
                    if let Some(&cmd_idx) = self.filtered_indices.get(self.selected) {
                        self.commands[cmd_idx].action.clone()
                    } else {
                        Action::None
                    }
                }
                Scancode::Up => {
                    if self.selected > 0 {
                        self.selected -= 1;
                    }
                    Action::None
                }
                Scancode::Down => {
                    if self.selected + 1 < self.filtered_indices.len() {
                        self.selected += 1;
                    }
                    Action::None
                }
                Scancode::Char(c) => {
                    self.search.push(c);
                    self.update_filter();
                    Action::None
                }
                Scancode::Backspace => {
                    if !self.search.is_empty() {
                        self.search.pop();
                        self.update_filter();
                    }
                    Action::None
                }
                _ => Action::None,
            },
            _ => Action::None,
        }
    }

    fn title(&self) -> &str {
        "Command Palette"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{KeyEvent, Modifiers, Scancode};
    use ratatui_core::backend::TestBackend;
    use ratatui_core::terminal::Terminal;

    fn key_event(scancode: Scancode, modifiers: Modifiers) -> KeyEvent {
        KeyEvent {
            scancode,
            modifiers,
        }
    }

    fn commands() -> Vec<PaletteCommand> {
        vec![
            PaletteCommand {
                name: "Help",
                slash_command: "/help",
                keybinding: "Ctrl+X H",
                action: Action::OpenModal(ModalId::Help),
            },
            PaletteCommand {
                name: "New Session",
                slash_command: "/new",
                keybinding: "Ctrl+X N",
                action: Action::NewSession,
            },
            PaletteCommand {
                name: "Exit",
                slash_command: "/exit",
                keybinding: "Ctrl+X Q",
                action: Action::Quit,
            },
        ]
    }

    #[test]
    fn palette_renders_all_commands_when_search_empty() {
        let palette = CommandPaletteModal::new(commands());
        let theme = Theme::default();
        let backend = TestBackend::new(60, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                palette.render(frame, &theme, Rect::new(5, 2, 50, 15));
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let screen: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(screen.contains("Help"), "Should show Help");
        assert!(screen.contains("New Session"), "Should show New Session");
        assert!(screen.contains("Exit"), "Should show Exit");
    }

    #[test]
    fn palette_filters_on_search_typing() {
        let mut palette = CommandPaletteModal::new(commands());
        palette.handle_event(Event::Key(key_event(
            Scancode::Char('e'),
            Modifiers::default(),
        )));
        palette.handle_event(Event::Key(key_event(
            Scancode::Char('x'),
            Modifiers::default(),
        )));
        assert_eq!(palette.search(), "ex");
        assert_eq!(palette.filtered_count(), 1, "only 'Exit' matches 'ex'");
    }

    #[test]
    fn palette_filters_by_slash_command() {
        let mut palette = CommandPaletteModal::new(commands());
        palette.handle_event(Event::Key(key_event(
            Scancode::Char('/'),
            Modifiers::default(),
        )));
        palette.handle_event(Event::Key(key_event(
            Scancode::Char('n'),
            Modifiers::default(),
        )));
        assert_eq!(palette.search(), "/n");
        // /new matches, /help does not
        assert_eq!(palette.filtered_count(), 1);
    }

    #[test]
    fn palette_backspace_removes_filter_char() {
        let mut palette = CommandPaletteModal::new(commands());
        palette.handle_event(Event::Key(key_event(
            Scancode::Char('h'),
            Modifiers::default(),
        )));
        assert_eq!(palette.search(), "h");
        assert_eq!(palette.filtered_count(), 1); // Help matches
        palette.handle_event(Event::Key(key_event(
            Scancode::Backspace,
            Modifiers::default(),
        )));
        assert_eq!(palette.search(), "");
        assert_eq!(
            palette.filtered_count(),
            3,
            "all commands shown after backspace"
        );
    }

    #[test]
    fn palette_arrow_keys_navigate_filtered_list() {
        let mut palette = CommandPaletteModal::new(commands());
        assert_eq!(palette.selected_index(), 0);

        palette.handle_event(Event::Key(key_event(Scancode::Down, Modifiers::default())));
        assert_eq!(palette.selected_index(), 1);

        palette.handle_event(Event::Key(key_event(Scancode::Down, Modifiers::default())));
        assert_eq!(palette.selected_index(), 2);

        // down at end stays at end
        palette.handle_event(Event::Key(key_event(Scancode::Down, Modifiers::default())));
        assert_eq!(palette.selected_index(), 2);

        palette.handle_event(Event::Key(key_event(Scancode::Up, Modifiers::default())));
        assert_eq!(palette.selected_index(), 1);

        palette.handle_event(Event::Key(key_event(Scancode::Up, Modifiers::default())));
        assert_eq!(palette.selected_index(), 0);

        // up at start stays at start
        palette.handle_event(Event::Key(key_event(Scancode::Up, Modifiers::default())));
        assert_eq!(palette.selected_index(), 0);
    }

    #[test]
    fn palette_enter_returns_selected_command_action() {
        let mut palette = CommandPaletteModal::new(commands());
        // Select "Exit" (index 2)
        palette.handle_event(Event::Key(key_event(Scancode::Down, Modifiers::default())));
        palette.handle_event(Event::Key(key_event(Scancode::Down, Modifiers::default())));
        let action =
            palette.handle_event(Event::Key(key_event(Scancode::Enter, Modifiers::default())));
        assert_eq!(action, Action::Quit);
    }

    #[test]
    fn palette_escape_returns_close_modal() {
        let mut palette = CommandPaletteModal::new(commands());
        let action = palette.handle_event(Event::Key(key_event(
            Scancode::Escape,
            Modifiers::default(),
        )));
        assert_eq!(action, Action::CloseModal);
    }

    #[test]
    fn palette_enter_on_filtered_list_returns_correct_action() {
        let mut palette = CommandPaletteModal::new(commands());
        // Filter to only "Help"
        palette.handle_event(Event::Key(key_event(
            Scancode::Char('h'),
            Modifiers::default(),
        )));
        assert_eq!(palette.filtered_count(), 1);
        // Enter should return Help's action
        let action =
            palette.handle_event(Event::Key(key_event(Scancode::Enter, Modifiers::default())));
        assert_eq!(action, Action::OpenModal(ModalId::Help));
    }

    #[test]
    fn palette_renders_search_input() {
        let mut palette = CommandPaletteModal::new(commands());
        palette.handle_event(Event::Key(key_event(
            Scancode::Char('h'),
            Modifiers::default(),
        )));
        let theme = Theme::default();
        let backend = TestBackend::new(60, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                palette.render(frame, &theme, Rect::new(5, 2, 50, 15));
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let screen: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(screen.contains("> h"), "Search input should show '> h'");
    }

    #[test]
    fn palette_empty_search_shows_all_commands() {
        let mut palette = CommandPaletteModal::new(commands());
        // Type something then clear it
        palette.handle_event(Event::Key(key_event(
            Scancode::Char('h'),
            Modifiers::default(),
        )));
        assert_eq!(palette.filtered_count(), 1);
        palette.handle_event(Event::Key(key_event(
            Scancode::Backspace,
            Modifiers::default(),
        )));
        assert_eq!(
            palette.filtered_count(),
            3,
            "empty search shows all commands"
        );
    }
}
