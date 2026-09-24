//! Opening files: normal tabs, working-tree diff tabs, commit diff tabs and
//! the preview tab.

use super::*;

pub(in crate::app::update) fn open_path(model: &mut Model, path: PathBuf) -> Vec<Cmd> {
    open_path_at(model, path, 0)
}

pub(in crate::app::update) fn open_path_at(
    model: &mut Model,
    path: PathBuf,
    line: usize,
) -> Vec<Cmd> {
    cancel_preview(model, &PreviewKey::File(path.clone()));
    if let Some(i) = model.tab_index_for(&path) {
        model.tabs[i].preview = false;
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

/// Opens a file as a diff-mode tab (from the Git panel): reuses an existing diff
/// tab for the path, otherwise loads a fresh one flagged via `pending_diff`.
pub(in crate::app::update) fn open_diff(model: &mut Model, path: PathBuf) -> Vec<Cmd> {
    cancel_preview(model, &PreviewKey::Diff(path.clone()));
    if let Some(i) = model.diff_tab_index_for(&path) {
        model.tabs[i].preview = false;
        model.active_tab = Some(i);
        model.focus = Focus::Editor;
        ensure_cursor_visible(model);
        return Vec::new();
    }
    model.pending_diff = Some(path.clone());
    vec![Cmd::ReadFile(path)]
}

/// Opens the patch of a history commit as a read-only "<hash> diff" tab: focuses
/// the tab if it is already open, otherwise asks git for the patch (the tab is
/// created when `Msg::CommitDiffLoaded` arrives).
pub(in crate::app::update) fn open_commit_diff(model: &mut Model, hash: String) -> Vec<Cmd> {
    cancel_preview(model, &PreviewKey::Commit(hash.clone()));
    if let Some(i) = model.commit_diff_tab_index(&hash) {
        model.tabs[i].preview = false;
        model.active_tab = Some(i);
        model.focus = Focus::Editor;
        ensure_cursor_visible(model);
        return Vec::new();
    }
    vec![Cmd::LoadCommitDiff(hash)]
}

/// Creates (or refreshes) the read-only diff tab holding a commit's changes.
/// A `preview` load reuses the preview tab and leaves focus in the sidebar.
pub(in crate::app::update) fn show_commit_diff(
    model: &mut Model,
    hash: &str,
    diff: &crate::services::git::CommitDiff,
    preview: bool,
) -> Vec<Cmd> {
    let mut tab = Tab::commit_diff(hash, diff);
    let (i, cmds) = match model.commit_diff_tab_index(hash) {
        Some(i) => {
            tab.preview = preview && model.tabs[i].preview;
            model.tabs[i] = tab;
            (i, Vec::new())
        }
        None => {
            tab.preview = preview;
            place_tab(model, tab)
        }
    };
    model.active_tab = Some(i);
    if !preview {
        model.focus = Focus::Editor;
    }
    // The green/red backgrounds come from the parent-vs-commit diff: it has to be
    // computed for the new tab before the first render.
    model.invalidate_highlight();
    model.mark_git_dirty();
    ensure_cursor_visible(model);
    cmds
}

/// Previews `key` in the preview tab while the keyboard stays in the sidebar
/// (arrow keys in the Files/Git panel). An already-open tab for it is just
/// shown; otherwise the load is requested and tracked in `preview_loads`.
pub(in crate::app::update) fn preview(model: &mut Model, key: PreviewKey) -> Vec<Cmd> {
    let open = match &key {
        PreviewKey::File(p) => model.tab_index_for(p),
        PreviewKey::Diff(p) => model.diff_tab_index_for(p),
        PreviewKey::Commit(h) => model.commit_diff_tab_index(h),
    };
    if let Some(i) = open {
        model.pending_preview = None;
        model.active_tab = Some(i);
        ensure_cursor_visible(model);
        return Vec::new();
    }
    let cmd = match &key {
        PreviewKey::File(p) | PreviewKey::Diff(p) => Cmd::ReadFile(p.clone()),
        PreviewKey::Commit(h) => Cmd::LoadCommitDiff(h.clone()),
    };
    *model.preview_loads.entry(key.clone()).or_insert(0) += 1;
    model.pending_preview = Some(key);
    vec![cmd]
}

/// A real open (Enter/click) of `key` while its preview load is still in
/// flight: that load must not claim the tab, so it is dropped as stale and the
/// open's own load creates a normal tab.
fn cancel_preview(model: &mut Model, key: &PreviewKey) {
    if model.pending_preview.as_ref() == Some(key) {
        model.pending_preview = None;
    }
}

/// How an arriving load relates to the preview machinery.
pub(in crate::app::update) enum PreviewLoad {
    /// Not a preview load: open it the normal way.
    NotPreview,
    /// A preview load the user has already arrowed past: discard it.
    Stale,
    /// The preview the user is currently on.
    Current(PreviewKey),
}

/// Consumes one in-flight preview load matching `is_match`, reporting whether
/// it is still the wanted preview.
pub(in crate::app::update) fn take_preview_load(
    model: &mut Model,
    is_match: impl Fn(&PreviewKey) -> bool,
) -> PreviewLoad {
    let Some(key) = model.preview_loads.keys().find(|k| is_match(k)).cloned() else {
        return PreviewLoad::NotPreview;
    };
    if let Some(n) = model.preview_loads.get_mut(&key) {
        *n -= 1;
        if *n == 0 {
            model.preview_loads.remove(&key);
        }
    }
    if model.pending_preview.as_ref() == Some(&key) {
        model.pending_preview = None;
        PreviewLoad::Current(key)
    } else {
        PreviewLoad::Stale
    }
}

/// A file read finished (`Msg::FileLoaded`): opens it as a tab (normal,
/// diff or preview), applying any pending session restore or goto.
pub(in crate::app::update) fn file_loaded(
    model: &mut Model,
    path: PathBuf,
    text: String,
) -> Vec<Cmd> {
    let buffer = Buffer::new(Some(path.clone()), &text);
    let mut tab = Tab::new(buffer);
    // A preview load (arrow keys in the Files/Git panel): drop it when
    // the user has already moved on, else it becomes the preview tab.
    let preview = match take_preview_load(model, |p| match p {
        PreviewKey::File(p) | PreviewKey::Diff(p) => *p == path,
        PreviewKey::Commit(_) => false,
    }) {
        PreviewLoad::NotPreview => false,
        PreviewLoad::Stale => return Vec::new(),
        PreviewLoad::Current(key) => {
            if matches!(key, PreviewKey::Diff(_)) {
                tab.diff_mode = true;
                model.pending_diff_scroll = Some(path.clone());
            }
            true
        }
    };
    tab.preview = preview;
    // A load requested from the Git panel becomes a diff-mode tab.
    if !preview && model.pending_diff.as_deref() == Some(path.as_path()) {
        tab.diff_mode = true;
        model.pending_diff = None;
        // Scroll to the first change once HEAD text arrives (marks need it).
        model.pending_diff_scroll = Some(path.clone());
    }

    // Session restore: cursor/scroll for every restored tab, and for a
    // dirty file that was checkpointed as a diff (large file), rebuild
    // its unsaved content by applying the stored hunks over the disk
    // text just read.
    let restore = model.pending_session_restore.remove(&path);
    if let Some(r) = &restore {
        if let Some(hunks) = &r.dirty_hunks {
            match crate::services::session::apply_hunks(&text, hunks) {
                Some(restored) => {
                    tab.buffer = Buffer::new(Some(path.clone()), &restored);
                    tab.buffer.dirty = true;
                }
                None => model.notify(format!(
                    "Could not restore unsaved changes for {}: the file changed too much",
                    path.display()
                )),
            }
        }
        let last = tab.buffer.line_count().saturating_sub(1);
        let line = r.line.min(last);
        let col = r.col.min(tab.buffer.line_len(line));
        tab.buffer.cursor = Cursor { line, col };
        tab.buffer.scroll_y = r.scroll_y.min(last);
        tab.buffer.scroll_x = r.scroll_x;
    }
    let is_restore = restore.is_some();

    // A duplicate load (double Enter, or a user open racing a session
    // restore of the same file) must not open a second tab — that would
    // also send the language server a second `didOpen` for the URI.
    if !tab.diff_mode
        && !tab.preview
        && let Some(i) = model.tab_index_for(&path)
    {
        if !is_restore {
            model.active_tab = Some(i);
            model.focus = Focus::Editor;
        } else if model.session_active_path.as_deref() == Some(path.as_path()) {
            // Release the restore focus guard (see below) for this path.
            model.session_active_path = None;
            model.active_tab = Some(i);
        }
        return Vec::new();
    }

    let (idx, mut cmds) = place_tab(model, tab);

    // While a session restore is choosing which tab should end up
    // focused, only the matching load may claim `active_tab` —
    // otherwise whichever file's async read happens to finish last
    // would win the focus race.
    let restoring_to_other = model
        .session_active_path
        .as_deref()
        .is_some_and(|p| p != path.as_path());
    if !restoring_to_other {
        model.active_tab = Some(idx);
        // A preview keeps the keyboard in the sidebar list.
        if !preview {
            model.focus = Focus::Editor;
        }
        if model.session_active_path.as_deref() == Some(path.as_path()) {
            model.session_active_path = None;
        }
        if !is_restore {
            // Apply a pending goto if there is one (from a search
            // result): center it. Restored cursor/scroll (above) is
            // already exact, so this only runs for a fresh open.
            let goto = model.pending_goto.take().filter(|(gp, _)| *gp == path);
            if let Some((_, line)) = goto {
                if let Some(buf) = model.active_buffer_mut() {
                    buf.goto_line(line);
                }
                center_cursor_in_view(model);
            } else {
                ensure_cursor_visible(model);
            }
        }
    }
    // Load the HEAD content for the change gutter, and open the document
    // with its language server (if any).
    cmds.push(Cmd::LoadHeadText(path));
    cmds.extend(lsp::open_tab(model, idx));
    cmds
}

/// A file read failed (`Msg::FileLoadFailed`): shows a read-only notice tab
/// (or a toast, for a tab with unsaved edits).
pub(in crate::app::update) fn file_load_failed(
    model: &mut Model,
    path: PathBuf,
    error: String,
) -> Vec<Cmd> {
    // A session restore was waiting on this file (see `update::session`):
    // release the guard so it doesn't block every load after it from
    // ever claiming `active_tab` again.
    model.pending_session_restore.remove(&path);
    if model.session_active_path.as_deref() == Some(path.as_path()) {
        model.session_active_path = None;
    }
    // Settle the in-flight preview count, or a later successful load of
    // this path would be judged stale and dropped.
    let preview = match take_preview_load(model, |p| match p {
        PreviewKey::File(p) | PreviewKey::Diff(p) => *p == path,
        PreviewKey::Commit(_) => false,
    }) {
        PreviewLoad::NotPreview => false,
        PreviewLoad::Stale => return Vec::new(),
        PreviewLoad::Current(_) => true,
    };
    // Reuse an already-open tab for this file, else open a read-only notice tab.
    if let Some(i) = model.tab_index_for(&path) {
        if model.tabs[i].buffer.dirty {
            // Never turn a tab with unsaved edits into an uneditable notice.
            model.notify(format!("Could not read {}: {error}", path.display()));
            return Vec::new();
        }
        model.tabs[i].notice = Some(error);
        model.active_tab = Some(i);
    } else {
        let mut tab = Tab::notice(path, error);
        tab.preview = preview;
        let (idx, cmds) = place_tab(model, tab);
        model.active_tab = Some(idx);
        if !preview {
            model.focus = Focus::Editor;
        }
        return cmds;
    }
    if !preview {
        model.focus = Focus::Editor;
    }
    Vec::new()
}

/// A commit's patch arrived (`Msg::CommitDiffLoaded`).
pub(in crate::app::update) fn commit_diff_loaded(
    model: &mut Model,
    hash: String,
    diff: crate::services::git::CommitDiff,
) -> Vec<Cmd> {
    let preview = match take_preview_load(model, |k| *k == PreviewKey::Commit(hash.clone())) {
        PreviewLoad::NotPreview => false,
        PreviewLoad::Stale => return Vec::new(),
        PreviewLoad::Current(_) => true,
    };
    show_commit_diff(model, &hash, &diff, preview)
}

#[cfg(test)]
mod preview_tests {
    use super::*;

    fn load(model: &mut Model, path: &str) -> Vec<Cmd> {
        crate::app::update::update(
            model,
            Msg::FileLoaded {
                path: PathBuf::from(path),
                text: format!("{path}\n"),
            },
        )
    }

    #[test]
    fn arrowing_through_files_reuses_one_preview_tab() {
        let mut model = Model::new(std::env::temp_dir());
        model.focus = Focus::Sidebar;
        preview(&mut model, PreviewKey::File("/w/a.rs".into()));
        load(&mut model, "/w/a.rs");
        assert_eq!(model.tabs.len(), 1);
        assert!(model.tabs[0].preview);
        assert_eq!(
            model.focus,
            Focus::Sidebar,
            "preview keeps the keyboard in the list"
        );

        preview(&mut model, PreviewKey::File("/w/b.rs".into()));
        load(&mut model, "/w/b.rs");
        assert_eq!(model.tabs.len(), 1, "the preview tab is replaced in place");
        assert_eq!(
            model.tabs[0].buffer.path.as_deref(),
            Some(std::path::Path::new("/w/b.rs"))
        );
    }

    #[test]
    fn stale_preview_loads_are_dropped() {
        let mut model = Model::new(std::env::temp_dir());
        preview(&mut model, PreviewKey::File("/w/a.rs".into()));
        preview(&mut model, PreviewKey::File("/w/b.rs".into()));
        load(&mut model, "/w/a.rs"); // the user already moved past a.rs
        assert!(model.tabs.is_empty());
        load(&mut model, "/w/b.rs");
        assert_eq!(model.tabs.len(), 1);
        assert!(model.preview_loads.is_empty());
    }

    #[test]
    fn opening_the_previewed_file_makes_it_permanent() {
        let mut model = Model::new(std::env::temp_dir());
        preview(&mut model, PreviewKey::File("/w/a.rs".into()));
        load(&mut model, "/w/a.rs");
        open_path(&mut model, "/w/a.rs".into());
        assert!(!model.tabs[0].preview);
        assert_eq!(model.focus, Focus::Editor);

        preview(&mut model, PreviewKey::File("/w/b.rs".into()));
        load(&mut model, "/w/b.rs");
        assert_eq!(model.tabs.len(), 2, "a permanent tab is never replaced");
    }

    #[test]
    fn git_preview_opens_a_diff_tab() {
        let mut model = Model::new(std::env::temp_dir());
        preview(&mut model, PreviewKey::Diff("/w/a.rs".into()));
        load(&mut model, "/w/a.rs");
        assert!(model.tabs[0].preview && model.tabs[0].diff_mode);
    }
}
