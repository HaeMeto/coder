//! Modal dialog input handling.

use super::*;

/// Handles keyboard input while a dialog is open.
pub(super) fn dialog_key(model: &mut Model, key: KeyEvent) -> Vec<Cmd> {
    let Some(d) = model.dialog.as_mut() else {
        return Vec::new();
    };
    match d.kind {
        DialogKind::Info => match key.code {
            KeyCode::Enter | KeyCode::Esc => dialog_confirm(model),
            _ => Vec::new(),
        },
        DialogKind::Ask => match key.code {
            KeyCode::Left | KeyCode::Right | KeyCode::Tab => {
                d.selected ^= 1;
                Vec::new()
            }
            KeyCode::Char('e') | KeyCode::Char('y') => dialog_confirm(model),
            KeyCode::Char('h') | KeyCode::Char('n') | KeyCode::Esc => dialog_cancel(model),
            KeyCode::Enter => {
                if d.selected == 0 {
                    dialog_confirm(model)
                } else {
                    dialog_cancel(model)
                }
            }
            _ => Vec::new(),
        },
        DialogKind::Input => match key.code {
            KeyCode::Char(c) => {
                d.input.push(c);
                Vec::new()
            }
            KeyCode::Backspace => {
                d.input.pop();
                Vec::new()
            }
            KeyCode::Enter => dialog_confirm(model),
            KeyCode::Esc => dialog_cancel(model),
            _ => Vec::new(),
        },
    }
}

/// Handles mouse clicks while a dialog is open (buttons).
pub(super) fn dialog_mouse(model: &mut Model, m: MouseEvent) -> Vec<Cmd> {
    if let MouseEventKind::Down(MouseButton::Left) = m.kind {
        let term = full_rect(model);
        if let Some(d) = model.dialog.as_ref() {
            match ui::dialog::hit(d, term, m.column, m.row) {
                Some(true) => return dialog_confirm(model),
                Some(false) => return dialog_cancel(model),
                None => {}
            }
        }
    }
    Vec::new()
}

/// Confirms the dialog: runs the action and closes it.
fn dialog_confirm(model: &mut Model) -> Vec<Cmd> {
    let Some(d) = model.dialog.take() else {
        return Vec::new();
    };
    match d.action {
        DialogAction::GitRevert(rel) => vec![Cmd::GitRevert(rel)],
        DialogAction::None => Vec::new(),
    }
}

/// Cancels the dialog (closes it without running the action).
fn dialog_cancel(model: &mut Model) -> Vec<Cmd> {
    model.dialog = None;
    Vec::new()
}
