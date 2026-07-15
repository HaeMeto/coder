//! Settings panel.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::model::{Focus, Model, SettingsState};

use super::panel_area;

pub(super) fn render(frame: &mut Frame, area: Rect, model: &Model) {
    let s = &model.sidebar.settings;
    let th = &model.theme;

    let mut lines: Vec<Line> = Vec::new();
    for i in 0..SettingsState::COUNT {
        let selected = i == s.selected;
        let checked = s.value(i);
        let checkbox = if checked { "[x]" } else { "[ ]" };
        let box_style = Style::new().fg(if checked { th.accent } else { th.fg_dim });
        let name_style = if selected {
            Style::new().fg(th.fg)
        } else {
            Style::new().fg(th.fg_dim)
        };
        let line_style = if selected && model.focus == Focus::Sidebar {
            Style::new().bg(th.selection)
        } else {
            Style::new().bg(th.bg_alt)
        };
        lines.push(
            Line::from(vec![
                Span::styled(format!(" {checkbox} "), box_style),
                Span::styled(SettingsState::label(i).to_string(), name_style),
            ])
            .style(line_style),
        );
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " ⏎/click toggles · applied on save",
        Style::new().fg(th.fg_dim),
    )));

    let p = Paragraph::new(lines).style(Style::new().bg(th.bg_alt));
    frame.render_widget(p, area);
}

/// Returns the settings row index for a mouse y. `area` is the full sidebar.
pub fn settings_row_at(area: Rect, y: u16) -> Option<usize> {
    let body = panel_area(area);
    if y < body.y {
        return None;
    }
    let idx = (y - body.y) as usize;
    if idx < SettingsState::COUNT {
        Some(idx)
    } else {
        None
    }
}
