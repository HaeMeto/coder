//! Per-cell overlays drawn on top of the rendered editor text: diagnostic
//! squiggles, the selection background, and find / search-panel matches.
//! Every overlay only visits what the viewport shows.

use ratatui::Frame;
use ratatui::style::{Color, Modifier, Style};

use super::Viewport;
use super::diagnostics::severity_color;
use crate::app::model::{Diagnostic, Model};
use crate::core::buffer::{Buffer, Cursor};
use crate::core::theme::Theme;

/// Underlines each visible diagnostic's range with a severity-colored underline.
pub(super) fn overlay_diagnostics(
    frame: &mut Frame,
    vp: &Viewport,
    theme: &Theme,
    diags: &[Diagnostic],
) {
    let bufmut = frame.buffer_mut();
    for d in diags {
        // O(1) viewport check: off-screen lines are skipped without scanning.
        let Some(y) = vp.row_y(d.line) else {
            continue;
        };
        let style = Style::new()
            .add_modifier(Modifier::UNDERLINED)
            .underline_color(severity_color(theme, d.severity));
        vp.paint_cols(bufmut, y, d.col_start, d.col_end, style);
    }
}

/// Paints the selection background, visiting only the selected lines on screen.
pub(super) fn overlay_selection(
    frame: &mut Frame,
    vp: &Viewport,
    model: &Model,
    buf: &Buffer,
    start: Cursor,
    end: Cursor,
) {
    let Some((first, last)) = vp.visible_lines() else {
        return;
    };
    let style = Style::new().bg(model.theme.selection);
    let bufmut = frame.buffer_mut();
    for row in start.line.max(first)..=end.line.min(last) {
        let Some(y) = vp.row_y(row) else {
            continue;
        };
        let sel_start = if row == start.line { start.col } else { 0 };
        let sel_end = if row == end.line {
            end.col
        } else {
            buf.line_len(row)
        };
        vp.paint_cols(bufmut, y, sel_start, sel_end, style);
    }
}

/// Paints the background of every visible find match (yellow), with the active
/// match distinguished (blue). Only the matched cells' background changes.
/// `matches` are non-overlapping `[start, end)` char ranges sorted by start, so
/// the visible window is found by binary search rather than a full scan.
pub(super) fn overlay_find_matches(
    frame: &mut Frame,
    vp: &Viewport,
    model: &Model,
    buf: &Buffer,
    matches: &[(usize, usize)],
    current: Option<usize>,
) {
    let Some((first, last)) = vp.visible_lines() else {
        return;
    };
    let rope = &buf.rope;
    let len = rope.len_chars();
    let n_lines = rope.len_lines();
    if first >= n_lines {
        return;
    }
    // Visible char window [vis_start, vis_end).
    let vis_start = rope.line_to_char(first);
    let vis_end = if last + 1 < n_lines {
        rope.line_to_char(last + 1)
    } else {
        len
    };
    // First match starting at/after the window; step back one so a match that
    // starts above the viewport but runs into it is still painted (matches do
    // not overlap, so at most one can straddle the top edge).
    let from = matches
        .partition_point(|&(s, _)| s < vis_start)
        .saturating_sub(1);

    let bufmut = frame.buffer_mut();
    for (mi, &(s, e)) in matches.iter().enumerate().skip(from) {
        if s >= vis_end {
            break;
        }
        // Guard against ranges left over from a pre-edit buffer state
        // (char_to_line panics on an out-of-bounds index).
        if s >= e || s > len || e > len {
            continue;
        }
        // Active match: blue background with white text; others: yellow background.
        let style = if current == Some(mi) {
            Style::new().bg(model.theme.find_current).fg(Color::White)
        } else {
            Style::new().bg(model.theme.find_match)
        };
        // A match is a [start, end) char range; paint it line by line.
        let s_line = rope.char_to_line(s);
        let e_line = rope.char_to_line(e);
        for row in s_line.max(first)..=e_line.min(last) {
            let Some(y) = vp.row_y(row) else {
                continue;
            };
            let line_start = rope.line_to_char(row);
            let col_start = if row == s_line { s - line_start } else { 0 };
            let col_end = if row == e_line {
                (e - line_start).min(buf.line_len(row))
            } else {
                buf.line_len(row)
            };
            vp.paint_cols(bufmut, y, col_start, col_end, style);
        }
    }
}
