//! Application state (the `Model` of the Elm Architecture).

use std::path::PathBuf;

use crate::core::buffer::Buffer;
use crate::core::filetree::FileTree;
use crate::core::highlight::{self, HlLine, Highlighter};
use crate::core::theme::Theme;
use crate::services::git::{GitEntry, GutterKind};
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
    /// Editor preferences (format on save, etc.) — right below Themes.
    Settings,
}

impl Panel {
    pub const ALL: [Panel; 6] = [
        Panel::Files,
        Panel::Search,
        Panel::Git,
        Panel::Extensions,
        Panel::Themes,
        Panel::Settings,
    ];

    pub fn icon(&self) -> &'static str {
        match self {
            Panel::Files => "\u{f4a5}", // file
            Panel::Search => "\u{f002}",
            Panel::Git => "\u{f1d3}",
            Panel::Extensions => "\u{f12e}",
            Panel::Themes => "\u{f1fc}", // palette
            Panel::Settings => "\u{f013}", // gear
        }
    }

    pub fn ascii_icon(&self) -> &'static str {
        match self {
            Panel::Files => "Fil",
            Panel::Search => "Src",
            Panel::Git => "Git",
            Panel::Extensions => "Ext",
            Panel::Themes => "Thm",
            Panel::Settings => "Set",
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            Panel::Files => "EXPLORER",
            Panel::Search => "SEARCH",
            Panel::Git => "SOURCE CONTROL",
            Panel::Extensions => "EXTENSIONS",
            Panel::Themes => "THEMES",
            Panel::Settings => "SETTINGS",
        }
    }
}

/// Editor preferences shown in the Settings panel. Boolean toggles applied at save time.
pub struct SettingsState {
    /// Keyboard/mouse selection index into the settings list.
    pub selected: usize,
    /// Master switch: run the enabled format actions when saving.
    pub format_on_save: bool,
    /// Strip trailing spaces/tabs from each line on save (when format_on_save).
    pub trim_trailing_whitespace: bool,
    /// Ensure the file ends with a single newline on save (when format_on_save).
    pub insert_final_newline: bool,
}

impl SettingsState {
    /// Number of toggleable settings.
    pub const COUNT: usize = 3;

    pub fn label(idx: usize) -> &'static str {
        match idx {
            0 => "Format on save",
            1 => "Trim trailing whitespace",
            2 => "Insert final newline",
            _ => "",
        }
    }

    pub fn value(&self, idx: usize) -> bool {
        match idx {
            0 => self.format_on_save,
            1 => self.trim_trailing_whitespace,
            2 => self.insert_final_newline,
            _ => false,
        }
    }

    pub fn toggle(&mut self, idx: usize) {
        match idx {
            0 => self.format_on_save = !self.format_on_save,
            1 => self.trim_trailing_whitespace = !self.trim_trailing_whitespace,
            2 => self.insert_final_newline = !self.insert_final_newline,
            _ => {}
        }
    }
}

impl Default for SettingsState {
    fn default() -> Self {
        SettingsState {
            selected: 0,
            format_on_save: false,
            trim_trailing_whitespace: true,
            insert_final_newline: true,
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
    /// The in-editor find/replace widget.
    Find,
}

/// Which input field of the in-editor find widget is active.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindField {
    #[default]
    Query,
    Replace,
}

/// In-editor find / find-and-replace widget (floats over the top-right of the editor).
#[derive(Default)]
pub struct FindState {
    pub open: bool,
    /// Whether the replace row (input + Replace/Replace All buttons) is shown.
    pub replace_mode: bool,
    pub query: String,
    pub replace: String,
    pub field: FindField,
    /// Match ranges in the active buffer, as absolute [start, end) character indices.
    pub matches: Vec<(usize, usize)>,
    /// Index of the current match within `matches`.
    pub current: Option<usize>,
}

impl FindState {
    /// "cur/total" indicator (1-based); "0/0" when there are no matches.
    pub fn count_label(&self) -> String {
        let total = self.matches.len();
        let cur = self.current.map(|i| i + 1).unwrap_or(0);
        format!("{cur}/{total}")
    }
}

/// Mouse drag target (panel resizing / text selection).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragTarget {
    SidebarBorder,
    TerminalBorder,
    EditorSelect,
    /// Dragging the editor scrollbar thumb.
    Scrollbar,
}

pub struct Tab {
    pub buffer: Buffer,
    pub highlighter: Highlighter,
    /// The file's content at git HEAD, for the change gutter. Loaded async.
    pub head_text: Option<String>,
    /// Opened from the Git panel as a diff: changed lines get a colored background.
    pub diff_mode: bool,
}

impl Tab {
    pub fn new(buffer: Buffer) -> Self {
        let highlighter = Highlighter::for_path(buffer.path.as_deref());
        Tab {
            buffer,
            highlighter,
            head_text: None,
            diff_mode: false,
        }
    }

