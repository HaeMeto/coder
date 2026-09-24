//! Keeping open tabs in sync with the disk: watcher-driven reloads and forced
//! buffer replacement.

use super::*;

/// Replaces the buffer of any open tab for `path` with `text`, unconditionally
/// (unlike `apply_reload`, which preserves unsaved edits). Marks it saved so the
/// subsequent watcher event is a no-op.
pub(in crate::app::update) fn force_replace_open_buffer(
    model: &mut Model,
    path: &std::path::Path,
    text: &str,
) {
    let target = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    for i in 0..model.tabs.len() {
        if !buf_path_eq(&model.tabs[i].buffer.path, &target) {
            continue;
        }
        // `replace_all` (not a fresh `Buffer`) keeps the version monotonic, so
        // highlight/LSP tokens tied to the old text can't match the new one.
        model.tabs[i].buffer.replace_all(text);
        model.tabs[i].buffer.mark_saved();
        if model.active_tab == Some(i) {
            model.invalidate_highlight();
        }
    }
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
pub(in crate::app::update) fn reload_if_clean(model: &mut Model, path: PathBuf) -> Vec<Cmd> {
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
pub(in crate::app::update) fn apply_reload(
    model: &mut Model,
    path: PathBuf,
    text: String,
) -> Vec<Cmd> {
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
        // `replace_all` keeps undo history (the reload is one undo step), keeps the
        // buffer version monotonic for highlight/LSP tokens, and clamps the cursor.
        let (sy, sx) = (tab.buffer.scroll_y, tab.buffer.scroll_x);
        tab.buffer.replace_all(&text);
        tab.buffer.scroll_y = sy.min(tab.buffer.line_count().saturating_sub(1));
        tab.buffer.scroll_x = sx;
        tab.buffer.mark_saved();
        reloaded = true;
        if model.active_tab == Some(i) {
            model.invalidate_highlight();
        }
    }
    if reloaded {
        model.notify(format!("Reloaded from disk: {}", target.display()));
        let mut cmds = vec![Cmd::LoadHeadText(target)];
        // Keep the language server's copy in sync with the reloaded text.
        cmds.extend(super::lsp::notify_change(model));
        cmds
    } else {
        Vec::new()
    }
}

/// The filesystem watcher saw `path` change (`Msg::DiskChanged`).
pub(in crate::app::update) fn disk_changed(model: &mut Model, path: PathBuf) -> Vec<Cmd> {
    // A change inside `.git` (external `git commit`/stage/checkout, or the
    // embedded terminal) moves HEAD/index/refs. A change to a working-tree
    // file alters its status too. Refresh the git panel so the branch,
    // ahead/behind counts, buttons and the Changes list stay live — but
    // only while it is open, to avoid running `git status` on every keystroke.
    let in_git = path.components().any(|c| c.as_os_str() == ".git");
    // Live-refresh the file tree: if the changed path sits in a directory
    // the tree has loaded, rescan that directory so new/removed entries
    // appear without reopening the folder. `set_children` merges, so
    // expanded subdirs are preserved. Only watched (already-scanned) dirs
    // fire here, so nothing is walked eagerly.
    let mut cmds = Vec::new();
    if !in_git
        && let Some(parent) = path.parent()
        && (parent == model.sidebar.files.root || model.sidebar.files.is_loaded(parent))
    {
        cmds.push(Cmd::ScanDir(parent.to_path_buf()));
    }
    cmds.extend(reload_if_clean(model, path));
    if in_git || model.sidebar.active == Panel::Git {
        cmds.push(Cmd::LoadGitStatus);
    }
    cmds
}

/// A path was renamed (`Msg::PathRenamed`).
pub(in crate::app::update) fn path_renamed(
    model: &mut Model,
    from: PathBuf,
    to: PathBuf,
) -> Vec<Cmd> {
    // Re-point open tabs: a renamed directory moves every file under it.
    let mut cmds = Vec::new();
    for tab in model.tabs.iter_mut() {
        let Some(old) = tab.buffer.path.clone() else {
            continue;
        };
        let Ok(rest) = old.strip_prefix(&from) else {
            continue;
        };
        let new = if rest.as_os_str().is_empty() {
            to.clone()
        } else {
            to.join(rest)
        };
        tab.buffer.path = Some(new.clone());
        cmds.push(Cmd::LoadHeadText(new));
    }
    model.notify(format!("Renamed: {} -> {}", name_of(&from), name_of(&to)));
    cmds.push(Cmd::LoadGitStatus);
    cmds
}

/// A path was deleted (`Msg::PathDeleted`).
pub(in crate::app::update) fn path_deleted(model: &mut Model, path: PathBuf) -> Vec<Cmd> {
    // Close the tabs of deleted files (a whole subtree for a directory).
    // A tab with unsaved edits stays open (dirty): saving recreates it.
    let gone: Vec<usize> = model
        .tabs
        .iter()
        .enumerate()
        .filter(|(_, t)| t.buffer.path.as_ref().is_some_and(|p| p.starts_with(&path)))
        .filter(|(_, t)| !t.buffer.dirty)
        .map(|(i, _)| i)
        .collect();
    let mut cmds: Vec<Cmd> = Vec::new();
    for i in gone.into_iter().rev() {
        cmds.extend(close_tab(model, i));
    }
    model.notify(format!("Deleted '{}'", name_of(&path)));
    cmds.push(Cmd::LoadGitStatus);
    cmds
}

/// The file name of a path, for status messages.
fn name_of(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}
