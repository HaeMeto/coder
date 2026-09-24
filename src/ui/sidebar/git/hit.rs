//! Git panel mouse hit-testing. Mirrors `render` through the shared geometry
//! in `layout`.

use ratatui::layout::Rect;

use crate::app::model::Model;

use super::super::panel_area;
use super::layout::{
    COMMIT_INPUT_H, COMMIT_LABEL, EntryButtons, FILE_ICON_W, GitAction, GitRowKind, REFRESH_W,
    UNCOMMIT_LABEL, action_at_col, entry_cols, git_layout,
};

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
    UndoLastCommit,
    Fetch,
    Pull,
    Push,
    /// Reload git status (the Refresh button on the branch row).
    Refresh,
    /// Open the plain file (not the diff) — the file icon on a change row.
    OpenFile(String),
}

/// Converts the mouse (x, y) into a Git panel target. `area` is the full sidebar area.
pub fn git_hit(model: &Model, area: Rect, x: u16, y: u16) -> Option<GitHit> {
    let area = panel_area(area);
    let l = git_layout(model, area);
    let g = &model.sidebar.git;
    // Refresh button: right end of the branch row.
    if l.branch_shown && y == l.branch_y {
        let rel = x.saturating_sub(area.x) as usize;
        if rel >= (area.width as usize).saturating_sub(REFRESH_W) {
            return Some(GitHit::Refresh);
        }
        return None;
    }
    if l.has_box {
        if y == l.actions_y {
            return match action_at_col(area, x)? {
                GitAction::Fetch => Some(GitHit::Fetch),
                GitAction::Pull => Some(GitHit::Pull),
                GitAction::Push => Some(GitHit::Push),
            };
        }
        if y == l.button_y {
            let rel = x.saturating_sub(area.x) as usize;
            let width = area.width as usize;
            let uncommit_w = UNCOMMIT_LABEL.chars().count();
            let commit_w = COMMIT_LABEL.chars().count();
            // Uncommit is left-aligned, Commit is right-aligned; the gap is inert.
            if rel < uncommit_w {
                return Some(GitHit::UndoLastCommit);
            }
            if rel >= width.saturating_sub(commit_w) {
                return Some(GitHit::CommitButton);
            }
            return None;
        }
        if y >= l.input_top && y < l.input_top + COMMIT_INPUT_H {
            return Some(GitHit::CommitInput);
        }
        if y < l.content_y {
            return None; // branch / blanks in the fixed top block
        }
    }
    if y < l.content_y || y >= l.content_y + l.list_h {
        return None;
    }
    let row_idx = l.offset + (y - l.content_y) as usize;
    let kind = l.rows.get(row_idx)?;
    let col = x.saturating_sub(area.x) as usize;
    let width = area.width as usize;
    match kind {
        GitRowKind::Staged { idx, depth } => {
            let e = g.staged.get(*idx)?;
            let unstage = match entry_cols(width, *depth, true).buttons {
                Some(EntryButtons::Staged { unstage }) => Some(unstage),
                _ => None,
            };
            if col < FILE_ICON_W {
                Some(GitHit::OpenFile(e.rel.clone()))
            } else if unstage == Some(col) {
                Some(GitHit::Unstage(e.rel.clone()))
            } else {
                Some(GitHit::Entry(*idx))
            }
        }
        GitRowKind::StageAll => Some(GitHit::StageAll),
        GitRowKind::UnstageAll => Some(GitHit::UnstageAll),
        GitRowKind::Unstaged { idx, depth } => {
            let e = g.unstaged.get(*idx)?;
            // Revert owns its 2 cells ("↺ "); the gap before "+" is inert.
            let (revert, stage) = match entry_cols(width, *depth, false).buttons {
                Some(EntryButtons::Unstaged { revert, stage }) => (Some(revert), Some(stage)),
                _ => (None, None),
            };
            if col < FILE_ICON_W {
                Some(GitHit::OpenFile(e.rel.clone()))
            } else if stage == Some(col) {
                Some(GitHit::Stage(e.rel.clone()))
            } else if revert.is_some_and(|r| col == r || col == r + 1) {
                Some(GitHit::Revert(e.rel.clone()))
            } else {
                Some(GitHit::Entry(g.staged.len() + *idx))
            }
        }
        // A history row opens the commit's patch; its combined index sits after
        // the change rows.
        GitRowKind::Commit { idx } => Some(GitHit::Entry(g.changes_len() + *idx)),
        _ => None,
    }
}
