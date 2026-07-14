//! Repo status and stage/unstage/revert/commit operations via git2.

use std::path::{Path, PathBuf};

use git2::build::CheckoutBuilder;
use git2::{Repository, Status, StatusOptions};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitState {
    Added,
    Modified,
    Deleted,
    Untracked,
    Renamed,
    Conflicted,
}

impl GitState {
    pub fn short(&self) -> char {
        match self {
            GitState::Added => 'A',
            GitState::Modified => 'M',
            GitState::Deleted => 'D',
            GitState::Untracked => 'U',
            GitState::Renamed => 'R',
            GitState::Conflicted => 'C',
        }
    }
}

#[derive(Clone, Debug)]
pub struct GitEntry {
    pub path: PathBuf,
    pub rel: String,
    pub state: GitState,
}

#[derive(Default, Clone)]
pub struct GitStatus {
    pub branch: Option<String>,
    /// Changes added to the index (staged).
    pub staged: Vec<GitEntry>,
    /// Changes in the working tree (unstaged).
    pub unstaged: Vec<GitEntry>,
    pub is_repo: bool,
}

/// Collects the status of the git repository under `root` (blocking; call inside spawn_blocking).
pub fn load_status(root: &Path) -> GitStatus {
    let repo = match Repository::discover(root) {
        Ok(r) => r,
        Err(_) => return GitStatus::default(),
    };

    let branch = repo
        .head()
        .ok()
        .and_then(|h| h.shorthand().map(|s| s.to_string()));

    let workdir = repo
        .workdir()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| root.to_path_buf());

    let mut opts = StatusOptions::new();
    opts.include_untracked(true).recurse_untracked_dirs(true);

    let mut staged = Vec::new();
    let mut unstaged = Vec::new();
    if let Ok(statuses) = repo.statuses(Some(&mut opts)) {
        for entry in statuses.iter() {
            let Some(rel) = entry.path() else { continue };
            let s = entry.status();
            let path = workdir.join(rel);
            // The same file can be both staged and unstaged (partial stage).
            if let Some(state) = classify_index(s) {
                staged.push(GitEntry {
                    path: path.clone(),
                    rel: rel.to_string(),
                    state,
                });
            }
            if let Some(state) = classify_worktree(s) {
                unstaged.push(GitEntry {
                    path,
                    rel: rel.to_string(),
                    state,
                });
            }
        }
    }

    GitStatus {
        branch,
        staged,
        unstaged,
        is_repo: true,
    }
}

/// Status of the index (staged) side.
fn classify_index(s: Status) -> Option<GitState> {
    if s.is_index_new() {
        Some(GitState::Added)
    } else if s.is_index_deleted() {
        Some(GitState::Deleted)
    } else if s.is_index_renamed() {
        Some(GitState::Renamed)
    } else if s.is_index_modified() || s.is_index_typechange() {
        Some(GitState::Modified)
    } else {
        None
    }
}

/// Status of the working tree (unstaged) side.
fn classify_worktree(s: Status) -> Option<GitState> {
    if s.is_conflicted() {
        Some(GitState::Conflicted)
    } else if s.is_wt_new() {
        Some(GitState::Untracked)
    } else if s.is_wt_deleted() {
        Some(GitState::Deleted)
    } else if s.is_wt_renamed() {
        Some(GitState::Renamed)
    } else if s.is_wt_modified() || s.is_wt_typechange() {
        Some(GitState::Modified)
    } else {
        None
    }
}

/// Adds the file to the index (git add). If deleted, removes it from the index.
pub fn stage(root: &Path, rel: &str) -> Result<(), git2::Error> {
    let repo = Repository::discover(root)?;
    let workdir = repo.workdir().map(|p| p.to_path_buf());
    let mut index = repo.index()?;
    let rel_path = Path::new(rel);
    let exists = workdir.map(|w| w.join(rel).exists()).unwrap_or(false);
    if exists {
        index.add_path(rel_path)?;
    } else {
        index.remove_path(rel_path)?;
    }
    index.write()
}

/// Adds all changes to the index (git add -A).
pub fn stage_all(root: &Path) -> Result<(), git2::Error> {
    let repo = Repository::discover(root)?;
    let mut index = repo.index()?;
    // With the "*" pathspec, adds new/changed files and removes deleted ones from the index.
    index.add_all(["*"], git2::IndexAddOption::DEFAULT, None)?;
    index.write()
}

/// Removes all staged changes from the index (git reset).
pub fn unstage_all(root: &Path) -> Result<(), git2::Error> {
    let repo = Repository::discover(root)?;
    match repo.head().ok().and_then(|h| h.peel_to_commit().ok()) {
        Some(commit) => {
            // Collect the staged paths and reset them to HEAD.
            let mut opts = StatusOptions::new();
            opts.include_untracked(false);
            let paths: Vec<String> = repo
                .statuses(Some(&mut opts))?
                .iter()
                .filter(|e| classify_index(e.status()).is_some())
                .filter_map(|e| e.path().map(|p| p.to_string()))
                .collect();
            if !paths.is_empty() {
                repo.reset_default(Some(commit.as_object()), paths.iter())?;
            }
        }
        None => {
            // No HEAD: clear the index.
            let mut index = repo.index()?;
            index.remove_all(["*"], None)?;
            index.write()?;
        }
    }
    Ok(())
}

/// Removes the file from the index (git reset -- <file>).
pub fn unstage(root: &Path, rel: &str) -> Result<(), git2::Error> {
    let repo = Repository::discover(root)?;
    let rel_path = Path::new(rel);
    match repo.head().ok().and_then(|h| h.peel_to_commit().ok()) {
        Some(commit) => {
            repo.reset_default(Some(commit.as_object()), [rel_path])?;
        }
        None => {
            // No HEAD (no first commit yet): remove from the index.
            let mut index = repo.index()?;
            index.remove_path(rel_path)?;
            index.write()?;
        }
    }
    Ok(())
}

/// Reverts the change in the working tree (git checkout -- <file> / delete untracked).
pub fn revert(root: &Path, rel: &str) -> Result<(), git2::Error> {
    let repo = Repository::discover(root)?;
    let rel_path = Path::new(rel);
    let status = repo.status_file(rel_path)?;
    if status.is_wt_new() {
        // Untracked new file: delete it from disk.
        if let Some(workdir) = repo.workdir() {
            let _ = std::fs::remove_file(workdir.join(rel));
        }
        return Ok(());
    }
    // Tracked file: restore the working tree to the index (staged) state.
    let mut co = CheckoutBuilder::new();
    co.force().update_index(false).path(rel);
    repo.checkout_index(None, Some(&mut co))
}

/// Commits the changes in the index.
pub fn commit(root: &Path, message: &str) -> Result<(), git2::Error> {
    let repo = Repository::discover(root)?;
    let sig = repo.signature()?;
    let mut index = repo.index()?;
    let tree_oid = index.write_tree()?;
    let tree = repo.find_tree(tree_oid)?;
    let parents: Vec<git2::Commit> = repo
        .head()
        .ok()
        .and_then(|h| h.peel_to_commit().ok())
        .into_iter()
        .collect();
    let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parent_refs)?;
    Ok(())
}


