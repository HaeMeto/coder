//! Rope-based text buffer: cursor, selection, undo/redo, dirty flag.

use std::path::PathBuf;
use std::time::Instant;

use ropey::Rope;

/// A position within the text (line, column) — both 0-based, in character units.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Cursor {
    pub line: usize,
    pub col: usize,
}

/// A single undoable edit: replaces the `before` text with `after` at
/// position `char_idx`. Undo applies these in reverse.
#[derive(Clone, Debug)]
struct Edit {
    char_idx: usize,
    before: String,
    after: String,
    cursor_before: Cursor,
    cursor_after: Cursor,
    /// Time of the last change, used to group consecutive typing.
    stamp: Instant,
    /// Is this edit a pure "typing" edit (consecutive character insertion)?
    typing: bool,
}

pub struct Buffer {
    pub path: Option<PathBuf>,
    pub rope: Rope,
    pub cursor: Cursor,
    /// Selection anchor. When `Some`, the range between it and the cursor is selected.
    pub anchor: Option<Cursor>,
    /// Topmost visible line of the editor viewport.
    pub scroll_y: usize,
    pub scroll_x: usize,
    pub dirty: bool,
    /// Version that increments on every edit; used to invalidate the highlight cache.
    pub version: u64,
    undo_stack: Vec<Edit>,
    redo_stack: Vec<Edit>,
}

impl Buffer {
    pub fn new(path: Option<PathBuf>, text: &str) -> Self {
        Buffer {
            path,
            rope: Rope::from_str(text),
            cursor: Cursor::default(),
            anchor: None,
            scroll_y: 0,
            scroll_x: 0,
            dirty: false,
            version: 0,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    #[cfg(test)]
    pub fn scratch() -> Self {
        Buffer::new(None, "")
    }

    pub fn display_name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "untitled".to_string())
    }

    pub fn line_count(&self) -> usize {
        self.rope.len_lines().max(1)
    }

    /// Character length of the given line (excluding the line ending).
    pub fn line_len(&self, line: usize) -> usize {
        if line >= self.rope.len_lines() {
            return 0;
        }
        let slice = self.rope.line(line);
        let mut len = slice.len_chars();
        // Exclude line-ending characters from the count.
        while len > 0 {
            let c = slice.char(len - 1);
            if c == '\n' || c == '\r' {
                len -= 1;
            } else {
                break;
            }
        }
        len
    }

    pub fn line_text(&self, line: usize) -> String {
        if line >= self.rope.len_lines() {
            return String::new();
        }
        let slice = self.rope.line(line);
        let mut s: String = slice.chars().collect();
        while s.ends_with('\n') || s.ends_with('\r') {
            s.pop();
        }
        s
    }

    pub fn full_text(&self) -> String {
        self.rope.to_string()
    }

    /// Absolute character index of the cursor (for find/replace positioning).
    pub fn cursor_char_index(&self) -> usize {
        self.cursor_to_char(self.cursor)
    }

    fn cursor_to_char(&self, c: Cursor) -> usize {
        let line = c.line.min(self.rope.len_lines().saturating_sub(1));
        let line_start = self.rope.line_to_char(line);
        let max_col = self.line_len(line);
        line_start + c.col.min(max_col)
    }

    fn char_to_cursor(&self, idx: usize) -> Cursor {
        let idx = idx.min(self.rope.len_chars());
        let line = self.rope.char_to_line(idx);
        let line_start = self.rope.line_to_char(line);
        Cursor {
            line,
            col: idx - line_start,
        }
    }

    // ----- LSP position conversion (UTF-16 code units <-> char columns) -----
    //
    // LSP `Position.character` counts UTF-16 code units within a line, while the
    // rope (and `Cursor.col`) uses Unicode-scalar (char) indices. These differ
    // for any non-BMP character (emoji, some CJK), so every LSP boundary must
    // convert. Do this only here (rope is the source of truth), never in async.

    /// Char column within a line -> UTF-16 code-unit offset (outgoing to LSP).
    pub fn char_col_to_utf16(&self, line: usize, col: usize) -> u32 {
        if line >= self.rope.len_lines() {
            return 0;
        }
        let slice = self.rope.line(line);
        let col = col.min(self.line_len(line));
        slice.char_to_utf16_cu(col) as u32
    }

