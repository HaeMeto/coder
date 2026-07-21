//! Panel selection, tab lifecycle, and opening files (normal + diff tabs).

use super::*;

pub(super) fn select_panel(model: &mut Model, p: Panel) -> Vec<Cmd> {
    // The gear "panel" is an action, not a sidebar view: it opens config.toml in
    // the editor so settings + languages can be hand-edited.
    if p == Panel::Settings {
        return open_config(model);
    }
    // Clicking the already-active panel toggles the sidebar shut; clicking a
    // different panel (or the same one while collapsed) opens it on that panel.
    if model.layout.sidebar_open && model.sidebar.active == p {
        model.layout.sidebar_open = false;
        if matches!(model.focus, Focus::Sidebar | Focus::SearchInput) {
            model.focus = Focus::Editor;
        }
        return Vec::new();
    }

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
        // Re-probe tool availability each time the panel opens (a binary may have
        // been installed since startup).
        Panel::Extensions => vec![Cmd::CheckTools(model.extensions.tool_commands())],
        _ => Vec::new(),
    }
}

pub(super) fn close_active_tab(model: &mut Model) -> Vec<Cmd> {
    if let Some(i) = model.active_tab {
        if model.tabs[i].buffer.dirty {
            if let Some(ref path) = model.tabs[i].buffer.path {
                let display = path.display().to_string();
                model.focus = Focus::Editor;
                model.dialog = Some(Dialog::ask(
                    "Close tab".to_string(),
                    format!("Changes in '{display}' will be lost. Close anyway?"),
                    DialogAction::CloseTab(i, display),
                ));
                return Vec::new();
            }
        }
        close_tab(model, i)
    } else {
        Vec::new()
    }
}

pub(super) fn close_tab_with_dirty_check(model: &mut Model, i: usize) -> Vec<Cmd> {
    if i >= model.tabs.len() {
        return Vec::new();
    }
    if model.tabs[i].buffer.dirty {
        if let Some(ref path) = model.tabs[i].buffer.path {
            let display = path.display().to_string();
            model.focus = Focus::Editor;
            model.dialog = Some(Dialog::ask(
                "Close tab".to_string(),
                format!("Changes in '{display}' will be lost. Close anyway?"),
                DialogAction::CloseTab(i, display),
            ));
            return Vec::new();
        }
    }
    close_tab(model, i)
}

/// Closes the tab at the given index and fixes up the active tab. Returns a
/// `didClose` for the language server when the last tab of the file is closed.
pub(super) fn close_tab(model: &mut Model, i: usize) -> Vec<Cmd> {
    if i >= model.tabs.len() {
        return Vec::new();
    }
    let path = model.tabs[i].buffer.path.clone();
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
    // Only notify the server once no tab holds the file anymore.
    match path {
        Some(p) if !model.tabs.iter().any(|t| t.buffer.path.as_deref() == Some(p.as_path())) => {
            super::lsp::did_close(model, &p)
        }
        _ => Vec::new(),
    }
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

/// Opens the user config file as an editor tab, creating it (with the current
/// defaults) first if it doesn't exist yet so there is always something to edit.
pub(super) fn open_config(model: &mut Model) -> Vec<Cmd> {
    let Some(path) = crate::services::config::config_path() else {
        model.status_message = "No config path available".to_string();
        return Vec::new();
    };
    if !path.exists() {
        crate::services::config::save(&model.config_snapshot());
    }
    open_path(model, path)
}

/// Compares an open buffer's path to a canonicalized disk path.
fn buf_path_eq(p: &Option<PathBuf>, target: &std::path::Path) -> bool {
    match p {
        Some(p) => p.as_path() == target || p.canonicalize().ok().as_deref() == Some(target),
        None => false,
    }
}

/// A watched file changed on disk: reload it only if it is open and has no
/// unsaved edits (a dirty buffer keeps its live text — see `apply_reload`).
pub(super) fn reload_if_clean(model: &mut Model, path: PathBuf) -> Vec<Cmd> {
    let target = path.canonicalize().unwrap_or(path);
    let has_clean = model
        .tabs
        .iter()
        .any(|t| !t.buffer.dirty && buf_path_eq(&t.buffer.path, &target));
    if has_clean {
        vec![Cmd::ReloadFile(target)]
    } else {
        Vec::new()
    }
}

/// Applies fresh on-disk content to every clean tab of a file, preserving the
/// cursor and scroll position. Dirty tabs and no-op reloads (e.g. our own save)
/// are skipped so unsaved work is never clobbered.
pub(super) fn apply_reload(model: &mut Model, path: PathBuf, text: String) -> Vec<Cmd> {
    let target = path.canonicalize().unwrap_or(path);
    let mut reloaded = false;
    for i in 0..model.tabs.len() {
        if !buf_path_eq(&model.tabs[i].buffer.path, &target) {
            continue;
        }
        let tab = &mut model.tabs[i];
        if tab.buffer.dirty || tab.buffer.full_text() == text {
            continue; // live edits present, or nothing actually changed
        }
        let cur = tab.buffer.cursor;
        let (sy, sx) = (tab.buffer.scroll_y, tab.buffer.scroll_x);
        let keep_path = tab.buffer.path.clone();
        tab.buffer = Buffer::new(keep_path, &text);
        // The new buffer restarts at version 0, colliding with the highlighter's
        // per-version cache — drop it so the reloaded text is re-highlighted
        // instead of redrawing the stale (pre-change) spans.
        tab.highlighter.invalidate();
        // Restore cursor / scroll, clamped to the (possibly shorter) new content.
        let last = tab.buffer.line_count().saturating_sub(1);
        let line = cur.line.min(last);
        let col = cur.col.min(tab.buffer.line_len(line));
        tab.buffer.cursor = Cursor { line, col };
        tab.buffer.scroll_y = sy.min(last);
        tab.buffer.scroll_x = sx;
        tab.buffer.mark_saved();
        reloaded = true;
        if model.active_tab == Some(i) {
            model.invalidate_highlight();
        }
    }
    if reloaded {
        model.status_message = format!("Reloaded from disk: {}", target.display());
        vec![Cmd::LoadHeadText(target)]
    } else {
        Vec::new()
    }
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
            // Center the match (opened from a search result / goto).
            center_cursor_in_view(model);
        } else {
            ensure_cursor_visible(model);
        }
        Vec::new()
    } else {
        if line > 0 {
            model.pending_goto = Some((path.clone(), line));
        }
        vec![Cmd::ReadFile(path)]
    }
}
