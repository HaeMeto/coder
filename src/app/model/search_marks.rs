//! Search-panel query matches painted in the editor.

use super::{Model, Panel};

impl Model {
    /// Recomputes `search_marks` for the active buffer while the Search panel
    /// is shown with a query (VSCode-style: the workspace search's term is
    /// highlighted in the editor too). Cleared otherwise. Pure, no IO; cached
    /// by `search_marks_key` so it runs only when the buffer or query changes.
    pub fn refresh_search_marks(&mut self) {
        let s = &self.sidebar.search;
        let shown = self.layout.sidebar_open && self.sidebar.active == Panel::Search;
        let query = s.query.content();
        let tab = self.active_tab.filter(|_| shown && !query.is_empty());
        let Some(i) = tab else {
            self.search_marks.clear();
            self.search_marks_key = None;
            return;
        };
        let buf = &self.tabs[i].buffer;
        // Compared field by field: no per-frame allocation of the query.
        let (id, ver) = (self.tabs[i].id, buf.version);
        if let Some((kid, kver, kq, kre, kcase)) = &self.search_marks_key
            && (*kid, *kver, kq.as_str(), *kre, *kcase)
                == (id, ver, query, s.use_regex, s.match_case)
        {
            return;
        }
        let key = (id, ver, query.to_string(), s.use_regex, s.match_case);
        self.search_marks = crate::services::search::match_ranges(
            &buf.full_text(),
            query,
            s.use_regex,
            s.match_case,
        );
        self.search_marks_key = Some(key);
    }

    /// Index into `search_marks` of the selected search result's match, when
    /// that result is in the active buffer (painted like the current find match).
    pub fn current_search_mark(&self) -> Option<usize> {
        let s = &self.sidebar.search;
        let m = s.results.get(s.selected)?;
        let buf = &self.tabs[self.active_tab?].buffer;
        if buf.path.as_deref() != Some(m.path.as_path()) {
            return None;
        }
        let line = m.line_no.checked_sub(1)?;
        if line >= buf.line_count() {
            return None;
        }
        let (start, end) = (
            buf.rope.line_to_char(line),
            buf.rope.line_to_char(line) + buf.line_len(line),
        );
        self.search_marks
            .iter()
            .position(|&(a, _)| a >= start && a < end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::model::Tab;
    use crate::core::buffer::Buffer;
    use std::path::PathBuf;

    #[test]
    fn search_query_is_marked_in_the_editor_only_while_the_panel_is_shown() {
        let mut model = Model::new(std::env::temp_dir());
        let path = PathBuf::from("/w/a.rs");
        model
            .tabs
            .push(Tab::new(Buffer::new(Some(path.clone()), "foo\nbar foo\n")));
        model.active_tab = Some(0);
        model.sidebar.search.query.insert_paste("foo", false);
        model.sidebar.search.results = vec![crate::services::search::SearchMatch {
            path,
            rel: "a.rs".into(),
            line_no: 2,
            line: "bar foo".into(),
            ranges: vec![(4, 7)],
        }];

        model.layout.sidebar_open = true;
        model.sidebar.active = Panel::Search;
        model.refresh_search_marks();
        assert_eq!(model.search_marks, vec![(0, 3), (8, 11)]);
        assert_eq!(
            model.current_search_mark(),
            Some(1),
            "the selected result's line"
        );

        model.sidebar.active = Panel::Files;
        model.refresh_search_marks();
        assert!(model.search_marks.is_empty());
    }
}
