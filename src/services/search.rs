//! Workspace text search (ignore + regex).

use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use regex::{NoExpand, Regex, RegexBuilder};

#[derive(Clone, Debug)]
pub struct SearchMatch {
    pub path: PathBuf,
    pub rel: String,
    pub line_no: usize,
    pub line: String,
}

/// Builds a Regex from `query`. If `use_regex` is false the pattern is escaped
/// (plain-text search). Case-insensitive unless `match_case` is true.
fn build_regex(query: &str, use_regex: bool, match_case: bool) -> Option<Regex> {
    let pattern = if use_regex {
        query.to_string()
    } else {
        regex::escape(query)
    };
    RegexBuilder::new(&pattern)
        .case_insensitive(!match_case)
        .build()
        .ok()
}

/// Builds the file walker. By default skips hidden (dot) files and .gitignore'd
/// paths; when `search_hidden` is true it descends into both.
fn walker(root: &Path, search_hidden: bool) -> ignore::Walk {
    WalkBuilder::new(root)
        .hidden(!search_hidden)
        .git_ignore(!search_hidden)
        .git_exclude(!search_hidden)
        .ignore(!search_hidden)
        .build()
}

/// Searches for `query` under `root` (plain/regex via `use_regex`).
/// The number of results is capped by `limit` (blocking; call inside spawn_blocking).
pub fn search(
    root: &Path,
    query: &str,
    use_regex: bool,
    match_case: bool,
    search_hidden: bool,
    limit: usize,
) -> Vec<SearchMatch> {
    let mut results = Vec::new();
    if query.is_empty() {
        return results;
    }
    let re = match build_regex(query, use_regex, match_case) {
        Some(re) => re,
        None => return results,
    };

    let walker = walker(root, search_hidden);

    for entry in walker.flatten() {
        if results.len() >= limit {
            break;
        }
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue, // skip binary files
        };
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned();
        for (i, line) in content.lines().enumerate() {
            if results.len() >= limit {
                break;
            }
            if re.is_match(line) {
                results.push(SearchMatch {
                    path: path.to_path_buf(),
                    rel: rel.clone(),
                    line_no: i + 1,
                    line: line.chars().take(200).collect(),
                });
            }
        }
    }
    results
}

/// Replaces `query` matches with `replace` in a single file. Returns the number
/// of replacements (0 if the file was unchanged). Blocking; call in spawn_blocking.
pub fn replace_in_file(
    path: &Path,
    query: &str,
    replace: &str,
    use_regex: bool,
    match_case: bool,
) -> usize {
    if query.is_empty() {
        return 0;
    }
    let re = match build_regex(query, use_regex, match_case) {
        Some(re) => re,
        None => return 0,
    };
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return 0,
    };
    let count = re.find_iter(&content).count();
    if count == 0 {
        return 0;
    }
    let new = if use_regex {
        re.replace_all(&content, replace)
    } else {
        re.replace_all(&content, NoExpand(replace))
    };
    if new != content && std::fs::write(path, new.as_bytes()).is_ok() {
        count
    } else {
        0
    }
}

/// Replaces `query` matches with `replace` in all files under `root`.
/// Returns the paths of the changed files and the total number of replacements
/// (blocking; call inside spawn_blocking).
///
/// If `use_regex` is true, group references like `$1` in `replace` are expanded;
/// if false, it is inserted as literal (unchanged) text.
pub fn replace_all(
    root: &Path,
    query: &str,
    replace: &str,
    use_regex: bool,
    match_case: bool,
    search_hidden: bool,
) -> (Vec<PathBuf>, usize) {
    let mut changed = Vec::new();
    let mut total = 0usize;
    if query.is_empty() {
        return (changed, total);
    }
    let re = match build_regex(query, use_regex, match_case) {
        Some(re) => re,
        None => return (changed, total),
    };

    let walker = walker(root, search_hidden);

    for entry in walker.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue, // skip binary files
        };
        let count = re.find_iter(&content).count();
        if count == 0 {
            continue;
        }
        let new = if use_regex {
            re.replace_all(&content, replace)
        } else {
            re.replace_all(&content, NoExpand(replace))
        };
        if new != content && std::fs::write(path, new.as_bytes()).is_ok() {
            total += count;
            changed.push(path.to_path_buf());
        }
    }
    (changed, total)
}
