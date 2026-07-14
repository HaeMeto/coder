//! update(): takes a Msg, updates the Model, returns side-effect Cmds.

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::app::cmd::Cmd;
use crate::app::model::{
    Dialog, DialogAction, DialogKind, DragTarget, Focus, Model, Panel, SearchField, Tab,
};
use crate::app::msg::Msg;
use crate::core::buffer::{Buffer, Cursor};
use crate::core::keymap::{self, Action, Motion};
use crate::ui;

pub fn update(model: &mut Model, msg: Msg) -> Vec<Cmd> {
    match msg {
        Msg::Key(key) => {
            // If a modal dialog is open it captures all keyboard input.
            if model.dialog.is_some() {
                return dialog_key(model, key);
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
            let tab = Tab::new(buffer);
            model.tabs.push(tab);
            model.active_tab = Some(model.tabs.len() - 1);
            model.focus = Focus::Editor;
            // Apply a pending goto if there is one.
            if let Some((gp, line)) = model.pending_goto.take()
                && gp == path
                    && let Some(buf) = model.active_buffer_mut() {
                        buf.goto_line(line);
                    }
            ensure_cursor_visible(model);
            Vec::new()
        }
        Msg::FileSaved { path } => {
            if let Some(i) = model.tab_index_for(&path) {
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
        } => {
            let g = &mut model.sidebar.git;
            g.branch = branch;
            g.staged = staged;
            g.unstaged = unstaged;
            g.is_repo = is_repo;
            let len = g.nav_len();
            if g.selected >= len {
                g.selected = len.saturating_sub(1);
            }
            Vec::new()
        }
        Msg::SearchResults { query, matches } => {
            if query == model.sidebar.search.query {
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
                if let Some(i) = model.tab_index_for(path)
                    && let Ok(text) = std::fs::read_to_string(path)
                {
                    model.tabs[i].buffer = Buffer::new(Some(path.clone()), &text);
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
                    query: s.query.clone(),
                    use_regex: s.use_regex,
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

fn apply_action(model: &mut Model, action: Action) -> Vec<Cmd> {
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
            Vec::new()
        }
        Action::NavDown => {
            nav(model, 1);
            Vec::new()
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
            Vec::new()
        }
        Action::SearchSubmit => {
            let s = &model.sidebar.search;
            let query = s.query.clone();
            let use_regex = s.use_regex;
            if query.is_empty() {
                return Vec::new();
            }
            match s.field {
                SearchField::Query => {
                    model.focus = Focus::Sidebar;
                    vec![Cmd::RunSearch { query, use_regex }]
                }
                // Enter in the Replace field -> replace across all files.
                SearchField::Replace => {
                    let replace = s.replace.clone();
                    model.status_message = "Replacing...".to_string();
                    vec![Cmd::RunReplace {
                        query,
                        replace,
                        use_regex,
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
        Action::GitCommitSubmit => git_commit(model),

        Action::PtyInput(bytes) => {
            if let Some(session) = model.terminal.session.as_mut() {
                session.write(&bytes);
            }
            Vec::new()
        }
        Action::Escape => {
            match model.focus {
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

// ----- Helpers -----

/// Applies an editor mutation and keeps the cursor visible.
fn edit(model: &mut Model, f: impl FnOnce(&mut Buffer)) -> Vec<Cmd> {
    if let Some(buf) = model.active_buffer_mut() {
        f(buf);
    }
    ensure_cursor_visible(model);
    Vec::new()
}

fn apply_motion(b: &mut Buffer, motion: Motion, extend: bool, page: usize) {
    match motion {
        Motion::Left => b.move_left(extend),
        Motion::Right => b.move_right(extend),
        Motion::Up => b.move_up(extend),
        Motion::Down => b.move_down(extend),
        Motion::Home => b.move_home(extend),
        Motion::End => b.move_end(extend),
        Motion::PageUp => b.move_page(-(page as isize), extend),
        Motion::PageDown => b.move_page(page as isize, extend),
    }
}

fn read_clipboard(model: &Model) -> String {
    if let Ok(mut cb) = arboard::Clipboard::new()
        && let Ok(text) = cb.get_text()
            && !text.is_empty() {
                return text;
            }
    model.internal_clipboard.clone()
}

fn select_panel(model: &mut Model, p: Panel) -> Vec<Cmd> {
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

fn close_active_tab(model: &mut Model) {
    if let Some(i) = model.active_tab {
        close_tab(model, i);
    }
}

/// Closes the tab at the given index and fixes up the active tab.
fn close_tab(model: &mut Model, i: usize) {
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

fn cycle_tab(model: &mut Model, delta: isize) {
    if model.tabs.is_empty() {
        return;
    }
    let n = model.tabs.len() as isize;
    let cur = model.active_tab.unwrap_or(0) as isize;
    let next = (cur + delta).rem_euclid(n) as usize;
    model.active_tab = Some(next);
    model.focus = Focus::Editor;
}

/// Moves the selection in the active sidebar list.
fn nav(model: &mut Model, delta: isize) {
    match model.sidebar.active {
        Panel::Files => {
            let len = model.sidebar.files.visible_rows().len();
            model.sidebar.files.selected = move_index(model.sidebar.files.selected, delta, len);
        }
        Panel::Git => {
            let len = model.sidebar.git.nav_len();
            model.sidebar.git.selected = move_index(model.sidebar.git.selected, delta, len);
        }
        Panel::Search => {
            let len = model.sidebar.search.results.len();
            model.sidebar.search.selected = move_index(model.sidebar.search.selected, delta, len);
        }
        Panel::Themes => {
            let len = model.sidebar.themes.names.len();
            let sel = move_index(model.sidebar.themes.selected, delta, len);
            model.apply_theme(sel); // live theme change with the arrow keys
        }
        Panel::Extensions => {}
    }
}

fn move_index(cur: usize, delta: isize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let v = (cur as isize + delta).clamp(0, len as isize - 1);
    v as usize
}

/// Activates the selected item via Enter/Right or a click.
fn activate_selection(model: &mut Model) -> Vec<Cmd> {
    match model.sidebar.active {
        Panel::Files => {
            let rows = model.sidebar.files.visible_rows();
            let Some(row) = rows.get(model.sidebar.files.selected) else {
                return Vec::new();
            };
            let path = row.path.clone();
            if row.is_dir {
                if model.sidebar.files.is_expanded(&path) {
                    model.sidebar.files.collapse(&path);
                    Vec::new()
                } else if model.sidebar.files.is_loaded(&path) {
                    model.sidebar.files.expand(&path);
                    Vec::new()
                } else {
                    vec![Cmd::ScanDir(path)]
                }
            } else {
                open_path(model, path)
            }
        }
        Panel::Git => {
            if let Some((entry, _)) = model.sidebar.git.entry_at(model.sidebar.git.selected) {
                let path = entry.path.clone();
                open_path(model, path)
            } else {
                Vec::new()
            }
        }
        Panel::Search => {
            let m = model.sidebar.search.results.get(model.sidebar.search.selected);
            if let Some(m) = m {
                let path = m.path.clone();
                let line = m.line_no.saturating_sub(1);
                open_path_at(model, path, line)
            } else {
                Vec::new()
            }
        }
        Panel::Themes => {
            model.apply_theme(model.sidebar.themes.selected);
            Vec::new()
        }
        Panel::Extensions => Vec::new(),
    }
}

/// Validates the commit message and returns a commit Cmd (optimistically clears the message).
fn git_commit(model: &mut Model) -> Vec<Cmd> {
    let g = &mut model.sidebar.git;
    let msg = g.commit_msg.trim().to_string();
    if msg.is_empty() {
        model.status_message = "Commit message is empty".to_string();
        return Vec::new();
    }
    if g.staged.is_empty() {
        model.status_message = "No staged changes".to_string();
        return Vec::new();
    }
    g.commit_msg.clear();
    model.focus = Focus::Sidebar;
    vec![Cmd::GitCommit(msg)]
}

fn open_path(model: &mut Model, path: PathBuf) -> Vec<Cmd> {
    open_path_at(model, path, 0)
}

fn open_path_at(model: &mut Model, path: PathBuf, line: usize) -> Vec<Cmd> {
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

/// Computes the editor viewport size (height, text width).
fn editor_viewport(model: &Model) -> (usize, usize) {
    let area = full_rect(model);
    let a = ui::compute_areas(model, area);
    let gutter = a.gutter_w;
    (
        a.editor.height as usize,
        a.editor.width.saturating_sub(gutter) as usize,
    )
}

fn full_rect(model: &Model) -> Rect {
    Rect {
        x: 0,
        y: 0,
        width: model.term_size.0,
        height: model.term_size.1,
    }
}

fn ensure_cursor_visible(model: &mut Model) {
    let (h, w) = editor_viewport(model);
    if let Some(buf) = model.active_buffer_mut() {
        buf.ensure_visible(h, w);
    }
}

/// Propagates the terminal area size to the vt100 parser and the PTY.
fn sync_terminal_size(model: &mut Model) {
    if !model.layout.terminal_open {
        return;
    }
    let area = full_rect(model);
    let a = ui::compute_areas(model, area);
    // the terminal area includes the top border (1 row).
    let rows = a.terminal.height.saturating_sub(1).max(1);
    let cols = a.terminal.width.max(1);
    model.terminal.resize(rows, cols);
}

// ----- Modal dialog -----

/// Handles keyboard input while a dialog is open.
fn dialog_key(model: &mut Model, key: KeyEvent) -> Vec<Cmd> {
    let Some(d) = model.dialog.as_mut() else {
        return Vec::new();
    };
    match d.kind {
        DialogKind::Info => match key.code {
            KeyCode::Enter | KeyCode::Esc => dialog_confirm(model),
            _ => Vec::new(),
        },
        DialogKind::Ask => match key.code {
            KeyCode::Left | KeyCode::Right | KeyCode::Tab => {
                d.selected ^= 1;
                Vec::new()
            }
            KeyCode::Char('e') | KeyCode::Char('y') => dialog_confirm(model),
            KeyCode::Char('h') | KeyCode::Char('n') | KeyCode::Esc => dialog_cancel(model),
            KeyCode::Enter => {
                if d.selected == 0 {
                    dialog_confirm(model)
                } else {
                    dialog_cancel(model)
                }
            }
            _ => Vec::new(),
        },
        DialogKind::Input => match key.code {
            KeyCode::Char(c) => {
                d.input.push(c);
                Vec::new()
            }
            KeyCode::Backspace => {
                d.input.pop();
                Vec::new()
            }
            KeyCode::Enter => dialog_confirm(model),
            KeyCode::Esc => dialog_cancel(model),
            _ => Vec::new(),
        },
    }
}

/// Handles mouse clicks while a dialog is open (buttons).
fn dialog_mouse(model: &mut Model, m: MouseEvent) -> Vec<Cmd> {
    if let MouseEventKind::Down(MouseButton::Left) = m.kind {
        let term = full_rect(model);
        if let Some(d) = model.dialog.as_ref() {
            match ui::dialog::hit(d, term, m.column, m.row) {
                Some(true) => return dialog_confirm(model),
                Some(false) => return dialog_cancel(model),
                None => {}
            }
        }
    }
    Vec::new()
}

/// Confirms the dialog: runs the action and closes it.
fn dialog_confirm(model: &mut Model) -> Vec<Cmd> {
    let Some(d) = model.dialog.take() else {
        return Vec::new();
    };
    match d.action {
        DialogAction::GitRevert(rel) => vec![Cmd::GitRevert(rel)],
        DialogAction::None => Vec::new(),
    }
}

/// Cancels the dialog (closes it without running the action).
fn dialog_cancel(model: &mut Model) -> Vec<Cmd> {
    model.dialog = None;
    Vec::new()
}

// ----- Mouse -----

fn handle_mouse(model: &mut Model, m: MouseEvent) -> Vec<Cmd> {
    let area = full_rect(model);
    let a = ui::compute_areas(model, area);
    let (x, y) = (m.column, m.row);

    match m.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            // Resize handles first.
            if a.sidebar_open && x == a.sidebar_border_x {
                model.drag = Some(DragTarget::SidebarBorder);
                return Vec::new();
            }
            if a.terminal_open && y == a.terminal_border_y && x >= a.tabs.x {
                model.drag = Some(DragTarget::TerminalBorder);
                return Vec::new();
            }
            mouse_click(model, &a, x, y)
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            match model.drag {
                Some(DragTarget::SidebarBorder) => {
                    let min_x = ui::ACTIVITY_WIDTH + 10;
                    let new_w = x.saturating_sub(ui::ACTIVITY_WIDTH).max(10);
                    if x > min_x {
                        model.layout.sidebar_width = new_w.min(model.term_size.0 / 2);
                    }
                }
                Some(DragTarget::TerminalBorder) => {
                    // Drag down -> terminal shrinks.
                    let bottom = model.term_size.1.saturating_sub(1); // statusbar
                    let new_h = bottom.saturating_sub(y).max(2);
                    model.layout.terminal_height = new_h.min(model.term_size.1.saturating_sub(4));
                    sync_terminal_size(model);
                }
                Some(DragTarget::EditorSelect) => {
                    if let Some(buf) = model.active_buffer_mut() {
                        let c = editor_cursor_at(buf, &a, x, y);
                        buf.set_cursor(c, true); // anchor is kept -> the selection grows
                    }
                    ensure_cursor_visible(model);
                }
                None => {}
            }
            Vec::new()
        }
        MouseEventKind::Up(MouseButton::Left) => {
            model.drag = None;
            Vec::new()
        }
        MouseEventKind::ScrollDown => mouse_scroll(model, &a, x, y, 3),
        MouseEventKind::ScrollUp => mouse_scroll(model, &a, x, y, -3),
        _ => Vec::new(),
    }
}

fn rect_contains(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

fn mouse_click(model: &mut Model, a: &ui::Areas, x: u16, y: u16) -> Vec<Cmd> {
    if rect_contains(a.activity, x, y) {
        if let Some(p) = ui::activity_bar::panel_at(a.activity, y) {
            return select_panel(model, p);
        }
        return Vec::new();
    }
    if a.sidebar_open && rect_contains(a.sidebar, x, y) {
        return sidebar_click(model, a, x, y);
    }
    if rect_contains(a.tabs, x, y) {
        match ui::tabs::tab_at(model, a.tabs, x) {
            Some(ui::tabs::TabHit::Close(i)) => close_tab(model, i),
            Some(ui::tabs::TabHit::Select(i)) => {
                model.active_tab = Some(i);
                model.focus = Focus::Editor;
            }
            None => {}
        }
        return Vec::new();
    }
    if a.terminal_open && rect_contains(a.terminal, x, y) {
        model.focus = Focus::Terminal;
        return Vec::new();
    }
    if rect_contains(a.editor, x, y) {
        model.focus = Focus::Editor;
        if let Some(buf) = model.active_buffer_mut() {
            let line = buf.scroll_y + (y - a.editor.y) as usize;
            let col_vis = x.saturating_sub(a.editor_text_x) as usize;
            let col = buf.scroll_x + col_vis;
            buf.set_cursor(Cursor { line, col }, false);
        }
        // Start a drag selection.
        model.drag = Some(DragTarget::EditorSelect);
        ensure_cursor_visible(model);
        return Vec::new();
    }
    Vec::new()
}

/// Converts a mouse position to an editor (line, column) cursor.
fn editor_cursor_at(buf: &Buffer, a: &ui::Areas, x: u16, y: u16) -> Cursor {
    // Clamp y to the editor area (drift when dragging past the top/bottom edge).
    let ey = y.clamp(a.editor.y, a.editor.y + a.editor.height.saturating_sub(1));
    let line = buf.scroll_y + (ey - a.editor.y) as usize;
    let col = buf.scroll_x + x.saturating_sub(a.editor_text_x) as usize;
    Cursor { line, col }
}

fn sidebar_click(model: &mut Model, a: &ui::Areas, x: u16, y: u16) -> Vec<Cmd> {
    match model.sidebar.active {
        Panel::Files => {
            if let Some(idx) = ui::sidebar::file_row_at(model, a.sidebar, y) {
                model.sidebar.files.selected = idx;
                model.focus = Focus::Sidebar;
                return activate_selection(model);
            }
            Vec::new()
        }
        Panel::Git => {
            use ui::sidebar::GitHit;
            match ui::sidebar::git_hit(model, a.sidebar, x, y) {
                Some(GitHit::Entry(idx)) => {
                    model.sidebar.git.selected = idx;
                    model.focus = Focus::Sidebar;
                    activate_selection(model)
                }
                Some(GitHit::Stage(rel)) => {
                    model.focus = Focus::Sidebar;
                    vec![Cmd::GitStage(rel)]
                }
                Some(GitHit::Unstage(rel)) => {
                    model.focus = Focus::Sidebar;
                    vec![Cmd::GitUnstage(rel)]
                }
                Some(GitHit::StageAll) => {
                    model.focus = Focus::Sidebar;
                    vec![Cmd::GitStageAll]
                }
                Some(GitHit::UnstageAll) => {
                    model.focus = Focus::Sidebar;
                    vec![Cmd::GitUnstageAll]
                }
                Some(GitHit::Revert(rel)) => {
                    model.focus = Focus::Sidebar;
                    model.dialog = Some(Dialog::ask(
                        "Revert changes".to_string(),
                        format!("Changes in '{rel}' will be reverted. This cannot be undone. Are you sure?"),
                        DialogAction::GitRevert(rel),
                    ));
                    Vec::new()
                }
                Some(GitHit::CommitInput) => {
                    model.focus = Focus::GitCommit;
                    Vec::new()
                }
                Some(GitHit::CommitButton) => git_commit(model),
                None => Vec::new(),
            }
        }
        Panel::Search => {
            // Row layout: title(0) query(1) replace(2) regex(3) count(4) results(5+).
            let base = a.sidebar.y;
            if y == base + 1 {
                model.sidebar.search.field = SearchField::Query;
                model.focus = Focus::SearchInput;
            } else if y == base + 2 {
                model.sidebar.search.field = SearchField::Replace;
                model.focus = Focus::SearchInput;
            } else if y == base + 3 {
                model.sidebar.search.use_regex = !model.sidebar.search.use_regex;
            } else if let Some(idx) = ui::sidebar::search_row_at(model, a.sidebar, y) {
                model.sidebar.search.selected = idx;
                model.focus = Focus::Sidebar;
                return activate_selection(model);
            } else {
                model.focus = Focus::Sidebar;
            }
            Vec::new()
        }
        Panel::Themes => {
            if let Some(i) = ui::sidebar::theme_row_at(model, a.sidebar, y) {
                model.focus = Focus::Sidebar;
                model.apply_theme(i);
            }
            Vec::new()
        }
        Panel::Extensions => Vec::new(),
    }
}

fn mouse_scroll(model: &mut Model, a: &ui::Areas, x: u16, y: u16, delta: isize) -> Vec<Cmd> {
    if rect_contains(a.editor, x, y) {
        if let Some(buf) = model.active_buffer_mut() {
            let max = buf.line_count().saturating_sub(1);
            let new = (buf.scroll_y as isize + delta).clamp(0, max as isize) as usize;
            buf.scroll_y = new;
        }
    } else if a.sidebar_open && rect_contains(a.sidebar, x, y) {
        nav(model, delta.signum());
    }
    Vec::new()
}
