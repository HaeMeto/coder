//! Code editor: line-number gutter + syntax highlight + selection + cursor.

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use crate::app::model::{Diagnostic, DiffRow, Focus, Model};
use crate::core::buffer::Cursor;
use crate::core::theme::Theme;
use crate::services::git::GutterKind;
use crate::services::lsp::Severity;

pub fn render(frame: &mut Frame, area: Rect, model: &Model, gutter_w: u16) {
    frame.render_widget(
        Paragraph::new("").style(Style::new().bg(model.theme.bg)),
        area,
    );

    // A binary / unreadable file opens as a read-only tab: show the error
    // message centered in the editor area instead of an (empty) buffer.
    if let Some(notice) = model.active_notice() {
        render_notice(frame, area, model, notice);
        return;
    }

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
    let git_on = model.git_gutter();

    // Visual rows: real buffer lines, with removed lines woven in for diff tabs.
    let display = model.diff_rows();
    let disp_start = model.diff_start(&display, top);

    // Diagnostics for this file (empty for files with no language server).
    let diags: &[Diagnostic] = buf
        .path
        .as_ref()
        .and_then(|p| model.diagnostics.get(p))
        .map(|v| v.as_slice())
        .unwrap_or(&[]);
    // Most-severe diagnostic per line, for coloring the line number.
    let mut sev_by_line: std::collections::HashMap<usize, Severity> = std::collections::HashMap::new();
    for d in diags {
        sev_by_line
            .entry(d.line)
            .and_modify(|s| {
                if severity_rank(d.severity) < severity_rank(*s) {
                    *s = d.severity;
                }
            })
            .or_insert(d.severity);
    }

    let mut lines: Vec<Line> = Vec::with_capacity(height);
    for i in 0..height {
        match display.get(disp_start + i) {
            None => lines.push(Line::from("")),
            Some(DiffRow::Real(row)) => {
                let row = *row;
                let is_cursor_line = row == buf.cursor.line;
                // A diagnostic on this line recolors its line number by severity.
                let ln_style = if let Some(sev) = sev_by_line.get(&row) {
                    Style::new().fg(severity_color(&model.theme, *sev))
                } else if is_cursor_line {
                    Style::new().fg(model.theme.fg)
                } else {
                    Style::new().fg(model.theme.line_number)
                };
                let mut spans: Vec<Span> = Vec::new();
                // Git change marker column (leftmost), when the file is tracked.
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
            Some(DiffRow::Deleted(text)) => {
                lines.push(deleted_row(model, text, gutter_w, git_on, scroll_x, text_w));
            }
        }
    }

    let p = Paragraph::new(lines).style(Style::new().bg(model.theme.bg));
    frame.render_widget(p, area);

    // Overlay diagnostic squiggles, then the selection background per cell.
    if !diags.is_empty() {
        overlay_diagnostics(frame, area, buf, gutter_w, diags, &display, disp_start);
    }
    if let Some((start, end)) = selection {
        overlay_selection(frame, area, model, buf, gutter_w, start, end, &display, disp_start);
    }
    // Find matches paint over the selection so the active match's color wins.
    if model.find.open && !model.find.matches.is_empty() {
        overlay_find_matches(frame, area, model, buf, gutter_w, &display, disp_start);
    }

    // Draw our own block cursor (only when the editor is focused). A manual
    // block keeps the caret always white instead of the native terminal cursor,
    // which reverse-videos the cell and vanishes on gray comment text.
    if model.focus == Focus::Editor
        && let Some((cx, cy)) = cursor_screen_pos(model, area, gutter_w)
        && let Some(cell) = frame.buffer_mut().cell_mut((cx, cy))
    {
        cell.set_style(
            Style::new()
                .bg(Color::White)
                .fg(model.theme.bg),
        );
    }
}

/// Renders a read-only notice (binary / unreadable file) centered in the editor.
fn render_notice(frame: &mut Frame, area: Rect, model: &Model, notice: &str) {
    frame.render_widget(
        Paragraph::new("").style(Style::new().bg(model.theme.bg)),
        area,
    );
    if area.height == 0 || area.width < 4 {
        return;
    }
    // Wrap width the message is laid out in (used to estimate its height).
    let wrap_w = area.width.saturating_sub(4).min(70).max(1) as usize;
    let est_lines: u16 = notice
        .split('\n')
        .map(|para| {
            let len = para.chars().count();
            (len.div_ceil(wrap_w)).max(1) as u16
        })
        .sum();
    // Vertical centering: pad the top so the block sits in the middle.
    let top_pad = area.height.saturating_sub(est_lines) / 2;
    let mut lines: Vec<Line> = Vec::new();
    for _ in 0..top_pad {
        lines.push(Line::from(""));
    }
    for para in notice.split('\n') {
        lines.push(Line::from(Span::styled(
            para.to_string(),
            Style::new().fg(model.theme.fg_dim),
        )));
    }
    let p = Paragraph::new(lines)
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true })
        .style(Style::new().bg(model.theme.bg));
    frame.render_widget(p, area);
}

