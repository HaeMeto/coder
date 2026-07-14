//! Application state (the `Model` of the Elm Architecture).

use std::path::PathBuf;

use crate::core::buffer::Buffer;
use crate::core::filetree::FileTree;
use crate::core::highlight::{self, HlLine, Highlighter};
use crate::core::theme::Theme;
use crate::services::git::GitEntry;
use crate::services::pty::PtySession;
use crate::services::search::SearchMatch;

/// Left activity bar panels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    Files,
    Search,
    Git,
    Extensions,
    /// Theme picker — right below Extensions.
    Themes,
}

impl Panel {
    pub const ALL: [Panel; 5] = [
        Panel::Files,
        Panel::Search,
        Panel::Git,
        Panel::Extensions,
        Panel::Themes,
    ];

    pub fn icon(&self) -> &'static str {
        match self {
            Panel::Files => "\u{f4a5}", // file
            Panel::Search => "\u{f002}",
            Panel::Git => "\u{f1d3}",
            Panel::Extensions => "\u{f12e}",
            Panel::Themes => "\u{f1fc}", // palette
        }
    }

    pub fn ascii_icon(&self) -> &'static str {
        match self {
            Panel::Files => "Fil",
            Panel::Search => "Src",
            Panel::Git => "Git",
            Panel::Extensions => "Ext",
            Panel::Themes => "Thm",
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            Panel::Files => "EXPLORER",
            Panel::Search => "SEARCH",
            Panel::Git => "SOURCE CONTROL",
            Panel::Extensions => "EXTENSIONS",
            Panel::Themes => "THEMES",
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

/// Focus — the target that keyboard events are routed to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Editor,
    Terminal,
    Sidebar,
    SearchInput,
    /// The commit message input in the Git panel.
    GitCommit,
}

/// Mouse drag target (panel resizing / text selection).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragTarget {
    SidebarBorder,
    TerminalBorder,
    EditorSelect,
}

pub struct Tab {
    pub buffer: Buffer,
    pub highlighter: Highlighter,
}

impl Tab {
    pub fn new(buffer: Buffer) -> Self {
        let highlighter = Highlighter::for_path(buffer.path.as_deref());
        Tab { buffer, highlighter }
    }
}

/// The active input field in the search panel.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchField {
    #[default]
    Query,
    Replace,
}

#[derive(Default)]
pub struct SearchState {
    pub query: String,
    pub replace: String,
    /// Should the query be interpreted as a regex?
    pub use_regex: bool,
    /// Which field keyboard input goes to.
    pub field: SearchField,
    pub results: Vec<SearchMatch>,
    pub selected: usize,
}


#[derive(Default)]
pub struct GitStatus {
    pub branch: Option<String>,
    pub staged: Vec<GitEntry>,
    pub unstaged: Vec<GitEntry>,
    pub is_repo: bool,
    /// Keyboard selection: index into the combined [staged..., unstaged...] list.
    pub selected: usize,
    /// Text in the commit message box.
    pub commit_msg: String,
}

impl GitStatus {
    /// Total number of keyboard-navigable items (staged + unstaged).
    pub fn nav_len(&self) -> usize {
        self.staged.len() + self.unstaged.len()
    }

    /// Returns the item at the combined index and whether it is staged.
    pub fn entry_at(&self, idx: usize) -> Option<(&GitEntry, bool)> {
        if idx < self.staged.len() {
            self.staged.get(idx).map(|e| (e, true))
        } else {
            self.unstaged.get(idx - self.staged.len()).map(|e| (e, false))
        }
    }
}

pub struct Sidebar {
    pub active: Panel,
    pub files: FileTree,
    pub git: GitStatus,
    pub search: SearchState,
    pub themes: ThemesState,
}

/// General-purpose modal dialog kind.
// The Info/Input kinds are not used yet; the general API is for future use.
#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DialogKind {
    /// Confirmation dialog (Yes / No).
    Ask,
    /// Information dialog (OK only).
    Info,
    /// Text input (OK / Cancel).
    Input,
}

/// Action to perform when the dialog is confirmed.
#[derive(Clone)]
pub enum DialogAction {
    /// No action (e.g. an information dialog).
    #[allow(dead_code)]
    None,
    /// Revert the working-tree change for the given path.
    GitRevert(String),
}

/// Modal dialog opened in the center of the screen. Captures all input while open.
pub struct Dialog {
    pub kind: DialogKind,
    pub title: String,
    pub message: String,
    /// Text entered for the `Input` kind.
    pub input: String,
    /// Button selection: 0 = confirm, 1 = cancel (Ask/Input).
    pub selected: usize,
    pub action: DialogAction,
}

impl Dialog {
    pub fn ask(title: String, message: String, action: DialogAction) -> Self {
        Dialog {
            kind: DialogKind::Ask,
            title,
            message,
            input: String::new(),
            selected: 0,
            action,
        }
    }

    /// General dialog constructor for future use.
    #[allow(dead_code)]
    pub fn info(title: String, message: String) -> Self {
        Dialog {
            kind: DialogKind::Info,
            title,
            message,
            input: String::new(),
            selected: 0,
            action: DialogAction::None,
        }
    }

    /// General dialog constructor for future use.
    #[allow(dead_code)]
    pub fn input(title: String, message: String, initial: String, action: DialogAction) -> Self {
        Dialog {
            kind: DialogKind::Input,
            title,
            message,
            input: initial,
            selected: 0,
            action,
        }
    }
}

