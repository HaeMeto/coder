//! Git panel drawing: branch row, fetch/pull/push, commit box, change list.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::model::{Focus, GitZone, Model};
use crate::services::git::{GitEntry, GitState};
use crate::ui::text_input::TextInput;

use super::layout::{
    COMMIT_INPUT_H, COMMIT_LABEL, GitLayout, GitRowKind, REFRESH_W, UNCOMMIT_LABEL,
    action_segments, entry_cols, git_layout,
};

/// Disabled-button colors, fixed regardless of the active theme.
const DISABLED_FG: Color = Color::Rgb(140, 140, 140);
const DISABLED_BG: Color = Color::Rgb(60, 60, 60);

/// Style of a panel button. The keyboard-focused one swaps its foreground and
/// background, which reads as a distinctly different colour in every theme and
/// on the disabled (grey) buttons alike — where a highlight colour of its own
/// would have to be picked per theme.
fn button_style(th: &crate::core::theme::Theme, enabled: bool, focused: bool) -> Style {
    let (fg, bg) = if enabled {
        (th.statusbar_fg, th.accent)
    } else {
        (DISABLED_FG, DISABLED_BG)
    };
    let (fg, bg) = if focused { (bg, fg) } else { (fg, bg) };
    let style = Style::new().fg(fg).bg(bg);
    if focused {
        style.add_modifier(Modifier::BOLD)
    } else {
        style
    }
}

pub(in crate::ui::sidebar) fn render(frame: &mut Frame, area: Rect, model: &Model) {
    let l = git_layout(model, area);
    let width = area.width as usize;
    let th = &model.theme;

    // Branch row at the very top, with a Refresh button pinned to the right.
    if l.branch_shown {
        // Both glyphs fill `REFRESH_W` cells, so the click target in
        // `git_hit` lines up either way.
        let refresh_icon = if model.ascii_icons { " ⟲ " } else { " ⟳ " };
        let name_w = width.saturating_sub(REFRESH_W);
        let name = model.sidebar.git.branch.clone().unwrap_or_default();
        let branch = Paragraph::new(Line::from(vec![
            Span::styled(format!("{name:<name_w$}"), Style::new().fg(th.fg)),
            Span::styled(refresh_icon.to_string(), Style::new().fg(th.accent)),
        ]))
        .style(Style::new().bg(th.bg_alt));
        frame.render_widget(
            branch,
            Rect {
                y: l.branch_y,
                height: 1,
                ..area
            },
        );
    }

    // Scrollable change list.
    let mut lines: Vec<Line> = Vec::new();
    for r in l.rows.iter().skip(l.offset).take(l.list_h as usize) {
        lines.push(git_row_line(model, r, width));
    }
    let list_area = Rect {
        y: l.content_y,
        height: l.list_h,
        ..area
    };
    let p = Paragraph::new(lines).style(Style::new().bg(th.bg_alt));
    frame.render_widget(p, list_area);

    if l.has_box {
        render_commit_box(frame, area, &l, model, width);
        render_git_actions(frame, area, &l, model, width);
    }
}

/// The fetch / pull / push button row at the bottom of the panel.
fn render_git_actions(frame: &mut Frame, area: Rect, l: &GitLayout, model: &Model, width: usize) {
    let th = &model.theme;
    let g = &model.sidebar.git;
    let (w0, w1, w2) = action_segments(width);

    let up = if model.ascii_icons { "" } else { "↑" };
    let down = if model.ascii_icons { "" } else { "↓" };
    let fetch_label = "Fetch".to_string();
    let pull_label = if g.behind > 0 {
        format!("Pull {down}{}", g.behind)
    } else {
        "Pull".to_string()
    };
    let push_label = if g.ahead > 0 {
        format!("Push {up}{}", g.ahead)
    } else {
        "Push".to_string()
    };

    // Fetch needs a remote; Pull needs an upstream; Push needs something to push.
    let fetch_enabled = g.has_remote;
    let pull_enabled = g.has_upstream;
    let push_enabled = g.can_push();

    // Distinct backgrounds separate the cells; widths match the thirds used by
    // `action_at_col` so the visuals and hit-testing line up exactly.
    let zone = g.zone;
    let cell = |label: &str, enabled: bool, w: usize, z: GitZone| -> Span<'static> {
        Span::styled(format!("{label:^w$}"), button_style(th, enabled, zone == z))
    };

    let gap = || Span::styled(" ", Style::new().bg(th.bg_alt));
    let spans = vec![
        cell(&fetch_label, fetch_enabled, w0, GitZone::Fetch),
        gap(),
        cell(&pull_label, pull_enabled, w1, GitZone::Pull),
        gap(),
        cell(&push_label, push_enabled, w2, GitZone::Push),
    ];
    let p = Paragraph::new(Line::from(spans)).style(Style::new().bg(th.bg_alt));
    frame.render_widget(
        p,
        Rect {
            y: l.actions_y,
            height: 1,
            ..area
        },
    );
}

