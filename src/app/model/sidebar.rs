//! Sidebar state: the per-panel state of Files, Search, Git, Themes and Settings.

use crate::core::filetree::FileTree;
use crate::core::highlight;
use crate::core::text_input::TextInputState;
use crate::services::git::{GitCommit, GitEntry};
use crate::services::search::SearchMatch;

use super::Panel;

/// Editor preferences applied at save time. Edited via `config.toml`, not the UI.
pub struct SettingsState {
    /// Master switch: run the enabled format actions when saving.
    pub format_on_save: bool,
    /// Run the language formatter after a paste (off by default).
    pub format_on_paste: bool,
    /// Strip trailing spaces/tabs from each line on save (when format_on_save).
    pub trim_trailing_whitespace: bool,
    /// Ensure the file ends with a single newline on save (when format_on_save).
    pub insert_final_newline: bool,
    /// Show LSP error/warning messages inline at the end of their line.
    pub inline_diagnostics: bool,
}

impl Default for SettingsState {
    fn default() -> Self {
        SettingsState {
            format_on_save: false,
            format_on_paste: false,
            trim_trailing_whitespace: true,
            insert_final_newline: true,
            inline_diagnostics: true,
        }
    }
}

/// State of the theme picker panel.
pub struct ThemesState {
    /// Names of the loaded syntect themes.
    pub names: Vec<String>,
    /// Index of the theme selected via keyboard/mouse.
    pub selected: usize,
}

impl Default for ThemesState {
    fn default() -> Self {
        let names = highlight::theme_names();
        let selected = names
            .iter()
            .position(|n| n == highlight::DEFAULT_THEME)
            .unwrap_or(0);
        ThemesState { names, selected }
    }
}

/// The keyboard-focused control in the search panel: one of the two text
/// inputs, or one of the option checkboxes (reached with Tab, toggled with
/// Enter/Space).
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchField {
    #[default]
    Query,
    /// The "[ ] Replace" checkbox.
    ReplaceToggle,
    Replace,
    Regex,
    MatchCase,
    SearchHidden,
}

impl SearchField {
    /// Tab order, top to bottom. The replace input is only reachable while
    /// replace mode shows it.
    fn order(replace_mode: bool) -> &'static [SearchField] {
        use SearchField::*;
        if replace_mode {
            &[
                Query,
                ReplaceToggle,
                Replace,
                Regex,
                MatchCase,
                SearchHidden,
            ]
        } else {
            &[Query, ReplaceToggle, Regex, MatchCase, SearchHidden]
        }
    }

    /// The control `dir` steps away in Tab order, wrapping at both ends.
    pub fn step(self, dir: isize, replace_mode: bool) -> SearchField {
        let order = Self::order(replace_mode);
        let len = order.len() as isize;
        let i = order.iter().position(|f| *f == self).unwrap_or(0) as isize;
        order[(i + dir).rem_euclid(len) as usize]
    }
}

#[derive(Default)]
pub struct SearchState {
    pub query: TextInputState,
    pub replace: TextInputState,
    /// Should the query be interpreted as a regex?
    pub use_regex: bool,
    /// Case-sensitive matching when true (default: case-insensitive).
    pub match_case: bool,
    /// Search .gitignore'd and hidden (dot) files when true (default: skip them).
    pub search_hidden: bool,
    /// Whether the replace input/buttons are shown at all. Off by default so
    /// the panel starts as a compact "just find" view instead of always
    /// showing replace UI up front — toggled via the "[ ] Replace" checkbox.
    pub replace_mode: bool,
    /// Which field keyboard input goes to.
    pub field: SearchField,
    pub results: Vec<SearchMatch>,
    pub selected: usize,
}

impl SearchState {
    /// Shows/hides the replace input. When it disappears, keyboard focus
    /// leaves it so keys never route to an input that is no longer drawn.
    pub fn toggle_replace_mode(&mut self) {
        self.replace_mode = !self.replace_mode;
        if !self.replace_mode && self.field == SearchField::Replace {
            self.field = SearchField::Query;
        }
    }
}

/// Keyboard focus zone inside the Git panel, cycled with Tab.
///
/// The zone decides what Enter activates and which widget is drawn highlighted.
/// `Message` is the one zone that also changes the app-level [`Focus`] (to
/// `Focus::GitCommit`, so typing reaches the commit input); every other zone
/// keeps `Focus::Sidebar`.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitZone {
    /// The commit message box.
    Message,
    Fetch,
    Pull,
    Push,
    Uncommit,
    Commit,
    /// The change / history list at the bottom of the panel.
    #[default]
    Files,
}

impl GitZone {
    /// Tab order, top to bottom, wrapping back to the start.
    const ORDER: [GitZone; 7] = [
        GitZone::Message,
        GitZone::Fetch,
        GitZone::Pull,
        GitZone::Push,
        GitZone::Uncommit,
        GitZone::Commit,
        GitZone::Files,
    ];

    /// Steps `delta` places through [`GitZone::ORDER`], wrapping at both ends.
    pub fn step(self, delta: isize) -> GitZone {
        let n = GitZone::ORDER.len() as isize;
        let cur = GitZone::ORDER.iter().position(|z| *z == self).unwrap_or(0) as isize;
        GitZone::ORDER[(cur + delta).rem_euclid(n) as usize]
    }
}

