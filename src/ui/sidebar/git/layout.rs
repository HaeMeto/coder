//! Git panel layout: row kinds, the change tree, the fixed top block and the
//! column geometry shared by `render` and `hit` (single source of truth).

use ratatui::layout::Rect;

use crate::app::model::{GitStatus, Model};
use crate::services::git::GitEntry;

use super::super::list_scroll;

/// Type of a scrollable content row in the Git panel.
#[derive(Clone)]
pub(super) enum GitRowKind {
    StagedHeader,
    ChangesHeader,
    /// "Unstage All -" row (under the staged header).
    UnstageAll,
    /// "Stage All +" row (under the unstaged header).
    StageAll,
    /// Separator line between the action row and the file list.
    Separator,
    /// A directory node in the change tree (display only, not selectable).
    Dir {
        name: String,
        depth: usize,
    },
    /// A staged file: `idx` into `staged`, `depth` in the tree.
    Staged {
        idx: usize,
        depth: usize,
    },
    /// An unstaged file: `idx` into `unstaged`, `depth` in the tree.
    Unstaged {
        idx: usize,
        depth: usize,
    },
    /// Keyboard hint under the change list ("Stage/Unstage: a, Revert: r").
    Hint,
    /// The "HISTORY" heading above the previous-commits list.
    HistoryHeader,
    /// Keyboard hint under the HISTORY heading ("Show diff: Enter").
    HistoryHint,
    /// A previous commit: `idx` into `history`.
    Commit {
        idx: usize,
    },
    Info(&'static str),
}

/// Emits tree rows (directory headers + file rows) for one section's entries,
/// grouping by path like the file explorer. `make_row` builds the file row
/// (`Staged`/`Unstaged`) from an entry index + its tree depth.
fn tree_rows(
    entries: &[GitEntry],
    make_row: impl Fn(usize, usize) -> GitRowKind,
    rows: &mut Vec<GitRowKind>,
) {
    // Sort entry indices by path so shared directories are contiguous.
    let mut order: Vec<usize> = (0..entries.len()).collect();
    order.sort_by(|&a, &b| entries[a].rel.cmp(&entries[b].rel));

    let mut prev: Vec<&str> = Vec::new();
    for &i in &order {
        let parts: Vec<&str> = entries[i].rel.split('/').collect();
        let dirs = &parts[..parts.len() - 1];
        // Emit directory headers for components not shared with the previous row.
        let mut common = 0;
        while common < dirs.len() && common < prev.len() && dirs[common] == prev[common] {
            common += 1;
        }
        for (d, name) in dirs.iter().enumerate().skip(common) {
            rows.push(GitRowKind::Dir {
                name: name.to_string(),
                depth: d,
            });
        }
        rows.push(make_row(i, dirs.len()));
        prev = dirs.to_vec();
    }
}

/// Height of the commit message box (rows).
pub(super) const COMMIT_INPUT_H: u16 = 4;

/// Display width of the leftmost file icon on each change row (glyph + space).
/// Clicking within these columns opens the plain file instead of the diff.
pub(super) const FILE_ICON_W: usize = 2;

/// Labels for the commit-row buttons (Uncommit left-aligned, Commit right-aligned).
pub(super) const UNCOMMIT_LABEL: &str = " Uncommit ";
pub(super) const COMMIT_LABEL: &str = " Commit ";

/// Width of the Refresh button at the right end of the branch row.
pub(super) const REFRESH_W: usize = 3;

/// The Git panel layout computed once; shared by render and mouse hit-testing.
pub(super) struct GitLayout {
    pub(super) rows: Vec<GitRowKind>,
    /// y of the branch row at the top (only meaningful when `branch_shown`).
    pub(super) branch_y: u16,
    /// Whether the branch row is drawn.
    pub(super) branch_shown: bool,
    /// y of the first scrollable change row (below the commit block).
    pub(super) content_y: u16,
    /// Scrollable list height.
    pub(super) list_h: u16,
    pub(super) offset: usize,
    /// y of the fetch/pull/push button row (just above the commit box).
    pub(super) actions_y: u16,
    /// Top y of the commit message input field (height `COMMIT_INPUT_H`).
    pub(super) input_top: u16,
    /// y of the commit button row.
    pub(super) button_y: u16,
    /// Whether the commit box is shown (whether a repo exists).
    pub(super) has_box: bool,
}

/// Computes the Git panel layout. `area` is the panel **body** region
/// (`panel_area`), i.e. below the sidebar title + spacer.
///
/// Top→bottom: branch row, blank, fetch/pull/push row, commit input box, commit
/// button, blank, then the scrollable change list (which runs to the bottom).
pub(super) fn git_layout(model: &Model, area: Rect) -> GitLayout {
    let g = &model.sidebar.git;
    let mut rows = Vec::new();
    if !g.is_repo {
        rows.push(GitRowKind::Info("No git repository"));
    } else {
        if !g.staged.is_empty() {
            rows.push(GitRowKind::StagedHeader);
            rows.push(GitRowKind::UnstageAll);
            rows.push(GitRowKind::Separator);
            tree_rows(
                &g.staged,
                |idx, depth| GitRowKind::Staged { idx, depth },
                &mut rows,
            );
        }
        if !g.unstaged.is_empty() {
            rows.push(GitRowKind::ChangesHeader);
            rows.push(GitRowKind::StageAll);
            rows.push(GitRowKind::Separator);
            tree_rows(
                &g.unstaged,
                |idx, depth| GitRowKind::Unstaged { idx, depth },
                &mut rows,
            );
        }
        if g.staged.is_empty() && g.unstaged.is_empty() {
            rows.push(GitRowKind::Info("No changes"));
        } else {
            // The row shortcuts, right under the last change and above the
            // HISTORY divider. Only worth showing when there is a row to act on.
            rows.push(GitRowKind::Hint);
        }
        // Previous commits below the changes, separated by a divider.
        if !g.history.is_empty() {
            rows.push(GitRowKind::Separator);
            rows.push(GitRowKind::HistoryHeader);
            rows.push(GitRowKind::HistoryHint);
            for idx in 0..g.history.len() {
                rows.push(GitRowKind::Commit { idx });
            }
        }
    }

    let has_box = g.is_repo;
    let top = area.y;
    let branch_shown = g.is_repo && g.branch.is_some();
    let branch_y = top;

    // Fixed top block (only with a repo): branch, blank, fetch/pull/push row,
    // input box, button, blank. The change list then runs to the bottom.
    let (content_y, input_top, button_y, actions_y, list_h) = if has_box {
        let actions_y = top + if branch_shown { 1 } else { 0 } + 1; // branch + blank
        let input_top = actions_y + 1; // right below the fetch/pull/push row
        let button_y = input_top + COMMIT_INPUT_H;
        let content_y = button_y + 2; // blank, then the change list
        let bottom = area.y + area.height;
        let list_h = bottom.saturating_sub(content_y);
        (content_y, input_top, button_y, actions_y, list_h)
    } else {
        (top, 0, 0, 0, area.height)
    };

    let sel_pos = selected_row_pos(g, &rows);
    let offset = list_scroll(sel_pos, rows.len(), list_h as usize);

    GitLayout {
        rows,
        branch_y,
        branch_shown,
        content_y,
        list_h,
        offset,
        actions_y,
        input_top,
        button_y,
        has_box,
    }
}

/// The three git action buttons, left to right.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum GitAction {
    Fetch,
    Pull,
    Push,
}

