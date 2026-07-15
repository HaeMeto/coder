//! update(): takes a Msg, updates the Model, returns side-effect Cmds.
//!
//! Split into focused submodules by concern. This file holds the top-level
//! Msg dispatcher plus the shared imports every submodule pulls in via
//! `use super::*`.

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::app::cmd::Cmd;
use crate::app::model::{
    Dialog, DialogAction, DialogKind, DragTarget, FindField, Focus, Model, Panel, SearchField, Tab,
};
use crate::app::msg::Msg;
use crate::core::buffer::{Buffer, Cursor};
use crate::core::keymap::{self, Action, Motion};
use crate::core::text_input::{InputOutcome, TextInputState};
use crate::ui;

mod action;
mod dialog;
mod editor;
mod find;
mod git;
mod mouse;
mod search;
mod sidebar_nav;
mod tabs;
mod terminal;

use action::apply_action;
use dialog::{dialog_key, dialog_mouse};
use editor::*;
use find::*;
use git::*;
use mouse::handle_mouse;
use search::*;
use sidebar_nav::*;
use tabs::*;
use terminal::sync_terminal_size;

pub fn update(model: &mut Model, msg: Msg) -> Vec<Cmd> {
    match msg {
        Msg::Key(key) => {
            // If a modal dialog is open it captures all keyboard input.
            if model.dialog.is_some() {
                return dialog_key(model, key);
            }
            // A focused text input handles its own editing/motion keys first and
            // reports back what it did; only keys it ignores fall through to the
            // keymap (Enter/Tab/Esc, Ctrl-shortcuts, match/result navigation).
            if let Some((input, multiline)) = focused_input(model) {
                match input.handle_key(key, multiline) {
                    InputOutcome::Ignored => {}
                    InputOutcome::Moved => return Vec::new(),
                    InputOutcome::Changed => {
                        // Live find: re-run matches when the query text changes.
                        if model.focus == Focus::Find && model.find.field == FindField::Query {
                            recompute_find(model);
                        }
                        return Vec::new();
                    }
                }
            }
            if let Some(action) = keymap::resolve(key, model.focus) {
                return apply_action(model, action);
            }
            Vec::new()
        }
        Msg::Mouse(m) => {
            if model.dialog.is_some() {
                return dialog_mouse(model, m);
            }
            handle_mouse(model, m)
        }
        Msg::Resize(w, h) => {
            model.term_size = (w, h);
            sync_terminal_size(model);
            Vec::new()
        }
        Msg::DirScanned { path, entries } => {
            model.sidebar.files.set_children(&path, entries);
            Vec::new()
        }
        Msg::FileLoaded { path, text } => {
            let buffer = Buffer::new(Some(path.clone()), &text);
            let mut tab = Tab::new(buffer);
            // Highlight with the active theme (Tab::new defaults to DEFAULT_THEME).
            tab.highlighter.set_theme(model.current_theme_name());
            // A load requested from the Git panel becomes a diff-mode tab.
            if model.pending_diff.as_deref() == Some(path.as_path()) {
                tab.diff_mode = true;
                model.pending_diff = None;
                // Scroll to the first change once HEAD text arrives (marks need it).
                model.pending_diff_scroll = Some(path.clone());
            }
            model.tabs.push(tab);
            model.active_tab = Some(model.tabs.len() - 1);
            model.focus = Focus::Editor;
            // Apply a pending goto if there is one (from a search result): center it.
            let goto = model.pending_goto.take().filter(|(gp, _)| *gp == path);
            if let Some((_, line)) = goto {
                if let Some(buf) = model.active_buffer_mut() {
                    buf.goto_line(line);
                }
                center_cursor_in_view(model);
            } else {
                ensure_cursor_visible(model);
            }
            // Load the HEAD content for the change gutter.
            vec![Cmd::LoadHeadText(path)]
        }
        Msg::HeadTextLoaded { path, text } => {
            // Update every open tab for this file (a normal tab and its diff tab).
            for i in model.all_tabs_for(&path) {
                model.tabs[i].head_text = text.clone();
                if model.active_tab == Some(i) {
                    model.invalidate_highlight();
                }
            }
            // First open of a diff tab: jump the cursor to the first changed line so
            // the diff is on screen without scrolling.
            if model.pending_diff_scroll.as_deref() == Some(path.as_path()) {
                model.pending_diff_scroll = None;
                model.refresh_git_marks();
                if let Some(first) = model.active_git_marks.keys().min().copied() {
                    if let Some(buf) = model.active_buffer_mut() {
                        buf.goto_line(first);
                    }
                    // Put the first change at the top, not the bottom of the viewport.
                    scroll_cursor_to_top(model);
                }
            }
            Vec::new()
        }
        Msg::DiskChanged(path) => {
            // A change inside `.git` (external `git commit`/stage/checkout, or the
            // embedded terminal) moves HEAD/index/refs — refresh the git panel so
            // the branch, ahead/behind counts and the fetch/pull/push buttons
            // reflect it. Reloading only open buffers would leave the panel stale.
            let in_git = path.components().any(|c| c.as_os_str() == ".git");
            let mut cmds = reload_if_clean(model, path);
            if in_git {
                cmds.push(Cmd::LoadGitStatus);
            }
            cmds
        }
        Msg::FileReloaded { path, text } => apply_reload(model, path, text),
        Msg::FileSaved { path } => {
            for i in model.all_tabs_for(&path) {
                model.tabs[i].buffer.mark_saved();
            }
            model.status_message = format!("Saved: {}", path.display());
            // Refresh git status after saving.
            vec![Cmd::LoadGitStatus]
        }
        Msg::GitStatusLoaded {
            branch,
            staged,
            unstaged,
            is_repo,
            ahead,
            behind,
            has_upstream,
            has_remote,
        } => {
            let g = &mut model.sidebar.git;
            g.branch = branch;
            g.staged = staged;
            g.unstaged = unstaged;
            g.is_repo = is_repo;
            g.ahead = ahead;
            g.behind = behind;
            g.has_upstream = has_upstream;
            g.has_remote = has_remote;
            let len = g.nav_len();
            if g.selected >= len {
                g.selected = len.saturating_sub(1);
            }
            // Refresh the change gutter for open files (HEAD may have moved after a commit/revert).
            model
                .tabs
                .iter()
                .filter_map(|t| t.buffer.path.clone())
                .map(Cmd::LoadHeadText)
                .collect()
        }
        Msg::SearchResults { query, matches } => {
            if query == model.sidebar.search.query.content() {
                model.sidebar.search.results = matches;
                model.sidebar.search.selected = 0;
                model.status_message =
                    format!("{} results found", model.sidebar.search.results.len());
            }
            Vec::new()
        }
        Msg::ReplaceDone { changed, count } => {
            // Reload buffers that are open and changed on disk.
            for path in &changed {
                if let Ok(text) = std::fs::read_to_string(path) {
                    for i in model.all_tabs_for(path) {
                        model.tabs[i].buffer = Buffer::new(Some(path.clone()), &text);
                    }
                }
            }
            model.invalidate_highlight();
            model.status_message =
                format!("{} changes, {} files", count, changed.len());
            // Refresh the results and git status.
            let mut cmds = vec![Cmd::LoadGitStatus];
            let s = &model.sidebar.search;
            if !s.query.is_empty() {
                cmds.push(Cmd::RunSearch {
                    query: s.query.content().to_string(),
                    use_regex: s.use_regex,
                    match_case: s.match_case,
                    search_hidden: s.search_hidden,
                });
            }
            cmds
        }
        Msg::PtyReady(session) => {
            model.terminal.session = Some(session);
            model.terminal.spawn_requested = false;
            sync_terminal_size(model);
            Vec::new()
        }
        Msg::PtyOutput(bytes) => {
            model.terminal.parser.process(&bytes);
            Vec::new()
        }
        Msg::PtyExited => {
            model.terminal.session = None;
            model.terminal.spawn_requested = false;
            model.status_message = "Terminal closed".to_string();
            Vec::new()
        }
        Msg::Status(s) => {
            model.status_message = s;
            Vec::new()
        }
        Msg::Error(e) => {
            model.status_message = format!("Error: {e}");
            Vec::new()
        }
    }
}

/// The text input the current focus routes keys to, and whether it is multi-line.
/// `None` when focus is not on a text field.
fn focused_input(model: &mut Model) -> Option<(&mut TextInputState, bool)> {
    match model.focus {
        Focus::Find => {
            let f = match model.find.field {
                FindField::Query => &mut model.find.query,
                FindField::Replace => &mut model.find.replace,
            };
            Some((f, false))
        }
        Focus::SearchInput => {
            let s = match model.sidebar.search.field {
                SearchField::Query => &mut model.sidebar.search.query,
                SearchField::Replace => &mut model.sidebar.search.replace,
            };
            Some((s, false))
        }
        Focus::GitCommit => Some((&mut model.sidebar.git.commit, true)),
        _ => None,
    }
}
