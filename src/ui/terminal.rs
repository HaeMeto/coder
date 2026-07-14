//! Draws the embedded PTY terminal from the vt100 screen into ratatui cells.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders};

use crate::app::model::{Focus, Model};

pub fn render(frame: &mut Frame, area: Rect, model: &Model) {
    let focused = model.focus == Focus::Terminal;
    let border_color = if focused {
        model.theme.accent
    } else {
        model.theme.border
    };
    let block = Block::new()
        .borders(Borders::TOP)
        .border_style(Style::new().fg(border_color))
        .title(" TERMINAL ")
        .title_style(Style::new().fg(model.theme.fg_dim))
        .style(Style::new().bg(Color::Rgb(20, 20, 20)));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let screen = model.terminal.parser.screen();
    let (rows, cols) = screen.size();
    let buf = frame.buffer_mut();

    for r in 0..inner.height.min(rows) {
        for c in 0..inner.width.min(cols) {
            let x = inner.x + c;
            let y = inner.y + r;
            let Some(cell) = screen.cell(r, c) else { continue };
            let Some(out) = buf.cell_mut((x, y)) else { continue };
            let contents = cell.contents();
            if contents.is_empty() {
                out.set_char(' ');
            } else {
                out.set_symbol(&contents);
            }
            let mut style = Style::new()
                .fg(conv_color(cell.fgcolor(), model.theme.fg))
                .bg(conv_color(cell.bgcolor(), Color::Rgb(20, 20, 20)));
            if cell.bold() {
                style = style.add_modifier(Modifier::BOLD);
            }
            if cell.italic() {
                style = style.add_modifier(Modifier::ITALIC);
            }
            if cell.underline() {
                style = style.add_modifier(Modifier::UNDERLINED);
            }
            if cell.inverse() {
                style = style.add_modifier(Modifier::REVERSED);
            }
            out.set_style(style);
        }
    }

    // Cursor.
    if focused && !screen.hide_cursor() {
        let (cr, cc) = screen.cursor_position();
        let x = inner.x + cc;
        let y = inner.y + cr;
        if x < inner.x + inner.width && y < inner.y + inner.height {
            frame.set_cursor_position((x, y));
        }
    }
}

fn conv_color(c: vt100::Color, default: Color) -> Color {
    match c {
        vt100::Color::Default => default,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}