/// Cell widths of the three action buttons, leaving a 1-column gap between them.
pub(super) fn action_segments(width: usize) -> (usize, usize, usize) {
    if width < 5 {
        return (width, 0, 0); // too narrow for gaps
    }
    let inner = width - 2; // two 1-col gaps
    let seg = inner / 3;
    (seg, seg, inner - 2 * seg)
}

/// Which action button covers column `x` within the sidebar `area` (gaps map to None).
pub(super) fn action_at_col(area: Rect, x: u16) -> Option<GitAction> {
    let rel = x.checked_sub(area.x)? as usize;
    let (w0, w1, _) = action_segments(area.width as usize);
    if rel < w0 {
        Some(GitAction::Fetch)
    } else if rel < w0 + 1 {
        None // gap
    } else if rel < w0 + 1 + w1 {
        Some(GitAction::Pull)
    } else if rel < w0 + 2 + w1 {
        None // gap
    } else {
        Some(GitAction::Push)
    }
}

/// Position of the selected item (combined index) in the row list.
fn selected_row_pos(g: &GitStatus, rows: &[GitRowKind]) -> usize {
    // The combined index runs staged, then unstaged, then the history commits.
    if let Some(commit_idx) = g.selected.checked_sub(g.changes_len()) {
        for (pos, r) in rows.iter().enumerate() {
            if matches!(r, GitRowKind::Commit { idx } if *idx == commit_idx) {
                return pos;
            }
        }
        return 0;
    }
    let (want_staged, want_idx) = if g.selected < g.staged.len() {
        (true, g.selected)
    } else {
        (false, g.selected - g.staged.len())
    };
    for (pos, r) in rows.iter().enumerate() {
        match r {
            GitRowKind::Staged { idx, .. } if want_staged && *idx == want_idx => return pos,
            GitRowKind::Unstaged { idx, .. } if !want_staged && *idx == want_idx => return pos,
            _ => {}
        }
    }
    0
}

