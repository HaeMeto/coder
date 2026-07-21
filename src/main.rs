//! coder — a VSCode-like editor running in the terminal (Rust + ratatui + Elm Architecture).

mod app;
mod cli;
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
    // The shell invoked us as a completion helper (`complete -C coder coder`):
    // reply with path candidates and exit before touching the terminal, so Tab
    // never launches the TUI and freezes the shell.
    if cli::is_completion_helper() {
        cli::completion_reply();
        return Ok(());
    }

    let args = <cli::Cli as clap::Parser>::parse();
    if let Some(cli::Command::Completions { shell }) = args.command {
        cli::print_completions(shell);
        return Ok(());
    }

    // The argument may be a directory (workspace root) or a single file.
    let arg = args.path;
    let (root, open_file) = match arg {
        Some(p) => {
            let p = p.canonicalize().unwrap_or(p);
            if p.is_file() {
                // Opened with a file: root is its directory, and we open the file.
                let parent = p
                    .parent()
                    .map(|d| d.to_path_buf())
                    .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| ".".into()));
                (parent, Some(p))
            } else {
                (p, None)
            }
        }
        None => (
            std::env::current_dir().unwrap_or_else(|_| ".".into()),
            None,
        ),
    };
    let root = root.canonicalize().unwrap_or(root);

    let mut terminal = setup_terminal()?;
    let result = run(&mut terminal, root, open_file).await;
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

async fn run(
    terminal: &mut Tui,
    root: std::path::PathBuf,
    open_file: Option<std::path::PathBuf>,
) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel::<Msg>();
    let mut model = Model::new(root.clone());

    // Initial size.
    if let Ok((w, h)) = crossterm::terminal::size() {
        model.term_size = (w, h);
    }

    // Watch the workspace for external file changes (best-effort; kept alive here).
    // Directories are watched non-recursively and lazily, one per scan, so opening a
    // large tree (e.g. $HOME from the app menu) never blocks startup.
    let mut watcher = create_watcher(tx.clone());

    // Initial side effects: scan the root directory + load git status.
    let mut cmds = vec![Cmd::ScanDir(root.clone()), Cmd::LoadGitStatus];
    // Opened with a file: keep the sidebar collapsed (the user opens it when needed)
    // and load the file straight into the editor.
    if let Some(file) = open_file {
        model.layout.sidebar_open = false;
        model.focus = crate::app::model::Focus::Editor;
        cmds.push(Cmd::ReadFile(file));
    }
    dispatch(cmds, &model, &tx);

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
                watch_scanned_dir(&mut watcher, &msg);
                let cmds = update(&mut model, msg);
                dispatch(cmds, &model, &tx);
                // Drain pending messages (e.g. heavy PTY output).
                while let Ok(msg) = rx.try_recv() {
                    watch_scanned_dir(&mut watcher, &msg);
                    let cmds = update(&mut model, msg);
                    dispatch(cmds, &model, &tx);
                }
            }
        }
    }
    Ok(())
}

/// Adds a non-recursive watch on a directory as soon as it is scanned, so changes
/// in the folders the user actually opened are picked up — without walking the
/// whole tree up front.
fn watch_scanned_dir(watcher: &mut Option<notify::RecommendedWatcher>, msg: &Msg) {
    use notify::{RecursiveMode, Watcher};
    if let (Some(w), Msg::DirScanned { path, .. }) = (watcher.as_mut(), msg) {
        let _ = w.watch(path, RecursiveMode::NonRecursive);
    }
}

/// Builds a filesystem watcher; each change becomes a `Msg::DiskChanged`. No paths
/// are watched yet — directories are added non-recursively as they are scanned (see
/// `watch_scanned_dir`), so startup never walks the whole tree. Returns the watcher
/// (must stay alive to keep watching); `None` if the platform watcher failed.
fn create_watcher(tx: UnboundedSender<Msg>) -> Option<notify::RecommendedWatcher> {
    use notify::EventKind;
    notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res
            && matches!(
                event.kind,
                EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
            )
        {
            for path in event.paths {
                let _ = tx.send(Msg::DiskChanged(path));
            }
        }
    })
    .ok()
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
