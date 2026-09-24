//! Editor tabs: `Tab`, preview keys, visual diff rows and the `Model`'s tab
//! lookups.

use std::path::PathBuf;

use crate::core::buffer::Buffer;
use crate::services::git::CommitRow;

use super::Model;

pub struct Tab {
    /// Process-unique id, stable while tabs open/close/reorder. Async results
    /// (LSP tokens), dialogs and menus hold this instead of a `tabs` index,
    /// which can point at a different tab by the time they resolve.
    pub id: usize,
    pub buffer: Buffer,
    /// The file's content at git HEAD, for the change gutter. Loaded async.
    pub head_text: Option<String>,
    /// Opened from the Git panel as a diff: changed lines get a colored background.
    pub diff_mode: bool,
    /// Set for files that could not be opened (binary / unreadable): the editor
    /// shows this message centered instead of the (empty) buffer, and editing is
    /// disabled so the file is never overwritten.
    pub notice: Option<String>,
    /// Tab bar label override, for tabs that are not a file on disk (a commit
    /// patch). `None` = derive it from the buffer's file name.
    pub label: Option<String>,
    /// Generated content that must never be edited or saved (a commit patch).
    pub read_only: bool,
    /// What each buffer line of a commit's diff view is: the gutter shows the
    /// file's own line numbers instead of this view's row count, and the heading
    /// rows are styled rather than highlighted as code. Empty for a normal file.
    pub commit_rows: Vec<CommitRow>,
    /// Stable id for a tab with no backing file (a scratch "Untitled-N" buffer
    /// created via `Action::NewUntitledFile`), used as its session key since it
    /// has no path. `None` once the buffer is saved to a real path, and for
    /// every other kind of tab.
    pub untitled_id: Option<String>,
    /// Preview tab (VSCode-style): opened by arrowing through the Files panel.
    /// The next preview replaces it in place instead of opening another tab.
    /// Cleared once the file is really opened (Enter/click) or edited.
    pub preview: bool,
}

impl Tab {
    pub fn new(buffer: Buffer) -> Self {
        static NEXT_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);
        Tab {
            id: NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            buffer,
            head_text: None,
            diff_mode: false,
            notice: None,
            label: None,
            read_only: false,
            commit_rows: Vec::new(),
            untitled_id: None,
            preview: false,
        }
    }

    /// A fresh scratch buffer with no backing file ("Untitled-N"), typed into
    /// first and named on save (`Action::Save` opens a Save As dialog for a
    /// pathless buffer). `id` keys it in the session file until it gets a path.
    pub fn untitled(id: String, seq: u64) -> Self {
        let mut tab = Tab::new(Buffer::new(None, ""));
        tab.label = Some(format!("Untitled-{seq}"));
        tab.untitled_id = Some(id);
        tab
    }

    /// A read-only diff tab for a history commit, titled "<hash> diff".
    ///
    /// It is an ordinary diff-mode tab: the commit's side of the changes is the
    /// buffer and the parent's side is the "HEAD" text, so the same machinery that
    /// paints an uncommitted change paints this one — added lines on a green
    /// background, removed lines woven in red.
    ///
    /// The buffer gets a synthetic `<hash>.<ext>` path, never written (the tab is
    /// read-only): it only picks the syntax the code is highlighted with, taken
    /// from the file the commit changed most.
    pub fn commit_diff(hash: &str, diff: &crate::services::git::CommitDiff) -> Self {
        let name = match &diff.syntax_ext {
            Some(ext) => format!("{hash}.{ext}"),
            None => hash.to_string(),
        };
        let mut tab = Tab::new(Buffer::new(Some(std::path::PathBuf::from(name)), &diff.new));
        tab.head_text = Some(diff.old.clone());
        tab.commit_rows = diff.rows.clone();
        tab.diff_mode = true;
        tab.label = Some(format!("{hash} diff"));
        tab.read_only = true;
        tab
    }

    /// A read-only tab that just shows an error message (binary / unreadable file).
    pub fn notice(path: std::path::PathBuf, message: String) -> Self {
        let mut tab = Tab::new(Buffer::new(Some(path), ""));
        tab.notice = Some(message);
        tab
    }

    /// Tab bar label: file name, with a "(diff)" suffix for diff-mode tabs.
    pub fn title(&self) -> String {
        if let Some(label) = &self.label {
            return label.clone();
        }
        let name = self.buffer.display_name();
        if self.diff_mode {
            format!("{name} (diff)")
        } else {
            name
        }
    }
}

/// A tab's session key: its file path, or `"untitled:<id>"` for a scratch
/// buffer. `None` for a generated tab (commit patch, binary notice) that the
/// session never persists.
pub(super) fn tab_session_key(t: &Tab) -> Option<String> {
    if let Some(id) = &t.untitled_id {
        return Some(format!("untitled:{id}"));
    }
    t.buffer.path.as_ref().map(|p| p.display().to_string())
}

