//! Extensions panel (placeholder).

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::model::Model;

pub(super) fn render(frame: &mut Frame, area: Rect, model: &Model) {
    let lines = vec![
        Line::from(Span::styled(
            " Extensions (coming soon)",
            Style::new().fg(model.theme.fg_dim),
        )),
        Line::from(""),
        Line::from(Span::styled(
            " Vim shortcuts, LSP, and the",
            Style::new().fg(model.theme.fg_dim),
        )),
        Line::from(Span::styled(
            " plugin system will land here.",
            Style::new().fg(model.theme.fg_dim),
        )),
    ];
    let p = Paragraph::new(lines).style(Style::new().bg(model.theme.bg_alt));
    frame.render_widget(p, area);
}
