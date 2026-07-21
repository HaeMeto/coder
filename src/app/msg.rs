//! Application messages (the `Msg` of the Elm Architecture).

use std::path::PathBuf;

use crossterm::event::{KeyEvent, MouseEvent};

use crate::services::git::GitEntry;
use crate::services::lsp::{CompletionItem, LspHandle, RawDiagnostic, RawTextEdit, Token};
use crate::services::pty::PtySession;
use crate::services::search::SearchMatch;

pub enum Msg {
    // Input events
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),

    // Async results
    DirScanned {
        path: PathBuf,
        entries: Vec<(PathBuf, bool)>,
    },
    FileLoaded {
        path: PathBuf,
        text: String,
    },
    /// A file that could not be opened (binary / unreadable): opens a read-only
    /// tab showing the error message instead of surfacing it only on the statusbar.
    FileLoadFailed {
        path: PathBuf,
        error: String,
    },
    FileSaved {
        path: PathBuf,
    },
    /// A file/directory was renamed on disk (open tabs under it move too).
    PathRenamed {
        from: PathBuf,
        to: PathBuf,
    },
    /// A file/directory was deleted on disk (its open tabs close).
    PathDeleted(PathBuf),
    GitStatusLoaded {
        branch: Option<String>,
        staged: Vec<GitEntry>,
        unstaged: Vec<GitEntry>,
        is_repo: bool,
        ahead: usize,
        behind: usize,
        has_upstream: bool,
        has_remote: bool,
    },
    SearchResults {
        query: String,
        matches: Vec<SearchMatch>,
    },
    ReplaceDone {
        changed: Vec<PathBuf>,
        count: usize,
    },
    /// The last commit was undone (soft reset); carries its message to refill the box.
    GitCommitUndone {
        message: String,
    },
    /// The HEAD content of a file (for the change gutter).
    HeadTextLoaded {
        path: PathBuf,
        text: Option<String>,
    },
    /// A file changed on disk (from the filesystem watcher).
    DiskChanged(PathBuf),
    /// Fresh on-disk content for an externally-changed, unmodified open file.
    FileReloaded {
        path: PathBuf,
        text: String,
    },

    // LSP
    /// A language server was spawned; carries its intent-sender handle.
    LspSessionReady {
        language: String,
        handle: LspHandle,
    },
    /// The server finished its `initialize` handshake.
    LspInitialized {
        language: String,
    },
    /// Diagnostics for a file (raw LSP UTF-16 positions; converted in update).
    LspDiagnostics {
        path: PathBuf,
        diagnostics: Vec<RawDiagnostic>,
    },
    /// Completion results for an earlier request (guarded by `token`).
    LspCompletions {
        token: Token,
        items: Vec<CompletionItem>,
    },
    /// Formatting edits for an earlier request (guarded by `token`).
    LspFormatEdits {
        token: Token,
        edits: Vec<RawTextEdit>,
    },
    /// The server process exited.
    LspExited {
        language: String,
    },
    /// The server could not be started or errored fatally.
    LspError {
        language: String,
        message: String,
    },
    /// A debounced didChange fired; send it only if the buffer version matches.
    DidChangeDue {
        path: PathBuf,
        version: u64,
    },
    /// A standalone formatter produced new text for a file.
    FormatterOutput {
        path: PathBuf,
        text: String,
        token: Token,
        save_after: bool,
    },
    /// A standalone linter produced diagnostics as `(line0, col0, message)`.
    LinterDiagnostics {
        path: PathBuf,
        items: Vec<(usize, usize, String)>,
    },

    // Terminal
    PtyReady(PtySession),
    PtyOutput(Vec<u8>),
    PtyExited,

    /// Neutral status bar message (without the "Error:" prefix).
    Status(String),
    Error(String),
    /// The toast duration elapsed: clear the toast if it is actually expired.
    ToastExpired,
}