    /// UTF-16 code-unit offset within a line -> char column (incoming from LSP),
    /// clamped to the line's content length.
    pub fn utf16_to_char_col(&self, line: usize, utf16: u32) -> usize {
        if line >= self.rope.len_lines() {
            return 0;
        }
        let slice = self.rope.line(line);
        let max_col = self.line_len(line);
        let max_u16 = slice.char_to_utf16_cu(max_col);
        let u = (utf16 as usize).min(max_u16);
        slice.utf16_cu_to_char(u).min(max_col)
    }

    /// Absolute char index of an LSP position (line + UTF-16 character). Clamps a
    /// past-the-end line to the document end (LSP edits can target EOF).
    pub fn lsp_pos_to_char(&self, line: usize, utf16: u32) -> usize {
        if line >= self.rope.len_lines() {
            return self.rope.len_chars();
        }
        self.rope.line_to_char(line) + self.utf16_to_char_col(line, utf16)
    }

    // ----- Selection -----

    /// The selection's (start, end) cursors in sorted order.
    pub fn selection_range(&self) -> Option<(Cursor, Cursor)> {
        let a = self.anchor?;
        if a == self.cursor {
            return None;
        }
        if a < self.cursor {
            Some((a, self.cursor))
        } else {
            Some((self.cursor, a))
        }
    }

