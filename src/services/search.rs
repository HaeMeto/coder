//! Workspace text search (ignore + regex).

use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use regex::{NoExpand, Regex, RegexBuilder};

/// Files larger than this are skipped by search, replace and the quickbar's
/// file list, so a stray multi-GB log or dump is never read into memory.
pub const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct SearchMatch {
    pub path: PathBuf,
    pub rel: String,
    pub line_no: usize,
    pub line: String,
    /// Byte ranges of the query's matches within `line`, for highlighting.
    pub ranges: Vec<(usize, usize)>,
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

/// Every match of the search query in `text`, as [start, end) **char**
/// indices (the editor's rope units), using the same regex rules as the
/// workspace search. Pure; used to highlight the query in the open editor.
pub fn match_ranges(
    text: &str,
    query: &str,
    use_regex: bool,
    match_case: bool,
) -> Vec<(usize, usize)> {
    if query.is_empty() {
        return Vec::new();
    }
    let Some(re) = build_regex(query, use_regex, match_case) else {
        return Vec::new();
    };
    // Walk byte offsets once, converting to char offsets incrementally.
    let mut out = Vec::new();
    let (mut byte, mut chars) = (0usize, 0usize);
    let mut to_char = |b: usize| {
        chars += text[byte..b].chars().count();
        byte = b;
        chars
    };
    for m in re.find_iter(text) {
        if m.start() == m.end() {
            continue; // an empty regex match highlights nothing
        }
        let s = to_char(m.start());
        let e = to_char(m.end());
        out.push((s, e));
    }
    out
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

/// Whether a walked entry is a regular file worth reading: not a symlink (the
/// walker doesn't follow links, so a linked file could point outside the
/// workspace — and Replace All would then write there), and no larger than
/// [`MAX_FILE_BYTES`].
fn searchable_file(entry: &ignore::DirEntry) -> bool {
    entry.file_type().is_some_and(|t| t.is_file())
        && entry.metadata().is_ok_and(|m| m.len() <= MAX_FILE_BYTES)
}

/// Splits a line segment (as yielded by `split_inclusive('\n')`) into its body
/// and terminator (`"\r\n"`, `"\n"` or `""` for a final unterminated line), so
/// patterns see exactly the line text `str::lines` gives the search preview.
fn split_terminator(seg: &str) -> (&str, &str) {
    match seg.strip_suffix('\n') {
        Some(t) => match t.strip_suffix('\r') {
            Some(t) => (t, "\r\n"),
            None => (t, "\n"),
        },
        None => (seg, ""),
    }
}

/// Applies the replacement line by line — the same per-line view the search
/// preview matches against, so `^`/`$`, `\s+` and CRLF endings behave exactly
/// as the preview showed — preserving every line's original terminator.
/// `only_line` (0-based) restricts it to a single line. Returns the new text
/// and the number of replacements.
fn replace_lines(
    content: &str,
    re: &Regex,
    replace: &str,
    use_regex: bool,
    only_line: Option<usize>,
) -> (String, usize) {
    let mut out = String::with_capacity(content.len());
    let mut count = 0usize;
    for (i, seg) in content.split_inclusive('\n').enumerate() {
        if only_line.is_some_and(|l| l != i) {
            out.push_str(seg);
            continue;
        }
        let (body, term) = split_terminator(seg);
        let n = re.find_iter(body).count();
        if n == 0 {
            out.push_str(seg);
            continue;
        }
        count += n;
        let replaced = if use_regex {
            re.replace_all(body, replace)
        } else {
            re.replace_all(body, NoExpand(replace))
        };
        out.push_str(&replaced);
        out.push_str(term);
    }
    (out, count)
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
        if !searchable_file(&entry) {
            continue;
        }
        let path = entry.path();
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
                let shown: String = line.chars().take(200).collect();
                // Ranges are found on the full line, then clipped to the
                // shown (truncated) part; `shown` is a prefix so offsets agree.
                let ranges = re
                    .find_iter(line)
                    .map(|m| (m.start(), m.end().min(shown.len())))
                    .filter(|(s, e)| s < e)
                    .collect();
                results.push(SearchMatch {
                    path: path.to_path_buf(),
                    rel: rel.clone(),
                    line_no: i + 1,
                    line: shown,
                    ranges,
                });
            }
        }
    }
    results
}

/// Enumerates every workspace file under `root` (honoring ignore rules:
/// hidden dot-files and `.gitignore`d paths are skipped), returning absolute
/// paths. Used to build the quickbar's "search file" list. Blocking; call
/// inside `spawn_blocking`.
pub fn list_files(root: &Path) -> Vec<PathBuf> {
    walker(root, false)
        .flatten()
        .filter(searchable_file)
        .map(|e| e.into_path())
        .collect()
}

