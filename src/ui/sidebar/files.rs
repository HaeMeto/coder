//! File-tree panel.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::model::Model;

use super::{list_scroll, panel_area};

/// Columns reserved at the right edge of a directory row for the
/// "new file" / "new folder" buttons: `[icon][space][icon][space]`.
const ACTION_COLS: usize = 4;
/// A row narrower than this has no room for the action icons.
const MIN_ACTION_WIDTH: usize = 8;

/// What a click in the file tree landed on.
pub enum FileHit {
    /// A tree row (open the file / toggle the directory).
    Row(usize),
    /// The "new file" button on the directory row at this index.
    NewFile(usize),
    /// The "new folder" button on the directory row at this index.
    NewFolder(usize),
}

/// (new file, new folder) button glyphs — codicons, or ASCII when `CODER_ASCII` is set.
fn action_icons(model: &Model) -> (&'static str, &'static str) {
    if model.ascii_icons {
        ("f", "d")
    } else {
        ("\u{ea7f}", "\u{ea80}") // new-file, new-folder
    }
}

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
        let mut spans = vec![
            Span::raw(indent.clone()),
            Span::styled(icon, Style::new().fg(model.theme.fg_dim)),
        ];
        // Directories get "new file" / "new folder" buttons pinned to the right edge.
        let width = area.width as usize;
        if row.is_dir && width >= MIN_ACTION_WIDTH {
            let avail = width.saturating_sub(indent.len() + 2 + ACTION_COLS);
            let (new_file, new_folder) = action_icons(model);
            spans.push(Span::styled(
                format!("{:<avail$}", fit_name(&row.name, avail)),
                name_style,
            ));
            spans.push(Span::styled(new_file, Style::new().fg(model.theme.fg_dim)));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(new_folder, Style::new().fg(model.theme.fg_dim)));
            spans.push(Span::raw(" "));
        } else {
            spans.push(Span::styled(row.name.clone(), name_style));
        }
        lines.push(Line::from(spans).style(line_style));
    }
    let p = Paragraph::new(lines).style(Style::new().bg(model.theme.bg_alt));
    frame.render_widget(p, area);
}

/// Truncates a name to `max` columns, marking the cut with `…`.
fn fit_name(name: &str, max: usize) -> String {
    let len = name.chars().count();
    if len <= max {
        return name.to_string();
    }
    if max <= 1 {
        return "…".to_string();
    }
    let head: String = name.chars().take(max - 1).collect();
    format!("{head}…")
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

/// Converts a click into a file-tree target. Mirrors `render`: on a directory row
/// the last 4 columns are the new-file (width-4) and new-folder (width-2) buttons.
pub fn file_hit(model: &Model, area: Rect, x: u16, y: u16) -> Option<FileHit> {
    let idx = file_row_at(model, area, y)?;
    let body = panel_area(area);
    let rows = model.sidebar.files.visible_rows();
    let row = rows.get(idx)?;
    let width = body.width as usize;
    let col = x.saturating_sub(body.x) as usize;
    if row.is_dir && width >= MIN_ACTION_WIDTH {
        if col >= width - 2 {
            return Some(FileHit::NewFolder(idx));
        }
        if col >= width - ACTION_COLS {
            return Some(FileHit::NewFile(idx));
        }
    }
    Some(FileHit::Row(idx))
}