    pub fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection_range()?;
        let s = self.cursor_to_char(start);
        let e = self.cursor_to_char(end);
        Some(self.rope.slice(s..e).to_string())
    }

    pub fn clear_selection(&mut self) {
        self.anchor = None;
    }

    /// Selection handling before a move: when `extend` is true the anchor is kept.
    fn pre_move(&mut self, extend: bool) {
        if extend {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = None;
        }
    }

    // ----- Cursor movements -----

    pub fn move_left(&mut self, extend: bool) {
        self.pre_move(extend);
        if self.cursor.col > 0 {
            self.cursor.col -= 1;
        } else if self.cursor.line > 0 {
            self.cursor.line -= 1;
            self.cursor.col = self.line_len(self.cursor.line);
        }
    }

    pub fn move_right(&mut self, extend: bool) {
        self.pre_move(extend);
        let len = self.line_len(self.cursor.line);
        if self.cursor.col < len {
            self.cursor.col += 1;
        } else if self.cursor.line + 1 < self.line_count() {
            self.cursor.line += 1;
            self.cursor.col = 0;
        }
    }

    pub fn move_up(&mut self, extend: bool) {
        self.pre_move(extend);
        if self.cursor.line > 0 {
            self.cursor.line -= 1;
            self.cursor.col = self.cursor.col.min(self.line_len(self.cursor.line));
        } else {
            self.cursor.col = 0;
        }
    }

    pub fn move_down(&mut self, extend: bool) {
        self.pre_move(extend);
        if self.cursor.line + 1 < self.line_count() {
            self.cursor.line += 1;
            self.cursor.col = self.cursor.col.min(self.line_len(self.cursor.line));
        } else {
            self.cursor.col = self.line_len(self.cursor.line);
        }
    }

    pub fn move_home(&mut self, extend: bool) {
        self.pre_move(extend);
        self.cursor.col = 0;
    }

    pub fn move_end(&mut self, extend: bool) {
        self.pre_move(extend);
        self.cursor.col = self.line_len(self.cursor.line);
    }

    /// Moves the cursor left to the previous word boundary (Ctrl+Left).
    /// Skips whitespace, then a run of same-class characters (word vs. symbol).
    pub fn move_word_left(&mut self, extend: bool) {
        self.pre_move(extend);
        let mut i = self.cursor_to_char(self.cursor);
        let is_word = |ch: char| ch.is_alphanumeric() || ch == '_';
        while i > 0 && self.rope.char(i - 1).is_whitespace() {
            i -= 1;
        }
        if i > 0 {
            let word = is_word(self.rope.char(i - 1));
            while i > 0 {
                let c = self.rope.char(i - 1);
                if c.is_whitespace() || is_word(c) != word {
                    break;
                }
                i -= 1;
            }
        }
        self.cursor = self.char_to_cursor(i);
    }

    /// Moves the cursor right to the next word boundary (Ctrl+Right).
    pub fn move_word_right(&mut self, extend: bool) {
        self.pre_move(extend);
        let len = self.rope.len_chars();
        let mut i = self.cursor_to_char(self.cursor);
        let is_word = |ch: char| ch.is_alphanumeric() || ch == '_';
        while i < len && self.rope.char(i).is_whitespace() {
            i += 1;
        }
        if i < len {
            let word = is_word(self.rope.char(i));
            while i < len {
                let c = self.rope.char(i);
                if c.is_whitespace() || is_word(c) != word {
                    break;
                }
                i += 1;
            }
        }
        self.cursor = self.char_to_cursor(i);
    }

    pub fn move_page(&mut self, delta: isize, extend: bool) {
        self.pre_move(extend);
        let target = (self.cursor.line as isize + delta)
            .clamp(0, self.line_count().saturating_sub(1) as isize)
            as usize;
        self.cursor.line = target;
        self.cursor.col = self.cursor.col.min(self.line_len(self.cursor.line));
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(Cursor { line: 0, col: 0 });
        let last = self.line_count().saturating_sub(1);
        self.cursor = Cursor {
            line: last,
            col: self.line_len(last),
        };
    }

    /// Selects the word (identifier run) at the given position. No-op when there
    /// is no word character to select there. Used by editor double-click.
    pub fn select_word_at(&mut self, c: Cursor) {
        let line = c.line.min(self.line_count().saturating_sub(1));
        let text: Vec<char> = self.line_text(line).chars().collect();
        let len = text.len();
        let col = c.col.min(len);
        let is_word = |ch: char| ch.is_alphanumeric() || ch == '_';
        if !text.get(col).copied().map(is_word).unwrap_or(false) {
            return; // not on a word character
        }
        let (mut s, mut e) = (col, col);
        while s > 0 && is_word(text[s - 1]) {
            s -= 1;
        }
        while e < len && is_word(text[e]) {
            e += 1;
        }
        if s == e {
            return;
        }
        self.anchor = Some(Cursor { line, col: s });
        self.cursor = Cursor { line, col: e };
    }

    /// Selects the character range [start, end) given in absolute character
    /// indices (used to highlight a find match). Clamps into the text.
    pub fn select_char_range(&mut self, start: usize, end: usize) {
        self.anchor = Some(self.char_to_cursor(start));
        self.cursor = self.char_to_cursor(end);
    }

    pub fn set_cursor(&mut self, c: Cursor, extend: bool) {
        self.pre_move(extend);
        let line = c.line.min(self.line_count().saturating_sub(1));
        self.cursor = Cursor {
            line,
            col: c.col.min(self.line_len(line)),
        };
    }

    // ----- Editing -----

    /// Deletes the selection (if any) and records the edit (merged into a single undo step).
    fn delete_selection_internal(&mut self) -> bool {
        if let Some((start, end)) = self.selection_range() {
            let s = self.cursor_to_char(start);
            let e = self.cursor_to_char(end);
            let removed = self.rope.slice(s..e).to_string();
            let cursor_before = self.cursor;
            self.rope.remove(s..e);
            self.anchor = None;
            self.cursor = start;
            self.push_edit(Edit {
                char_idx: s,
                before: removed,
                after: String::new(),
                cursor_before,
                cursor_after: self.cursor,
                stamp: Instant::now(),
                typing: false,
            });
            true
        } else {
            false
        }
    }

    pub fn insert_char(&mut self, ch: char) {
        self.delete_selection_internal();
        let idx = self.cursor_to_char(self.cursor);
        let cursor_before = self.cursor;
        self.rope.insert_char(idx, ch);
        self.cursor = self.char_to_cursor(idx + 1);
        let is_word = !ch.is_whitespace();
        self.push_edit(Edit {
            char_idx: idx,
            before: String::new(),
            after: ch.to_string(),
            cursor_before,
            cursor_after: self.cursor,
            stamp: Instant::now(),
            typing: is_word,
        });
    }

    pub fn insert_str(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.delete_selection_internal();
        let idx = self.cursor_to_char(self.cursor);
        let cursor_before = self.cursor;
        self.rope.insert(idx, text);
        let char_len = text.chars().count();
        self.cursor = self.char_to_cursor(idx + char_len);
        self.push_edit(Edit {
            char_idx: idx,
            before: String::new(),
            after: text.to_string(),
            cursor_before,
            cursor_after: self.cursor,
            stamp: Instant::now(),
            typing: false,
        });
    }

    /// The current line's leading whitespace, clipped at the cursor so pressing
    /// Enter *inside* the indent only carries the part the cursor is past.
    fn indent_at_cursor(&self) -> String {
        self.line_text(self.cursor.line)
            .chars()
            .take(self.cursor.col)
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect()
    }

    /// Inserts a line break, carrying the current line's indentation onto the
    /// new line. The break and the indent are one edit, so undo takes both and
    /// the typing group ends here.
    pub fn insert_newline(&mut self) {
        self.delete_selection_internal();
        let indent = self.indent_at_cursor();
        self.insert_str(&format!("\n{indent}"));
    }

    pub fn backspace(&mut self) {
        if self.delete_selection_internal() {
            return;
        }
        let idx = self.cursor_to_char(self.cursor);
        if idx == 0 {
            return;
        }
        let removed: String = self.rope.slice(idx - 1..idx).to_string();
        let cursor_before = self.cursor;
        self.rope.remove(idx - 1..idx);
        self.cursor = self.char_to_cursor(idx - 1);
        self.push_edit(Edit {
            char_idx: idx - 1,
            before: removed,
            after: String::new(),
            cursor_before,
            cursor_after: self.cursor,
            stamp: Instant::now(),
            typing: false,
        });
    }

    pub fn delete_forward(&mut self) {
        if self.delete_selection_internal() {
            return;
        }
        let idx = self.cursor_to_char(self.cursor);
        if idx >= self.rope.len_chars() {
            return;
        }
        let removed: String = self.rope.slice(idx..idx + 1).to_string();
        let cursor_before = self.cursor;
        self.rope.remove(idx..idx + 1);
        self.push_edit(Edit {
            char_idx: idx,
            before: removed,
            after: String::new(),
            cursor_before,
            cursor_after: self.cursor,
            stamp: Instant::now(),
            typing: false,
        });
    }

    fn push_edit(&mut self, edit: Edit) {
        self.dirty = true;
        self.version += 1;
        self.redo_stack.clear();

        // Merge consecutive typed characters into a single undo step.
        if edit.typing
            && let Some(last) = self.undo_stack.last_mut() {
                let contiguous = last.typing
                    && last.before.is_empty()
                    && edit.before.is_empty()
                    && last.char_idx + last.after.chars().count() == edit.char_idx
                    && edit.stamp.duration_since(last.stamp).as_millis() < 600;
                if contiguous {
                    last.after.push_str(&edit.after);
                    last.cursor_after = edit.cursor_after;
                    last.stamp = edit.stamp;
                    return;
                }
            }
        self.undo_stack.push(edit);
    }

    pub fn undo(&mut self) {
        if let Some(edit) = self.undo_stack.pop() {
            let start = edit.char_idx;
            let after_len = edit.after.chars().count();
            // Remove the `after` text and restore the `before` text.
            self.rope.remove(start..start + after_len);
            if !edit.before.is_empty() {
                self.rope.insert(start, &edit.before);
            }
            self.cursor = edit.cursor_before;
            self.anchor = None;
            self.version += 1;
            self.dirty = true;
            self.redo_stack.push(edit);
        }
    }

    pub fn redo(&mut self) {
        if let Some(edit) = self.redo_stack.pop() {
            let start = edit.char_idx;
            let before_len = edit.before.chars().count();
            self.rope.remove(start..start + before_len);
            if !edit.after.is_empty() {
                self.rope.insert(start, &edit.after);
            }
            self.cursor = edit.cursor_after;
            self.anchor = None;
            self.version += 1;
            self.dirty = true;
            self.undo_stack.push(edit);
        }
    }

    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }

    /// Replaces the entire buffer content, recorded as a single undo step.
    /// The cursor is clamped into the new text. No-op if the text is unchanged.
    pub fn replace_all(&mut self, text: &str) {
        let old = self.rope.to_string();
        if old == text {
            return;
        }
        let cursor_before = self.cursor;
        self.rope = Rope::from_str(text);
        let line = self.cursor.line.min(self.line_count().saturating_sub(1));
        self.cursor = Cursor {
            line,
            col: self.cursor.col.min(self.line_len(line)),
        };
        self.anchor = None;
        self.push_edit(Edit {
            char_idx: 0,
            before: old,
            after: text.to_string(),
            cursor_before,
            cursor_after: self.cursor,
            stamp: Instant::now(),
            typing: false,
        });
    }

    /// Deletes the selected text (recorded as a single undo operation). Returns false if there is no selection.
    pub fn delete_selection(&mut self) -> bool {
        self.delete_selection_internal()
    }

    /// Moves the cursor to the start of the given line (for search results / goto).
    pub fn goto_line(&mut self, line: usize) {
        self.anchor = None;
        let line = line.min(self.line_count().saturating_sub(1));
        self.cursor = Cursor { line, col: 0 };
    }

    /// Scrolls so the cursor line sits at the top of the viewport.
    pub fn scroll_cursor_to_top(&mut self) {
        self.scroll_y = self.cursor.line;
        self.scroll_x = 0;
    }

    /// Scrolls so the cursor line sits (roughly) in the vertical center.
    pub fn center_cursor(&mut self, height: usize) {
        self.scroll_y = self.cursor.line.saturating_sub(height / 2);
    }

    /// Adjusts scroll to keep the cursor visible given the viewport height.
    pub fn ensure_visible(&mut self, height: usize, width: usize) {
        if height == 0 {
            return;
        }
        if self.cursor.line < self.scroll_y {
            self.scroll_y = self.cursor.line;
        } else if self.cursor.line >= self.scroll_y + height {
            self.scroll_y = self.cursor.line + 1 - height;
        }
        if width > 0 {
            if self.cursor.col < self.scroll_x {
                self.scroll_x = self.cursor.col;
            } else if self.cursor.col >= self.scroll_x + width {
                self.scroll_x = self.cursor.col + 1 - width;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_offsets_handle_emoji() {
        // "a😀b": 'a'=1 utf16, '😀'=2 utf16 (surrogate pair), 'b'=1 utf16.
        let b = Buffer::new(None, "a😀b");
        // char col -> utf16
        assert_eq!(b.char_col_to_utf16(0, 0), 0);
        assert_eq!(b.char_col_to_utf16(0, 1), 1); // after 'a'
        assert_eq!(b.char_col_to_utf16(0, 2), 3); // after emoji (1 + 2)
        assert_eq!(b.char_col_to_utf16(0, 3), 4); // after 'b'
        // utf16 -> char col (round trip)
        assert_eq!(b.utf16_to_char_col(0, 0), 0);
        assert_eq!(b.utf16_to_char_col(0, 1), 1);
        assert_eq!(b.utf16_to_char_col(0, 3), 2);
        assert_eq!(b.utf16_to_char_col(0, 4), 3);
        // Out-of-range utf16 clamps to line end.
        assert_eq!(b.utf16_to_char_col(0, 99), 3);
    }

    #[test]
    fn anchor_selection_replaces_prefix() {
        // Completion accept: select the typed prefix, then insert replaces it.
        let mut b = Buffer::new(None, "prin");
        b.cursor = Cursor { line: 0, col: 4 };
        b.anchor = Some(Cursor { line: 0, col: 0 });
        b.insert_str("println!");
        assert_eq!(b.full_text(), "println!");
        assert_eq!(b.cursor.col, 8);
    }

    #[test]
    fn insert_and_undo() {
        let mut b = Buffer::scratch();
        for c in "hello".chars() {
            b.insert_char(c);
        }
        assert_eq!(b.full_text(), "hello");
        b.undo();
        assert_eq!(b.full_text(), "");
        b.redo();
        assert_eq!(b.full_text(), "hello");
    }

    #[test]
    fn newline_keeps_indentation() {
        let mut b = Buffer::new(None, "    let x = 1;");
        b.cursor = Cursor { line: 0, col: 14 }; // end of line
        b.insert_newline();
        assert_eq!(b.full_text(), "    let x = 1;\n    ");
        assert_eq!(b.cursor, Cursor { line: 1, col: 4 });
    }

    #[test]
    fn newline_indent_splits_line_at_cursor() {
        let mut b = Buffer::new(None, "\tfoobar");
        b.cursor = Cursor { line: 0, col: 4 }; // between "foo" and "bar"
        b.insert_newline();
        assert_eq!(b.full_text(), "\tfoo\n\tbar");
    }

    #[test]
    fn newline_inside_indent_carries_only_what_cursor_passed() {
        let mut b = Buffer::new(None, "        x");
        b.cursor = Cursor { line: 0, col: 4 }; // inside the 8-space indent
        b.insert_newline();
        assert_eq!(b.full_text(), "    \n        x");
    }

    #[test]
    fn newline_on_unindented_line_adds_nothing() {
        let mut b = Buffer::new(None, "x");
        b.cursor = Cursor { line: 0, col: 1 };
        b.insert_newline();
        assert_eq!(b.full_text(), "x\n");
    }

    #[test]
    fn newline_indent_undoes_as_one_step() {
        let mut b = Buffer::new(None, "    ab");
        b.cursor = Cursor { line: 0, col: 6 };
        b.insert_newline();
        assert_eq!(b.full_text(), "    ab\n    ");
        b.undo();
        assert_eq!(b.full_text(), "    ab", "the break and its indent undo together");
    }

    #[test]
    fn newline_replaces_selection_then_indents() {
        let mut b = Buffer::new(None, "    abcd");
        b.cursor = Cursor { line: 0, col: 8 };
        b.anchor = Some(Cursor { line: 0, col: 6 }); // select "cd"
        b.insert_newline();
        assert_eq!(b.full_text(), "    ab\n    ");
    }

    #[test]
    fn newline_splits_undo_groups() {
        let mut b = Buffer::scratch();
        for c in "ab".chars() {
            b.insert_char(c);
        }
        b.insert_newline();
        for c in "cd".chars() {
            b.insert_char(c);
        }
        assert_eq!(b.full_text(), "ab\ncd");
        b.undo(); // cd
        assert_eq!(b.full_text(), "ab\n");
        b.undo(); // newline
        assert_eq!(b.full_text(), "ab");
        b.undo(); // ab
        assert_eq!(b.full_text(), "");
    }

    #[test]
    fn replace_all_is_undoable() {
        let mut b = Buffer::new(None, "a  \nb\n");
        b.replace_all("a\nb\n");
        assert_eq!(b.full_text(), "a\nb\n");
        assert!(b.dirty);
        b.undo();
        assert_eq!(b.full_text(), "a  \nb\n");
        // No-op when unchanged: no new undo step.
        b.replace_all("a  \nb\n");
        b.undo();
        assert_eq!(b.full_text(), "a  \nb\n");
    }

    #[test]
    fn double_click_selects_word() {
        let mut b = Buffer::new(None, "foo bar_baz qux");
        b.select_word_at(Cursor { line: 0, col: 5 }); // inside "bar_baz"
        assert_eq!(b.selected_text().as_deref(), Some("bar_baz"));
        // Clicking on whitespace selects nothing.
        b.clear_selection();
        b.select_word_at(Cursor { line: 0, col: 3 });
        assert!(b.selected_text().is_none());
    }

    #[test]
    fn select_char_range_spans_lines() {
        let b0 = Buffer::new(None, "abc\ndef");
        let mut b = b0;
        b.select_char_range(1, 5); // "bc\nd"
        assert_eq!(b.selected_text().as_deref(), Some("bc\nd"));
    }

    #[test]
    fn word_motion() {
        let mut b = Buffer::new(None, "foo bar_baz  qux");
        b.move_word_right(false); // start -> after "foo"
        assert_eq!(b.cursor, Cursor { line: 0, col: 3 });
        b.move_word_right(false); // -> after "bar_baz"
        assert_eq!(b.cursor, Cursor { line: 0, col: 11 });
        b.move_word_left(false); // back to start of "bar_baz"
        assert_eq!(b.cursor, Cursor { line: 0, col: 4 });
        // Shift extends: anchor stays put.
        b.move_word_right(true);
        assert_eq!(b.selected_text().as_deref(), Some("bar_baz"));
    }

    #[test]
    fn selection_delete() {
        let mut b = Buffer::new(None, "hello world");
        b.move_right(false);
        b.move_right(true);
        b.move_right(true);
        assert_eq!(b.selected_text().as_deref(), Some("el"));
        b.backspace();
        assert_eq!(b.full_text(), "hlo world");
    }
}
