use ratatui_core::style::{Color, Modifier, Style};

/// A collection of semantic styles used throughout the TUI.
///
/// Every screen and widget receives a `&Theme` and applies styles by
/// name instead of hardcoding colours. This makes it possible to
/// swap themes later (load from `tui.json`, offer `/themes`, etc.)
/// without touching any rendering code.
///
/// A struct (not a trait) — themes are data, not behaviour.
#[derive(Debug, Clone)]
pub struct Theme {
    // -- Base --
    pub bg: Style,
    pub text: Style,
    pub text_dim: Style,
    pub text_accent: Style,
    pub text_error: Style,

    // -- Input / PromptBar --
    pub input: Style,
    pub input_cursor: Style,
    pub input_hint: Style,

    // -- Messages --
    pub message_user: Style,
    pub message_assistant: Style,

    // -- Parts (within assistant messages) --
    pub part_text: Style,
    pub part_reasoning: Style,
    pub part_tool_idle: Style,
    pub part_tool_running: Style,
    pub part_tool_done: Style,
    pub part_tool_error: Style,
    pub part_step_divider: Style,

    // -- UI chrome --
    pub border: Style,
    pub scrollbar: Style,
    pub selection: Style,
    pub logo: Style,

    // -- Agent indicator --
    pub agent_indicator: Style,
}

impl Default for Theme {
    /// TokyoNight-inspired dark palette.
    fn default() -> Self {
        use Color::*;

        let bg = Rgb(0x1a, 0x1b, 0x26);
        let fg = Rgb(0xa9, 0xb1, 0xd6);
        let dim = Rgb(0x56, 0x5f, 0x89);
        let blue = Rgb(0x7a, 0xa2, 0xf7);
        let cyan = Rgb(0x7d, 0xcf, 0xff);
        let green = Rgb(0x9e, 0xce, 0x6a);
        let red = Rgb(0xf7, 0x76, 0x8e);
        let yellow = Rgb(0xe0, 0xaf, 0x68);
        let surface = Rgb(0x24, 0x25, 0x3a);

        Self {
            bg: Style::new().bg(bg),
            text: Style::new().fg(fg),
            text_dim: Style::new().fg(dim),
            text_accent: Style::new().fg(blue),
            text_error: Style::new().fg(red),

            input: Style::new().fg(fg).bg(surface),
            input_cursor: Style::new().bg(blue).fg(bg),
            input_hint: Style::new().fg(dim),

            message_user: Style::new().fg(cyan),
            message_assistant: Style::new().fg(fg),

            part_text: Style::new().fg(fg),
            part_reasoning: Style::new().fg(yellow).add_modifier(Modifier::ITALIC),
            part_tool_idle: Style::new().fg(dim),
            part_tool_running: Style::new().fg(blue),
            part_tool_done: Style::new().fg(green),
            part_tool_error: Style::new().fg(red),
            part_step_divider: Style::new().fg(dim),

            border: Style::new().fg(dim),
            scrollbar: Style::new().fg(dim).bg(surface),
            selection: Style::new().bg(blue).fg(bg),
            logo: Style::new().fg(blue),

            agent_indicator: Style::new().fg(green).bg(surface),
        }
    }
}
