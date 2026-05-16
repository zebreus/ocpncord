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
    pub part_file: Style,
    pub part_snapshot: Style,
    pub part_patch: Style,
    pub part_agent: Style,
    pub part_subtask: Style,
    pub part_retry: Style,
    pub part_compaction: Style,

    // -- UI chrome --
    pub border: Style,
    pub scrollbar: Style,
    pub selection: Style,
    pub logo: Style,

    // -- Agent indicator --
    pub agent_indicator: Style,

    // -- Terminal (PTY) --
    pub pty_output: Style,
    pub pty_error: Style,
    pub pty_status_bar: Style,
    pub pty_cursor_line: Style,

    // -- Toasts --
    pub toast_bg: Style,
    pub toast_info: Style,
    pub toast_success: Style,
    pub toast_warning: Style,
    pub toast_error: Style,
    pub toast_border: Style,

    // -- Side Panel --
    pub side_panel_bg: Style,
    pub side_panel_title: Style,
    pub side_panel_tab_active: Style,
    pub side_panel_tab_inactive: Style,
    pub side_panel_border: Style,

    // -- Dialogs (Permission, Question) --
    pub dialog_bg: Style,
    pub dialog_border: Style,
    pub dialog_title: Style,
    pub dialog_button: Style,
    pub dialog_button_focused: Style,
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
        let surface2 = Rgb(0x31, 0x32, 0x4a);
        let _teal = Rgb(0x7a, 0xdc, 0xbb);
        let orange = Rgb(0xff, 0x9e, 0x64);
        let maroon = Rgb(0xd1, 0x86, 0x16);

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
            part_file: Style::new().fg(cyan),
            part_snapshot: Style::new().fg(yellow),
            part_patch: Style::new().fg(green),
            part_agent: Style::new().fg(blue),
            part_subtask: Style::new().fg(maroon),
            part_retry: Style::new().fg(orange),
            part_compaction: Style::new().fg(dim),

            border: Style::new().fg(dim),
            scrollbar: Style::new().fg(dim).bg(surface),
            selection: Style::new().bg(blue).fg(bg),
            logo: Style::new().fg(blue),

            agent_indicator: Style::new().fg(green).bg(surface),

            pty_output: Style::new().fg(fg),
            pty_error: Style::new().fg(red),
            pty_status_bar: Style::new().bg(surface2).fg(dim),
            pty_cursor_line: Style::new().bg(dim),

            toast_bg: Style::new().bg(surface2),
            toast_info: Style::new().fg(blue),
            toast_success: Style::new().fg(green),
            toast_warning: Style::new().fg(orange),
            toast_error: Style::new().fg(red),
            toast_border: Style::new().fg(dim),

            side_panel_bg: Style::new().bg(surface2),
            side_panel_title: Style::new().fg(blue),
            side_panel_tab_active: Style::new().fg(bg).bg(blue),
            side_panel_tab_inactive: Style::new().fg(dim),
            side_panel_border: Style::new().fg(dim),

            dialog_bg: Style::new().bg(surface2),
            dialog_border: Style::new().fg(blue),
            dialog_title: Style::new().fg(blue),
            dialog_button: Style::new().fg(fg),
            dialog_button_focused: Style::new().fg(bg).bg(blue),
        }
    }
}
