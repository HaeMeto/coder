//! coder — a VSCode-like editor running in the terminal (Rust + ratatui + Elm Architecture).

mod app;
mod core;
mod services;
mod ui;

use std::io::{self, Stdout};

use anyhow::Result;
use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use futures::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::sync::mpsc::{self, UnboundedSender};

use crate::app::cmd::{self, Cmd};
use crate::app::model::Model;
use crate::app::msg::Msg;
use crate::app::update::update;

type Tui = Terminal<CrosstermBackend<Stdout>>;

#[tokio::main]
async fn main() -> Result<()> {
    let root = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| ".".into()));
    let root = root.canonicalize().unwrap_or(root);

    let mut terminal = setup_terminal()?;
    let result = run(&mut terminal, root).await;
    restore_terminal(&mut terminal)?;
    result
}

fn setup_terminal() -> Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    // Restore the terminal on panic.
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        original_hook(info);
    }));

    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

fn restore_terminal(terminal: &mut Tui) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

async fn run(terminal: &mut Tui, root: std::path::PathBuf) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel::<Msg>();
    let mut model = Model::new(root.clone());

    // Initial size.
    if let Ok((w, h)) = crossterm::terminal::size() {
        model.term_size = (w, h);
    }

    // Initial side effects: scan the root directory + load git status.
    dispatch(
        vec![Cmd::ScanDir(root.clone()), Cmd::LoadGitStatus],
        &model,
        &tx,
    );

    let mut events = EventStream::new();

    loop {
        model.refresh_highlight();
        model.refresh_git_marks();
        terminal.draw(|f| ui::view(f, &model))?;
        if model.should_quit {
            break;
        }

        tokio::select! {
            maybe_event = events.next() => {
                match maybe_event {
                    Some(Ok(event)) => {
                        for msg in map_event(event) {
                            let cmds = update(&mut model, msg);
                            dispatch(cmds, &model, &tx);
                        }
                    }
                    Some(Err(_)) | None => break,
                }
            }
            maybe_msg = rx.recv() => {
                let Some(msg) = maybe_msg else { break };
                let cmds = update(&mut model, msg);
                dispatch(cmds, &model, &tx);
                // Drain pending messages (e.g. heavy PTY output).
                while let Ok(msg) = rx.try_recv() {
                    let cmds = update(&mut model, msg);
                    dispatch(cmds, &model, &tx);
                }
            }
        }
    }
    Ok(())
}

/// Converts a crossterm event into application messages.
fn map_event(event: Event) -> Vec<Msg> {
    match event {
        Event::Key(key) => {
            // Only handle press/repeat (ignore release events).
            if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                vec![Msg::Key(key)]
            } else {
                Vec::new()
            }
        }
        Event::Mouse(m) => vec![Msg::Mouse(m)],
        Event::Resize(w, h) => vec![Msg::Resize(w, h)],
        _ => Vec::new(),
    }
}

fn dispatch(cmds: Vec<Cmd>, model: &Model, tx: &UnboundedSender<Msg>) {
    for c in cmds {
        cmd::execute(c, model.root.clone(), tx.clone());
    }
}
