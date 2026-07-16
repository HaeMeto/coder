//! Mouse event routing: resize handles, editor selection, tabs, sidebar, scrollbar.

use super::*;

pub(super) fn handle_mouse(model: &mut Model, m: MouseEvent) -> Vec<Cmd> {
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
            // The find widget floats over the editor; intercept its clicks.
            if model.find.open
                && let Some(hit) = ui::find::hit(model, a.editor, x, y)
            {
                return handle_find_hit(model, hit);
            }
            // Scrollbar thumb drag.
            if a.scrollbar.width > 0 && rect_contains(a.scrollbar, x, y) {
                model.drag = Some(DragTarget::Scrollbar);
                scrollbar_jump(model, &a, y);
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
                    let c = editor_cursor_at(model, &a, x, y);
                    if let Some(buf) = model.active_buffer_mut() {
                        buf.set_cursor(c, true); // anchor is kept -> the selection grows
                    }
                    ensure_cursor_visible(model);
                }
                Some(DragTarget::Scrollbar) => scrollbar_jump(model, &a, y),
                None => {}
            }
            Vec::new()
        }
        MouseEventKind::Up(MouseButton::Left) => {
            model.drag = None;
            Vec::new()
        }
        // Middle-click anywhere on a tab closes it (like clicking its ✕).
        MouseEventKind::Down(MouseButton::Middle) => {
            if rect_contains(a.tabs, x, y)
                && let Some(hit) = ui::tabs::tab_at(model, a.tabs, x)
            {
                let i = match hit {
                    ui::tabs::TabHit::Select(i) | ui::tabs::TabHit::Close(i) => i,
                };
                return close_tab(model, i);
            }
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

/// Handles a click on the find widget.
fn handle_find_hit(model: &mut Model, hit: ui::find::FindHit) -> Vec<Cmd> {
    use ui::find::FindHit;
    match hit {
        FindHit::QueryField => {
            model.focus = Focus::Find;
            model.find.field = FindField::Query;
            model.find.query.cursor_to_end();
            Vec::new()
        }
        FindHit::ReplaceField => {
            model.focus = Focus::Find;
            model.find.field = FindField::Replace;
            model.find.replace.cursor_to_end();
            Vec::new()
        }
        FindHit::Prev => {
            find_step(model, -1);
            Vec::new()
        }
        FindHit::Next => {
            find_step(model, 1);
            Vec::new()
        }
        FindHit::Close => {
            close_find(model);
            Vec::new()
        }
        FindHit::ReplaceOne => find_replace_one(model),
        FindHit::ReplaceAll => find_replace_all(model),
    }
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
            Some(ui::tabs::TabHit::Close(i)) => return close_tab(model, i),
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
        // Double-click (same cell within 400ms) selects the word under the cursor.
        let now = std::time::Instant::now();
        let double = model
            .last_click
            .map(|(t, cx, cy)| cx == x && cy == y && now.duration_since(t).as_millis() < 400)
            .unwrap_or(false);
        model.last_click = Some((now, x, y));
        let cur = editor_cursor_at(model, a, x, y);
        if let Some(buf) = model.active_buffer_mut() {
            if double {
                buf.select_word_at(cur);
            } else {
                buf.set_cursor(cur, false);
            }
        }
        // A single click starts a drag selection; a double-click keeps the word.
        if !double {
            model.drag = Some(DragTarget::EditorSelect);
        }
        ensure_cursor_visible(model);
        return Vec::new();
    }
    Vec::new()
}

