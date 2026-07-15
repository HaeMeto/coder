//! VSCode-style keyboard mapping: (KeyEvent, Focus) -> Action.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::model::{Focus, Panel};

/// High-level action to be applied by `update`.
pub enum Action {
    Quit,
    ToggleSidebar,
    ToggleTerminal,
    SelectPanel(Panel),
    Save,
    CloseTab,
    NextTab,
    PrevTab,

    // Editor
    Insert(char),
    Newline,
    InsertTab,
    Backspace,
    Delete,
    Move(Motion, bool), // (direction, extend selection)
    SelectAll,
    Copy,
    Cut,
    Paste,
    Undo,
    Redo,

    // Sidebar navigation
    NavUp,
    NavDown,
    Activate,

    // Search input (text editing is handled by the focused input widget itself)
    SearchSubmit,
    SearchToggleField,
    SearchToggleRegex,

    // In-editor find / replace widget (text editing handled by the input widget)
    OpenFind,
    OpenFindReplace,
    FindNext,
    FindPrev,
    FindToggleField,

    // Terminal raw input
    PtyInput(Vec<u8>),

    Escape,
}

#[derive(Clone, Copy)]
pub enum Motion {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    WordLeft,
    WordRight,
}

pub fn resolve(key: KeyEvent, focus: Focus) -> Option<Action> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    // ----- Global shortcuts (in any focus) -----
    if ctrl {
        match key.code {
            KeyCode::Char('q') => return Some(Action::Quit),
            KeyCode::Char('b') => return Some(Action::ToggleSidebar),
            KeyCode::Char('j') => return Some(Action::ToggleTerminal),
            KeyCode::Char('w') => return Some(Action::CloseTab),
            KeyCode::Char('s') => return Some(Action::Save),
            KeyCode::Char('e') if shift => return Some(Action::SelectPanel(Panel::Files)),
            KeyCode::Char('f') if shift => return Some(Action::SelectPanel(Panel::Search)),
            KeyCode::Char('g') if shift => return Some(Action::SelectPanel(Panel::Git)),
            KeyCode::Char('x') if shift => return Some(Action::SelectPanel(Panel::Extensions)),
            // Ctrl+F: in-editor find. (Ctrl+Shift+F above is the workspace search panel.)
            KeyCode::Char('f') => return Some(Action::OpenFind),
            KeyCode::Char('h') => return Some(Action::OpenFindReplace),
            KeyCode::Char(',') => return Some(Action::SelectPanel(Panel::Settings)),
            KeyCode::Tab => {
                return Some(if shift {
                    Action::PrevTab
                } else {
                    Action::NextTab
                });
            }
            _ => {}
        }
    }

    match focus {
        Focus::Terminal => resolve_terminal(key, ctrl),
        Focus::Editor => resolve_editor(key, ctrl, shift),
        Focus::Sidebar => resolve_sidebar(key),
        Focus::SearchInput => resolve_search(key),
        Focus::GitCommit => resolve_git_commit(key),
        Focus::Find => resolve_find(key, shift),
    }
}

fn resolve_find(key: KeyEvent, shift: bool) -> Option<Action> {
    // Editing/motion keys are consumed by the focused input widget upstream; only
    // find-specific keys reach here.
    match key.code {
        KeyCode::Tab => Some(Action::FindToggleField), // query <-> replace
        KeyCode::Enter => Some(if shift { Action::FindPrev } else { Action::FindNext }),
        // Up/Down step through matches (the inputs are single-line).
        KeyCode::Down => Some(Action::FindNext),
        KeyCode::Up => Some(Action::FindPrev),
        KeyCode::Esc => Some(Action::Escape),
        _ => None,
    }
}

fn resolve_git_commit(key: KeyEvent) -> Option<Action> {
    // The commit box is multi-line: the input widget consumes typing, motion, and
    // Enter (newline). Committing is button-only; Esc blurs back to the sidebar.
    match key.code {
        KeyCode::Esc => Some(Action::Escape),
        _ => None,
    }
}

fn resolve_editor(key: KeyEvent, ctrl: bool, shift: bool) -> Option<Action> {
    if ctrl {
        return match key.code {
            KeyCode::Char('c') => Some(Action::Copy),
            KeyCode::Char('x') => Some(Action::Cut),
            KeyCode::Char('v') => Some(Action::Paste),
            KeyCode::Char('z') => Some(Action::Undo),
            KeyCode::Char('y') => Some(Action::Redo),
            KeyCode::Char('a') => Some(Action::SelectAll),
            // Ctrl+Left/Right: jump by word (Shift extends the selection).
            KeyCode::Left => Some(Action::Move(Motion::WordLeft, shift)),
            KeyCode::Right => Some(Action::Move(Motion::WordRight, shift)),
            _ => None,
        };
    }
    match key.code {
        KeyCode::Char(c) => Some(Action::Insert(c)),
        KeyCode::Enter => Some(Action::Newline),
        KeyCode::Tab => Some(Action::InsertTab),
        KeyCode::Backspace => Some(Action::Backspace),
        KeyCode::Delete => Some(Action::Delete),
        KeyCode::Left => Some(Action::Move(Motion::Left, shift)),
        KeyCode::Right => Some(Action::Move(Motion::Right, shift)),
        KeyCode::Up => Some(Action::Move(Motion::Up, shift)),
        KeyCode::Down => Some(Action::Move(Motion::Down, shift)),
        KeyCode::Home => Some(Action::Move(Motion::Home, shift)),
        KeyCode::End => Some(Action::Move(Motion::End, shift)),
        KeyCode::PageUp => Some(Action::Move(Motion::PageUp, shift)),
        KeyCode::PageDown => Some(Action::Move(Motion::PageDown, shift)),
        KeyCode::Esc => Some(Action::Escape),
        _ => None,
    }
}

fn resolve_sidebar(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Up => Some(Action::NavUp),
        KeyCode::Down => Some(Action::NavDown),
        KeyCode::Enter | KeyCode::Right => Some(Action::Activate),
        KeyCode::Esc => Some(Action::Escape),
        _ => None,
    }
}

fn resolve_search(key: KeyEvent) -> Option<Action> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    if ctrl {
        // Ctrl+R: toggle regex.
        return match key.code {
            KeyCode::Char('r') => Some(Action::SearchToggleRegex),
            _ => None,
        };
    }
    // Typing/motion is consumed by the focused input widget upstream.
    match key.code {
        KeyCode::Tab => Some(Action::SearchToggleField), // query <-> replace
        KeyCode::Enter => Some(Action::SearchSubmit),
        KeyCode::Up => Some(Action::NavUp),
        KeyCode::Down => Some(Action::NavDown),
        KeyCode::Esc => Some(Action::Escape),
        _ => None,
    }
}

/// When the terminal is focused, converts keys into raw bytes to send to the PTY.
fn resolve_terminal(key: KeyEvent, ctrl: bool) -> Option<Action> {
    let bytes: Vec<u8> = match key.code {
        KeyCode::Char(c) => {
            if ctrl {
                // Ctrl+letter -> control character (0x01..0x1a)
                let up = c.to_ascii_uppercase();
                if up.is_ascii_alphabetic() {
                    vec![(up as u8) - 0x40]
                } else {
                    let mut b = [0u8; 4];
                    c.encode_utf8(&mut b).as_bytes().to_vec()
                }
            } else {
                let mut b = [0u8; 4];
                c.encode_utf8(&mut b).as_bytes().to_vec()
            }
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        _ => return None,
    };
    Some(Action::PtyInput(bytes))
}