pub struct TerminalState {
    pub parser: vt100::Parser,
    pub session: Option<PtySession>,
    pub rows: u16,
    pub cols: u16,
    /// The PTY spawn Cmd was sent but the session is not ready yet.
    pub spawn_requested: bool,
}

impl TerminalState {
    fn new() -> Self {
        TerminalState {
            parser: vt100::Parser::new(24, 80, 2000),
            session: None,
            rows: 24,
            cols: 80,
            spawn_requested: false,
        }
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        if rows == 0 || cols == 0 {
            return;
        }
        if rows != self.rows || cols != self.cols {
            self.rows = rows;
            self.cols = cols;
            self.parser.set_size(rows, cols);
            if let Some(s) = self.session.as_ref() {
                s.resize(rows, cols);
            }
        }
    }
}

pub struct LayoutState {
    pub sidebar_width: u16,
    pub terminal_height: u16,
    pub sidebar_open: bool,
    pub terminal_open: bool,
}

impl Default for LayoutState {
    fn default() -> Self {
        LayoutState {
            sidebar_width: 30,
            terminal_height: 12,
            sidebar_open: true,
            terminal_open: false,
        }
    }
}

pub struct Model {
    pub root: PathBuf,
    pub tabs: Vec<Tab>,
    pub active_tab: Option<usize>,
    pub sidebar: Sidebar,
    pub terminal: TerminalState,
    pub layout: LayoutState,
    pub focus: Focus,
    pub should_quit: bool,
    pub status_message: String,
    pub internal_clipboard: String,
    pub theme: Theme,
    /// Last known terminal size — for mouse hit-testing and layout.
    pub term_size: (u16, u16),
    pub drag: Option<DragTarget>,
    /// Use ASCII instead of Nerd Font icons (for compatibility).
    pub ascii_icons: bool,
    /// Render-ready highlight lines for the active buffer.
    pub active_hl: Vec<HlLine>,
    /// The (tab index, buffer version) that active_hl belongs to.
    active_hl_key: Option<(usize, u64)>,
    /// Line to jump to after the file is loaded (opening from a search result).
    pub pending_goto: Option<(PathBuf, usize)>,
    /// The open modal dialog (captures all input when present).
    pub dialog: Option<Dialog>,
}

impl Model {
    pub fn new(root: PathBuf) -> Self {
        Model {
            sidebar: Sidebar {
                active: Panel::Files,
                files: FileTree::new(root.clone()),
                git: GitStatus::default(),
                search: SearchState::default(),
                themes: ThemesState::default(),
            },
            tabs: Vec::new(),
            active_tab: None,
            terminal: TerminalState::new(),
            layout: LayoutState::default(),
            focus: Focus::Sidebar,
            should_quit: false,
            status_message: String::from("Coder — Ctrl+Q quit · Ctrl+J terminal · Ctrl+B sidebar"),
            internal_clipboard: String::new(),
            // Keep the UI palette and the syntax theme consistent at startup.
            theme: highlight::theme_for(highlight::DEFAULT_THEME),
            term_size: (80, 24),
            drag: None,
            ascii_icons: std::env::var("CODER_ASCII").is_ok(),
            active_hl: Vec::new(),
            active_hl_key: None,
            pending_goto: None,
            dialog: None,
            root,
        }
    }

    /// Refreshes the highlight cache if the active buffer changed (called before render).
    pub fn refresh_highlight(&mut self) {
        if let Some(i) = self.active_tab {
            let ver = self.tabs[i].buffer.version;
            if self.active_hl_key != Some((i, ver)) {
                let text = self.tabs[i].buffer.full_text();
                let hl = self.tabs[i].highlighter.highlight(&text, ver).to_vec();
                self.active_hl = hl;
                self.active_hl_key = Some((i, ver));
            }
        } else {
            self.active_hl.clear();
            self.active_hl_key = None;
        }
    }

    /// Invalidates the highlight cache (when the buffer changes externally).
    pub fn invalidate_highlight(&mut self) {
        self.active_hl_key = None;
    }

    /// Applies the theme at the given index: UI palette + syntax theme for all tabs.
    pub fn apply_theme(&mut self, idx: usize) {
        let Some(name) = self.sidebar.themes.names.get(idx).cloned() else {
            return;
        };
        self.sidebar.themes.selected = idx;
        self.theme = highlight::theme_for(&name);
        for tab in &mut self.tabs {
            tab.highlighter.set_theme(&name);
        }
        // Forces a refresh of the active tab; the others are already cache-reset by
        // set_theme and get recomputed when they are selected.
        self.invalidate_highlight();
    }

    pub fn active_buffer(&self) -> Option<&Buffer> {
        self.active_tab.map(|i| &self.tabs[i].buffer)
    }

    pub fn active_buffer_mut(&mut self) -> Option<&mut Buffer> {
        let i = self.active_tab?;
        Some(&mut self.tabs[i].buffer)
    }

    /// Is there already an open tab for a given file?
    pub fn tab_index_for(&self, path: &std::path::Path) -> Option<usize> {
        self.tabs
            .iter()
            .position(|t| t.buffer.path.as_deref() == Some(path))
    }

    pub fn panel_icon(&self, panel: Panel) -> &'static str {
        if self.ascii_icons {
            panel.ascii_icon()
        } else {
            panel.icon()
        }
    }
}
