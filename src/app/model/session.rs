//! Session persistence glue: restore bookkeeping, checkpoint scheduling and
//! the snapshot written by `services::session`.

use super::{Model, tab::tab_session_key};

/// What to restore onto a tab once its async `Cmd::ReadFile` result arrives
/// during session restore (see `Model.pending_session_restore`).
pub struct SessionRestore {
    pub line: usize,
    pub col: usize,
    pub scroll_y: usize,
    pub scroll_x: usize,
    /// Present only for a dirty file that was stored as a diff against its
    /// on-disk baseline: hunks to apply to the freshly loaded disk content.
    pub dirty_hunks: Option<Vec<crate::services::session::Hunk>>,
}

impl Model {
    /// Schedules a debounced session checkpoint (reset on every edit), so a
    /// burst of typing writes the session file once it pauses rather than on
    /// every keystroke.
    pub fn schedule_session_save(&mut self) {
        self.session_save_at =
            Some(std::time::Instant::now() + crate::services::session::CHECKPOINT_DEBOUNCE);
    }

    /// Next number to hand out for a new "Untitled-N" scratch buffer.
    pub fn next_untitled_seq(&mut self) -> u64 {
        self.untitled_seq += 1;
        self.untitled_seq
    }

    /// A snapshot of every open tab plus window layout, for the session
    /// checkpoint. Content is included only for dirty tabs (see
    /// `services::session::Content`); large dirty files diff against their
    /// current on-disk text where one is still readable, else keep full text.
    pub fn session_snapshot(&self) -> crate::services::session::SessionSnapshot {
        use crate::services::session::{Content, TabEntry};
        let tabs = self
            .tabs
            .iter()
            // Generated / read-only tabs (a commit patch, a binary-file notice)
            // are not something the user edited — never worth restoring.
            .filter(|t| !t.read_only && t.notice.is_none())
            .map(|t| {
                let buf = &t.buffer;
                // Only a dirty buffer's text is stored. A large one is shrunk to
                // a diff against disk later, off the UI thread
                // (`services::session::compact`) — no IO here.
                let content = buf.dirty.then(|| Content::Full {
                    text: buf.full_text(),
                });
                TabEntry {
                    kind: if t.untitled_id.is_some() {
                        "untitled".into()
                    } else {
                        "file".into()
                    },
                    path: buf.path.as_ref().map(|p| p.display().to_string()),
                    untitled_id: t.untitled_id.clone(),
                    // Only an untitled tab's label ("Untitled-N") is worth
                    // persisting — a file tab derives its title from the path.
                    label: if t.untitled_id.is_some() {
                        t.label.clone()
                    } else {
                        None
                    },
                    line: buf.cursor.line,
                    col: buf.cursor.col,
                    scroll_y: buf.scroll_y,
                    scroll_x: buf.scroll_x,
                    dirty: buf.dirty,
                    content,
                }
            })
            .collect();
        // Identified by key (path, or "untitled:<id>"), not raw index: the
        // filter above already dropped generated tabs, so a plain position
        // would drift out from under the active tab it meant to name.
        let active = self
            .active_tab
            .and_then(|i| self.tabs.get(i))
            .and_then(tab_session_key);
        crate::services::session::SessionSnapshot {
            root: self.root.display().to_string(),
            generation: 0, // filled in by `services::session::save`
            active,
            sidebar_panel: format!("{:?}", self.sidebar.active),
            sidebar_width: self.layout.sidebar_width,
            terminal_open: self.layout.terminal_open,
            untitled_seq: self.untitled_seq,
            tabs,
        }
    }
}
