//! Git panel keyboard handling: zone focus, buttons, and the commit input.

use super::*;

use crate::app::model::GitZone;

/// Validates the commit message and returns a commit Cmd. The message is cleared
/// only once the commit succeeds (`Msg::GitCommitted`).
pub(super) fn git_commit(model: &mut Model) -> Vec<Cmd> {
    let g = &mut model.sidebar.git;
    let msg = g.commit.content().trim().to_string();
    if msg.is_empty() {
        model.notify("Commit message is empty".to_string());
        return Vec::new();
    }
    if g.staged.is_empty() {
        model.notify("No staged changes".to_string());
        return Vec::new();
    }
    model.focus = Focus::Sidebar;
    model.sidebar.git.zone = GitZone::Files;
    vec![Cmd::GitCommit(msg)]
}

/// Whether the keyboard is currently inside the Git panel (either on the commit
/// box or on one of its lists/buttons). Guards the panel's own shortcuts so they
/// stay inert in the Files/Search/Themes/Settings panels.
pub(super) fn in_git_panel(model: &Model) -> bool {
    model.sidebar.active == Panel::Git
        && model.layout.sidebar_open
        && matches!(model.focus, Focus::Sidebar | Focus::GitCommit)
}

/// Moves the keyboard `delta` zones through the panel (Tab / Shift+Tab).
///
/// The commit box is the one zone that needs the app-level focus too, so typing
/// is routed into its text input; every other zone stays on `Focus::Sidebar`.
pub(super) fn git_cycle_zone(model: &mut Model, delta: isize) -> Vec<Cmd> {
    // Without a repository the panel is a single "No git repository" line: there
    // are no buttons or commit box to tab to.
    if !in_git_panel(model) || !model.sidebar.git.is_repo {
        return Vec::new();
    }
    let zone = model.sidebar.git.zone.step(delta);
    set_git_zone(model, zone);
    Vec::new()
}

/// Focuses `zone`, keeping the app-level focus in sync with it.
pub(super) fn set_git_zone(model: &mut Model, zone: GitZone) {
    model.sidebar.git.zone = zone;
    if zone == GitZone::Message {
        model.focus = Focus::GitCommit;
        model.sidebar.git.commit.cursor_to_end();
    } else {
        model.focus = Focus::Sidebar;
    }
}

/// Enter in the Git panel: presses the focused button, or opens the selected
/// change as a diff tab when the change list has the focus.
pub(super) fn git_zone_activate(model: &mut Model) -> Vec<Cmd> {
    match model.sidebar.git.zone {
        GitZone::Files => activate_selection(model),
        // Enter inside the commit box is a newline (handled by the input widget),
        // so this is only reachable defensively.
        GitZone::Message => Vec::new(),
        zone => git_button(model, zone),
    }
}

/// Runs a Git panel button, honouring the same enabled/disabled rules the
/// rendering uses — a disabled button does nothing when "pressed".
pub(super) fn git_button(model: &mut Model, zone: GitZone) -> Vec<Cmd> {
    let g = &model.sidebar.git;
    match zone {
        GitZone::Fetch => {
            if !g.has_remote {
                return Vec::new();
            }
            model.show_toast("Fetching…");
            vec![Cmd::GitFetch]
        }
        GitZone::Pull => {
            if !g.has_upstream {
                return Vec::new();
            }
            model.show_toast("Pulling…");
            vec![Cmd::GitPull]
        }
        GitZone::Push => {
            if !g.can_push() {
                return Vec::new();
            }
            model.show_toast("Pushing…");
            vec![Cmd::GitPush]
        }
        GitZone::Uncommit => {
            if !g.can_undo_commit() {
                return Vec::new();
            }
            model.notify("Undoing last commit…".to_string());
            vec![Cmd::GitUndoLastCommit]
        }
        GitZone::Commit => git_commit(model),
        GitZone::Message | GitZone::Files => Vec::new(),
    }
}

/// `a` on the selected change: stage it, or unstage it when it is already staged.
pub(super) fn git_toggle_stage(model: &mut Model) -> Vec<Cmd> {
    if !in_git_panel(model) {
        return Vec::new();
    }
    let g = &model.sidebar.git;
    let Some((entry, staged)) = g.entry_at(g.selected) else {
        return Vec::new();
    };
    let rel = entry.rel.clone();
    if staged {
        vec![Cmd::GitUnstage(rel)]
    } else {
        vec![Cmd::GitStage(rel)]
    }
}

