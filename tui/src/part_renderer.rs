use alloc::vec;
use alloc::vec::Vec;

use ratatui::style::Style;
use ratatui::text::Line;

use crate::theme::Theme;

/// Converts a single `Part` into styled ratatui `Line`s for rendering.
///
/// - `show_details`: when `false`, reasoning and tool parts render as a single collapsed line.
pub fn render_part<'a>(
    part: &'a ocpncord_backend::Part,
    theme: &'a Theme,
    show_details: bool,
) -> Vec<Line<'a>> {
    match part {
        ocpncord_backend::Part::Text(tp) => styled_lines(tp.text.as_str(), theme.part_text),
        ocpncord_backend::Part::Reasoning(rp) => {
            if show_details {
                styled_lines(rp.text.as_str(), theme.part_reasoning)
            } else {
                vec![Line::from("reasoning hidden").style(theme.text_dim)]
            }
        }
        ocpncord_backend::Part::Tool(tp) => {
            let (icon, style) = match &tp.state {
                ocpncord_backend::ToolState::Pending { .. } => ("...", theme.part_tool_idle),
                ocpncord_backend::ToolState::Running { .. } => (">>>", theme.part_tool_running),
                ocpncord_backend::ToolState::Completed { .. } => ("[ok]", theme.part_tool_done),
                ocpncord_backend::ToolState::Error { .. } => ("[!!]", theme.part_tool_error),
            };
            if show_details {
                let summary = match &tp.state {
                    ocpncord_backend::ToolState::Completed { output, .. } => {
                        alloc::format!("{icon} {} - done: {output}", tp.tool)
                    }
                    ocpncord_backend::ToolState::Error { error, .. } => {
                        alloc::format!("{icon} {} - error: {error}", tp.tool)
                    }
                    _ => alloc::format!("{icon} {}", tp.tool),
                };
                styled_lines_owned(summary, style)
            } else {
                vec![Line::from(alloc::format!("{icon} {}", tp.tool)).style(style)]
            }
        }
        ocpncord_backend::Part::StepStart(sp) => {
            let text = match &sp.snapshot {
                Some(s) => alloc::format!("--- step start - {s} ---"),
                None => "--- step start ---".into(),
            };
            vec![Line::from(text).style(theme.part_step_divider)]
        }
        ocpncord_backend::Part::StepFinish(fp) => {
            let text = match &fp.reason {
                Some(r) => alloc::format!("--- step finish - {r} ---"),
                None => "--- step finish ---".into(),
            };
            vec![Line::from(text).style(theme.part_step_divider)]
        }
        ocpncord_backend::Part::File(fp) => {
            let label = fp.filename.as_deref().unwrap_or(&fp.url);
            vec![Line::from(alloc::format!("[file] {label}")).style(theme.part_text)]
        }
        ocpncord_backend::Part::Snapshot(sp) => {
            vec![Line::from(alloc::format!("[snapshot] {}", sp.snapshot)).style(theme.text_dim)]
        }
        ocpncord_backend::Part::Patch(pp) => {
            vec![
                Line::from(alloc::format!("[patch] {} files", pp.files.len()))
                    .style(theme.text_dim),
            ]
        }
        ocpncord_backend::Part::Agent(ap) => {
            vec![Line::from(alloc::format!("[agent] {}", ap.name)).style(theme.text_dim)]
        }
        ocpncord_backend::Part::Subtask(st) => {
            vec![Line::from(alloc::format!("[subtask] {}", st.description)).style(theme.text_dim)]
        }
        ocpncord_backend::Part::Retry(rp) => {
            vec![Line::from(alloc::format!("[retry #{}]", rp.attempt)).style(theme.text_dim)]
        }
        ocpncord_backend::Part::Compaction(cp) => {
            let label = if cp.overflow == Some(true) {
                "compaction (overflow)"
            } else {
                "compaction"
            };
            vec![Line::from(label).style(theme.text_dim)]
        }
    }
}

fn styled_lines(text: &str, style: Style) -> Vec<Line<'_>> {
    let mut lines = Vec::new();
    for line in text.split('\n') {
        lines.push(Line::from(line).style(style));
    }
    if lines.is_empty() {
        lines.push(Line::from("").style(style));
    }
    lines
}

fn styled_lines_owned(text: alloc::string::String, style: Style) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for line in text.split('\n') {
        lines.push(Line::from(alloc::string::String::from(line)).style(style));
    }
    if lines.is_empty() {
        lines.push(Line::from("").style(style));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use ocpncord_backend::*;

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
    fn text_part_newlines_render_as_distinct_lines() {
        let theme = Theme::default();
        let part = Part::Text(TextPart {
            text: "hello\nworld".into(),
        });
        let lines = render_part(&part, &theme, true);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].to_string(), "hello");
        assert_eq!(lines[1].to_string(), "world");
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
            state: ToolState::Pending {
                input: alloc::collections::BTreeMap::new(),
                raw: "".into(),
            },
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
            state: ToolState::Running {
                input: alloc::collections::BTreeMap::new(),
                title: None,
                metadata: None,
                time: None,
            },
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
                input: alloc::collections::BTreeMap::new(),
                output: "found 3 matches".into(),
                title: "grep".into(),
                metadata: alloc::collections::BTreeMap::new(),
                time: ocpncord_backend::ToolTimeCompleted { start: 0, end: 1 },
                attachments: Vec::new(),
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
                input: alloc::collections::BTreeMap::new(),
                error: "timeout".into(),
                metadata: None,
                time: ocpncord_backend::ToolTimeCompleted { start: 0, end: 1 },
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
