//! File-tree panel.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::model::Model;

use super::{list_scroll, panel_area};

pub(super) fn render(frame: &mut Frame, area: Rect, model: &Model) {
    let rows = model.sidebar.files.visible_rows();
    let height = area.height as usize;
    let offset = list_scroll(model.sidebar.files.selected, rows.len(), height);

    // Path of the file open in the active tab, to mark its row in the tree.
    let active_path = model.active_buffer().and_then(|b| b.path.clone());

    let mut lines: Vec<Line> = Vec::new();
    for (i, row) in rows.iter().enumerate().skip(offset).take(height) {
        let selected = i == model.sidebar.files.selected;
        let is_active = !row.is_dir && active_path.as_deref() == Some(row.path.as_path());
        let indent = "  ".repeat(row.depth);
        let icon = if row.is_dir {
            if row.expanded { "▾ " } else { "▸ " }
        } else {
            "  "
        };
        let name_style = if row.is_dir {
            Style::new().fg(model.theme.fg)
        } else {
            Style::new().fg(model.theme.fg_dim)
        };
        // Selected row and the open file both get the darkened selection bg.
        let line_style = if selected {
            Style::new().bg(model.theme.selected_bg())
        } else if is_active {
            Style::new().fg(model.theme.fg).bg(model.theme.selected_bg())
        } else {
            Style::new().bg(model.theme.bg_alt)
        };
        lines.push(
            Line::from(vec![
                Span::raw(indent),
                Span::styled(icon, Style::new().fg(model.theme.fg_dim)),
                Span::styled(row.name.clone(), name_style),
            ])
            .style(line_style),
        );
    }
    let p = Paragraph::new(lines).style(Style::new().bg(model.theme.bg_alt));
    frame.render_widget(p, area);
}

/// Returns the visible row index in the file tree based on the mouse y.
pub fn file_row_at(model: &Model, area: Rect, y: u16) -> Option<usize> {
    let body = panel_area(area);
    if y < body.y || y >= body.y + body.height {
        return None;
    }
    let rows_len = model.sidebar.files.visible_rows().len();
    let offset = list_scroll(model.sidebar.files.selected, rows_len, body.height as usize);
    let idx = offset + (y - body.y) as usize;
    if idx < rows_len { Some(idx) } else { None }
}
