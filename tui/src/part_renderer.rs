use alloc::vec;
use alloc::vec::Vec;

use ratatui_core::text::Line;

use crate::theme::Theme;

/// Converts a single `Part` into styled ratatui `Line`s for rendering.
///
/// - `show_details`: when `false`, reasoning and tool parts render as a single collapsed line.
pub fn render_part<'a>(
    part: &'a opencode_backend::Part,
    theme: &'a Theme,
    show_details: bool,
) -> Vec<Line<'a>> {
    match part {
        opencode_backend::Part::Text(tp) => {
            vec![Line::from(tp.text.as_str()).style(theme.part_text)]
        }
        opencode_backend::Part::Reasoning(rp) => {
            if show_details {
                vec![Line::from(rp.text.as_str()).style(theme.part_reasoning)]
            } else {
                vec![Line::from("reasoning hidden").style(theme.text_dim)]
            }
        }
        opencode_backend::Part::Tool(tp) => {
            let (icon, style) = match &tp.state {
                opencode_backend::ToolState::Pending => ("...", theme.part_tool_idle),
                opencode_backend::ToolState::Running => (">>>", theme.part_tool_running),
                opencode_backend::ToolState::Completed { .. } => ("[ok]", theme.part_tool_done),
                opencode_backend::ToolState::Error { .. } => ("[!!]", theme.part_tool_error),
            };
            if show_details {
                let summary = match &tp.state {
                    opencode_backend::ToolState::Completed { output } => {
                        alloc::format!("{icon} {} - done: {output}", tp.tool)
                    }
                    opencode_backend::ToolState::Error { error } => {
                        alloc::format!("{icon} {} - error: {error}", tp.tool)
                    }
                    _ => alloc::format!("{icon} {}", tp.tool),
                };
                vec![Line::from(summary).style(style)]
            } else {
                vec![Line::from(alloc::format!("{icon} {}", tp.tool)).style(style)]
            }
        }
        opencode_backend::Part::StepStart(sp) => {
            let text = match &sp.snapshot {
                Some(s) => alloc::format!("--- step start - {s} ---"),
                None => "--- step start ---".into(),
            };
            vec![Line::from(text).style(theme.part_step_divider)]
        }
        opencode_backend::Part::StepFinish(fp) => {
            let text = match &fp.reason {
                Some(r) => alloc::format!("--- step finish - {r} ---"),
                None => "--- step finish ---".into(),
            };
            vec![Line::from(text).style(theme.part_step_divider)]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opencode_backend::*;

    #[test]
    fn text_part_renders_with_part_text_style() {
        let theme = Theme::default();
        let part = Part::Text(TextPart {
            text: "hello world".into(),
        });
        let lines = render_part(&part, &theme, true);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].to_string(), "hello world");
    }

    #[test]
    fn reasoning_part_shows_full_when_details_on() {
        let theme = Theme::default();
        let part = Part::Reasoning(ReasoningPart {
            text: "thinking...".into(),
        });
        let lines = render_part(&part, &theme, true);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].to_string(), "thinking...");
    }

    #[test]
    fn reasoning_part_collapsed_when_details_off() {
        let theme = Theme::default();
        let part = Part::Reasoning(ReasoningPart {
            text: "thinking...".into(),
        });
        let lines = render_part(&part, &theme, false);
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn tool_pending_shows_idle_style() {
        let theme = Theme::default();
        let part = Part::Tool(ToolPart {
            tool: "read".into(),
            state: ToolState::Pending,
        });
        let lines = render_part(&part, &theme, true);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].to_string().contains("read"));
    }

    #[test]
    fn tool_running_shows_running_style() {
        let theme = Theme::default();
        let part = Part::Tool(ToolPart {
            tool: "write".into(),
            state: ToolState::Running,
        });
        let lines = render_part(&part, &theme, true);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].to_string().contains("write"));
    }

    #[test]
    fn tool_completed_shows_output() {
        let theme = Theme::default();
        let part = Part::Tool(ToolPart {
            tool: "grep".into(),
            state: ToolState::Completed {
                output: "found 3 matches".into(),
            },
        });
        let lines = render_part(&part, &theme, true);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].to_string().contains("found 3 matches"));
    }

    #[test]
    fn tool_error_shows_error_message() {
        let theme = Theme::default();
        let part = Part::Tool(ToolPart {
            tool: "curl".into(),
            state: ToolState::Error {
                error: "timeout".into(),
            },
        });
        let lines = render_part(&part, &theme, true);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].to_string().contains("timeout"));
    }

    #[test]
    fn step_start_renders_divider() {
        let theme = Theme::default();
        let part = Part::StepStart(StepStartPart {
            snapshot: None,
            session_id: None,
        });
        let lines = render_part(&part, &theme, true);
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn step_start_with_snapshot() {
        let theme = Theme::default();
        let part = Part::StepStart(StepStartPart {
            snapshot: Some("planning".into()),
            session_id: None,
        });
        let lines = render_part(&part, &theme, true);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].to_string().contains("planning"));
    }

    #[test]
    fn step_finish_with_reason() {
        let theme = Theme::default();
        let part = Part::StepFinish(StepFinishPart {
            reason: Some("done".into()),
            snapshot: None,
            session_id: None,
        });
        let lines = render_part(&part, &theme, true);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].to_string().contains("done"));
    }

    #[test]
    fn step_finish_without_reason() {
        let theme = Theme::default();
        let part = Part::StepFinish(StepFinishPart {
            reason: None,
            snapshot: None,
            session_id: None,
        });
        let lines = render_part(&part, &theme, true);
        assert_eq!(lines.len(), 1);
    }
}
