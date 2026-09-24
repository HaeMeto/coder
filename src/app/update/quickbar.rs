//! Quickbar (command palette) handling: opening, keyboard routing, item
//! execution, and live filtering of the workspace file list against the query.

use super::*;

/// Opens the quickbar and requests the workspace file list asynchronously (a
/// huge repo must not block the UI). The file list arrives later as
/// `Msg::FilesListed` and re-filters the palette.
pub(super) fn open_quickbar(model: &mut Model) -> Vec<Cmd> {
    model.quickbar = Some(QuickbarState::new());
    model.focus = Focus::Sidebar;
    rebuild_items(model);
    vec![Cmd::ListFiles]
}

/// `Msg::FilesListed` handler: store the file list (sorted) and re-filter.
pub(super) fn files_listed(model: &mut Model, mut paths: Vec<PathBuf>) -> Vec<Cmd> {
    let Some(qb) = model.quickbar.as_mut() else {
        return Vec::new();
    };
    paths.sort();
    let root = &model.root;
    qb.files = paths
        .into_iter()
        .map(|path| {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            let key = rel.to_lowercase();
            crate::app::model::QuickbarFile { path, rel, key }
        })
        .collect();
    qb.files_loaded = true;
    rebuild_items(model);
    Vec::new()
}

/// Handles keyboard input while the quickbar is open. It captures every key.
pub(super) fn quickbar_key(model: &mut Model, key: KeyEvent) -> Vec<Cmd> {
    let Some(qb) = model.quickbar.as_mut() else {
        return Vec::new();
    };
    use crate::core::text_input::InputOutcome::{Changed, Ignored, Moved};
    match qb.input.handle_key(key, false) {
        // Typing changes the query: re-filter the list live.
        Changed => {
            rebuild_items(model);
            Vec::new()
        }
        Moved => Vec::new(),
        // Keys the input ignores: list navigation / execute / close, else the
        // key falls through to global shortcuts (Ctrl+Q quit, Ctrl+B sidebar,
        // Ctrl+W close tab, panel switches, ...) so quitting / navigating still
        // works while the palette is open — Ctrl+Q must not be swallowed.
        Ignored => match key.code {
            KeyCode::Up => move_selection(model, -1),
            KeyCode::Down => move_selection(model, 1),
            KeyCode::PageUp => {
                let rows = visible_rows(model) as isize;
                move_selection(model, -rows)
            }
            KeyCode::PageDown => {
                let rows = visible_rows(model) as isize;
                move_selection(model, rows)
            }
            KeyCode::Enter => execute_selected(model),
            KeyCode::Esc => {
                model.quickbar = None;
                model.focus = Focus::Editor;
                Vec::new()
            }
            // Anything else is a global shortcut: make sure it still works.
            _ => overlay_fallback(model, key),
        },
    }
}

/// List rows visible in the popup at the current terminal size.
fn visible_rows(model: &Model) -> usize {
    crate::ui::quickbar::visible_rows(full_rect(model))
}

/// Moves the highlight by `delta` rows (clamped to the list) and scrolls so it
/// stays visible.
fn move_selection(model: &mut Model, delta: isize) -> Vec<Cmd> {
    let rows = visible_rows(model);
    let Some(qb) = model.quickbar.as_mut() else {
        return Vec::new();
    };
    let len = qb.items.len();
    if len == 0 {
        return Vec::new();
    }
    qb.selected = qb.selected.saturating_add_signed(delta).min(len - 1);
    qb.ensure_visible(rows);
    Vec::new()
}

/// Pastes into the quickbar's query field (single-line: embedded newlines fold
/// to spaces) and re-filters the list.
pub(super) fn quickbar_paste(model: &mut Model, text: &str) -> Vec<Cmd> {
    let Some(qb) = model.quickbar.as_mut() else {
        return Vec::new();
    };
    qb.input.insert_paste(text, false);
    rebuild_items(model);
    Vec::new()
}

/// Runs the currently highlighted quickbar entry and closes the palette.
fn execute_selected(model: &mut Model) -> Vec<Cmd> {
    let Some(qb) = model.quickbar.as_mut() else {
        return Vec::new();
    };
    let Some(item) = qb.items.get(qb.selected).cloned() else {
        return Vec::new();
    };
    model.quickbar = None;
    match item {
        QuickbarItem::File { path, .. } => {
            model.focus = Focus::Editor;
            open_path(model, path)
        }
        QuickbarItem::OpenFolder => open_folder_dialog(model),
        QuickbarItem::NewFile => new_root_entry_dialog(model, false),
        QuickbarItem::NewFolder => new_root_entry_dialog(model, true),
        QuickbarItem::Panel(p) => select_panel(model, p),
    }
}

