//! Open file tabs.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use unicode_width::UnicodeWidthStr;

use crate::app::model::{Model, Tab};

/// Rows of the tab bar: the workspace-relative parent directory (+ ✕) on
/// top, the file name (+ dirty dot) below.
pub const TAB_BAR_HEIGHT: u16 = 2;

/// Longest parent label drawn before it is trimmed from the front.
const MAX_PARENT: usize = 24;

/// The dim second-row label: the file's parent directory relative to the
/// workspace root, with a trailing `/` (`src/ui/`). Empty for a file at the
/// root and for tabs without a real file (untitled, commit diff).
fn parent_label(model: &Model, tab: &Tab) -> String {
    if tab.untitled_id.is_some() || tab.read_only {
        return String::new();
    }
    let Some(parent) = tab.buffer.path.as_deref().and_then(|p| p.parent()) else {
        return String::new();
    };
    let rel = parent.strip_prefix(&model.root).unwrap_or(parent);
    let rel = rel.to_string_lossy();
    if rel.is_empty() {
        return String::new();
    }
    let label = format!("{rel}/");
    let len = label.chars().count();
    if len <= MAX_PARENT {
        label
    } else {
        let tail: String = label.chars().skip(len - (MAX_PARENT - 1)).collect();
        format!("…{tail}")
    }
}

/// Width of a tab's body (both rows), without the `│` separator. The top row
/// is " {parent} " then "✕ " pinned to the right; the bottom row is
/// " {title} {dirty} ". Measured in terminal cells (`UnicodeWidthStr`), the
/// same unit ratatui lays spans out in, so the hit-test in `tab_at` agrees with
/// what is drawn even for wide (CJK / emoji) names. `usize` so a long tab row
/// can never overflow.
fn body_width(model: &Model, tab: &Tab) -> usize {
    let top = parent_label(model, tab).width() + 4;
    let bottom = tab.title().width() + 4;
    top.max(bottom).max(6)
}

/// `s` right-padded with spaces to `w` terminal cells (`format!`'s `{:<w$}`
/// pads by chars, which disagrees with cell width for wide characters).
fn pad_cells(s: &str, w: usize) -> String {
    let pad = w.saturating_sub(s.width());
    format!("{s}{}", " ".repeat(pad))
}

pub fn render(frame: &mut Frame, area: Rect, model: &Model) {
    let th = &model.theme;
    let mut top: Vec<Span> = Vec::new();
    let mut bottom: Vec<Span> = Vec::new();
    if model.tabs.is_empty() {
        top.push(Span::styled(
            " No file open ",
            Style::new().fg(th.fg_dim).bg(th.tab_inactive_bg),
        ));
    }
    for (i, tab) in model.tabs.iter().enumerate() {
        let active = Some(i) == model.active_tab;
        let bg = if active {
            th.tab_active_bg
        } else {
            th.tab_inactive_bg
        };
        let w = body_width(model, tab);

        // Top row: dim parent dir, then "✕ " pinned to the right.
        let parent = format!(" {}", parent_label(model, tab));
        top.push(Span::styled(
            pad_cells(&parent, w - 2),
            Style::new().fg(th.fg_dim).bg(bg),
        ));
        top.push(Span::styled("✕ ", Style::new().fg(th.fg_dim).bg(bg)));
        top.push(Span::styled("│", Style::new().fg(th.border).bg(bg)));

        // Bottom row: file name in full color on every tab (the background
        // marks the active one), bold when active.
        let dirty = if tab.buffer.dirty { "●" } else { " " };
        let mut style = Style::new().fg(th.fg).bg(bg);
        if active {
            style = style.add_modifier(Modifier::BOLD);
        }
        // A preview tab (arrow-key browsing) is italic, like VSCode.
        if tab.preview {
            style = style.add_modifier(Modifier::ITALIC);
        }
        let name = format!(" {} {} ", tab.title(), dirty);
        bottom.push(Span::styled(pad_cells(&name, w), style));
        bottom.push(Span::styled("│", Style::new().fg(th.border).bg(bg)));
    }
    let p = Paragraph::new(vec![Line::from(top), Line::from(bottom)])
        .style(Style::new().bg(th.tab_inactive_bg));
    frame.render_widget(p, area);
}

/// Target of a click on the tab bar.
pub enum TabHit {
    /// Tab body — activate.
    Select(usize),
    /// Close button (✕) — close the tab.
    Close(usize),
}

/// Returns which tab / close button was clicked at (x, y). The ✕ is only on
/// the top row; anywhere else on a tab selects it.
pub fn tab_at(model: &Model, area: Rect, x: u16, y: u16) -> Option<TabHit> {
    // Summed in `usize`: many / long tabs can run past `u16::MAX` cells.
    let x = x as usize;
    let mut cursor = area.x as usize;
    for (i, tab) in model.tabs.iter().enumerate() {
        let w = body_width(model, tab);
        let total = w + 1; // body + separator "│"
        if x >= cursor && x < cursor + total {
            // "✕ " fills the last two body cells: a wide target for the mouse.
            let close_x = cursor + w - 2;
            if y == area.y && (x == close_x || x == close_x + 1) {
                return Some(TabHit::Close(i));
            }
            return Some(TabHit::Select(i));
        }
        cursor += total;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::buffer::Buffer;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn tab_shows_parent_dir_on_top_and_name_below() {
        let root = std::path::PathBuf::from("/w");
        let mut model = Model::new(root.clone());
        model.root = root.clone();
        let path = root.join("src/ui/mod.rs");
        model.tabs.push(Tab::new(Buffer::new(Some(path), "")));
        model.active_tab = Some(0);

        let mut term = Terminal::new(TestBackend::new(30, 2)).unwrap();
        term.draw(|f| render(f, f.area(), &model)).unwrap();
        let buf = term.backend().buffer();
        let row = |y: u16| {
            (0..30)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
        };
        assert!(row(0).starts_with(" src/ui/ ✕ │"), "{:?}", row(0));
        assert!(row(1).starts_with(" mod.rs    │"), "{:?}", row(1));

        let area = Rect::new(0, 0, 30, 2);
        let close_x = row(0).chars().position(|c| c == '✕').unwrap() as u16;
        assert!(matches!(
            tab_at(&model, area, close_x, 0),
            Some(TabHit::Close(0))
        ));
        assert!(matches!(
            tab_at(&model, area, close_x, 1),
            Some(TabHit::Select(0))
        ));
    }
}
