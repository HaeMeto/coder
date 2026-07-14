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
    /// Re-read a file that changed on disk (result -> `Msg::FileReloaded`).
    ReloadFile(PathBuf),
    WriteFile { path: PathBuf, contents: String },
    /// Load the HEAD content of a file for the change gutter.
    LoadHeadText(PathBuf),
    LoadGitStatus,
    GitStage(String),
    GitUnstage(String),
    GitStageAll,
    GitUnstageAll,
    GitRevert(String),
    GitCommit(String),
    GitFetch,
    GitPull,
    GitPush,
    RunSearch {
        query: String,
        use_regex: bool,
        match_case: bool,
        search_hidden: bool,
    },
    RunReplace {
        query: String,
        replace: String,
        use_regex: bool,
        match_case: bool,
        search_hidden: bool,
    },
    /// Replace within a single file (from the search panel's "Replace" button).
    RunReplaceFile {
        path: PathBuf,
        query: String,
        replace: String,
        use_regex: bool,
        match_case: bool,
    },
    SpawnPty { rows: u16, cols: u16 },
    SetClipboard(String),
    /// Persist user preferences (theme + settings) to the config file.
    SaveConfig(services::config::Config),
}

/// The first non-empty line of a message, for the one-line status bar.
fn first_line(s: &str) -> String {
    s.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string()
}

/// Loads the git status and sends `Msg::GitStatusLoaded`.
fn send_git_status(root: &std::path::Path, tx: &UnboundedSender<Msg>) {
    let status = services::git::load_status(root);
    let _ = tx.send(Msg::GitStatusLoaded {
        branch: status.branch,
        staged: status.staged,
        unstaged: status.unstaged,
        is_repo: status.is_repo,
        ahead: status.ahead,
        behind: status.behind,
        has_upstream: status.has_upstream,
        has_remote: status.has_remote,
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
        Cmd::ReloadFile(path) => {
            tokio::spawn(async move {
                if let Ok(text) = services::fs::read_file(&path).await {
                    let _ = tx.send(Msg::FileReloaded { path, text });
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
        Cmd::LoadHeadText(path) => {
            tokio::task::spawn_blocking(move || {
                let text = services::git::head_file(&path);
                let _ = tx.send(Msg::HeadTextLoaded { path, text });
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
        Cmd::GitFetch => {
            tokio::task::spawn_blocking(move || {
                match services::git::fetch(&root) {
                    Ok(_) => {
                        let _ = tx.send(Msg::Status("Fetched".to_string()));
                    }
                    Err(e) => {
                        let _ = tx.send(Msg::Error(format!("fetch failed: {}", first_line(&e))));
                    }
                }
                send_git_status(&root, &tx);
            });
        }
        Cmd::GitPull => {
            tokio::task::spawn_blocking(move || {
                match services::git::pull(&root) {
                    Ok(m) => {
                        let _ = tx.send(Msg::Status(format!("Pulled: {}", first_line(&m))));
                    }
                    Err(e) => {
                        let _ = tx.send(Msg::Error(format!("pull failed: {}", first_line(&e))));
                    }
                }
                send_git_status(&root, &tx);
            });
        }
        Cmd::GitPush => {
            tokio::task::spawn_blocking(move || {
                match services::git::push(&root) {
                    Ok(_) => {
                        let _ = tx.send(Msg::Status("Pushed".to_string()));
                    }
                    Err(e) => {
                        let _ = tx.send(Msg::Error(format!("push failed: {}", first_line(&e))));
                    }
                }
                send_git_status(&root, &tx);
            });
        }
        Cmd::RunSearch {
            query,
            use_regex,
            match_case,
            search_hidden,
        } => {
            tokio::task::spawn_blocking(move || {
                let matches =
                    services::search::search(&root, &query, use_regex, match_case, search_hidden, 500);
                let _ = tx.send(Msg::SearchResults { query, matches });
            });
        }
        Cmd::RunReplace {
            query,
            replace,
            use_regex,
            match_case,
            search_hidden,
        } => {
            tokio::task::spawn_blocking(move || {
                let (changed, count) = services::search::replace_all(
                    &root,
                    &query,
                    &replace,
                    use_regex,
                    match_case,
                    search_hidden,
                );
                let _ = tx.send(Msg::ReplaceDone { changed, count });
            });
        }
        Cmd::RunReplaceFile {
            path,
            query,
            replace,
            use_regex,
            match_case,
        } => {
            tokio::task::spawn_blocking(move || {
                let count =
                    services::search::replace_in_file(&path, &query, &replace, use_regex, match_case);
                let changed = if count > 0 { vec![path] } else { Vec::new() };
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
        Cmd::SaveConfig(config) => {
            tokio::task::spawn_blocking(move || {
                services::config::save(&config);
            });
        }
    }
}
