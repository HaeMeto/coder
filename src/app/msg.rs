//! Application messages (the `Msg` of the Elm Architecture).

use std::path::PathBuf;

use crossterm::event::{KeyEvent, MouseEvent};

use crate::services::git::GitEntry;
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
    FileSaved {
        path: PathBuf,
    },
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

    // Terminal
    PtyReady(PtySession),
    PtyOutput(Vec<u8>),
    PtyExited,

    /// Neutral status bar message (without the "Error:" prefix).
    Status(String),
    Error(String),
}
