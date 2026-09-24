//! Non-code rows of a commit's diff view (header, file headings, hunk gaps).

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::display_char;
use crate::app::model::Model;
use crate::services::git::CommitRow;

/// A non-code row of a commit's diff view: the author/date/message header, a
/// file heading, or the gap between two hunks. None of them belong to a file, so
/// the gutter stays blank and the text is styled rather than syntax-highlighted.
/// A file heading is drawn as a band across the full editor width, like the tab
/// bar, so the files a commit touched are easy to pick out while scrolling.
pub(super) fn commit_row_line(
    model: &Model,
    kind: CommitRow,
    text: &str,
    gutter_w: u16,
    width: usize,
) -> Line<'static> {
    let th = &model.theme;
    let gutter = " ".repeat(gutter_w as usize);
    let text_w = width.saturating_sub(gutter_w as usize);
    if kind == CommitRow::File {
        return file_heading_line(model, text, gutter, text_w);
    }
    let style = match kind {
        CommitRow::Author => Style::new().fg(th.fg).add_modifier(Modifier::BOLD),
        CommitRow::Message => Style::new().fg(th.fg),
        _ => Style::new().fg(th.fg_dim),
    };
    let body: String = text.chars().take(text_w).map(display_char).collect();
    Line::from(vec![Span::raw(gutter), Span::styled(body, style)])
}

/// The file heading band ("model.rs  src/app/  +3 -1"): name and directory in the
/// accent color, with the file's line counts in the diff's own green and red.
fn file_heading_line(model: &Model, text: &str, gutter: String, text_w: usize) -> Line<'static> {
    let th = &model.theme;
    let name_style = Style::new().fg(th.accent).add_modifier(Modifier::BOLD);
    let mut spans = vec![Span::raw(gutter)];
    match split_stats(text) {
        // The counts trail the name, so they only need their own colors when the
        // whole heading fits — otherwise it is clipped as one piece.
        Some((head, added, removed))
            if head.chars().count() + added.chars().count() + removed.chars().count() + 3
                <= text_w =>
        {
            let used = head.chars().count() + added.chars().count() + removed.chars().count() + 3;
            let head: String = head.chars().map(display_char).collect();
            spans.push(Span::styled(head, name_style));
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                added.to_string(),
                Style::new().fg(th.git_added),
            ));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                removed.to_string(),
                Style::new().fg(th.git_deleted),
            ));
            // Pad so the band reaches the right edge.
            spans.push(Span::raw(" ".repeat(text_w - used)));
        }
        _ => {
            let body: String = text.chars().take(text_w).map(display_char).collect();
            spans.push(Span::styled(format!("{body:<text_w$}"), name_style));
        }
    }
    Line::from(spans).style(Style::new().bg(th.bg_alt))
}

/// Splits a file heading into `(name and directory, "+added", "-removed")`, or
/// `None` when it does not end in a pair of counts.
fn split_stats(text: &str) -> Option<(&str, &str, &str)> {
    let (head, stats) = text.rsplit_once("  ")?;
    let (added, removed) = stats.split_once(' ')?;
    let counted = |s: &str, sign: char| {
        let mut chars = s.chars();
        chars.next() == Some(sign) && s.len() > 1 && chars.all(|c| c.is_ascii_digit())
    };
    (counted(added, '+') && counted(removed, '-')).then_some((head, added, removed))
}

#[cfg(test)]
mod tests {
    use super::split_stats;

    #[test]
    fn heading_counts_are_split_off_for_coloring() {
        assert_eq!(
            split_stats("model.rs  src/app/  +62 -0"),
            Some(("model.rs  src/app/", "+62", "-0"))
        );
        // A file in the repo root has no directory part.
        assert_eq!(
            split_stats("AGENTS.md  +5 -3"),
            Some(("AGENTS.md", "+5", "-3"))
        );
        // A status word stays with the name.
        assert_eq!(
            split_stats("git.rs  src/services/  (new file)  +12 -0"),
            Some(("git.rs  src/services/  (new file)", "+12", "-0"))
        );
        // Anything that does not end in a pair of counts is left whole.
        assert_eq!(split_stats("model.rs  src/app/"), None);
        assert_eq!(split_stats("plain heading"), None);
    }
}
