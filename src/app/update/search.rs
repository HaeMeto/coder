//! Search-panel run/replace commands.

use super::*;

/// Re-runs the workspace search with the current query/options (after a
/// checkbox toggle). No-op when the query is empty.
pub(super) fn rerun_search(model: &mut Model) -> Vec<Cmd> {
    let s = &model.sidebar.search;
    if s.query.is_empty() {
        return Vec::new();
    }
    vec![Cmd::RunSearch {
        query: s.query.content().to_string(),
        use_regex: s.use_regex,
        match_case: s.match_case,
        search_hidden: s.search_hidden,
    }]
}

/// Search panel "Replace All": replaces across all files under the workspace.
pub(super) fn search_replace_all(model: &mut Model) -> Vec<Cmd> {
    let s = &model.sidebar.search;
    if s.query.is_empty() {
        return Vec::new();
    }
    let (query, replace, use_regex, match_case, search_hidden) = (
        s.query.content().to_string(),
        s.replace.content().to_string(),
        s.use_regex,
        s.match_case,
        s.search_hidden,
    );
    model.notify("Replacing…".to_string());
    vec![Cmd::RunReplace {
        query,
        replace,
        use_regex,
        match_case,
        search_hidden,
    }]
}

/// Search panel "Replace": replaces only on the selected result's line.
pub(super) fn search_replace_one(model: &mut Model) -> Vec<Cmd> {
    let s = &model.sidebar.search;
    if s.query.is_empty() {
        return Vec::new();
    }
    let Some(m) = s.results.get(s.selected) else {
        return Vec::new();
    };
    let (path, line_no) = (m.path.clone(), m.line_no);
    let (query, replace, use_regex, match_case) = (
        s.query.content().to_string(),
        s.replace.content().to_string(),
        s.use_regex,
        s.match_case,
    );
    model.notify("Replacing…".to_string());
    vec![Cmd::RunReplaceLine {
        path,
        line_no,
        query,
        replace,
        use_regex,
        match_case,
    }]
}

/// Workspace search results arrived (`Msg::SearchResults`); dropped when the
/// query has changed since.
pub(super) fn search_results(
    model: &mut Model,
    query: String,
    matches: Vec<crate::services::search::SearchMatch>,
) -> Vec<Cmd> {
    if query == model.sidebar.search.query.content() {
        model.sidebar.search.results = matches;
        model.sidebar.search.selected = 0;
        model.notify(format!(
            "{} results found",
            model.sidebar.search.results.len()
        ));
    }
    Vec::new()
}

/// A workspace replace finished (`Msg::ReplaceDone`).
pub(super) fn replace_done(model: &mut Model, changed: Vec<PathBuf>, count: usize) -> Vec<Cmd> {
    // Reload open clean buffers of the changed files the ordinary async way.
    // A dirty buffer keeps its unsaved edits (never clobbered); saving it
    // would overwrite the replacement, so say so.
    let mut cmds = vec![Cmd::LoadGitStatus];
    let mut dirty = 0;
    for path in changed.iter().cloned() {
        if model
            .all_tabs_for(&path)
            .iter()
            .any(|&i| model.tabs[i].buffer.dirty)
        {
            dirty += 1;
        }
        cmds.extend(reload_if_clean(model, path));
    }
    if dirty > 0 {
        model.notify(format!(
            "{dirty} open file(s) have unsaved edits and were not reloaded"
        ));
    }
    model.notify(format!("{} changes, {} files", count, changed.len()));
    // Refresh the results.
    let s = &model.sidebar.search;
    if !s.query.is_empty() {
        cmds.push(Cmd::RunSearch {
            query: s.query.content().to_string(),
            use_regex: s.use_regex,
            match_case: s.match_case,
            search_hidden: s.search_hidden,
        });
    }
    cmds
}
