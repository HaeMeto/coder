//! Code editor: line-number gutter + syntax highlight + selection + cursor.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::model::{Focus, Model};
use crate::core::buffer::Cursor;
use crate::services::git::GutterKind;

pub fn render(frame: &mut Frame, area: Rect, model: &Model, gutter_w: u16) {
    frame.render_widget(
        Paragraph::new("").style(Style::new().bg(model.theme.bg)),
        area,
    );

    let Some(buf) = model.active_buffer() else {
        let hint = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  Select a file from the tree on the left to open it (Enter).",
                Style::new().fg(model.theme.fg_dim),
            )),
        ])
        .style(Style::new().bg(model.theme.bg));
        frame.render_widget(hint, area);
        return;
    };

    let height = area.height as usize;
    let text_w = area.width.saturating_sub(gutter_w) as usize;
    let top = buf.scroll_y;
    let scroll_x = buf.scroll_x;

    let selection = buf.selection_range();
    // Changed-line backgrounds are only drawn for diff-mode tabs (opened from Git).
    let diff_bg = model.active_is_diff();

    let mut lines: Vec<Line> = Vec::with_capacity(height);
    for row in top..top + height {
        if row >= buf.line_count() {
            lines.push(Line::from(""));
            continue;
        }
        let is_cursor_line = row == buf.cursor.line;
        let ln_style = if is_cursor_line {
            Style::new().fg(model.theme.fg)
        } else {
            Style::new().fg(model.theme.line_number)
        };
        let mut spans: Vec<Span> = Vec::new();
        // Git change marker column (leftmost), when the file is tracked.
        let git_on = model.git_gutter();
        let mark = if git_on { model.active_git_marks.get(&row).copied() } else { None };
        if git_on {
            let (ch, color) = match mark {
                Some(GutterKind::Added) => (if model.ascii_icons { "|" } else { "▍" }, model.theme.git_added),
                Some(GutterKind::Deleted) => (if model.ascii_icons { "_" } else { "▁" }, model.theme.git_deleted),
                None => (" ", model.theme.bg),
            };
            spans.push(Span::styled(ch.to_string(), Style::new().fg(color)));
        }
        let num_w = (gutter_w as usize).saturating_sub(if git_on { 2 } else { 1 });
        let gutter = format!("{:>num_w$} ", row + 1);
        spans.push(Span::styled(gutter, ln_style));

        // Highlighted text pieces (clipped by scroll_x).
        let hl_line = model.active_hl.get(row);
        append_text_spans(&mut spans, hl_line, buf, row, scroll_x, text_w, model);

        let mut line = Line::from(spans);
        // Only diff-mode changed lines get a background; the cursor line is not filled.
        let bg = match mark {
            Some(GutterKind::Added) if diff_bg => Some(model.theme.diff_add_bg),
            Some(GutterKind::Deleted) if diff_bg => Some(model.theme.diff_del_bg),
            _ => None,
        };
        if let Some(bg) = bg {
            line = line.style(Style::new().bg(bg));
        }
        lines.push(line);
    }

    let p = Paragraph::new(lines).style(Style::new().bg(model.theme.bg));
    frame.render_widget(p, area);

    // Overlay the selection background per cell.
    if let Some((start, end)) = selection {
        overlay_selection(frame, area, model, buf, gutter_w, start, end);
    }

    // Position the cursor (only when the editor is focused).
    if model.focus == Focus::Editor {
        let cy = area.y + (buf.cursor.line.saturating_sub(top)) as u16;
        let cx = area.x + gutter_w + (buf.cursor.col.saturating_sub(scroll_x)) as u16;
        if cy < area.y + area.height && cx < area.x + area.width {
            frame.set_cursor_position((cx, cy));
        }
    }
}

