//! Sidebar panel selection (keyboard shortcut / activity-bar click).

use super::*;

/// Selects a sidebar panel from the keyboard (a panel shortcut).
///
/// The shortcut always *goes to* the panel: it opens the sidebar, switches to
/// `p`, and moves focus there — so it works while the editor, terminal or find
/// widget holds focus. Only when the panel is already open **and** focused does
/// a second press collapse the sidebar again.
pub(in crate::app::update) fn select_panel(model: &mut Model, p: Panel) -> Vec<Cmd> {
    let focused_here = matches!(
        model.focus,
        Focus::Sidebar | Focus::SearchInput | Focus::GitCommit
    );
    open_panel(model, p, focused_here)
}

/// Selects a sidebar panel from a click on its activity-bar icon. Clicking the
/// already-active icon collapses the sidebar regardless of where focus sits,
/// which is what the mouse is expected to do.
pub(in crate::app::update) fn toggle_panel(model: &mut Model, p: Panel) -> Vec<Cmd> {
    open_panel(model, p, true)
}

/// Shared body: `collapse_if_active` decides whether re-selecting the panel that
/// is already open closes the sidebar or just focuses it.
fn open_panel(model: &mut Model, p: Panel, collapse_if_active: bool) -> Vec<Cmd> {
    if model.layout.sidebar_open && model.sidebar.active == p && collapse_if_active {
        model.layout.sidebar_open = false;
        if matches!(
            model.focus,
            Focus::Sidebar | Focus::SearchInput | Focus::GitCommit
        ) {
            model.focus = Focus::Editor;
        }
        return Vec::new();
    }

    model.sidebar.active = p;
    model.layout.sidebar_open = true;
    // Search lands in its query input so a search can be typed immediately; the
    // other panels focus their list.
    model.focus = if p == Panel::Search {
        Focus::SearchInput
    } else {
        Focus::Sidebar
    };
    if p == Panel::Search {
        model.sidebar.search.field = SearchField::Query;
        model.sidebar.search.query.cursor_to_end();
    }
    if p == Panel::Git {
        // Focus is `Focus::Sidebar` here, so the zone has to agree: start on the
        // change list, from where Tab reaches the commit box and the buttons.
        model.sidebar.git.zone = crate::app::model::GitZone::Files;
    }
    match p {
        Panel::Git => vec![Cmd::LoadGitStatus],
        Panel::Files if model.sidebar.files.children.is_none() => {
            vec![Cmd::ScanDir(model.root.clone())]
        }
        // Re-probe tool availability each time the panel opens (a binary may have
        // been installed since startup).
        Panel::Extensions => vec![Cmd::CheckTools(model.extensions.tool_commands())],
        _ => Vec::new(),
    }
}