/// Converts a single git content row into a drawable `Line`.
fn git_row_line(model: &Model, kind: &GitRowKind, width: usize) -> Line<'static> {
    let g = &model.sidebar.git;
    let th = &model.theme;
    match kind {
        GitRowKind::StagedHeader => header_line(format!(" STAGED ({})", g.staged.len()), th),
        GitRowKind::ChangesHeader => header_line(format!(" CHANGES ({})", g.unstaged.len()), th),
        GitRowKind::Info(s) => {
            Line::from(Span::styled(format!(" {s}"), Style::new().fg(th.fg_dim)))
        }
        GitRowKind::StageAll => action_line(" Stage All", "+", th.git_added, th, width),
        GitRowKind::UnstageAll => action_line(" Unstage All", "-", th.git_deleted, th, width),
        GitRowKind::Separator => {
            Line::from(Span::styled("─".repeat(width), Style::new().fg(th.border)))
                .style(Style::new().bg(th.bg_alt))
        }
        GitRowKind::Dir { name, depth } => Line::from(vec![
            Span::raw("  ".repeat(*depth)),
            Span::styled("▾ ", Style::new().fg(th.fg_dim)),
            Span::styled(name.clone(), Style::new().fg(th.fg)),
        ])
        .style(Style::new().bg(th.bg_alt)),
        GitRowKind::Staged { idx, depth } => {
            let sel = g.selected == *idx;
            entry_line(model, &g.staged[*idx], true, sel, *depth, width)
        }
        GitRowKind::Unstaged { idx, depth } => {
            let sel = g.selected == g.staged.len() + *idx;
            entry_line(model, &g.unstaged[*idx], false, sel, *depth, width)
        }
        GitRowKind::Hint => {
            // Drop the labels the panel is too narrow for rather than letting the
            // line wrap into the next row.
            let full = " Stage/Unstage: a, Revert: r";
            let text = if full.chars().count() <= width {
                full
            } else if " a: stage, r: revert".chars().count() <= width {
                " a: stage, r: revert"
            } else {
                ""
            };
            hint_line(text, th)
        }
        GitRowKind::HistoryHeader => header_line(" HISTORY".to_string(), th),
        GitRowKind::HistoryHint => hint_line(" Show diff: Enter", th),
        GitRowKind::Commit { idx } => {
            let sel = g.selected == g.changes_len() + *idx;
            commit_line(&g.history[*idx], th, width, sel)
        }
    }
}

/// A previous-commit row: short hash (dim) + summary, trimmed to the panel width.
fn commit_line(
    c: &crate::services::git::GitCommit,
    th: &crate::core::theme::Theme,
    width: usize,
    selected: bool,
) -> Line<'static> {
    let hash = format!(" {} ", c.hash);
    let avail = width.saturating_sub(hash.chars().count());
    let summary = if c.summary.chars().count() > avail {
        let keep = avail.saturating_sub(1);
        format!("{}…", c.summary.chars().take(keep).collect::<String>())
    } else {
        c.summary.clone()
    };
    let bg = if selected {
        th.selected_bg()
    } else {
        th.bg_alt
    };
    Line::from(vec![
        Span::styled(hash, Style::new().fg(th.accent)),
        Span::styled(summary, Style::new().fg(th.fg_dim)),
    ])
    .style(Style::new().bg(bg))
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

/// A dim italic keyboard hint row.
fn hint_line(text: &str, th: &crate::core::theme::Theme) -> Line<'static> {
    Line::from(Span::styled(
        text.to_string(),
        Style::new().fg(th.fg_dim).add_modifier(Modifier::ITALIC),
    ))
    .style(Style::new().bg(th.bg_alt))
}

