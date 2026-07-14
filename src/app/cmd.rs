//! Side effects (the `Cmd` of the Elm Architecture) and the async executor.

use std::path::PathBuf;

use tokio::sync::mpsc::UnboundedSender;

use crate::app::msg::Msg;
use crate::services;

/// A side-effect description returned by `update`. The executor runs these on
/// tokio and sends the result back as a `Msg`.
pub enum Cmd {
    ScanDir(PathBuf),
    ReadFile(PathBuf),
    WriteFile { path: PathBuf, contents: String },
    LoadGitStatus,
    GitStage(String),
    GitUnstage(String),
    GitStageAll,
    GitUnstageAll,
    GitRevert(String),
    GitCommit(String),
    RunSearch {
        query: String,
        use_regex: bool,
    },
    RunReplace {
        query: String,
        replace: String,
        use_regex: bool,
    },
    SpawnPty { rows: u16, cols: u16 },
    SetClipboard(String),
}

/// Loads the git status and sends `Msg::GitStatusLoaded`.
fn send_git_status(root: &std::path::Path, tx: &UnboundedSender<Msg>) {
    let status = services::git::load_status(root);
    let _ = tx.send(Msg::GitStatusLoaded {
        branch: status.branch,
        staged: status.staged,
        unstaged: status.unstaged,
        is_repo: status.is_repo,
    });
}

/// Runs a Cmd; results come back as Msg over `tx`.
pub fn execute(cmd: Cmd, root: PathBuf, tx: UnboundedSender<Msg>) {
    match cmd {
        Cmd::ScanDir(path) => {
            tokio::task::spawn_blocking(move || {
                match services::fs::scan_dir(&path) {
                    Ok(entries) => {
                        let _ = tx.send(Msg::DirScanned { path, entries });
                    }
                    Err(e) => {
                        let _ = tx.send(Msg::Error(format!("could not scan directory: {e}")));
                    }
                }
            });
        }
        Cmd::ReadFile(path) => {
            tokio::spawn(async move {
                match services::fs::read_file(&path).await {
                    Ok(text) => {
                        let _ = tx.send(Msg::FileLoaded { path, text });
                    }
                    Err(e) => {
                        let _ = tx.send(Msg::Error(format!("could not open file: {e}")));
                    }
                }
            });
        }
        Cmd::WriteFile { path, contents } => {
            tokio::spawn(async move {
                match services::fs::write_file(&path, &contents).await {
                    Ok(()) => {
                        let _ = tx.send(Msg::FileSaved { path });
                    }
                    Err(e) => {
                        let _ = tx.send(Msg::Error(format!("could not save: {e}")));
                    }
                }
            });
        }
        Cmd::LoadGitStatus => {
            tokio::task::spawn_blocking(move || {
                send_git_status(&root, &tx);
            });
        }
        Cmd::GitStage(rel) => {
            tokio::task::spawn_blocking(move || {
                if let Err(e) = services::git::stage(&root, &rel) {
                    let _ = tx.send(Msg::Error(format!("stage failed: {e}")));
                }
                send_git_status(&root, &tx);
            });
        }
        Cmd::GitUnstage(rel) => {
            tokio::task::spawn_blocking(move || {
                if let Err(e) = services::git::unstage(&root, &rel) {
                    let _ = tx.send(Msg::Error(format!("unstage failed: {e}")));
                }
                send_git_status(&root, &tx);
            });
        }
        Cmd::GitStageAll => {
            tokio::task::spawn_blocking(move || {
                if let Err(e) = services::git::stage_all(&root) {
                    let _ = tx.send(Msg::Error(format!("stage all failed: {e}")));
                }
                send_git_status(&root, &tx);
            });
        }
        Cmd::GitUnstageAll => {
            tokio::task::spawn_blocking(move || {
                if let Err(e) = services::git::unstage_all(&root) {
                    let _ = tx.send(Msg::Error(format!("unstage all failed: {e}")));
                }
                send_git_status(&root, &tx);
            });
        }
        Cmd::GitRevert(rel) => {
            tokio::task::spawn_blocking(move || {
                if let Err(e) = services::git::revert(&root, &rel) {
                    let _ = tx.send(Msg::Error(format!("revert failed: {e}")));
                }
                send_git_status(&root, &tx);
            });
        }
        Cmd::GitCommit(message) => {
            tokio::task::spawn_blocking(move || {
                match services::git::commit(&root, &message) {
                    Ok(()) => {
                        let _ = tx.send(Msg::Status("Committed".to_string()));
                    }
                    Err(e) => {
                        let _ = tx.send(Msg::Error(format!("commit failed: {e}")));
                    }
                }
                send_git_status(&root, &tx);
            });
        }
        Cmd::RunSearch { query, use_regex } => {
            tokio::task::spawn_blocking(move || {
                let matches = services::search::search(&root, &query, use_regex, 500);
                let _ = tx.send(Msg::SearchResults { query, matches });
            });
        }
        Cmd::RunReplace {
            query,
            replace,
            use_regex,
        } => {
            tokio::task::spawn_blocking(move || {
                let (changed, count) =
                    services::search::replace_all(&root, &query, &replace, use_regex);
                let _ = tx.send(Msg::ReplaceDone { changed, count });
            });
        }
        Cmd::SpawnPty { rows, cols } => {
            tokio::task::spawn_blocking(move || {
                match services::pty::spawn(root, rows, cols, tx.clone()) {
                    Ok(session) => {
                        let _ = tx.send(Msg::PtyReady(session));
                    }
                    Err(e) => {
                        let _ = tx.send(Msg::Error(format!("could not start terminal: {e}")));
                    }
                }
            });
        }
        Cmd::SetClipboard(text) => {
            tokio::task::spawn_blocking(move || {
                if let Ok(mut cb) = arboard::Clipboard::new() {
                    let _ = cb.set_text(text);
                }
            });
        }
    }
}