/// Appends highlight pieces to spans within the scroll_x/width window.
fn append_text_spans(
    spans: &mut Vec<Span<'static>>,
    hl_line: Option<&crate::core::highlight::HlLine>,
    buf: &crate::core::buffer::Buffer,
    row: usize,
    scroll_x: usize,
    width: usize,
    model: &Model,
) {
    if width == 0 {
        return;
    }
    let mut col = 0usize; // source character column
    let mut taken = 0usize; // visible column

    fn push_piece(
        spans: &mut Vec<Span<'static>>,
        text: &str,
        color: ratatui::style::Color,
        scroll_x: usize,
        width: usize,
        col: &mut usize,
        taken: &mut usize,
    ) {
        for ch in text.chars() {
            if *taken >= width {
                break;
            }
            if *col >= scroll_x {
                let display = if ch == '\t' { ' ' } else { ch };
                spans.push(Span::styled(display.to_string(), Style::new().fg(color)));
                *taken += 1;
            }
            *col += 1;
        }
    }

    match hl_line {
        Some(pieces) if !pieces.is_empty() => {
            for (color, text) in pieces {
                if taken >= width {
                    break;
                }
                push_piece(spans, text, *color, scroll_x, width, &mut col, &mut taken);
            }
        }
        _ => {
            // Plain text when there is no highlight.
            let text = buf.line_text(row);
            push_piece(spans, &text, model.theme.fg, scroll_x, width, &mut col, &mut taken);
        }
    }
}

/// Renders the editor scrollbar (rightmost column): a draggable thumb plus git
/// change marks (green line = addition, red line = deletion) at proportional rows.
pub fn render_scrollbar(frame: &mut Frame, area: Rect, model: &Model) {
    let th = &model.theme;
    let h = area.height as usize;
    if h == 0 {
        return;
    }
    let Some(buf) = model.active_buffer() else {
        frame.render_widget(
            Paragraph::new("").style(Style::new().bg(th.bg_alt)),
            area,
        );
        return;
    };
    let n = buf.line_count().max(1);

    // Thumb: the currently visible portion of the file.
    let (thumb_start, thumb_end) = if n > h {
        let ts = buf.scroll_y * h / n;
        let tl = (h * h / n).max(1);
        (ts, (ts + tl).min(h))
    } else {
        (0, h)
    };

    // Project git change marks onto scrollbar rows.
    let mut mark_rows: std::collections::HashMap<usize, GutterKind> =
        std::collections::HashMap::new();
    if model.git_gutter() {
        for (&ln, &kind) in &model.active_git_marks {
            let row = (ln * h / n).min(h - 1);
            // Deletions win over additions on a shared row so removals stay visible.
            mark_rows
                .entry(row)
                .and_modify(|k| {
                    if kind == GutterKind::Deleted {
                        *k = kind;
                    }
                })
                .or_insert(kind);
        }
    }

    let dash = if model.ascii_icons { "-" } else { "─" };
    let mut lines: Vec<Line> = Vec::with_capacity(h);
    for y in 0..h {
        let in_thumb = y >= thumb_start && y < thumb_end;
        let track_bg = if in_thumb { th.fg_dim } else { th.bg_alt };
        let span = match mark_rows.get(&y) {
            Some(GutterKind::Added) => {
                Span::styled(dash, Style::new().fg(th.git_added).bg(track_bg))
            }
            Some(GutterKind::Deleted) => {
                Span::styled(dash, Style::new().fg(th.git_deleted).bg(track_bg))
            }
            None => Span::styled(" ", Style::new().bg(track_bg)),
        };
        lines.push(Line::from(span));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn overlay_selection(
    frame: &mut Frame,
    area: Rect,
    model: &Model,
    buf: &crate::core::buffer::Buffer,
    gutter_w: u16,
    start: Cursor,
    end: Cursor,
) {
    let top = buf.scroll_y;
    let scroll_x = buf.scroll_x;
    let text_w = area.width.saturating_sub(gutter_w);
    let bufmut = frame.buffer_mut();

    for row in start.line..=end.line {
        if row < top || row >= top + area.height as usize {
            continue;
        }
        let line_len = buf.line_len(row);
        let sel_start = if row == start.line { start.col } else { 0 };
        let sel_end = if row == end.line { end.col } else { line_len };
        let y = area.y + (row - top) as u16;
        for col in sel_start..sel_end {
            if col < scroll_x {
                continue;
            }
            let vis = (col - scroll_x) as u16;
            if vis >= text_w {
                break;
            }
            let x = area.x + gutter_w + vis;
            if let Some(cell) = bufmut.cell_mut((x, y)) {
                cell.set_style(Style::new().bg(model.theme.selection));
            }
        }
    }
}
