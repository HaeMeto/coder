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

/// Builds a case-insensitive Regex from `query`. If `use_regex` is false the
/// pattern is escaped (plain-text search).
fn build_regex(query: &str, use_regex: bool) -> Option<Regex> {
    let pattern = if use_regex {
        query.to_string()
    } else {
        regex::escape(query)
    };
    RegexBuilder::new(&pattern)
        .case_insensitive(true)
        .build()
        .ok()
}

/// Searches for `query` under `root` (plain/regex via `use_regex`).
/// The number of results is capped by `limit` (blocking; call inside spawn_blocking).
pub fn search(root: &Path, query: &str, use_regex: bool, limit: usize) -> Vec<SearchMatch> {
    let mut results = Vec::new();
    if query.is_empty() {
        return results;
    }
    let re = match build_regex(query, use_regex) {
        Some(re) => re,
        None => return results,
    };

    let walker = WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .build();

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
) -> (Vec<PathBuf>, usize) {
    let mut changed = Vec::new();
    let mut total = 0usize;
    if query.is_empty() {
        return (changed, total);
    }
    let re = match build_regex(query, use_regex) {
        Some(re) => re,
        None => return (changed, total),
    };

    let walker = WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .build();

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