/// Screen cell of the active buffer's cursor within the editor area, or `None`
/// when it is scrolled off. Shared by the caret and the completion popup so they
/// never disagree (the same discipline as `compute_areas`).
pub fn cursor_screen_pos(model: &Model, area: Rect, gutter_w: u16) -> Option<(u16, u16)> {
    let buf = model.active_buffer()?;
    let display = model.diff_rows();
    let disp_start = model.diff_start(&display, buf.scroll_y);
    let cur_disp = real_display_index(&display, buf.cursor.line);
    if cur_disp < disp_start {
        return None;
    }
    let cy = area.y + (cur_disp - disp_start) as u16;
    let cx = area.x + gutter_w + (buf.cursor.col.saturating_sub(buf.scroll_x)) as u16;
    if cy >= area.y + area.height || cx >= area.x + area.width {
        return None;
    }
    Some((cx, cy))
}

/// Lower rank = more severe (Error wins over Warning wins over Info/Hint).
fn severity_rank(sev: Severity) -> u8 {
    match sev {
        Severity::Error => 0,
        Severity::Warning => 1,
        Severity::Info => 2,
        Severity::Hint => 3,
    }
}

/// The color used to mark a diagnostic of the given severity.
fn severity_color(th: &Theme, sev: Severity) -> Color {
    match sev {
        Severity::Error => th.git_deleted,
        Severity::Warning => th.git_modified,
        Severity::Info | Severity::Hint => th.accent,
    }
}

/// Underlines each diagnostic's range with a severity-colored underline.
#[allow(clippy::too_many_arguments)]
fn overlay_diagnostics(
    frame: &mut Frame,
    area: Rect,
    buf: &crate::core::buffer::Buffer,
    gutter_w: u16,
    diags: &[Diagnostic],
    display: &[DiffRow],
    disp_start: usize,
) {
    let scroll_x = buf.scroll_x;
    let text_w = area.width.saturating_sub(gutter_w);
    let theme_color = |sev| match sev {
        Severity::Error => Color::Red,
        Severity::Warning => Color::Yellow,
        Severity::Info | Severity::Hint => Color::Cyan,
    };
    let bufmut = frame.buffer_mut();
    for d in diags {
        let disp = real_display_index(display, d.line);
        if disp < disp_start || disp >= disp_start + area.height as usize {
            continue;
        }
        let y = area.y + (disp - disp_start) as u16;
        let color = theme_color(d.severity);
        for col in d.col_start..d.col_end {
            if col < scroll_x {
                continue;
            }
            let vis = (col - scroll_x) as u16;
            if vis >= text_w {
                break;
            }
            let x = area.x + gutter_w + vis;
            if let Some(cell) = bufmut.cell_mut((x, y)) {
                cell.set_style(
                    Style::new()
                        .add_modifier(Modifier::UNDERLINED)
                        .underline_color(color),
                );
            }
        }
    }
}

/// Display index of the row holding buffer line `line` (identity when there are
/// no woven deletions above it).
fn real_display_index(display: &[DiffRow], line: usize) -> usize {
    display
        .iter()
        .position(|r| matches!(r, DiffRow::Real(l) if *l == line))
        .unwrap_or(line)
}

