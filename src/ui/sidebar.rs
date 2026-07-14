//! Sidebar panels: file tree, search, git, extensions.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Wrap};

use crate::app::model::{Focus, GitStatus, Model, Panel};
use crate::services::git::{GitEntry, GitState};

/// List scroll offset — keeps the selected item visible.
pub fn list_scroll(selected: usize, len: usize, height: usize) -> usize {
    if height == 0 || len <= height {
        return 0;
    }
    if selected < height {
        0
    } else {
        (selected + 1).saturating_sub(height).min(len - height)
    }
}

pub fn render(frame: &mut Frame, area: Rect, model: &Model) {
    let block = Block::new().style(Style::new().bg(model.theme.bg_alt));
    frame.render_widget(block, area);

    // Title row.
    let title = Paragraph::new(Line::from(Span::styled(
        format!(" {}", model.sidebar.active.title()),
        Style::new().fg(model.theme.fg_dim).add_modifier(Modifier::BOLD),
    )))
    .style(Style::new().bg(model.theme.bg_alt));
    let title_area = Rect { height: 1, ..area };
    frame.render_widget(title, title_area);

    let content = Rect {
        y: area.y + 1,
        height: area.height.saturating_sub(1),
        ..area
    };

    match model.sidebar.active {
        Panel::Files => render_files(frame, content, model),
        Panel::Search => render_search(frame, content, model),
        // The Git panel uses the full sidebar area (title + list + commit box).
        Panel::Git => render_git(frame, area, model),
        Panel::Extensions => render_extensions(frame, content, model),
        Panel::Themes => render_themes(frame, content, model),
    }
}

fn render_themes(frame: &mut Frame, area: Rect, model: &Model) {
    let t = &model.sidebar.themes;
    let height = area.height as usize;
    let offset = list_scroll(t.selected, t.names.len(), height);

    let mut lines: Vec<Line> = Vec::new();
    for (i, name) in t.names.iter().enumerate().skip(offset).take(height) {
        let selected = i == t.selected;
        let marker = if i == t.selected { "● " } else { "  " };
        let name_style = if selected {
            Style::new().fg(model.theme.fg)
        } else {
            Style::new().fg(model.theme.fg_dim)
        };
        let line_style = if selected && model.focus == Focus::Sidebar {
            Style::new().bg(model.theme.selection)
        } else {
            Style::new().bg(model.theme.bg_alt)
        };
        lines.push(
            Line::from(vec![
                Span::styled(marker, Style::new().fg(model.theme.accent)),
                Span::styled(name.clone(), name_style),
            ])
            .style(line_style),
        );
    }
    let p = Paragraph::new(lines).style(Style::new().bg(model.theme.bg_alt));
    frame.render_widget(p, area);
}

/// Returns the row index in the theme list based on the mouse y.
pub fn theme_row_at(model: &Model, area: Rect, y: u16) -> Option<usize> {
    // area is the whole sidebar (including the title): content starts at area.y+1.
    let content_y = area.y + 1;
    if y < content_y {
        return None;
    }
    let len = model.sidebar.themes.names.len();
    let height = area.height.saturating_sub(1) as usize;
    let offset = list_scroll(model.sidebar.themes.selected, len, height);
    let idx = offset + (y - content_y) as usize;
    if idx < len { Some(idx) } else { None }
}

