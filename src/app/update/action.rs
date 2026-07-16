//! apply_action(): maps a resolved keymap Action onto the Model.

use super::*;

/// Runs a file-tree action on the selected row. A no-op unless the Files panel
/// is the active one — the shortcuts belong to the tree, not the other panels.
fn on_selected_row(model: &mut Model, f: impl Fn(&mut Model, usize) -> Vec<Cmd>) -> Vec<Cmd> {
    if model.sidebar.active != Panel::Files {
        return Vec::new();
    }
    let idx = model.sidebar.files.selected;
    f(model, idx)
}

pub(super) fn apply_action(model: &mut Model, action: Action) -> Vec<Cmd> {
    match action {
        Action::Quit => {
            model.should_quit = true;
            Vec::new()
        }
        Action::ToggleSidebar => {
            model.layout.sidebar_open = !model.layout.sidebar_open;
            if !model.layout.sidebar_open
                && matches!(model.focus, Focus::Sidebar | Focus::SearchInput)
            {
                model.focus = Focus::Editor;
            } else if model.layout.sidebar_open {
                model.focus = if model.sidebar.active == Panel::Search {
                    Focus::SearchInput
                } else {
                    Focus::Sidebar
                };
            }
            Vec::new()
        }
        Action::ToggleTerminal => {
            model.layout.terminal_open = !model.layout.terminal_open;
            if model.layout.terminal_open {
                model.focus = Focus::Terminal;
                sync_terminal_size(model);
                if model.terminal.session.is_none() && !model.terminal.spawn_requested {
                    model.terminal.spawn_requested = true;
                    return vec![Cmd::SpawnPty {
                        rows: model.terminal.rows,
                        cols: model.terminal.cols,
                    }];
                }
            } else if model.focus == Focus::Terminal {
                model.focus = Focus::Editor;
            }
            Vec::new()
        }
        Action::SelectPanel(p) => select_panel(model, p),
        Action::Save => {
            // Cheap whitespace formatting runs synchronously first.
            apply_format_on_save(model);
            let Some(path) = model.active_buffer().and_then(|b| b.path.clone()) else {
                model.status_message = "No file path to save to".to_string();
                return Vec::new();
            };
            // When format-on-save is on and the language has a formatter (LSP or
            // tool), format asynchronously and defer the write until edits apply.
            if model.sidebar.settings.format_on_save {
                let fmt = super::lsp::request_format(model, true);
                if !fmt.is_empty() {
                    return fmt;
                }
            }
            let contents = model.active_buffer().map(|b| b.full_text()).unwrap_or_default();
            vec![Cmd::WriteFile { path, contents }]
        }
        Action::CloseTab => close_active_tab(model),
        Action::NextTab => {
            cycle_tab(model, 1);
            Vec::new()
        }
        Action::PrevTab => {
            cycle_tab(model, -1);
            Vec::new()
        }

        // ----- Editor -----
        Action::Insert(c) => {
            let mut cmds = edit(model, |b| b.insert_char(c));
            // Auto-trigger completions while typing an identifier or after '.'.
            if c.is_alphanumeric() || c == '_' || c == '.' {
                cmds.extend(super::lsp::request_completion(model));
            }
            cmds
        }
        Action::Newline => edit(model, |b| b.insert_newline()),
        Action::InsertTab => edit(model, |b| b.insert_str("    ")),
        Action::Backspace => {
            let mut cmds = edit(model, |b| b.backspace());
            // Keep an open popup fresh as the prefix shrinks.
            if model.completion.is_some() {
                cmds.extend(super::lsp::request_completion(model));
            }
            cmds
        }
        Action::Delete => edit(model, |b| b.delete_forward()),
        Action::TriggerCompletion => super::lsp::request_completion(model),
        Action::Format => super::lsp::request_format(model, false),
        Action::SelectAll => edit(model, |b| b.select_all()),
        Action::Undo => edit(model, |b| b.undo()),
        Action::Redo => edit(model, |b| b.redo()),
        Action::Move(motion, extend) => {
            let (h, _) = editor_viewport(model);
            edit(model, |b| apply_motion(b, motion, extend, h))
        }
        Action::Copy => {
            if let Some(buf) = model.active_buffer()
                && let Some(sel) = buf.selected_text() {
                    model.internal_clipboard = sel.clone();
                    return vec![Cmd::SetClipboard(sel)];
                }
            Vec::new()
        }
        Action::Cut => {
            if let Some(buf) = model.active_buffer_mut()
                && let Some(sel) = buf.selected_text() {
                    buf.delete_selection();
                    model.internal_clipboard = sel.clone();
                    ensure_cursor_visible(model);
                    let mut cmds = vec![Cmd::SetClipboard(sel)];
                    cmds.extend(super::lsp::notify_change(model));
                    return cmds;
                }
            Vec::new()
        }
        Action::Paste => {
            let text = read_clipboard(model);
            if !text.is_empty() {
                return edit(model, |b| b.insert_str(&text));
            }
            Vec::new()
        }

        // ----- Sidebar navigation -----
        Action::NavUp => {
            nav(model, -1);
            post_nav_persist(model)
        }
        Action::NavDown => {
            nav(model, 1);
            post_nav_persist(model)
        }
        Action::Activate => activate_selection(model),

        // ----- File tree entry management (Files panel only) -----
        Action::NewFile => on_selected_row(model, |m, i| new_entry_dialog(m, i, false)),
        Action::NewFolder => on_selected_row(model, |m, i| new_entry_dialog(m, i, true)),
        Action::RenameEntry => on_selected_row(model, rename_dialog),
        Action::DeleteEntry => on_selected_row(model, delete_dialog),

        // ----- Search (typing handled by the focused input widget) -----
        Action::SearchToggleField => {
            model.sidebar.search.field = match model.sidebar.search.field {
                SearchField::Query => SearchField::Replace,
                SearchField::Replace => SearchField::Query,
            };
            Vec::new()
        }
        Action::SearchToggleRegex => {
            model.sidebar.search.use_regex = !model.sidebar.search.use_regex;
            rerun_search(model)
        }
        Action::SearchSubmit => {
            let s = &model.sidebar.search;
            let query = s.query.content().to_string();
            let (use_regex, match_case, search_hidden) =
                (s.use_regex, s.match_case, s.search_hidden);
            if query.is_empty() {
                return Vec::new();
            }
            match s.field {
                SearchField::Query => {
                    model.focus = Focus::Sidebar;
                    vec![Cmd::RunSearch {
                        query,
                        use_regex,
                        match_case,
                        search_hidden,
                    }]
                }
                // Enter in the Replace field -> replace across all files.
                SearchField::Replace => {
                    let replace = s.replace.content().to_string();
                    model.status_message = "Replacing...".to_string();
                    vec![Cmd::RunReplace {
                        query,
                        replace,
                        use_regex,
                        match_case,
                        search_hidden,
                    }]
                }
            }
        }

        // ----- In-editor find / replace (typing handled by the input widget) -----
        Action::OpenFind => open_find(model, false),
        Action::OpenFindReplace => open_find(model, true),
        Action::FindNext => {
            find_step(model, 1);
            Vec::new()
        }
        Action::FindPrev => {
            find_step(model, -1);
            Vec::new()
        }
        Action::FindToggleField => {
            if model.find.replace_mode {
                model.find.field = match model.find.field {
                    FindField::Query => FindField::Replace,
                    FindField::Replace => FindField::Query,
                };
            }
            Vec::new()
        }
        Action::PtyInput(bytes) => {
            if let Some(session) = model.terminal.session.as_mut() {
                session.write(&bytes);
            }
            Vec::new()
        }
        Action::Escape => {
            match model.focus {
                Focus::Find => close_find(model),
                Focus::Editor => {
                    if let Some(buf) = model.active_buffer_mut() {
                        buf.clear_selection();
                    }
                }
                Focus::Sidebar if model.sidebar.active == Panel::Search => {
                    model.focus = Focus::SearchInput;
                }
                Focus::SearchInput => model.focus = Focus::Editor,
                Focus::GitCommit => model.focus = Focus::Sidebar,
                _ => {}
            }
            Vec::new()
        }
    }
}