/// A removed (red) diff row: blank line-number gutter, `-` change marker, and the
/// removed source text on the deletion background.
fn deleted_row(
    model: &Model,
    text: &str,
    gutter_w: u16,
    git_on: bool,
    scroll_x: usize,
    text_w: usize,
) -> Line<'static> {
    let th = &model.theme;
    let mut spans: Vec<Span> = Vec::new();
    if git_on {
        spans.push(Span::styled(
            (if model.ascii_icons { "-" } else { "▁" }).to_string(),
            Style::new().fg(th.git_deleted),
        ));
    }
    // Empty line-number column (the removed line has no number in the new file).
    let num_w = (gutter_w as usize).saturating_sub(if git_on { 2 } else { 1 });
    spans.push(Span::styled(format!("{:>num_w$} ", "-"), Style::new().fg(th.git_deleted)));
    // Removed text, clipped to the horizontal scroll window.
    let mut taken = 0usize;
    for (col, ch) in text.chars().enumerate() {
        if taken >= text_w {
            break;
        }
        if col >= scroll_x {
            let display = if ch == '\t' { ' ' } else { ch };
            spans.push(Span::styled(display.to_string(), Style::new().fg(th.fg)));
            taken += 1;
        }
    }
    Line::from(spans).style(Style::new().bg(th.diff_del_bg))
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

/// Paints the background of every find match (yellow), with the active match
/// distinguished (blue, blinking). Only the matched cells' background changes.
#[allow(clippy::too_many_arguments)]
fn overlay_find_matches(
    frame: &mut Frame,
    area: Rect,
    model: &Model,
    buf: &crate::core::buffer::Buffer,
    gutter_w: u16,
    display: &[DiffRow],
    disp_start: usize,
) {
    let scroll_x = buf.scroll_x;
    let text_w = area.width.saturating_sub(gutter_w);
    let current = model.find.current;
    let bufmut = frame.buffer_mut();

    for (mi, &(s, e)) in model.find.matches.iter().enumerate() {
        if s >= e {
            continue;
        }
        let is_current = current == Some(mi);
        // Active match: blue background with white text; others: yellow background.
        let style = if is_current {
            Style::new().bg(model.theme.find_current).fg(Color::White)
        } else {
            Style::new().bg(model.theme.find_match)
        };
        // A match is a [start, end) char range; paint it line by line.
        let s_line = buf.rope.char_to_line(s);
        let e_line = buf.rope.char_to_line(e);
        for row in s_line..=e_line {
            let disp = real_display_index(display, row);
            if disp < disp_start || disp >= disp_start + area.height as usize {
                continue;
            }
            let line_start = buf.rope.line_to_char(row);
            let line_len = buf.line_len(row);
            let col_start = if row == s_line { s - line_start } else { 0 };
            let col_end = if row == e_line { (e - line_start).min(line_len) } else { line_len };
            let y = area.y + (disp - disp_start) as u16;
            for col in col_start..col_end {
                if col < scroll_x {
                    continue;
                }
                let vis = (col - scroll_x) as u16;
                if vis >= text_w {
                    break;
                }
                let x = area.x + gutter_w + vis;
                if let Some(cell) = bufmut.cell_mut((x, y)) {
                    cell.set_style(style);
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn overlay_selection(
    frame: &mut Frame,
    area: Rect,
    model: &Model,
    buf: &crate::core::buffer::Buffer,
    gutter_w: u16,
    start: Cursor,
    end: Cursor,
    display: &[DiffRow],
    disp_start: usize,
) {
    let scroll_x = buf.scroll_x;
    let text_w = area.width.saturating_sub(gutter_w);
    let bufmut = frame.buffer_mut();

    for row in start.line..=end.line {
        // Map the buffer line to its on-screen row (accounts for woven deletions).
        let disp = real_display_index(display, row);
        if disp < disp_start || disp >= disp_start + area.height as usize {
            continue;
        }
        let line_len = buf.line_len(row);
        let sel_start = if row == start.line { start.col } else { 0 };
        let sel_end = if row == end.line { end.col } else { line_len };
        let y = area.y + (disp - disp_start) as u16;
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
