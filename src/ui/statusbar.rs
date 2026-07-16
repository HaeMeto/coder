//! Bottom status bar.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::model::Model;

pub fn render(frame: &mut Frame, area: Rect, model: &Model) {
    let base = Style::new()
        .fg(model.theme.statusbar_fg)
        .bg(model.theme.statusbar_bg);

    let mut left = String::new();
    if let Some(branch) = &model.sidebar.git.branch {
        left.push_str(&format!(" ⎇ {branch} "));
    }
    // A diagnostic under the cursor takes over the message area.
    if let Some(diag) = model.diagnostic_at_cursor() {
        let tag = match diag.severity {
            crate::services::lsp::Severity::Error => "error",
            crate::services::lsp::Severity::Warning => "warning",
            crate::services::lsp::Severity::Info => "info",
            crate::services::lsp::Severity::Hint => "hint",
        };
        left.push_str(&format!(" {tag}: {} ", diag.message.replace('\n', " ")));
    } else {
        left.push_str(&format!(" {} ", model.status_message));
    }

    let mut right = String::new();
    if let Some(buf) = model.active_buffer() {
        right.push_str(&format!(
            "Ln {}, Col {}  ",
            buf.cursor.line + 1,
            buf.cursor.col + 1
        ));
        if buf.dirty {
            right.push_str("● ");
        }
        if let Some(text) = buf.selected_text() {
            right.push_str(&format!("Selected: {}  ", text.chars().count()));
        }
    }
    let focus = match model.focus {
        crate::app::model::Focus::Editor => "EDITOR",
        crate::app::model::Focus::Terminal => "TERMINAL",
        crate::app::model::Focus::Sidebar => "SIDEBAR",
        crate::app::model::Focus::SearchInput => "SEARCH",
        crate::app::model::Focus::GitCommit => "COMMIT",
        crate::app::model::Focus::Find => "FIND",
    };
    right.push_str(focus);
    right.push(' ');

    let total = area.width as usize;
    let lw = left.chars().count();
    let rw = right.chars().count();
    let pad = total.saturating_sub(lw + rw);
    let line = Line::from(vec![
        Span::styled(left, base),
        Span::styled(" ".repeat(pad), base),
        Span::styled(right, base),
    ]);
    frame.render_widget(Paragraph::new(line).style(base), area);
}
