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

/// A previous commit shown in the HISTORY section of the Git panel.
#[derive(Clone, Debug)]
pub struct GitCommit {
    /// Abbreviated commit hash (7 chars).
    pub hash: String,
    /// First line of the commit message.
    pub summary: String,
}

#[derive(Default, Clone)]
pub struct GitStatus {
    pub branch: Option<String>,
    /// Changes added to the index (staged).
    pub staged: Vec<GitEntry>,
    /// Changes in the working tree (unstaged).
    pub unstaged: Vec<GitEntry>,
    pub is_repo: bool,
    /// Commits the local branch is ahead of its upstream.
    pub ahead: usize,
    /// Commits the local branch is behind its upstream.
    pub behind: usize,
    /// Whether the current branch has a configured upstream.
    pub has_upstream: bool,
    /// Whether the repository has at least one remote configured.
    pub has_remote: bool,
    /// Recent commits (newest first) shown under the HISTORY heading.
    pub history: Vec<GitCommit>,
}

/// Number of recent commits loaded for the HISTORY section.
const HISTORY_LIMIT: usize = 30;

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

    let (ahead, behind, has_upstream) = ahead_behind(&repo);
    let has_remote = repo.remotes().map(|r| !r.is_empty()).unwrap_or(false);
    let history = load_history(&repo);

    GitStatus {
        branch,
        staged,
        unstaged,
        is_repo: true,
        ahead,
        behind,
        has_upstream,
        has_remote,
        history,
    }
}

/// Walks back from HEAD collecting up to `HISTORY_LIMIT` recent commits (newest first).
fn load_history(repo: &Repository) -> Vec<GitCommit> {
    let mut walk = match repo.revwalk() {
        Ok(w) => w,
        Err(_) => return Vec::new(),
    };
    if walk.push_head().is_err() {
        return Vec::new(); // no HEAD yet (empty repo)
    }
    let mut out = Vec::new();
    for oid in walk.flatten().take(HISTORY_LIMIT) {
        let Ok(commit) = repo.find_commit(oid) else {
            continue;
        };
        let hash = oid.to_string().chars().take(7).collect();
        let summary = commit.summary().unwrap_or("").to_string();
        out.push(GitCommit { hash, summary });
    }
    out
}