fn render_files(frame: &mut Frame, area: Rect, model: &Model) {
    let rows = model.sidebar.files.visible_rows();
    let height = area.height as usize;
    let offset = list_scroll(model.sidebar.files.selected, rows.len(), height);

    let mut lines: Vec<Line> = Vec::new();
    for (i, row) in rows.iter().enumerate().skip(offset).take(height) {
        let selected = i == model.sidebar.files.selected && model.focus == Focus::Sidebar;
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
        let line_style = if selected {
            Style::new().bg(model.theme.selection)
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
    // here area is the content region (title not included): sidebar starts at area.y+1.
    let content_y = area.y + 1;
    if y < content_y {
        return None;
    }
    let rows_len = model.sidebar.files.visible_rows().len();
    let height = area.height.saturating_sub(1) as usize;
    let offset = list_scroll(model.sidebar.files.selected, rows_len, height);
    let idx = offset + (y - content_y) as usize;
    if idx < rows_len { Some(idx) } else { None }
}

fn render_search(frame: &mut Frame, area: Rect, model: &Model) {
    use crate::app::model::SearchField;
    let s = &model.sidebar.search;
    let input_focused = model.focus == Focus::SearchInput;

    // Cursor and highlight based on the active field.
    let field_style = |active: bool| {
        if input_focused && active {
            Style::new().fg(model.theme.fg).bg(model.theme.bg)
        } else {
            Style::new().fg(model.theme.fg_dim).bg(model.theme.bg)
        }
    };
    let query_active = input_focused && s.field == SearchField::Query;
    let replace_active = input_focused && s.field == SearchField::Replace;
    let qcur = if query_active { "█" } else { "" };
    let rcur = if replace_active { "█" } else { "" };

    let mut lines: Vec<Line> = Vec::new();
    // 1) Search input.
    lines.push(Line::from(Span::styled(
        format!(" 🔍 {}{}", s.query, qcur),
        field_style(s.field == SearchField::Query),
    )));
    // 2) Replace input.
    lines.push(Line::from(Span::styled(
        format!(" ⇄  {}{}", s.replace, rcur),
        field_style(s.field == SearchField::Replace),
    )));
    // 3) Regex checkbox + shortcut hint.
    let checkbox = if s.use_regex { "[x]" } else { "[ ]" };
    lines.push(Line::from(vec![
        Span::styled(
            format!(" {checkbox} regex"),
            Style::new().fg(if s.use_regex {
                model.theme.accent
            } else {
                model.theme.fg_dim
            }),
        ),
        Span::styled(
            "  ⇥ field · ⌃R regex · ⏎ replace",
            Style::new().fg(model.theme.fg_dim),
        ),
    ]));
    // 4) Result count.
    lines.push(Line::from(Span::styled(
        format!(" {} results", s.results.len()),
        Style::new().fg(model.theme.fg_dim),
    )));

    let height = area.height as usize;
    let list_h = height.saturating_sub(4);
    let offset = list_scroll(s.selected, s.results.len(), list_h);
    for (i, m) in s.results.iter().enumerate().skip(offset).take(list_h) {
        let selected = i == s.selected && model.focus == Focus::Sidebar;
        let style = if selected {
            Style::new().bg(model.theme.selection).fg(model.theme.fg)
        } else {
            Style::new().bg(model.theme.bg_alt).fg(model.theme.fg_dim)
        };
        lines.push(
            Line::from(vec![
                Span::styled(format!("{}:{} ", m.rel, m.line_no), Style::new().fg(model.theme.accent)),
                Span::raw(m.line.trim().to_string()),
            ])
            .style(style),
        );
    }
    let p = Paragraph::new(lines).style(Style::new().bg(model.theme.bg_alt));
    frame.render_widget(p, area);
}

/// Result index in the search panel based on the mouse y (excluding the input rows).
pub fn search_row_at(model: &Model, area: Rect, y: u16) -> Option<usize> {
    let s = &model.sidebar.search;
    // title(0) query(1) replace(2) regex(3) count(4) results(5+).
    let start = area.y + 5;
    if y < start {
        return None;
    }
    let list_h = area.height.saturating_sub(5) as usize;
    let offset = list_scroll(s.selected, s.results.len(), list_h);
    let idx = offset + (y - start) as usize;
    if idx < s.results.len() { Some(idx) } else { None }
}

/// Type of a scrollable content row in the Git panel.
#[derive(Clone, Copy)]
pub enum GitRowKind {
    Branch,
    StagedHeader,
    ChangesHeader,
    /// "Unstage All -" row (under the staged header).
    UnstageAll,
    /// "Stage All +" row (under the unstaged header).
    StageAll,
    /// Separator line between the action row and the file list.
    Separator,
    Staged(usize),
    Unstaged(usize),
    Info(&'static str),
}

/// Height of the commit message box (rows).
const COMMIT_INPUT_H: u16 = 4;

/// The Git panel layout computed once; shared by render and mouse hit-testing.
pub struct GitLayout {
    pub rows: Vec<GitRowKind>,
    /// y of the first content row (below the title row).
    pub content_y: u16,
    /// Scrollable list height.
    pub list_h: u16,
    pub offset: usize,
    /// y of the commit box separator row.
    pub sep_y: u16,
    /// Top y of the commit message input field (height `COMMIT_INPUT_H`).
    pub input_top: u16,
    /// y of the commit button row (a blank row is left below it).
    pub button_y: u16,
    /// Whether the commit box is shown (whether a repo exists).
    pub has_box: bool,
}

/// Computes the Git panel layout. `area` is the full sidebar area (title included).
pub fn git_layout(model: &Model, area: Rect) -> GitLayout {
    let g = &model.sidebar.git;
    let mut rows = Vec::new();
    if !g.is_repo {
        rows.push(GitRowKind::Info("No git repository"));
    } else {
        if g.branch.is_some() {
            rows.push(GitRowKind::Branch);
        }
        if !g.staged.is_empty() {
            rows.push(GitRowKind::StagedHeader);
            rows.push(GitRowKind::UnstageAll);
            rows.push(GitRowKind::Separator);
            rows.extend((0..g.staged.len()).map(GitRowKind::Staged));
        }
        if !g.unstaged.is_empty() {
            rows.push(GitRowKind::ChangesHeader);
            rows.push(GitRowKind::StageAll);
            rows.push(GitRowKind::Separator);
            rows.extend((0..g.unstaged.len()).map(GitRowKind::Unstaged));
        }
        if g.staged.is_empty() && g.unstaged.is_empty() {
            rows.push(GitRowKind::Info("No changes"));
        }
    }

    let content_y = area.y + 1;
    let has_box = g.is_repo;
    // Commit box at the bottom: blank at the very bottom, button above it, 4 input rows above that, separator above.
    let bottom = area.y + area.height.saturating_sub(1); // blank row
    let button_y = bottom.saturating_sub(1);
    let input_top = button_y.saturating_sub(COMMIT_INPUT_H);
    let sep_y = input_top.saturating_sub(1);
    let list_h = if has_box {
        sep_y.saturating_sub(content_y)
    } else {
        area.height.saturating_sub(1)
    };

    let sel_pos = selected_row_pos(g, &rows);
    let offset = list_scroll(sel_pos, rows.len(), list_h as usize);

    GitLayout {
        rows,
        content_y,
        list_h,
        offset,
        sep_y,
        input_top,
        button_y,
        has_box,
    }
}

/// Position of the selected item (combined index) in the row list.
fn selected_row_pos(g: &GitStatus, rows: &[GitRowKind]) -> usize {
    let (want_staged, want_idx) = if g.selected < g.staged.len() {
        (true, g.selected)
    } else {
        (false, g.selected - g.staged.len())
    };
    for (pos, r) in rows.iter().enumerate() {
        match r {
            GitRowKind::Staged(i) if want_staged && *i == want_idx => return pos,
            GitRowKind::Unstaged(i) if !want_staged && *i == want_idx => return pos,
            _ => {}
        }
    }
    0
}

fn render_git(frame: &mut Frame, area: Rect, model: &Model) {
    let l = git_layout(model, area);
    let width = area.width as usize;

    let mut lines: Vec<Line> = Vec::new();
    for r in l.rows.iter().skip(l.offset).take(l.list_h as usize) {
        lines.push(git_row_line(model, r, width));
    }
    let list_area = Rect {
        y: l.content_y,
        height: l.list_h,
        ..area
    };
    let p = Paragraph::new(lines).style(Style::new().bg(model.theme.bg_alt));
    frame.render_widget(p, list_area);

    if l.has_box {
        render_commit_box(frame, area, &l, model, width);
    }
}

/// Converts a single git content row into a drawable `Line`.
fn git_row_line(model: &Model, kind: &GitRowKind, width: usize) -> Line<'static> {
    let g = &model.sidebar.git;
    let th = &model.theme;
    match kind {
        GitRowKind::Branch => Line::from(Span::styled(
            format!(" ⎇ {}", g.branch.clone().unwrap_or_default()),
            Style::new().fg(th.accent).add_modifier(Modifier::BOLD),
        )),
        GitRowKind::StagedHeader => header_line(format!(" STAGED ({})", g.staged.len()), th),
        GitRowKind::ChangesHeader => {
            header_line(format!(" CHANGES ({})", g.unstaged.len()), th)
        }
        GitRowKind::Info(s) => {
            Line::from(Span::styled(format!(" {s}"), Style::new().fg(th.fg_dim)))
        }
        GitRowKind::StageAll => action_line(" Stage All", "+", th.git_added, th, width),
        GitRowKind::UnstageAll => action_line(" Unstage All", "-", th.git_deleted, th, width),
        GitRowKind::Separator => Line::from(Span::styled(
            "─".repeat(width),
            Style::new().fg(th.border),
        ))
        .style(Style::new().bg(th.bg_alt)),
        GitRowKind::Staged(i) => {
            let sel = model.focus == Focus::Sidebar && g.selected == *i;
            entry_line(model, &g.staged[*i], true, sel, width)
        }
        GitRowKind::Unstaged(i) => {
            let sel = model.focus == Focus::Sidebar && g.selected == g.staged.len() + *i;
            entry_line(model, &g.unstaged[*i], false, sel, width)
        }
    }
}

/// Bulk action row ("Stage All +" / "Unstage All -"); icon on the right at width-2 (aligned with entry).
fn action_line(
    label: &str,
    icon: &str,
    icon_color: ratatui::style::Color,
    th: &crate::core::theme::Theme,
    width: usize,
) -> Line<'static> {
    let w = width.saturating_sub(4);
    let field = format!("{label:<w$}");
    Line::from(vec![
        Span::styled(field, Style::new().fg(th.accent)),
        Span::raw("  "),
        Span::styled(icon.to_string(), Style::new().fg(icon_color)),
        Span::raw(" "),
    ])
    .style(Style::new().bg(th.bg_alt))
}

fn header_line(text: String, th: &crate::core::theme::Theme) -> Line<'static> {
    Line::from(Span::styled(
        text,
        Style::new().fg(th.fg_dim).add_modifier(Modifier::BOLD),
    ))
    .style(Style::new().bg(th.bg_alt))
}