/// What a preview tab shows: a file (Files panel), a file's working-tree diff
/// or a commit's patch (Git panel). Identifies an in-flight preview load.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PreviewKey {
    File(PathBuf),
    Diff(PathBuf),
    Commit(String),
}

/// One visual row of the editor. In a diff tab, removed lines are woven in as
/// `Deleted` rows between the real buffer lines; every other tab is all `Real`.
#[derive(Clone)]
pub enum DiffRow {
    /// A real buffer line (0-based index).
    Real(usize),
    /// A removed line's text (shown red, not part of the buffer).
    Deleted(String),
}

impl Model {
    pub fn active_buffer(&self) -> Option<&Buffer> {
        self.active_tab.map(|i| &self.tabs[i].buffer)
    }

    pub fn active_buffer_mut(&mut self) -> Option<&mut Buffer> {
        let i = self.active_tab?;
        Some(&mut self.tabs[i].buffer)
    }

    /// Current index of the tab with stable id `id` (see `Tab::id`).
    pub fn tab_by_id(&self, id: usize) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    /// Is there already an open *normal* (non-diff) tab for a given file?
    /// Diff tabs are excluded so a file and its diff live in separate tabs.
    pub fn tab_index_for(&self, path: &std::path::Path) -> Option<usize> {
        self.tabs
            .iter()
            .position(|t| !t.diff_mode && t.buffer.path.as_deref() == Some(path))
    }

    /// Indices of every open tab (normal or diff) for a given file.
    pub fn all_tabs_for(&self, path: &std::path::Path) -> Vec<usize> {
        self.tabs
            .iter()
            .enumerate()
            .filter(|(_, t)| t.buffer.path.as_deref() == Some(path))
            .map(|(i, _)| i)
            .collect()
    }

    /// Is there already an open diff-mode tab for a given file?
    pub fn diff_tab_index_for(&self, path: &std::path::Path) -> Option<usize> {
        self.tabs
            .iter()
            .position(|t| t.diff_mode && t.buffer.path.as_deref() == Some(path))
    }

    /// Whether the active tab is a diff-mode tab (changed lines get a colored background).
    pub fn active_is_diff(&self) -> bool {
        self.active_tab
            .map(|i| self.tabs[i].diff_mode)
            .unwrap_or(false)
    }

    /// The open "<hash> diff" tab for a commit, if any.
    pub fn commit_diff_tab_index(&self, hash: &str) -> Option<usize> {
        let label = format!("{hash} diff");
        self.tabs
            .iter()
            .position(|t| t.label.as_deref() == Some(label.as_str()))
    }

    /// What the active tab's buffer line `row` is, when it is a commit's diff
    /// view: the gutter and the row styling follow from it. `None` for a file.
    pub fn commit_row(&self, row: usize) -> Option<CommitRow> {
        let i = self.active_tab?;
        self.tabs[i].commit_rows.get(row).copied()
    }

    /// The largest number the gutter has to fit: the buffer's line count, or the
    /// highest file line number in a commit's diff view (which skips lines, so it
    /// can run past the number of rows shown).
    pub fn max_gutter_number(&self) -> usize {
        let Some(i) = self.active_tab else {
            return 1;
        };
        let tab = &self.tabs[i];
        if tab.commit_rows.is_empty() {
            return tab.buffer.line_count();
        }
        tab.commit_rows
            .iter()
            .filter_map(|r| match r {
                CommitRow::Line(n) => Some(*n),
                _ => None,
            })
            .max()
            .unwrap_or(1)
    }

    /// Whether the active tab holds generated content that must not be edited.
    pub fn active_read_only(&self) -> bool {
        self.active_tab
            .map(|i| self.tabs[i].read_only)
            .unwrap_or(false)
    }

    /// The notice message of the active tab, if it is a read-only error tab.
    pub fn active_notice(&self) -> Option<&str> {
        let i = self.active_tab?;
        self.tabs[i].notice.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_diff_tab_is_a_read_only_diff_tab() {
        let diff = crate::services::git::CommitDiff {
            old: "a\nb\n".to_string(),
            new: "a\nB\n".to_string(),
            rows: vec![CommitRow::Line(1), CommitRow::Line(2)],
            syntax_ext: Some("rs".to_string()),
        };
        let tab = Tab::commit_diff("2ea14b1", &diff);
        assert_eq!(tab.title(), "2ea14b1 diff");
        assert!(tab.read_only);
        assert!(tab.diff_mode); // green/red backgrounds, like an uncommitted change
        assert_eq!(tab.head_text.as_deref(), Some("a\nb\n"));
        // The synthetic path only picks the syntax the code is colored with.
        assert_eq!(
            tab.buffer.path.as_deref(),
            Some(std::path::Path::new("2ea14b1.rs"))
        );
    }
}