/// Ahead/behind commit counts vs the upstream, and whether an upstream is set.
/// Local only (no network); the counts reflect the last fetch.
fn ahead_behind(repo: &Repository) -> (usize, usize, bool) {
    let head = match repo.head() {
        Ok(h) if h.is_branch() => h,
        _ => return (0, 0, false),
    };
    let Some(local_oid) = head.target() else {
        return (0, 0, false);
    };
    let branch = git2::Branch::wrap(head);
    let Ok(upstream) = branch.upstream() else {
        return (0, 0, false);
    };
    let Some(up_oid) = upstream.get().target() else {
        return (0, 0, true);
    };
    match repo.graph_ahead_behind(local_oid, up_oid) {
        Ok((a, b)) => (a, b, true),
        Err(_) => (0, 0, true),
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

/// Runs a `git` CLI subcommand in `root`, returning the combined output on success
/// or the error text on failure. Uses the CLI so it inherits the user's auth
/// (credential helpers, ssh-agent) exactly like their shell.
fn run_git(root: &Path, args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .map_err(|e| format!("could not run git: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let msg = format!("{stdout}{stderr}").trim().to_string();
    if out.status.success() {
        Ok(msg)
    } else if msg.is_empty() {
        Err("git command failed".to_string())
    } else {
        Err(msg)
    }
}

/// `git fetch` — download remote refs (updates ahead/behind on the next status).
pub fn fetch(root: &Path) -> Result<String, String> {
    run_git(root, &["fetch"])
}

/// `git pull --ff-only` — fast-forward the current branch to its upstream.
pub fn pull(root: &Path) -> Result<String, String> {
    run_git(root, &["pull", "--ff-only"])
}

/// `git push` — publish local commits. Falls back to `-u origin HEAD` when the
/// branch has no upstream yet.
pub fn push(root: &Path) -> Result<String, String> {
    match run_git(root, &["push"]) {
        Err(e) if e.contains("upstream") || e.contains("set-upstream") => {
            run_git(root, &["push", "-u", "origin", "HEAD"])
        }
        other => other,
    }
}

/// Per-line marker in the editor gutter (working tree vs HEAD).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GutterKind {
    /// Added or modified line (green bar).
    Added,
    /// A deletion boundary sits right at this line (red wedge).
    Deleted,
}

/// The content of `path` at HEAD, or `None` if there is no repo / the file is not tracked.
pub fn head_file(path: &Path) -> Option<String> {
    let repo = Repository::discover(path.parent()?).ok()?;
    let workdir = repo.workdir()?.to_path_buf();
    let rel = path.strip_prefix(&workdir).ok()?;
    let tree = repo.head().ok()?.peel_to_tree().ok()?;
    let entry = tree.get_path(rel).ok()?;
    let blob = entry.to_object(&repo).ok()?.peel_to_blob().ok()?;
    Some(String::from_utf8_lossy(blob.content()).into_owned())
}

/// Per-line gutter markers for the difference between `old` (HEAD) and `new` (buffer).
/// Pure (no IO): computes the diff in memory, safe to call from `update`.
pub fn gutter_marks(old: &str, new: &str) -> Vec<(usize, GutterKind)> {
    let mut opts = git2::DiffOptions::new();
    opts.context_lines(0);
    let patch = match git2::Patch::from_buffers(
        old.as_bytes(),
        None,
        new.as_bytes(),
        None,
        Some(&mut opts),
    ) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    let mut marks: std::collections::HashMap<usize, GutterKind> = std::collections::HashMap::new();
    for h in 0..patch.num_hunks() {
        let Ok((hunk, _)) = patch.hunk(h) else { continue };
        let new_start = hunk.new_start() as usize;
        let new_lines = hunk.new_lines() as usize;
        if new_lines > 0 {
            // Added / modified lines -> green. new_start is 1-based.
            for i in 0..new_lines {
                marks.insert(new_start.saturating_sub(1) + i, GutterKind::Added);
            }
        } else if hunk.old_lines() > 0 {
            // Pure deletion -> red wedge on the line before the removed block.
            let anchor = new_start.saturating_sub(1);
            marks.entry(anchor).or_insert(GutterKind::Deleted);
        }
    }
    marks.into_iter().collect()
}

/// Removed line blocks for an inline diff view. Each entry is `(anchor, lines)`:
/// the removed `lines` render right after buffer line `anchor` (`None` = before
/// the first line). Includes the old side of modifications so replaced code is
/// shown alongside the new lines. Pure (no IO).
pub fn deleted_blocks(old: &str, new: &str) -> Vec<(Option<usize>, Vec<String>)> {
    let mut opts = git2::DiffOptions::new();
    opts.context_lines(0);
    let patch = match git2::Patch::from_buffers(
        old.as_bytes(),
        None,
        new.as_bytes(),
        None,
        Some(&mut opts),
    ) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    let old_lines: Vec<&str> = old.lines().collect();
    let mut out = Vec::new();
    for h in 0..patch.num_hunks() {
        let Ok((hunk, _)) = patch.hunk(h) else { continue };
        let ol = hunk.old_lines() as usize;
        if ol == 0 {
            continue; // pure addition — nothing removed
        }
        let os = hunk.old_start() as usize; // 1-based
        let removed: Vec<String> = (0..ol)
            .filter_map(|i| old_lines.get(os - 1 + i).map(|s| s.to_string()))
            .collect();
        if removed.is_empty() {
            continue;
        }
        let nl = hunk.new_lines() as usize;
        let ns = hunk.new_start() as usize; // 1-based
        let anchor = if nl > 0 {
            // Render right before the first new (added/modified) line.
            let first_new0 = ns.saturating_sub(1); // 0-based first new line
            if first_new0 == 0 { None } else { Some(first_new0 - 1) }
        } else {
            // Pure deletion: right after the line the removal follows.
            if ns == 0 { None } else { Some(ns - 1) }
        };
        out.push((anchor, removed));
    }
    out
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

/// Undoes the last commit (`git reset --soft HEAD~1`): moves HEAD to the parent
/// while leaving the index and worktree untouched, so the commit's changes stay
/// staged. Returns the message of the undone commit so the UI can repopulate the
/// commit box. Fails on the initial (parentless) commit.
pub fn undo_last_commit(root: &Path) -> Result<String, git2::Error> {
    let repo = Repository::discover(root)?;
    let head = repo.head()?.peel_to_commit()?;
    let message = head.message().unwrap_or("").to_string();
    let parent = head.parent(0)?; // errors on the root commit (no parent)
    repo.reset(parent.as_object(), git2::ResetType::Soft, None)?;
    Ok(message)
}



#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(old: &str, new: &str) -> Vec<(usize, GutterKind)> {
        let mut v = gutter_marks(old, new);
        v.sort_by_key(|(l, _)| *l);
        v
    }

    #[test]
    fn added_lines_are_green() {
        // Insert a new line after line 1.
        let m = kinds("a\nb\n", "a\nx\nb\n");
        assert_eq!(m, vec![(1, GutterKind::Added)]);
    }

    #[test]
    fn modified_line_is_green() {
        let m = kinds("a\nb\nc\n", "a\nB\nc\n");
        assert_eq!(m, vec![(1, GutterKind::Added)]);
    }

    #[test]
    fn pure_deletion_marks_boundary_red() {
        // Delete line 2 ("b"); wedge anchors on the line before it (index 0).
        let m = kinds("a\nb\nc\n", "a\nc\n");
        assert_eq!(m, vec![(0, GutterKind::Deleted)]);
    }

    #[test]
    fn no_changes_no_marks() {
        assert!(kinds("a\nb\n", "a\nb\n").is_empty());
    }

    #[test]
    fn deleted_block_after_anchor_line() {
        // Remove "b" -> shown right after line 0 ("a").
        let d = deleted_blocks("a\nb\nc\n", "a\nc\n");
        assert_eq!(d, vec![(Some(0), vec!["b".to_string()])]);
    }

    #[test]
    fn modification_shows_old_line() {
        // "b" -> "B": the old "b" is a removed row before the new "B".
        let d = deleted_blocks("a\nb\nc\n", "a\nB\nc\n");
        assert_eq!(d, vec![(Some(0), vec!["b".to_string()])]);
    }

    #[test]
    fn deletion_at_top_anchors_before_first_line() {
        let d = deleted_blocks("a\nb\n", "b\n");
        assert_eq!(d, vec![(None, vec!["a".to_string()])]);
    }

    #[test]
    fn pure_addition_has_no_deleted_blocks() {
        assert!(deleted_blocks("a\nb\n", "a\nx\nb\n").is_empty());
    }
}
