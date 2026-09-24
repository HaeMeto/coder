//! Overlays drawn on top of the editor: modal dialogs, context menus, the
//! quickbar (command palette) and toasts.

use std::path::PathBuf;

use crate::core::text_input::TextInputState;

use super::Panel;

/// General-purpose modal dialog kind.
// The Info kind is not used yet; the general API is for future use.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DialogKind {
    /// Confirmation dialog (Yes / No).
    Ask,
    /// Information dialog (OK only).
    #[allow(dead_code)]
    Info,
    /// Text input (OK / Cancel).
    Input,
    /// Three-way confirmation (Save / Don't Save / Cancel) — the quit prompt
    /// when there are unsaved changes. `selected`: 0 = Save, 1 = Don't Save,
    /// 2 = Cancel.
    AskSave,
}

/// Action to perform when the dialog is confirmed.
#[derive(Clone)]
pub enum DialogAction {
    /// No action (e.g. an information dialog).
    #[allow(dead_code)]
    None,
    /// Revert the working-tree change for the given path.
    GitRevert(String),
    /// Create a new file with the entered name inside the given directory.
    NewFile(PathBuf),
    /// Create a new directory with the entered name inside the given directory.
    NewFolder(PathBuf),
    /// Rename the given path to the entered name (kept in the same directory).
    Rename(PathBuf),
    /// Delete the given path (recursively for a directory).
    Delete(PathBuf),
    /// Close the tab with the given id (`Tab::id`; user confirmed the dirty-tab dialog).
    CloseTab(usize, String),
    /// Overwrite `keybindings.toml` with the built-in defaults.
    ResetKeybindings,
    /// Overwrite `config.toml` with the seeded defaults.
    ResetConfig,
    /// Switch the workspace root to the folder typed in the dialog (VSCode
    /// "open folder").
    OpenWorkspace,
    /// The quit confirmation when unsaved changes exist (`DialogKind::AskSave`):
    /// branches on `Dialog.selected` rather than carrying its own payload.
    QuitPrompt,
    /// Save As for a pathless (untitled) buffer: the tab id (`Tab::id`), and the path
    /// typed in the dialog becomes its file.
    SaveAs(usize),
}

/// Modal dialog opened in the center of the screen. Captures all input while open.
pub struct Dialog {
    pub kind: DialogKind,
    pub title: String,
    pub message: String,
    /// Text entered for the `Input` kind (with caret, editable).
    pub input: TextInputState,
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
            input: TextInputState::default(),
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
            input: TextInputState::default(),
            selected: 0,
            action: DialogAction::None,
        }
    }

    /// Save / Don't Save / Cancel — the quit prompt when tabs are dirty.
    pub fn ask_save(title: String, message: String, action: DialogAction) -> Self {
        Dialog {
            kind: DialogKind::AskSave,
            title,
            message,
            input: TextInputState::default(),
            selected: 0,
            action,
        }
    }

    pub fn input(title: String, message: String, initial: String, action: DialogAction) -> Self {
        let mut input = TextInputState::default();
        input.set_content(initial);
        Dialog {
            kind: DialogKind::Input,
            title,
            message,
            input,
            selected: 0,
            action,
        }
    }
}

/// An entry of a right-click context menu (file tree or tab bar).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuItem {
    NewFile,
    NewFolder,
    Rename,
    Delete,
    CloseOthers,
    CloseRight,
    CloseLeft,
    CloseAll,
}

impl MenuItem {
    /// The items shown for a tree row, in order.
    pub const FILE_TREE: [MenuItem; 4] = [
        MenuItem::NewFile,
        MenuItem::NewFolder,
        MenuItem::Rename,
        MenuItem::Delete,
    ];

    /// The items shown for a tab, in order.
    pub const TAB: [MenuItem; 4] = [
        MenuItem::CloseOthers,
        MenuItem::CloseRight,
        MenuItem::CloseLeft,
        MenuItem::CloseAll,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            MenuItem::NewFile => "New File",
            MenuItem::NewFolder => "New Folder",
            MenuItem::Rename => "Rename",
            MenuItem::Delete => "Delete",
            MenuItem::CloseOthers => "Close Others",
            MenuItem::CloseRight => "Close Right",
            MenuItem::CloseLeft => "Close Left",
            MenuItem::CloseAll => "Close All",
        }
    }

    /// The keyboard shortcut for the same action (also listed in the status bar).
    pub fn shortcut(&self) -> &'static str {
        match self {
            MenuItem::NewFile => "Ctrl+N",
            MenuItem::NewFolder => "Ctrl+Shift+N",
            MenuItem::Rename => "F2",
            MenuItem::Delete => "Del",
            MenuItem::CloseOthers
            | MenuItem::CloseRight
            | MenuItem::CloseLeft
            | MenuItem::CloseAll => "",
        }
    }
}

/// What a context menu was opened on; decides its items.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuKind {
    FileTree,
    Tab,
}