#[derive(Default)]
pub struct GitStatus {
    pub branch: Option<String>,
    pub staged: Vec<GitEntry>,
    pub unstaged: Vec<GitEntry>,
    pub is_repo: bool,
    /// Keyboard selection: index into the combined [staged..., unstaged...] list.
    pub selected: usize,
    /// The commit message input box.
    pub commit: TextInputState,
    /// Commits the local branch is ahead of its upstream.
    pub ahead: usize,
    /// Commits the local branch is behind its upstream.
    pub behind: usize,
    /// Whether the current branch has a configured upstream.
    pub has_upstream: bool,
    /// Whether the repository has at least one remote configured.
    pub has_remote: bool,
    /// Recent commits (newest first) shown under the HISTORY heading.
    pub history: Vec<GitCommit>,
    /// Which part of the panel the keyboard is on (cycled with Tab).
    pub zone: GitZone,
}

impl GitStatus {
    /// Total number of keyboard-navigable items: the changes followed by the
    /// history commits, in the one combined index the selection uses.
    pub fn nav_len(&self) -> usize {
        self.staged.len() + self.unstaged.len() + self.history.len()
    }

    /// Number of change rows — the combined index where the history starts.
    pub fn changes_len(&self) -> usize {
        self.staged.len() + self.unstaged.len()
    }

    /// The history commit at a combined index, or `None` if it names a change row.
    pub fn commit_at(&self, idx: usize) -> Option<&GitCommit> {
        self.history.get(idx.checked_sub(self.changes_len())?)
    }

    /// Whether there is anything to push: a remote must exist and the branch is
    /// either ahead of its upstream or not yet published (no upstream).
    pub fn can_push(&self) -> bool {
        self.is_repo
            && self.branch.is_some()
            && self.has_remote
            && (self.ahead > 0 || !self.has_upstream)
    }

    /// Whether the last commit can be undone: a repo with a local commit that has
    /// not been pushed (ahead of its upstream, or no upstream configured yet).
    pub fn can_undo_commit(&self) -> bool {
        self.is_repo && self.branch.is_some() && (self.ahead > 0 || !self.has_upstream)
    }

    /// Returns the item at the combined index and whether it is staged.
    pub fn entry_at(&self, idx: usize) -> Option<(&GitEntry, bool)> {
        if idx < self.staged.len() {
            self.staged.get(idx).map(|e| (e, true))
        } else {
            self.unstaged
                .get(idx - self.staged.len())
                .map(|e| (e, false))
        }
    }
}

pub struct Sidebar {
    pub active: Panel,
    pub files: FileTree,
    pub git: GitStatus,
    pub search: SearchState,
    pub themes: ThemesState,
    pub settings: SettingsState,
    /// Selected row in the Settings panel (index into its action list).
    pub settings_selected: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn git_zone_tab_order_wraps_both_ways() {
        // Tab walks top to bottom and wraps back to the commit box.
        assert_eq!(GitZone::Message.step(1), GitZone::Fetch);
        assert_eq!(GitZone::Push.step(1), GitZone::Uncommit);
        assert_eq!(GitZone::Commit.step(1), GitZone::Files);
        assert_eq!(GitZone::Files.step(1), GitZone::Message);
        // Shift+Tab is the exact inverse.
        assert_eq!(GitZone::Message.step(-1), GitZone::Files);
        assert_eq!(GitZone::Fetch.step(-1), GitZone::Message);
        for z in GitZone::ORDER {
            assert_eq!(z.step(1).step(-1), z);
        }
    }

    fn entry(rel: &str) -> GitEntry {
        GitEntry {
            path: PathBuf::from(rel),
            rel: rel.to_string(),
            state: crate::services::git::GitState::Modified,
        }
    }

    #[test]
    fn selection_runs_changes_then_history() {
        let mut g = GitStatus {
            staged: vec![entry("a.rs")],
            unstaged: vec![entry("b.rs"), entry("c.rs")],
            ..GitStatus::default()
        };
        g.history = vec![
            GitCommit {
                hash: "aaaaaaa".to_string(),
                summary: "first".to_string(),
            },
            GitCommit {
                hash: "bbbbbbb".to_string(),
                summary: "second".to_string(),
            },
        ];
        assert_eq!(g.changes_len(), 3);
        assert_eq!(g.nav_len(), 5);
        // The change rows come first: they resolve as entries, not commits.
        assert_eq!(
            g.entry_at(0).map(|(e, staged)| (e.rel.as_str(), staged)),
            Some(("a.rs", true))
        );
        assert_eq!(
            g.entry_at(2).map(|(e, staged)| (e.rel.as_str(), staged)),
            Some(("c.rs", false))
        );
        assert!(g.commit_at(2).is_none());
        // The history follows, in order.
        assert_eq!(g.commit_at(3).map(|c| c.hash.as_str()), Some("aaaaaaa"));
        assert_eq!(g.commit_at(4).map(|c| c.hash.as_str()), Some("bbbbbbb"));
        assert!(g.entry_at(4).is_none());
        assert!(g.commit_at(5).is_none());
    }
}
