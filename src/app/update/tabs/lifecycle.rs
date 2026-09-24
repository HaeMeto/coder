//! Tab lifecycle: closing (single / bulk), cycling, new untitled buffers and
//! placing a freshly loaded tab (preview-tab replacement).

use super::*;

pub(in crate::app::update) fn close_active_tab(model: &mut Model) -> Vec<Cmd> {
    if let Some(i) = model.active_tab {
        close_tab_with_dirty_check(model, i)
    } else {
        Vec::new()
    }
}

pub(in crate::app::update) fn close_tab_with_dirty_check(model: &mut Model, i: usize) -> Vec<Cmd> {
    if i >= model.tabs.len() {
        return Vec::new();
    }
    if model.tabs[i].buffer.dirty {
        // Untitled buffers have no path but are just as much "unsaved work" —
        // name the tab by its title (e.g. "Untitled-2") instead of requiring a path.
        let display = model.tabs[i].title();
        model.focus = Focus::Editor;
        model.dialog = Some(Dialog::ask(
            "Close tab".to_string(),
            format!("Changes in '{display}' will be lost. Close anyway?"),
            DialogAction::CloseTab(model.tabs[i].id, display),
        ));
        return Vec::new();
    }
    close_tab(model, i)
}

/// Bulk close from the tab context menu (Close Others/Right/Left/All). Tabs
/// with unsaved changes are kept open (reported in a toast) rather than
/// asking once per tab. `keep` — the tab the menu was opened on, when it
/// survives — becomes the active tab.
pub(in crate::app::update) fn close_tabs(
    model: &mut Model,
    indices: Vec<usize>,
    keep: Option<usize>,
) -> Vec<Cmd> {
    let mut cmds = Vec::new();
    let mut kept_dirty = 0;
    let mut removed_before_keep = 0;
    let mut sorted = indices;
    sorted.sort_unstable_by(|a, b| b.cmp(a)); // highest first: indices stay valid
    for i in sorted {
        if i >= model.tabs.len() {
            continue;
        }
        if model.tabs[i].buffer.dirty {
            kept_dirty += 1;
            continue;
        }
        if keep.is_some_and(|k| i < k) {
            removed_before_keep += 1;
        }
        cmds.extend(close_tab(model, i));
    }
    if let Some(k) = keep
        .map(|k| k - removed_before_keep)
        .filter(|&k| k < model.tabs.len())
    {
        model.active_tab = Some(k);
        model.invalidate_highlight();
    }
    if kept_dirty > 0 {
        model.notify(format!(
            "{kept_dirty} tab(s) with unsaved changes kept open"
        ));
    }
    cmds
}

/// Closes the tab at the given index and fixes up the active tab. Returns a
/// `didClose` for the language server when the last tab of the file is closed.
pub(in crate::app::update) fn close_tab(model: &mut Model, i: usize) -> Vec<Cmd> {
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
        Some(p)
            if !model
                .tabs
                .iter()
                .any(|t| t.buffer.path.as_deref() == Some(p.as_path())) =>
        {
            lsp::did_close(model, &p)
        }
        _ => Vec::new(),
    }
}

pub(in crate::app::update) fn cycle_tab(model: &mut Model, delta: isize) {
    if model.tabs.is_empty() {
        return;
    }
    let n = model.tabs.len() as isize;
    let cur = model.active_tab.unwrap_or(0) as isize;
    let next = (cur + delta).rem_euclid(n) as usize;
    model.active_tab = Some(next);
    model.focus = Focus::Editor;
}

/// Creates a fresh "Untitled-N" scratch buffer (`Action::NewUntitledFile`,
/// Ctrl+N from the editor) — typed into first, named on save.
pub(in crate::app::update) fn new_untitled_tab(model: &mut Model) -> Vec<Cmd> {
    let seq = model.next_untitled_seq();
    let tab = Tab::untitled(crate::services::session::new_untitled_id(), seq);
    model.tabs.push(tab);
    model.active_tab = Some(model.tabs.len() - 1);
    model.focus = Focus::Editor;
    model.invalidate_highlight();
    Vec::new()
}

/// Adds a freshly loaded tab. A preview tab takes the place of the current
/// (clean) preview tab instead of opening another one. Returns the tab's index
/// and the LSP `didClose` for the file it displaced, if any.
pub(in crate::app::update) fn place_tab(model: &mut Model, tab: Tab) -> (usize, Vec<Cmd>) {
    let old = if tab.preview {
        model.tabs.iter().position(|t| t.preview && !t.buffer.dirty)
    } else {
        None
    };
    let Some(i) = old else {
        model.tabs.push(tab);
        return (model.tabs.len() - 1, Vec::new());
    };
    let old_path = std::mem::replace(&mut model.tabs[i], tab).buffer.path;
    model.invalidate_highlight();
    let cmds = match old_path {
        Some(p)
            if !model
                .tabs
                .iter()
                .any(|t| t.buffer.path.as_deref() == Some(p.as_path())) =>
        {
            lsp::did_close(model, &p)
        }
        _ => Vec::new(),
    };
    (i, cmds)
}

#[cfg(test)]
mod untitled_tests {
    use super::*;

    #[test]
    fn new_untitled_tab_opens_a_focused_pathless_buffer() {
        let mut model = Model::new(std::env::temp_dir());
        new_untitled_tab(&mut model);
        assert_eq!(model.tabs.len(), 1);
        assert!(model.tabs[0].buffer.path.is_none());
        assert!(model.tabs[0].untitled_id.is_some());
        assert_eq!(model.tabs[0].title(), "Untitled-1");
        assert_eq!(model.active_tab, Some(0));
        assert_eq!(model.focus, Focus::Editor);

        // A second one gets the next number, not a repeat.
        new_untitled_tab(&mut model);
        assert_eq!(model.tabs[1].title(), "Untitled-2");
    }

    #[test]
    fn closing_a_dirty_untitled_tab_asks_first() {
        let mut model = Model::new(std::env::temp_dir());
        new_untitled_tab(&mut model);
        model.tabs[0].buffer.insert_char('x');
        assert!(model.tabs[0].buffer.dirty);

        let cmds = close_tab_with_dirty_check(&mut model, 0);
        assert!(cmds.is_empty());
        assert!(
            model.dialog.is_some(),
            "an untitled dirty tab must be confirmed, not silently dropped"
        );
        assert_eq!(model.tabs.len(), 1, "not closed yet");
    }
}

#[cfg(test)]
mod close_many_tests {
    use super::*;

    fn model_with(n: usize) -> Model {
        let mut model = Model::new(std::env::temp_dir());
        for i in 0..n {
            let path = PathBuf::from(format!("/w/{i}.rs"));
            model.tabs.push(Tab::new(Buffer::new(Some(path), "x\n")));
        }
        model.active_tab = Some(0);
        model
    }

    fn names(model: &Model) -> Vec<String> {
        model.tabs.iter().map(|t| t.title()).collect()
    }

    #[test]
    fn close_others_keeps_the_clicked_tab_active() {
        let mut model = model_with(4);
        close_tabs(&mut model, vec![0, 1, 3], Some(2));
        assert_eq!(names(&model), ["2.rs"]);
        assert_eq!(model.active_tab, Some(0));
    }

    #[test]
    fn dirty_tabs_survive_a_bulk_close() {
        let mut model = model_with(3);
        model.tabs[0].buffer.dirty = true;
        close_tabs(&mut model, vec![0, 2], Some(1));
        assert_eq!(names(&model), ["0.rs", "1.rs"]);
        assert_eq!(model.active_tab, Some(1));
    }
}