/// Fewest name cells a change row keeps before it gives up its buttons.
const MIN_NAME_W: usize = 1;

/// Right-hand buttons of a change row, as columns relative to the row's left edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EntryButtons {
    /// Staged row: `-` (unstage) at column `unstage`.
    Staged { unstage: usize },
    /// Unstaged row: `↺ ` (revert) over `revert..revert + 2`, then a 1-cell gap,
    /// then `+` (stage) at column `stage`.
    Unstaged { revert: usize, stage: usize },
}

/// Column layout of one change row: `[icon][indent][ M ][name…][buttons]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct EntryCols {
    /// Cells for the file name.
    pub(super) name_w: usize,
    /// The row's buttons, or `None` when the row is too narrow (deep indent /
    /// narrow sidebar) — then they are neither drawn nor clickable.
    pub(super) buttons: Option<EntryButtons>,
}

/// The single source of truth for a change row's columns, used by both
/// `render::entry_line` and `hit::git_hit`.
///
/// The file-icon glyph is 1 cell but `FILE_ICON_W` reserves 2, so the drawn row
/// ends one short of the right edge: the suffix (`"  -"` staged, `"↺  +"`
/// unstaged) puts its last button at `width - 2`, with `width - 1` left blank.
pub(super) fn entry_cols(width: usize, depth: usize, staged: bool) -> EntryCols {
    let fixed = FILE_ICON_W + 2 * depth + 3; // icon + indent + " M "
    let suffix_w = if staged { 3 } else { 4 };
    if width >= fixed + MIN_NAME_W + suffix_w {
        let suffix_x = width - suffix_w - 1;
        let buttons = if staged {
            EntryButtons::Staged {
                unstage: suffix_x + 2,
            }
        } else {
            EntryButtons::Unstaged {
                revert: suffix_x,
                stage: suffix_x + 3,
            }
        };
        EntryCols {
            name_w: width - fixed - suffix_w,
            buttons: Some(buttons),
        }
    } else {
        EntryCols {
            name_w: width.saturating_sub(fixed),
            buttons: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_buttons_sit_at_the_right_edge() {
        assert_eq!(
            entry_cols(30, 0, true).buttons,
            Some(EntryButtons::Staged { unstage: 28 })
        );
        assert_eq!(
            entry_cols(30, 1, false).buttons,
            Some(EntryButtons::Unstaged {
                revert: 25,
                stage: 28
            })
        );
    }

    #[test]
    fn narrow_or_deep_rows_drop_their_buttons() {
        // icon(2) + indent(2*6) + prefix(3) + name(1) + suffix(4) = 22.
        assert!(entry_cols(22, 6, false).buttons.is_some());
        assert_eq!(entry_cols(21, 6, false).buttons, None);
        assert_eq!(entry_cols(4, 0, true).buttons, None);
    }
}