    /// Tab bar label: file name, with a "(diff)" suffix for diff-mode tabs.
    pub fn title(&self) -> String {
        let name = self.buffer.display_name();
        if self.diff_mode {
            format!("{name} (diff)")
        } else {
            name
        }
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
    /// Case-sensitive matching when true (default: case-insensitive).
    pub match_case: bool,
    /// Search .gitignore'd and hidden (dot) files when true (default: skip them).
    pub search_hidden: bool,
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
    /// Commits the local branch is ahead of its upstream.
    pub ahead: usize,
    /// Commits the local branch is behind its upstream.
    pub behind: usize,
    /// Whether the current branch has a configured upstream.
    pub has_upstream: bool,
    /// Whether the repository has at least one remote configured.
    pub has_remote: bool,
}

impl GitStatus {
    /// Total number of keyboard-navigable items (staged + unstaged).
    pub fn nav_len(&self) -> usize {
        self.staged.len() + self.unstaged.len()
    }

    /// Whether there is anything to push: a remote must exist and the branch is
    /// either ahead of its upstream or not yet published (no upstream).
    pub fn can_push(&self) -> bool {
        self.is_repo
            && self.branch.is_some()
            && self.has_remote
            && (self.ahead > 0 || !self.has_upstream)
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
    pub settings: SettingsState,
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
    /// A file whose next load should become a diff-mode tab (opened from the Git panel).
    pub pending_diff: Option<PathBuf>,
    /// The open modal dialog (captures all input when present).
    pub dialog: Option<Dialog>,
    /// Change-gutter markers for the active buffer, keyed by line index.
    pub active_git_marks: std::collections::HashMap<usize, GutterKind>,
    /// The (tab index, buffer version) that active_git_marks belongs to.
    active_git_marks_key: Option<(usize, u64)>,
    /// In-editor find / replace widget state.
    pub find: FindState,
    /// Last left-click (time, column, row) for editor double-click detection.
    pub last_click: Option<(std::time::Instant, u16, u16)>,
}

impl Model {
    pub fn new(root: PathBuf) -> Self {
        let config = crate::services::config::load();
        let mut model = Model::with_defaults(root);
        model.apply_config(&config);
        model
    }

    fn with_defaults(root: PathBuf) -> Self {
        Model {
            sidebar: Sidebar {
                active: Panel::Files,
                files: FileTree::new(root.clone()),
                git: GitStatus::default(),
                search: SearchState::default(),
                themes: ThemesState::default(),
                settings: SettingsState::default(),
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
            pending_diff: None,
            dialog: None,
            active_git_marks: std::collections::HashMap::new(),
            active_git_marks_key: None,
            find: FindState::default(),
            last_click: None,
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
        self.active_git_marks_key = None;
    }

    /// Recomputes the change-gutter markers if the active buffer changed (called before render).
    pub fn refresh_git_marks(&mut self) {
        match self.active_tab {
            Some(i) => {
                let ver = self.tabs[i].buffer.version;
                if self.active_git_marks_key != Some((i, ver)) {
                    self.active_git_marks.clear();
                    if let Some(head) = self.tabs[i].head_text.clone() {
                        let new = self.tabs[i].buffer.full_text();
                        for (ln, kind) in crate::services::git::gutter_marks(&head, &new) {
                            self.active_git_marks.insert(ln, kind);
                        }
                    }
                    self.active_git_marks_key = Some((i, ver));
                }
            }
            None => {
                self.active_git_marks.clear();
                self.active_git_marks_key = None;
            }
        }
    }

    /// Whether the editor should reserve a change-gutter column (active file is tracked).
    pub fn git_gutter(&self) -> bool {
        self.active_tab
            .map(|i| self.tabs[i].head_text.is_some())
            .unwrap_or(false)
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

    /// Applies persisted preferences: selected theme + editor settings.
    pub fn apply_config(&mut self, config: &crate::services::config::Config) {
        if let Some(idx) = self
            .sidebar
            .themes
            .names
            .iter()
            .position(|n| n == &config.theme)
        {
            self.apply_theme(idx);
        }
        let s = &mut self.sidebar.settings;
        s.format_on_save = config.format_on_save;
        s.trim_trailing_whitespace = config.trim_trailing_whitespace;
        s.insert_final_newline = config.insert_final_newline;
    }

    /// Snapshot of the current preferences, for persisting to disk.
    pub fn config_snapshot(&self) -> crate::services::config::Config {
        let s = &self.sidebar.settings;
        crate::services::config::Config {
            theme: self.current_theme_name().to_string(),
            format_on_save: s.format_on_save,
            trim_trailing_whitespace: s.trim_trailing_whitespace,
            insert_final_newline: s.insert_final_newline,
        }
    }

    /// Name of the currently selected theme.
    pub fn current_theme_name(&self) -> &str {
        let t = &self.sidebar.themes;
        t.names
            .get(t.selected)
            .map(String::as_str)
            .unwrap_or(highlight::DEFAULT_THEME)
    }

    pub fn active_buffer(&self) -> Option<&Buffer> {
        self.active_tab.map(|i| &self.tabs[i].buffer)
    }

    pub fn active_buffer_mut(&mut self) -> Option<&mut Buffer> {
        let i = self.active_tab?;
        Some(&mut self.tabs[i].buffer)
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
        self.active_tab.map(|i| self.tabs[i].diff_mode).unwrap_or(false)
    }

    pub fn panel_icon(&self, panel: Panel) -> &'static str {
        if self.ascii_icons {
            panel.ascii_icon()
        } else {
            panel.icon()
        }
    }
}
