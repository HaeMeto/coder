//! apply_action(): maps a resolved keymap Action onto the Model.

use super::*;

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
            apply_format_on_save(model);
            if let Some(buf) = model.active_buffer() {
                if let Some(path) = buf.path.clone() {
                    let contents = buf.full_text();
                    return vec![Cmd::WriteFile { path, contents }];
                }
                model.status_message = "No file path to save to".to_string();
            }
            Vec::new()
        }
        Action::CloseTab => {
            close_active_tab(model);
            Vec::new()
        }
        Action::NextTab => {
            cycle_tab(model, 1);
            Vec::new()
        }
        Action::PrevTab => {
            cycle_tab(model, -1);
            Vec::new()
        }

        // ----- Editor -----
        Action::Insert(c) => edit(model, |b| b.insert_char(c)),
        Action::Newline => edit(model, |b| b.insert_newline()),
        Action::InsertTab => edit(model, |b| b.insert_str("    ")),
        Action::Backspace => edit(model, |b| b.backspace()),
        Action::Delete => edit(model, |b| b.delete_forward()),
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
                    return vec![Cmd::SetClipboard(sel)];
                }
            Vec::new()
        }
        Action::Paste => {
            let text = read_clipboard(model);
            if !text.is_empty() {
                edit(model, |b| b.insert_str(&text));
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

        // ----- Search -----
        Action::SearchChar(c) => {
            match model.sidebar.search.field {
                SearchField::Query => model.sidebar.search.query.push(c),
                SearchField::Replace => model.sidebar.search.replace.push(c),
            }
            Vec::new()
        }
        Action::SearchBackspace => {
            match model.sidebar.search.field {
                SearchField::Query => model.sidebar.search.query.pop(),
                SearchField::Replace => model.sidebar.search.replace.pop(),
            };
            Vec::new()
        }
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
            let query = s.query.clone();
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
                    let replace = s.replace.clone();
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

        // ----- Git commit input -----
        Action::GitCommitChar(c) => {
            model.sidebar.git.commit_msg.push(c);
            Vec::new()
        }
        Action::GitCommitBackspace => {
            model.sidebar.git.commit_msg.pop();
            Vec::new()
        }
        // Enter inserts a newline (multi-line messages); committing is button-only.
        Action::GitCommitSubmit => {
            model.sidebar.git.commit_msg.push('\n');
            Vec::new()
        }

        // ----- In-editor find / replace -----
        Action::OpenFind => open_find(model, false),
        Action::OpenFindReplace => open_find(model, true),
        Action::FindChar(c) => {
            match model.find.field {
                FindField::Query => {
                    model.find.query.push(c);
                    recompute_find(model);
                }
                FindField::Replace => model.find.replace.push(c),
            }
            Vec::new()
        }
        Action::FindBackspace => {
            match model.find.field {
                FindField::Query => {
                    model.find.query.pop();
                    recompute_find(model);
                }
                FindField::Replace => {
                    model.find.replace.pop();
                }
            }
            Vec::new()
        }
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