/// VSCode "open folder": shows a dialog to pick a folder path (pre-filled with
/// the current workspace root, editable). On confirm it switches the whole
/// workspace to that folder.
fn open_folder_dialog(model: &mut Model) -> Vec<Cmd> {
    let current = model.root.to_string_lossy().into_owned();
    model.focus = Focus::Sidebar;
    model.dialog = Some(Dialog::input(
        "Open Folder".to_string(),
        "Folder path to open as the new workspace:".to_string(),
        current,
        DialogAction::OpenWorkspace,
    ));
    Vec::new()
}

/// Rebuilds `items` (and clamps `selected`) from the current query: command
/// entries are always offered, plus the workspace files whose relative path
/// matches the query. The match is a case-insensitive substring of the
/// relative path.
fn rebuild_items(model: &mut Model) {
    let rows = visible_rows(model);
    let Some(qb) = model.quickbar.as_mut() else {
        return;
    };
    let query = qb.input.content().trim().to_lowercase();

    // Startup template of commands: open folder, new entries, plus the panels.
    let mut commands: Vec<QuickbarItem> = Vec::new();
    commands.push(QuickbarItem::OpenFolder);
    commands.push(QuickbarItem::NewFile);
    commands.push(QuickbarItem::NewFolder);
    for p in Panel::ALL {
        commands.push(QuickbarItem::Panel(p));
    }

    // Cap the palette to a reasonable viewport so a huge workspace doesn't draw
    // thousands of rows; the query narrows it down.
    const MAX: usize = 100;

    // Commands first, then matching files in sorted order. The file scan stops
    // as soon as the cap is reached, and only matches are cloned.
    let mut items: Vec<QuickbarItem> = commands
        .into_iter()
        .filter(|it| query.is_empty() || it.filter_text().contains(&query))
        .collect();
    let room = MAX.saturating_sub(items.len());
    items.extend(
        qb.files
            .iter()
            .filter(|f| query.is_empty() || f.key.contains(&query))
            .take(room)
            .map(|f| QuickbarItem::File {
                path: f.path.clone(),
                rel: f.rel.clone(),
            }),
    );
    items.truncate(MAX);

    qb.selected = qb.selected.min(items.len().saturating_sub(1));
    qb.items = items;
    qb.ensure_visible(rows);
}

/// Handles mouse input while the quickbar is open. A click on a list row
/// runs it; a click anywhere else closes the palette. The wheel scrolls the
/// list.
pub(super) fn quickbar_mouse(model: &mut Model, m: MouseEvent) -> Vec<Cmd> {
    if model.quickbar.is_none() {
        return Vec::new();
    }
    match m.kind {
        MouseEventKind::ScrollDown => return scroll_list(model, 3),
        MouseEventKind::ScrollUp => return scroll_list(model, -3),
        _ => {}
    }
    if let MouseEventKind::Down(MouseButton::Left) = m.kind {
        let term = full_rect(model);
        if let Some(row) = crate::ui::quickbar::hit(model, term, m.column, m.row) {
            if let Some(qb) = model.quickbar.as_mut() {
                qb.selected = row;
            }
            return execute_selected(model);
        } else {
            // Click outside the palette closes it.
            model.quickbar = None;
        }
    }
    Vec::new()
}

/// Wheel scroll: moves the viewport by `delta` rows and drags the highlight
/// along so it stays on screen (and Enter runs what is visible).
fn scroll_list(model: &mut Model, delta: isize) -> Vec<Cmd> {
    let rows = visible_rows(model).max(1);
    let Some(qb) = model.quickbar.as_mut() else {
        return Vec::new();
    };
    let max_scroll = qb.items.len().saturating_sub(rows);
    qb.scroll = qb.scroll.saturating_add_signed(delta).min(max_scroll);
    qb.selected = qb.selected.clamp(
        qb.scroll,
        (qb.scroll + rows - 1).min(qb.items.len().saturating_sub(1)),
    );
    Vec::new()
}