/// Replaces `query` matches with `replace` on a single 1-based line of `path`
/// (the line a search result points at). Only that line is touched; matches on
/// other lines are left alone. Returns the number of replacements (0 if none).
/// Blocking; call in spawn_blocking.
pub fn replace_in_line(
    path: &Path,
    line_no: usize,
    query: &str,
    replace: &str,
    use_regex: bool,
    match_case: bool,
) -> usize {
    if query.is_empty() || line_no == 0 {
        return 0;
    }
    let re = match build_regex(query, use_regex, match_case) {
        Some(re) => re,
        None => return 0,
    };
    // Same guard as the workspace walk: never write through a symlink, never
    // load a huge file.
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_file() && m.len() <= MAX_FILE_BYTES => {}
        _ => return 0,
    }
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return 0,
    };
    let (new_content, count) = replace_lines(&content, &re, replace, use_regex, Some(line_no - 1));
    if count > 0
        && new_content != content
        && crate::services::fs::write_atomic(path, new_content.as_bytes()).is_ok()
    {
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
        if !searchable_file(&entry) {
            continue;
        }
        let path = entry.path();
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue, // skip binary files
        };
        let (new, count) = replace_lines(&content, &re, replace, use_regex, None);
        if count == 0 {
            continue;
        }
        if new != content && crate::services::fs::write_atomic(path, new.as_bytes()).is_ok() {
            total += count;
            changed.push(path.to_path_buf());
        }
    }
    (changed, total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Writes `content` to a unique temp file and returns its path.
    fn temp_file(content: &str) -> PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("coder-search-test-{}-{n}.txt", std::process::id()));
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn replace_in_line_touches_only_that_line() {
        // "foo" appears on lines 1, 2 and 3; only line 2 must change.
        let path = temp_file("foo\nfoo bar foo\nfoo\n");
        let count = replace_in_line(&path, 2, "foo", "X", false, true);
        let out = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(count, 2); // both "foo" on line 2
        assert_eq!(out, "foo\nX bar X\nfoo\n");
    }

    #[test]
    fn replace_in_line_no_match_leaves_file() {
        let path = temp_file("alpha\nbeta\n");
        let count = replace_in_line(&path, 1, "zzz", "X", false, true);
        let out = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(count, 0);
        assert_eq!(out, "alpha\nbeta\n");
    }

    #[test]
    fn replace_in_line_preserves_crlf_terminator() {
        let path = temp_file("foo\r\nfoo\r\n");
        let count = replace_in_line(&path, 1, "foo", "bar", false, true);
        let out = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(count, 1);
        assert_eq!(out, "bar\r\nfoo\r\n");
    }

    #[test]
    fn replace_all_works_per_line_like_the_preview() {
        let dir = std::env::temp_dir().join(format!(
            "coder-search-replace-all-{}-{}",
            std::process::id(),
            line!()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.txt");
        std::fs::write(&file, "foo  \r\nbar\nfoo\n").unwrap();
        // `\s+$` must not eat the line break (a whole-file regex would join
        // lines), and `$` must match before a CRLF.
        let (changed, n) = replace_all(&dir, r"\s+$", "", true, true, false);
        assert_eq!(n, 1);
        assert_eq!(changed, vec![file.clone()]);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "foo\r\nbar\nfoo\n");
        // `^` anchors at each line start.
        let (_, n) = replace_all(&dir, "^foo", "X", true, true, false);
        assert_eq!(n, 2);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "X\r\nbar\nX\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn replace_all_skips_symlinked_files() {
        let base = std::env::temp_dir().join(format!("coder-search-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let ws = base.join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        let outside = base.join("outside.txt");
        std::fs::write(&outside, "foo\n").unwrap();
        std::os::unix::fs::symlink(&outside, ws.join("link.txt")).unwrap();
        let (changed, n) = replace_all(&ws, "foo", "X", false, true, false);
        assert_eq!((changed.len(), n), (0, 0));
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "foo\n");
        assert_eq!(
            replace_in_line(&ws.join("link.txt"), 1, "foo", "X", false, true),
            0
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn match_ranges_are_char_indices() {
        // "é" is 2 bytes but 1 char: the second match must start at char 6.
        let r = super::match_ranges("é foo foo", "foo", false, false);
        assert_eq!(r, vec![(2, 5), (6, 9)]);
        assert!(super::match_ranges("abc", "", false, false).is_empty());
    }
}