fn header_line(text: String, th: &crate::core::theme::Theme) -> Line<'static> {
    Line::from(Span::styled(
        text,
        Style::new().fg(th.fg_dim).add_modifier(Modifier::BOLD),
    ))
    .style(Style::new().bg(th.bg_alt))
}

/// A git change row shown in the tree: `[<indent> M name       + ↺]` (unstaged) /
/// `[<indent> M name   -]` (staged). Only the file name is drawn; the directory
/// path is conveyed by the tree indent and parent `Dir` rows.
fn entry_line(
    model: &Model,
    e: &GitEntry,
    staged: bool,
    selected: bool,
    depth: usize,
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
    let indent = "  ".repeat(depth);
    let name = e.rel.rsplit('/').next().unwrap_or(e.rel.as_str());
    let file_icon = if model.ascii_icons { "•" } else { "" };
    // Columns (name width, button positions) come from the layout shared with
    // `git_hit`; a row too narrow for its buttons draws none.
    let cols = entry_cols(width, depth, staged);
    let avail = cols.name_w;
    let name_field = format!("{:<avail$}", fit_path(name, avail));
    let line_bg = if selected {
        th.selected_bg()
    } else {
        th.bg_alt
    };

    // Leftmost: a file icon that opens the plain file (not the diff view).
    let mut spans = vec![
        Span::styled(file_icon.to_string(), Style::new().fg(th.fg_dim)),
        Span::raw(indent),
        Span::styled(format!(" {} ", e.state.short()), Style::new().fg(color)),
        Span::styled(name_field, Style::new().fg(th.fg)),
    ];

    if cols.buttons.is_none() {
        // No room: name only.
    } else if staged {
        spans.push(Span::from("  "));
        spans.push(Span::styled(
            "-".to_string(),
            Style::new().fg(th.git_deleted),
        ));
    } else {
        // Both are 2 cells wide, matching the `git_hit` revert target.
        let revert = if model.ascii_icons { "⟲ " } else { "↺ " };

        spans.push(Span::styled(
            revert.to_string(),
            Style::new().fg(th.git_deleted),
        ));
        spans.push(Span::raw(" "));
        spans.push(Span::styled("+".to_string(), Style::new().fg(th.git_added)));
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

/// The commit message box (4 rows) and the commit button, below the branch row.
fn render_commit_box(frame: &mut Frame, area: Rect, l: &GitLayout, model: &Model, width: usize) {
    let th = &model.theme;
    let g = &model.sidebar.git;
    let focused = model.focus == Focus::GitCommit;

    // Multi-line, vertically-scrolling commit input (shared text-input widget).
    frame.render_widget(
        TextInput::new(&g.commit, th)
            .placeholder("Message")
            .focused(focused)
            .pad(1),
        Rect {
            y: l.input_top,
            height: COMMIT_INPUT_H,
            ..area
        },
    );

    // Uncommit (left) and Commit (right), always shown, enabled per state. Both
    // share the Fetch/Pull button color scheme.
    let can_commit = !g.staged.is_empty() && !g.commit.content().trim().is_empty();
    let can_undo = g.can_undo_commit();

    // Same colors as the fetch/pull/push cells.
    let zone = g.zone;
    let cell = |label: &str, enabled: bool, z: GitZone| -> Span<'static> {
        Span::styled(
            label.to_string(),
            button_style(th, enabled, zone == z).add_modifier(Modifier::BOLD),
        )
    };

    let uncommit_w = UNCOMMIT_LABEL.chars().count();
    let commit_w = COMMIT_LABEL.chars().count();
    let filler = width.saturating_sub(uncommit_w + commit_w).max(1);
    let spans = vec![
        cell(UNCOMMIT_LABEL, can_undo, GitZone::Uncommit),
        Span::styled(" ".repeat(filler), Style::new().bg(th.bg_alt)),
        cell(COMMIT_LABEL, can_commit, GitZone::Commit),
    ];

    let btn = Paragraph::new(Line::from(spans)).style(Style::new().bg(th.bg_alt));
    frame.render_widget(
        btn,
        Rect {
            y: l.button_y,
            height: 1,
            ..area
        },
    );
}