/// Converts a mouse position to an editor (line, column) cursor. Maps the screen
/// row through the diff view so clicks land on the right buffer line even when
/// removed lines are woven in.
fn editor_cursor_at(model: &Model, a: &ui::Areas, x: u16, y: u16) -> Cursor {
    // Clamp y to the editor area (drift when dragging past the top/bottom edge).
    let ey = y.clamp(a.editor.y, a.editor.y + a.editor.height.saturating_sub(1));
    let line = model.screen_row_to_line((ey - a.editor.y) as usize);
    let scroll_x = model.active_buffer().map(|b| b.scroll_x).unwrap_or(0);
    let col = scroll_x + x.saturating_sub(a.editor_text_x) as usize;
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
                        format!(
                            "Changes in '{rel}' will be reverted. This cannot be undone. Are you sure?"
                        ),
                        DialogAction::GitRevert(rel),
                    ));
                    Vec::new()
                }
                Some(GitHit::CommitInput) => {
                    model.focus = Focus::GitCommit;
                    model.sidebar.git.commit.cursor_to_end();
                    Vec::new()
                }
                Some(GitHit::CommitButton) => git_commit(model),
                Some(GitHit::Fetch) => {
                    model.focus = Focus::Sidebar;
                    if model.sidebar.git.has_remote {
                        model.status_message = "Fetching…".to_string();
                        vec![Cmd::GitFetch]
                    } else {
                        Vec::new()
                    }
                }
                Some(GitHit::Pull) => {
                    model.focus = Focus::Sidebar;
                    // Disabled without an upstream to pull from.
                    if model.sidebar.git.has_upstream {
                        model.status_message = "Pulling…".to_string();
                        vec![Cmd::GitPull]
                    } else {
                        Vec::new()
                    }
                }
                Some(GitHit::Push) => {
                    model.focus = Focus::Sidebar;
                    // Disabled when there is nothing to push.
                    if model.sidebar.git.can_push() {
                        model.status_message = "Pushing…".to_string();
                        vec![Cmd::GitPush]
                    } else {
                        Vec::new()
                    }
                }
                None => Vec::new(),
            }
        }
        Panel::Search => {
            use ui::sidebar::SearchHit;
            match ui::sidebar::search_hit(model, a.sidebar, x, y) {
                Some(SearchHit::QueryField) => {
                    model.sidebar.search.field = SearchField::Query;
                    model.focus = Focus::SearchInput;
                }
                Some(SearchHit::ReplaceField) => {
                    model.sidebar.search.field = SearchField::Replace;
                    model.focus = Focus::SearchInput;
                }
                Some(SearchHit::RegexToggle) => {
                    model.sidebar.search.use_regex = !model.sidebar.search.use_regex;
                    return rerun_search(model);
                }
                Some(SearchHit::MatchCaseToggle) => {
                    model.sidebar.search.match_case = !model.sidebar.search.match_case;
                    return rerun_search(model);
                }
                Some(SearchHit::SearchHiddenToggle) => {
                    model.sidebar.search.search_hidden = !model.sidebar.search.search_hidden;
                    return rerun_search(model);
                }
                Some(SearchHit::ReplaceOne) => return search_replace_one(model),
                Some(SearchHit::ReplaceAll) => return search_replace_all(model),
                Some(SearchHit::Result(idx)) => {
                    model.sidebar.search.selected = idx;
                    model.focus = Focus::Sidebar;
                    return activate_selection(model);
                }
                None => model.focus = Focus::Sidebar,
            }
            Vec::new()
        }
        Panel::Themes => {
            if let Some(i) = ui::sidebar::theme_row_at(model, a.sidebar, y) {
                model.focus = Focus::Sidebar;
                model.apply_theme(i);
                return persist_config(model);
            }
            Vec::new()
        }
        Panel::Settings => {
            if let Some(i) = ui::sidebar::settings_row_at(a.sidebar, y) {
                model.sidebar.settings.selected = i;
                model.focus = Focus::Sidebar;
                model.sidebar.settings.toggle(i);
                return persist_config(model);
            }
            Vec::new()
        }
        Panel::Extensions => Vec::new(),
    }
}

/// Jumps the editor scroll so the clicked scrollbar row is centered in the viewport.
fn scrollbar_jump(model: &mut Model, a: &ui::Areas, y: u16) {
    let h = a.scrollbar.height as usize;
    if h == 0 {
        return;
    }
    let row = y.saturating_sub(a.scrollbar.y) as usize;
    if let Some(buf) = model.active_buffer_mut() {
        let n = buf.line_count().max(1);
        let target = row * n / h;
        let max = n.saturating_sub(h);
        buf.scroll_y = target.saturating_sub(h / 2).min(max);
    }
}

fn mouse_scroll(model: &mut Model, a: &ui::Areas, x: u16, y: u16, delta: isize) -> Vec<Cmd> {
    let over_scrollbar = a.scrollbar.width > 0 && rect_contains(a.scrollbar, x, y);
    if rect_contains(a.editor, x, y) || over_scrollbar {
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