/// `r` on the selected change: confirm, then restore the file to its committed
/// state (the same destructive action as the row's ↺ button).
pub(super) fn git_revert_entry(model: &mut Model) -> Vec<Cmd> {
    if !in_git_panel(model) {
        return Vec::new();
    }
    let g = &model.sidebar.git;
    let Some((entry, staged)) = g.entry_at(g.selected) else {
        return Vec::new();
    };
    // Reverting restores the working tree to the *index*, so a staged change has
    // to be unstaged first or the revert would be a no-op.
    if staged {
        model.notify("Unstage it first (a), then revert".to_string());
        return Vec::new();
    }
    let rel = entry.rel.clone();
    model.dialog = Some(Dialog::ask(
        "Revert changes".to_string(),
        format!("Changes in '{rel}' will be reverted. This cannot be undone. Are you sure?"),
        DialogAction::GitRevert(rel),
    ));
    Vec::new()
}

/// The file's HEAD content arrived (`Msg::HeadTextLoaded`).
pub(super) fn head_text_loaded(model: &mut Model, path: PathBuf, text: Option<String>) -> Vec<Cmd> {
    // Update every open tab for this file (a normal tab and its diff tab).
    // Every git status refresh reloads HEAD for all tabs; skip the
    // (whole-file) re-diff when it didn't actually move. HEAD text only
    // feeds the change gutter, never syntax colors — no re-highlight.
    for i in model.all_tabs_for(&path) {
        if model.tabs[i].head_text != text {
            model.tabs[i].head_text = text.clone();
            if model.active_tab == Some(i) {
                model.mark_git_dirty();
            }
        }
    }
    // First open of a diff tab: jump the cursor to the first changed line so
    // the diff is on screen without scrolling.
    if model.pending_diff_scroll.as_deref() == Some(path.as_path()) {
        model.pending_diff_scroll = None;
        model.refresh_git_marks();
        if let Some(first) = model.active_git_marks.keys().min().copied() {
            // Leave ~10 lines of context above the first change so it sits
            // a bit below the top edge rather than flush against it.
            const DIFF_TOP_MARGIN: usize = 10;
            if let Some(buf) = model.active_buffer_mut() {
                buf.goto_line(first);
                buf.scroll_y = first.saturating_sub(DIFF_TOP_MARGIN);
                buf.scroll_x = 0;
            }
        }
    }
    Vec::new()
}

/// A fresh `git status` arrived (`Msg::GitStatusLoaded`).
pub(super) fn status_loaded(
    model: &mut Model,
    status: crate::services::git::GitStatus,
) -> Vec<Cmd> {
    let crate::services::git::GitStatus {
        branch,
        staged,
        unstaged,
        is_repo,
        ahead,
        behind,
        has_upstream,
        has_remote,
        history,
    } = status;
    // Close diff-mode tabs for files no longer in the change list (reverted /
    // committed). Their diff is gone, leaving a stale editor view otherwise.
    let stale: Vec<usize> = model
        .tabs
        .iter()
        .enumerate()
        // A commit's diff tab is read-only history, not a live change —
        // it must survive every status refresh.
        .filter(|(_, t)| t.diff_mode && !t.read_only)
        .filter_map(|(i, t)| {
            let path = t.buffer.path.as_ref()?;
            let in_list = staged
                .iter()
                .chain(unstaged.iter())
                .any(|e| e.path == path.as_path());
            if !in_list { Some(i) } else { None }
        })
        .collect();
    let mut cmds: Vec<Cmd> = Vec::new();
    for i in stale.into_iter().rev() {
        if model.tabs[i].buffer.dirty {
            // Never drop unsaved edits: demote it to a normal tab instead.
            model.tabs[i].diff_mode = false;
            if model.active_tab == Some(i) {
                model.mark_git_dirty();
            }
        } else {
            cmds.extend(close_tab(model, i));
        }
    }
    let g = &mut model.sidebar.git;
    g.branch = branch;
    g.staged = staged;
    g.unstaged = unstaged;
    g.is_repo = is_repo;
    g.ahead = ahead;
    g.behind = behind;
    g.has_upstream = has_upstream;
    g.has_remote = has_remote;
    g.history = history;
    let len = g.nav_len();
    if g.selected >= len {
        g.selected = len.saturating_sub(1);
    }
    // Refresh the change gutter for open files (HEAD may have moved after a commit/revert).
    cmds.extend(
        model
            .tabs
            .iter()
            // Generated tabs (a commit patch) have no file behind their
            // synthetic path — there is no HEAD text to load.
            .filter(|t| !t.read_only)
            .filter_map(|t| t.buffer.path.clone())
            .map(Cmd::LoadHeadText),
    );
    cmds
}
