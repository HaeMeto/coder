//! Syntax highlighting glue: ships jobs to the off-thread highlight worker
//! (`app::hlworker`) and holds its results for render.

use crate::core::highlight::HlLine;

use super::Model;

impl Model {
    /// The colored pieces for buffer line `row`, or `None` when the worker has not
    /// colored it yet (the renderer then draws it as plain text). Colors belong to
    /// the active tab and may trail the current version by a frame while typing.
    pub fn hl_line(&self, row: usize) -> Option<&HlLine> {
        let (tab_id, _) = self.display_key?;
        let active_id = self.active_tab.and_then(|i| self.tabs.get(i)).map(|t| t.id);
        if Some(tab_id) != active_id || row < self.display_base {
            return None;
        }
        self.display_hl.get(row - self.display_base)
    }

    /// Wires up the highlight worker channel (called once at startup).
    pub fn set_hl_worker(&mut self, tx: std::sync::mpsc::Sender<crate::app::hlworker::HlJob>) {
        self.hl_tx = Some(tx);
    }

    /// Stores a worker result, ignoring one older than what is already shown.
    /// `tab` is the stable `Tab::id` (an index could name a different tab by now).
    pub fn set_display_hl(&mut self, tab: usize, version: u64, base: usize, lines: Vec<HlLine>) {
        if let Some((t, v)) = self.display_key
            && t == tab
            && v > version
        {
            return;
        }
        self.display_key = Some((tab, version));
        self.display_base = base;
        self.display_hl = lines;
    }

    /// Submits a highlight job for the active buffer's current viewport (called
    /// before render). Never runs syntect itself — the worker does, off-thread, so
    /// the render loop stays responsive no matter how slow the syntax is.
    pub fn refresh_highlight(&mut self) {
        if self.hl_tx.is_none() {
            return; // no worker wired up (e.g. in tests)
        }
        let Some(i) = self.active_tab else {
            return;
        };
        let id = self.tabs[i].id;
        let ver = self.tabs[i].buffer.version;
        let sy = self.tabs[i].buffer.scroll_y;
        // Lines from the top down to the viewport bottom need color; overestimate
        // with the full terminal height so a partial editor pane is always covered.
        let needed = sy + self.term_size.1 as usize + 8;
        // Resubmit when the buffer changed, the tab changed, a reset was forced, or
        // the viewport scrolled to reveal lines above (`sy < s`) or below (`needed
        // > n`) the slice last shipped.
        let need_send = self.hl_reset
            || match self.hl_sent {
                Some((t, v, s, n)) => t != id || v != ver || sy < s || needed > n,
                None => true,
            };
        if !need_send {
            return;
        }
        let (dirty_from, wide) = self.tabs[i].buffer.take_dirty();
        let job = crate::app::hlworker::HlJob {
            tab: id,
            version: ver,
            path: self.tabs[i].buffer.path.clone(),
            theme_name: self.current_theme_name().to_string(),
            reset: self.hl_reset,
            // A `Rope` clone is O(1) (shared, copy-on-write): the O(n) flatten
            // to a `String` happens on the worker thread, not per keystroke here.
            text: self.tabs[i].buffer.rope.clone(),
            dirty_from,
            wide,
            scroll_y: sy,
            needed,
        };
        self.hl_sent = Some((id, ver, sy, needed));
        self.hl_reset = false;
        if let Some(tx) = &self.hl_tx {
            let _ = tx.send(job);
        }
    }

    /// Invalidates highlighting (content replaced externally, or theme changed):
    /// drops the shown colors so text falls back to plain until the worker — which
    /// is told to reset its cache — returns fresh ones.
    pub fn invalidate_highlight(&mut self) {
        self.search_marks_key = None;
        self.hl_reset = true;
        self.hl_sent = None;
        self.display_key = None;
        self.active_display_key = None;
        // External content replacement (reload / format) changes the git diff too.
        self.git_marks_dirty = true;
    }
}