/// A git change row: `[ M path            + ↺]` (unstaged) / `[ M path   -]` (staged).
fn entry_line(
    model: &Model,
    e: &GitEntry,
    staged: bool,
    selected: bool,
    width: usize,
) -> Line<'static> {
    let th = &model.theme;
    let color = match e.state {
        GitState::Added => th.git_added,
        GitState::Modified => th.git_modified,
        GitState::Deleted => th.git_deleted,
        GitState::Untracked => th.git_untracked,
        _ => th.fg_dim,
    };
    // prefix (3) + path + suffix (4) = width
    let avail = width.saturating_sub(7);
    let path = fit_path(&e.rel, avail);
    let path_field = format!("{path:<avail$}");
    let line_bg = if selected { th.selection } else { th.bg_alt };

    let mut spans = vec![
        Span::styled(format!(" {} ", e.state.short()), Style::new().fg(color)),
        Span::styled(path_field, Style::new().fg(th.fg)),
    ];
    if staged {
        // Last 4 columns: "  - " → unstage button at width-2.
        spans.push(Span::raw("  "));
        spans.push(Span::styled("-".to_string(), Style::new().fg(th.git_deleted)));
        spans.push(Span::raw(" "));
    } else {
        let revert = if model.ascii_icons { "x" } else { "↺" };
        // Last 4 columns: "↺ + " → revert at width-4, stage at width-2 (a space between them).
        spans.push(Span::styled(revert.to_string(), Style::new().fg(th.git_deleted)));
        spans.push(Span::raw(" "));
        spans.push(Span::styled("+".to_string(), Style::new().fg(th.git_added)));
        spans.push(Span::raw(" "));
    }
    Line::from(spans).style(Style::new().bg(line_bg))
}