/// Context menu opened by right-clicking a file-tree row or a tab. Captures
/// all input while open, like `Dialog`.
pub struct ContextMenu {
    pub kind: MenuKind,
    /// The visible tree row (`FileTree`) or tab id (`Tab`, see `Tab::id`) the
    /// menu was opened on.
    pub row: usize,
    pub selected: usize,
    /// Top-left corner requested by the click; clamped to the screen on render.
    pub x: u16,
    pub y: u16,
}

impl ContextMenu {
    pub fn new(row: usize, x: u16, y: u16) -> Self {
        ContextMenu {
            kind: MenuKind::FileTree,
            row,
            selected: 0,
            x,
            y,
        }
    }

    /// The menu for the tab with id `tab` (`Tab::id`).
    pub fn tab(tab: usize, x: u16, y: u16) -> Self {
        ContextMenu {
            kind: MenuKind::Tab,
            ..ContextMenu::new(tab, x, y)
        }
    }

    pub fn items(&self) -> &'static [MenuItem] {
        match self.kind {
            MenuKind::FileTree => &MenuItem::FILE_TREE,
            MenuKind::Tab => &MenuItem::TAB,
        }
    }
}

/// A selectable entry in the quickbar (command palette), opened with Ctrl+P.
/// Entries mix workspace files, workspace directories, and built-in commands.
#[derive(Clone)]
pub enum QuickbarItem {
    /// Open this file in the editor. `rel` is the workspace-relative path used
    /// for display and prefix filtering (what the user sees and types against).
    File { path: PathBuf, rel: String },
    /// Open this folder as a new workspace root (VSCode "open folder"), switching
    /// the file explorer, git panel and search to that directory.
    OpenFolder,
    /// Create a new file inside the workspace root (opens the name dialog).
    NewFile,
    /// Create a new folder inside the workspace root (opens the name dialog).
    NewFolder,
    /// Open the given sidebar panel.
    Panel(Panel),
}

impl QuickbarItem {
    /// A one-char marker rendered before each entry so its kind is clear at a
    /// glance: file, directory, new-entry, or command.
    pub fn marker(&self) -> char {
        match self {
            QuickbarItem::File { .. } => 'F',
            QuickbarItem::OpenFolder => '>',
            QuickbarItem::NewFile => '+',
            QuickbarItem::NewFolder => '+',
            QuickbarItem::Panel(_) => '>',
        }
    }

    /// The label shown for the entry. For files/dirs this is the workspace-
    /// relative path; for commands a human title.
    pub fn label(&self) -> String {
        match self {
            QuickbarItem::File { rel, .. } => rel.clone(),
            QuickbarItem::OpenFolder => "Open Folder...".into(),
            QuickbarItem::NewFile => "New File".into(),
            QuickbarItem::NewFolder => "New Folder".into(),
            QuickbarItem::Panel(p) => format!("Open panel: {}", p.title()),
        }
    }

    /// The query text that selects this entry (what the filter matches).
    pub fn filter_text(&self) -> String {
        self.label().to_lowercase()
    }
}

/// A workspace file listed in the quickbar.
pub struct QuickbarFile {
    pub path: PathBuf,
    /// Workspace-relative label.
    pub rel: String,
    /// `rel` lowercased: what the query is matched against.
    pub key: String,
}

/// The quickbar overlay: an input query up top and a filtered list below.
/// Sort/filter happens in `update` (never here); this is pure state.
pub struct QuickbarState {
    /// The query being typed; filtered against entry `filter_text`.
    pub input: TextInputState,
    /// Every workspace file (from `Msg::FilesListed`), used to build `items`,
    /// with its label and lowercase match key computed once on arrival rather
    /// than per keystroke.
    pub files: Vec<QuickbarFile>,
    /// Whether the async workspace file listing has been delivered.
    pub files_loaded: bool,
    /// Candidate entries (workspace files plus commands), freshly filtered to
    /// the current query. This is what is rendered and traversed by ↑/↓/Enter.
    pub items: Vec<QuickbarItem>,
    /// Index of the highlighted row within `items`.
    pub selected: usize,
    /// Index of the first visible row (the list scrolls when `items` outgrows
    /// the popup).
    pub scroll: usize,
}

impl QuickbarState {
    pub fn new() -> Self {
        QuickbarState {
            input: TextInputState::default(),
            files: Vec::new(),
            files_loaded: false,
            items: Vec::new(),
            selected: 0,
            scroll: 0,
        }
    }

    /// Adjusts `scroll` so `selected` stays inside a `rows`-tall viewport.
    pub fn ensure_visible(&mut self, rows: usize) {
        let rows = rows.max(1);
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + rows {
            self.scroll = self.selected + 1 - rows;
        }
        self.scroll = self.scroll.min(self.items.len().saturating_sub(rows));
    }
}

/// How long a toast stays on screen.
pub const TOAST_DURATION: std::time::Duration = std::time::Duration::from_millis(2500);

/// A transient bottom-center notification (e.g. "Copied to clipboard").
pub struct Toast {
    pub message: String,
    /// When the toast was raised; it is shown while `elapsed < TOAST_DURATION`.
    pub shown_at: std::time::Instant,
}

impl Toast {
    pub fn is_expired(&self) -> bool {
        self.shown_at.elapsed() >= TOAST_DURATION
    }
}
