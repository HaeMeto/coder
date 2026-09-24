//! Switching the workspace root (VSCode "open folder").

use std::path::PathBuf;

use crate::core::filetree::FileTree;

use super::{FindState, Focus, GitStatus, LspState, Model, Panel, SearchState};

impl Model {
    /// VSCode "open folder": switch the whole workspace to `root`. The file
    /// explorer, git panel and search state reset for the new root, and every
    /// editor tab / LSP session is dropped, while user preferences (theme,
    /// settings, keybindings) and the window layout survive. This is pure state
    /// work — the caller must issue the rescan `Cmd`s afterwards.
    pub fn open_folder(&mut self, root: PathBuf) {
        self.root = root.clone();
        // Keep `sidebar.settings`/`sidebar.themes` (preferences) but reset every
        // root-dependent sub-panel.
        self.sidebar.files = FileTree::new(root.clone());
        self.sidebar.git = GitStatus::default();
        self.sidebar.search = SearchState::default();
        self.sidebar.settings_selected = 0;
        self.sidebar.active = Panel::Files;

        self.tabs = Vec::new();
        self.active_tab = None;
        self.find = FindState::default();
        self.lsp = LspState::default();
        self.diagnostics = std::collections::HashMap::new();
        self.completion = None;
        self.pending_format = None;

        // Close any transient overlay / drag so it never references a stale root.
        self.dialog = None;
        self.context_menu = None;
        self.quickbar = None;
        self.drag = None;
        self.pending_goto = None;
        self.pending_diff = None;
        self.pending_preview = None;
        self.preview_loads.clear();
        self.pending_diff_scroll = None;

        // Invalidate every cached render target.
        self.invalidate_highlight();
        self.active_git_marks = std::collections::HashMap::new();
        self.active_git_marks_tab = None;
        self.active_deleted = Vec::new();
        self.autocomplete_at = None;
        self.didchange_at = None;

        // Session state belongs to the old root: a pending checkpoint or the
        // multi-instance generation must not leak into the new root's file.
        self.session_seen_generation = None;
        self.session_save_at = None;
        self.pending_session_restore.clear();
        self.session_active_path = None;

        self.focus = Focus::Sidebar;
        self.layout.sidebar_open = true;
    }
}