/// Fits the path into `max` characters; if too long, trims from the front and prepends `…` (the file name stays visible).
fn fit_path(rel: &str, max: usize) -> String {
    let len = rel.chars().count();
    if len <= max {
        return rel.to_string();
    }
    if max <= 1 {
        return "…".to_string();
    }
    let tail: String = rel.chars().skip(len - (max - 1)).collect();
    format!("…{tail}")
}

/// The commit message box at the bottom (4 rows) and the commit button.
fn render_commit_box(frame: &mut Frame, area: Rect, l: &GitLayout, model: &Model, width: usize) {
    let th = &model.theme;
    let g = &model.sidebar.git;
    let focused = model.focus == Focus::GitCommit;

    // Separator row.
    let sep = Paragraph::new(Line::from(Span::styled(
        "─".repeat(width),
        Style::new().fg(th.border),
    )))
    .style(Style::new().bg(th.bg_alt));
    frame.render_widget(sep, Rect { y: l.sep_y, height: 1, ..area });

    // Input field (multi-line, wraps).
    let input_bg = if focused { th.bg } else { th.bg_alt };
    let input = if g.commit_msg.is_empty() && !focused {
        Paragraph::new(" Message (⏎ to commit)")
            .style(Style::new().fg(th.fg_dim).bg(input_bg))
    } else {
        let cursor = if focused { "█" } else { "" };
        Paragraph::new(format!(" {}{}", g.commit_msg, cursor))
            .style(Style::new().fg(th.fg).bg(input_bg))
            .wrap(Wrap { trim: false })
    };
    frame.render_widget(
        input,
        Rect {
            y: l.input_top,
            height: COMMIT_INPUT_H,
            ..area
        },
    );

    // Commit button.
    let label = if model.ascii_icons {
        " Commit "
    } else {
        " ✓ Commit "
    };
    let can_commit = !g.staged.is_empty() && !g.commit_msg.trim().is_empty();
    let btn_bg = if can_commit { th.accent } else { th.tab_inactive_bg };
    let btn = Paragraph::new(Line::from(Span::styled(
        format!("{label:^width$}"),
        Style::new().fg(th.statusbar_fg).add_modifier(Modifier::BOLD),
    )))
    .style(Style::new().bg(btn_bg));
    frame.render_widget(btn, Rect { y: l.button_y, height: 1, ..area });
}

