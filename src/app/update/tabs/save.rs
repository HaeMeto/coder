//! Saving: Save As for untitled buffers, the quit prompt's Save / Don't Save.

use super::*;

/// Quit confirmation "Save": writes every dirty, on-disk tab synchronously —
/// bypassing the usual async `Cmd::WriteFile` — so quitting right after can't
/// race the write and lose it (the same justified IO-in-`update` carve-out
/// `Msg::ReplaceDone` already uses for its own short synchronous read). This
/// skips the async LSP/tool formatter pass a single-file Ctrl+S runs; only the
/// plain trim/final-newline formatting applies. An untitled buffer has no path
/// to write to and stays dirty — the session checkpoint preserves it either way.
pub(in crate::app::update) fn save_all_and_quit(model: &mut Model) -> Vec<Cmd> {
    let s = &model.sidebar.settings;
    let (trim, final_nl, format_on_save) = (
        s.trim_trailing_whitespace,
        s.insert_final_newline,
        s.format_on_save,
    );
    for tab in model.tabs.iter_mut() {
        if !tab.buffer.dirty || tab.read_only || tab.notice.is_some() {
            continue;
        }
        let Some(path) = tab.buffer.path.clone() else {
            continue; // untitled: nothing to write to, stays dirty
        };
        if format_on_save {
            let formatted = format_text(&tab.buffer.full_text(), trim, final_nl);
            tab.buffer.replace_all(&formatted);
        }
        if std::fs::write(&path, tab.buffer.full_text()).is_ok() {
            tab.buffer.mark_saved();
        }
    }
    model.should_quit = true;
    Vec::new()
}

/// Quit confirmation "Don't Save": quits without writing dirty tabs back to
/// their real files. Nothing is lost — the debounced session checkpoint
/// already holds their content, and `main::run` flushes one final checkpoint
/// before the process exits.
pub(in crate::app::update) fn discard_and_quit(model: &mut Model) -> Vec<Cmd> {
    model.should_quit = true;
    Vec::new()
}

/// Confirms a Save As dialog for a pathless (untitled) tab: resolves the typed
/// name against the workspace root when relative, gives the tab that path
/// (so the async `Msg::FileSaved` that follows can find and mark it clean),
/// and writes it.
pub(in crate::app::update) fn save_as(model: &mut Model, tab_id: usize, name: &str) -> Vec<Cmd> {
    let name = name.trim();
    if name.is_empty() {
        model.notify("Save As: no name entered".to_string());
        return Vec::new();
    }
    let path = PathBuf::from(name);
    let path = if path.is_absolute() {
        path
    } else {
        model.root.join(path)
    };
    let Some(tab) = model.tab_by_id(tab_id) else {
        return Vec::new(); // the tab was closed while the dialog was open
    };
    let t = &mut model.tabs[tab];
    t.buffer.path = Some(path.clone());
    t.untitled_id = None;
    t.label = None; // title() now derives from the new path
    let contents = t.buffer.full_text();
    // The new extension may pick a different syntax.
    if model.active_tab == Some(tab) {
        model.invalidate_highlight();
    }
    vec![Cmd::WriteFile { path, contents }]
}

/// A write finished (`Msg::FileSaved`): marks matching tabs clean and
/// refreshes git / LSP / linter.
pub(in crate::app::update) fn file_saved(
    model: &mut Model,
    path: PathBuf,
    contents: String,
) -> Vec<Cmd> {
    for i in model.all_tabs_for(&path) {
        let buffer = &mut model.tabs[i].buffer;
        if buffer.rope == contents.as_str() {
            buffer.mark_saved();
        }
    }
    model.notify(format!("Saved: {}", path.display()));
    // Reveal the change gutter now: it was frozen while editing, so a save
    // is when the diff catches up to what is on disk.
    model.mark_git_dirty();
    // Refresh git status, notify the language server, and run a linter.
    let mut cmds = vec![Cmd::LoadGitStatus];
    cmds.extend(lsp::did_save(model, &path));
    cmds.extend(lsp::run_linter(model, &path));
    cmds
}

#[cfg(test)]
mod quit_tests {
    use super::*;

    #[test]
    fn save_all_and_quit_writes_dirty_files_but_leaves_untitled_buffers_dirty() {
        let dir = std::env::temp_dir().join(format!("coder-save-all-quit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.txt");
        std::fs::write(&path, "old").unwrap();

        let mut model = Model::new(dir.clone());
        model
            .tabs
            .push(Tab::new(Buffer::new(Some(path.clone()), "old")));
        model.tabs[0].buffer.insert_str("new");
        assert!(model.tabs[0].buffer.dirty);
        new_untitled_tab(&mut model); // untitled: nothing to write to
        model.tabs[1].buffer.insert_str("scratch");

        save_all_and_quit(&mut model);
        assert!(model.should_quit);
        assert!(
            !model.tabs[0].buffer.dirty,
            "on-disk tab is saved and marked clean"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "newold");
        assert!(
            model.tabs[1].buffer.dirty,
            "untitled buffer has nowhere to write to, stays dirty"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discard_and_quit_just_quits_without_writing() {
        let mut model = Model::new(std::env::temp_dir());
        new_untitled_tab(&mut model);
        model.tabs[0].buffer.insert_str("draft");
        discard_and_quit(&mut model);
        assert!(model.should_quit);
        assert!(
            model.tabs[0].buffer.dirty,
            "content is preserved (via the session checkpoint), just not written to a real file"
        );
    }

    #[test]
    fn save_as_gives_an_untitled_tab_a_path_and_writes_it() {
        let dir = std::env::temp_dir().join(format!("coder-save-as-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut model = Model::new(dir.clone());
        new_untitled_tab(&mut model);
        model.tabs[0].buffer.insert_str("hello");
        let id = model.tabs[0].id;
        let cmds = save_as(&mut model, id, "notes.txt");
        assert!(matches!(cmds.as_slice(), [Cmd::WriteFile { .. }]));
        assert_eq!(
            model.tabs[0].buffer.path.as_deref(),
            Some(dir.join("notes.txt").as_path())
        );
        assert!(
            model.tabs[0].untitled_id.is_none(),
            "no longer keyed as untitled"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
