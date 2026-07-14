//! Panel selection, tab lifecycle, and opening files (normal + diff tabs).

use super::*;

pub(super) fn select_panel(model: &mut Model, p: Panel) -> Vec<Cmd> {
    model.sidebar.active = p;
    model.layout.sidebar_open = true;
    model.focus = if p == Panel::Search {
        Focus::SearchInput
    } else {
        Focus::Sidebar
    };
    match p {
        Panel::Git => vec![Cmd::LoadGitStatus],
        Panel::Files if model.sidebar.files.children.is_none() => {
            vec![Cmd::ScanDir(model.root.clone())]
        }
        _ => Vec::new(),
    }
}

pub(super) fn close_active_tab(model: &mut Model) {
    if let Some(i) = model.active_tab {
        close_tab(model, i);
    }
}

/// Closes the tab at the given index and fixes up the active tab.
pub(super) fn close_tab(model: &mut Model, i: usize) {
    if i >= model.tabs.len() {
        return;
    }
    model.tabs.remove(i);
    match model.active_tab {
        Some(a) if a == i => {
            if model.tabs.is_empty() {
                model.active_tab = None;
            } else {
                model.active_tab = Some(a.min(model.tabs.len() - 1));
            }
        }
        // If the closed tab is before the active one, the index shifts.
        Some(a) if a > i => model.active_tab = Some(a - 1),
        _ => {}
    }
    model.invalidate_highlight();
}

pub(super) fn cycle_tab(model: &mut Model, delta: isize) {
    if model.tabs.is_empty() {
        return;
    }
    let n = model.tabs.len() as isize;
    let cur = model.active_tab.unwrap_or(0) as isize;
    let next = (cur + delta).rem_euclid(n) as usize;
    model.active_tab = Some(next);
    model.focus = Focus::Editor;
}

pub(super) fn open_path(model: &mut Model, path: PathBuf) -> Vec<Cmd> {
    open_path_at(model, path, 0)
}

/// Opens a file as a diff-mode tab (from the Git panel): reuses an existing diff
/// tab for the path, otherwise loads a fresh one flagged via `pending_diff`.
pub(super) fn open_diff(model: &mut Model, path: PathBuf) -> Vec<Cmd> {
    if let Some(i) = model.diff_tab_index_for(&path) {
        model.active_tab = Some(i);
        model.focus = Focus::Editor;
        ensure_cursor_visible(model);
        return Vec::new();
    }
    model.pending_diff = Some(path.clone());
    vec![Cmd::ReadFile(path)]
}

pub(super) fn open_path_at(model: &mut Model, path: PathBuf, line: usize) -> Vec<Cmd> {
    if let Some(i) = model.tab_index_for(&path) {
        model.active_tab = Some(i);
        model.focus = Focus::Editor;
        if line > 0 {
            model.tabs[i].buffer.goto_line(line);
        }
        ensure_cursor_visible(model);
        Vec::new()
    } else {
        if line > 0 {
            model.pending_goto = Some((path.clone(), line));
        }
        vec![Cmd::ReadFile(path)]
    }
}
