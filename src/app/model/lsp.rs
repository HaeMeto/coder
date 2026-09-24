//! Language-server state: running servers, completion popup, pending format
//! and diagnostics.

use crate::core::buffer::Cursor;

use super::Model;

/// Running language servers, keyed by language id.
#[derive(Default)]
pub struct LspState {
    /// Initialized-or-initializing servers we hold a handle for.
    pub sessions: std::collections::HashMap<String, crate::services::lsp::LspHandle>,
    /// Languages whose server spawn is in flight (prevents a double-spawn).
    pub starting: std::collections::HashSet<String>,
    /// Languages whose server finished the `initialize` handshake.
    pub initialized: std::collections::HashSet<String>,
}

/// The open completion popup: items from the server plus selection + the range
/// they replace. `requested_version`/`tab_id` discard a stale response.
pub struct CompletionState {
    pub items: Vec<crate::services::lsp::CompletionItem>,
    pub selected: usize,
    /// Start of the identifier prefix being completed (replaced on accept).
    pub anchor: Cursor,
    /// Id (`Tab::id`) of the tab the popup belongs to (guards accept against a
    /// tab switch).
    pub tab_id: usize,
}

/// An outstanding LSP format request: whether to write the file once its edits
/// apply (format-on-save). Staleness is guarded separately by the request token.
pub struct PendingFormat {
    pub save_after: bool,
    /// The tab being formatted (`Tab::id`): if the server dies or never answers,
    /// a pending format-on-save still writes this tab's text.
    pub tab_id: usize,
    /// Past this the server is presumed stuck and the save goes ahead unformatted.
    pub deadline: std::time::Instant,
}

/// A diagnostic in buffer char coordinates (converted from LSP on receipt).
#[derive(Clone)]
pub struct Diagnostic {
    pub line: usize,
    pub col_start: usize,
    pub col_end: usize,
    pub severity: crate::services::lsp::Severity,
    pub message: String,
}

impl Model {
    /// (errors, warnings) in the active buffer, from its stored diagnostics.
    pub fn active_diagnostic_counts(&self) -> (usize, usize) {
        use crate::services::lsp::Severity;
        self.active_buffer()
            .and_then(|b| b.path.as_ref())
            .and_then(|p| self.diagnostics.get(p))
            .map(|diags| {
                let e = diags
                    .iter()
                    .filter(|d| d.severity == Severity::Error)
                    .count();
                let w = diags
                    .iter()
                    .filter(|d| d.severity == Severity::Warning)
                    .count();
                (e, w)
            })
            .unwrap_or((0, 0))
    }

    /// The diagnostic under the active buffer's cursor, most severe first — used
    /// by the status bar to surface the message (a "hover" without LSP hover).
    pub fn diagnostic_at_cursor(&self) -> Option<&Diagnostic> {
        let buf = self.active_buffer()?;
        let path = buf.path.as_ref()?;
        let (line, col) = (buf.cursor.line, buf.cursor.col);
        self.diagnostics
            .get(path)?
            .iter()
            .filter(|d| d.line == line && col >= d.col_start && col <= d.col_end)
            .min_by_key(|d| match d.severity {
                crate::services::lsp::Severity::Error => 0u8,
                crate::services::lsp::Severity::Warning => 1,
                crate::services::lsp::Severity::Info => 2,
                crate::services::lsp::Severity::Hint => 3,
            })
    }
}
