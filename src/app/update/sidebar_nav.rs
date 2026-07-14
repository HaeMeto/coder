//! Sidebar list navigation, activation, and config persistence.

use super::*;

/// Moves the selection in the active sidebar list.
pub(super) fn nav(model: &mut Model, delta: isize) {
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
        Panel::Settings => {
            let sel = move_index(
                model.sidebar.settings.selected,
                delta,
                crate::app::model::SettingsState::COUNT,
            );
            model.sidebar.settings.selected = sel;
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
pub(super) fn activate_selection(model: &mut Model) -> Vec<Cmd> {
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
            // Clicking a git entry opens the file as a diff-mode tab; changed lines
            // get a green/red background.
            if let Some((entry, _)) = model.sidebar.git.entry_at(model.sidebar.git.selected) {
                let path = entry.path.clone();
                open_diff(model, path)
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
            persist_config(model)
        }
        Panel::Settings => {
            let i = model.sidebar.settings.selected;
            model.sidebar.settings.toggle(i);
            persist_config(model)
        }
        Panel::Extensions => Vec::new(),
    }
}

/// A Cmd that writes the current preferences (theme + settings) to disk.
pub(super) fn persist_config(model: &Model) -> Vec<Cmd> {
    vec![Cmd::SaveConfig(model.config_snapshot())]
}

/// Persists after a keyboard nav that changes the theme live (Themes panel only).
pub(super) fn post_nav_persist(model: &Model) -> Vec<Cmd> {
    if model.sidebar.active == Panel::Themes {
        persist_config(model)
    } else {
        Vec::new()
    }
}
