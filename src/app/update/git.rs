//! Git commit input handling.

use super::*;

/// Validates the commit message and returns a commit Cmd (optimistically clears the message).
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
    g.commit.clear();
    model.focus = Focus::Sidebar;
    vec![Cmd::GitCommit(msg)]
}