/// Target of a mouse click in the Git panel.
pub enum GitHit {
    /// Open the change row (combined index).
    Entry(usize),
    Stage(String),
    Unstage(String),
    Revert(String),
    StageAll,
    UnstageAll,
    CommitInput,
    CommitButton,
}

/// Converts the mouse (x, y) into a Git panel target. `area` is the full sidebar area.
pub fn git_hit(model: &Model, area: Rect, x: u16, y: u16) -> Option<GitHit> {
    let l = git_layout(model, area);
    let g = &model.sidebar.git;
    if l.has_box {
        if y == l.button_y {
            return Some(GitHit::CommitButton);
        }
        if y >= l.input_top && y < l.input_top + COMMIT_INPUT_H {
            return Some(GitHit::CommitInput);
        }
        if y >= l.sep_y {
            return None; // separator / blank
        }
    }
    if y < l.content_y || y >= l.content_y + l.list_h {
        return None;
    }
    let row_idx = l.offset + (y - l.content_y) as usize;
    let kind = l.rows.get(row_idx)?;
    let col = x.saturating_sub(area.x) as usize;
    let width = area.width as usize;
    let wide = width >= 8;
    match kind {
        GitRowKind::Staged(i) => {
            let e = g.staged.get(*i)?;
            if wide && col >= width - 2 {
                Some(GitHit::Unstage(e.rel.clone()))
            } else {
                Some(GitHit::Entry(*i))
            }
        }
        GitRowKind::StageAll => Some(GitHit::StageAll),
        GitRowKind::UnstageAll => Some(GitHit::UnstageAll),
        GitRowKind::Unstaged(i) => {
            let e = g.unstaged.get(*i)?;
            // Last 4 columns: revert (width-4) · space · stage (width-2).
            if wide && col >= width - 2 {
                Some(GitHit::Stage(e.rel.clone()))
            } else if wide && col >= width - 4 {
                Some(GitHit::Revert(e.rel.clone()))
            } else {
                Some(GitHit::Entry(g.staged.len() + *i))
            }
        }
        _ => None,
    }
}

fn render_extensions(frame: &mut Frame, area: Rect, model: &Model) {
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
