//! Change-gutter / inline-diff state for the active tab: git markers, woven
//! deletion rows and the visual row map built from them.

use super::{DiffRow, Model};

impl Model {
    /// Forces the change-gutter diff to recompute on the next `refresh_git_marks`
    /// (file saved, reloaded, changed on disk, or its HEAD text (re)loaded).
    pub fn mark_git_dirty(&mut self) {
        self.git_marks_dirty = true;
    }

    /// Refreshes the change-gutter state before render. The visual row map
    /// (`active_display`) is rebuilt immediately on any version change — the
    /// renderer maps screen rows to buffer lines through it, so it must never lag.
    /// The git *diff* itself recomputes only on a tab switch or when
    /// `git_marks_dirty` is set (save / reload / disk change / HEAD load); a plain
    /// edit leaves the last-computed markers frozen, so typing never runs the
    /// whole-file diff.
    pub fn refresh_git_marks(&mut self) {
        let Some(i) = self.active_tab else {
            self.active_git_marks.clear();
            self.active_deleted.clear();
            self.active_display.clear();
            self.active_git_marks_tab = None;
            self.active_display_key = None;
            return;
        };
        let ver = self.tabs[i].buffer.version;
        // On a tab switch or an explicit trigger, rerun the whole-file diff; it
        // rebuilds the row map itself (woven deletions may have changed). Otherwise
        // a plain edit only needs the row map to track the new line count.
        if self.active_git_marks_tab != Some(i) || self.git_marks_dirty {
            self.recompute_git_marks();
        } else if self.active_display_key != Some((i, ver)) {
            self.rebuild_display(i);
            self.active_display_key = Some((i, ver));
        }
    }

    /// Runs the whole-file HEAD-vs-buffer diff for the active tab, refreshing the
    /// gutter markers and any woven deletion rows. Called from `refresh_git_marks`
    /// only on a tab switch or an explicit `git_marks_dirty` trigger.
    fn recompute_git_marks(&mut self) {
        let Some(i) = self.active_tab else {
            return;
        };
        let ver = self.tabs[i].buffer.version;
        self.active_git_marks.clear();
        self.active_deleted.clear();
        let tab = &self.tabs[i];
        if let Some(head) = &tab.head_text {
            let new = tab.buffer.full_text();
            self.active_git_marks
                .extend(crate::services::git::gutter_marks(head, &new));
            // Removed lines are only woven into the inline diff view.
            if tab.diff_mode {
                self.active_deleted = crate::services::git::deleted_blocks(head, &new);
            }
        }
        // Woven deletions may have changed -> refresh the visual row map with them.
        self.rebuild_display(i);
        self.active_display_key = Some((i, ver));
        self.active_git_marks_tab = Some(i);
        self.git_marks_dirty = false;
    }

    /// Rebuilds `active_display` for tab `i` from its line count and any woven
    /// deletions. Called only when the buffer version changes, so the render path
    /// can borrow the result instead of rebuilding it every frame.
    fn rebuild_display(&mut self, i: usize) {
        let n = self.tabs[i].buffer.line_count();
        self.active_display.clear();
        if !self.has_inline_deletions() {
            self.active_display.extend((0..n).map(DiffRow::Real));
            return;
        }
        self.active_display.reserve(n + self.active_deleted.len());
        // Blocks ordered by anchor (`None` = before the first line sorts first),
        // merged in one pass: O(n + blocks), not a block scan per line.
        let mut blocks: Vec<&(Option<usize>, Vec<String>)> = self.active_deleted.iter().collect();
        blocks.sort_by_key(|(anchor, _)| *anchor);
        let mut next = blocks.into_iter().peekable();
        let mut emit_anchored = |display: &mut Vec<DiffRow>, at: Option<usize>| {
            while let Some((_, lines)) = next.next_if(|(anchor, _)| *anchor == at) {
                display.extend(lines.iter().cloned().map(DiffRow::Deleted));
            }
        };
        emit_anchored(&mut self.active_display, None);
        for r in 0..n {
            self.active_display.push(DiffRow::Real(r));
            emit_anchored(&mut self.active_display, Some(r));
        }
    }

    /// Whether the active tab weaves removed lines into its view (diff tab with deletions).
    pub fn has_inline_deletions(&self) -> bool {
        self.active_is_diff() && !self.active_deleted.is_empty()
    }

    /// The visual rows for the active tab: `Real(0..n)` normally, or real lines
    /// interleaved with `Deleted` rows in a diff tab that has removals. Borrowed
    /// from a cache rebuilt only on edit (see `refresh_git_marks`), so the render
    /// path pays nothing to read it.
    pub fn diff_rows(&self) -> &[DiffRow] {
        &self.active_display
    }

    /// Display index of the first row to draw for a given buffer scroll offset.
    pub fn diff_start(&self, rows: &[DiffRow], scroll_y: usize) -> usize {
        if scroll_y == 0 {
            return 0;
        }
        // No woven deletions means the rows are the identity mapping, so line
        // `scroll_y` is at index `scroll_y` — skip scanning the whole prefix.
        if matches!(rows.get(scroll_y), Some(DiffRow::Real(l)) if *l == scroll_y) {
            return scroll_y;
        }
        rows.iter()
            .position(|r| matches!(r, DiffRow::Real(l) if *l == scroll_y))
            .unwrap_or(0)
    }

    /// Buffer line under a viewport row `offset` (0 = top visible row), mapping
    /// `Deleted` rows to the nearest following (then preceding) real line.
    pub fn screen_row_to_line(&self, offset: usize) -> usize {
        let Some(i) = self.active_tab else {
            return 0;
        };
        let buf = &self.tabs[i].buffer;
        let last = buf.line_count().saturating_sub(1);
        if !self.has_inline_deletions() {
            return (buf.scroll_y + offset).min(last);
        }
        let rows = self.diff_rows();
        let start = self.diff_start(rows, buf.scroll_y);
        let idx = (start + offset).min(rows.len().saturating_sub(1));
        for r in &rows[idx..] {
            if let DiffRow::Real(l) = r {
                return *l;
            }
        }
        for r in rows[..=idx].iter().rev() {
            if let DiffRow::Real(l) = r {
                return *l;
            }
        }
        last
    }

    /// Whether the editor should reserve a change-gutter column (active file is tracked).
    pub fn git_gutter(&self) -> bool {
        self.active_tab
            .map(|i| self.tabs[i].head_text.is_some())
            .unwrap_or(false)
    }
}
