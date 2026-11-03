//! Search and pages of the rules table (iter2-egress-deny-rules design D8): every entry is
//! loaded, filtered in memory and cut into pages of [`PAGE_SIZE`] rows.

use df_db::EntryRecord;

/// Rows per page.
pub(super) const PAGE_SIZE: usize = 25;

/// What the rules page shows: the search, the requested page and the highlighted target.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(default)]
pub(in crate::egress) struct View {
    /// Rows whose target or scope contain this text (case-insensitive) are listed.
    pub(super) q: String,
    /// 1-based page; without it, the page of the first highlighted row (or the first page).
    pub(super) page: Option<usize>,
    /// `host:port` (`host:any` for a deny rule): the rows of that target are highlighted.
    pub(super) highlight: String,
}

/// The rows of one page and where they sit in the list.
pub(super) struct Page<'a> {
    pub(super) rows: Vec<&'a EntryRecord>,
    /// 1-based number of this page and the number of pages (at least 1).
    pub(super) number: usize,
    pub(super) pages: usize,
    /// Rows that match the search, over all pages.
    pub(super) matched: usize,
    /// 1-based position of the first and last row of this page in the matches; 0 and 0 without.
    pub(super) first: usize,
    pub(super) last: usize,
}

impl View {
    /// The highlighted target, if one is given.
    pub(super) fn highlight(&self) -> Option<&str> {
        Some(self.highlight.as_str()).filter(|h| !h.is_empty())
    }

    /// The search text, trimmed; `None` if empty.
    pub(super) fn search(&self) -> Option<&str> {
        Some(self.q.trim()).filter(|q| !q.is_empty())
    }

    /// The page to show of `entries`, which are in the order of the full list.
    pub(super) fn page<'a>(
        &self,
        entries: &'a [EntryRecord],
        is_highlighted: impl Fn(&EntryRecord) -> bool,
    ) -> Page<'a> {
        let needle = self.search().map(str::to_lowercase);
        let matches: Vec<&EntryRecord> = entries
            .iter()
            .filter(|entry| {
                needle
                    .as_deref()
                    .is_none_or(|needle| super::search_text(entry).contains(needle))
            })
            .collect();
        let pages = matches.len().div_ceil(PAGE_SIZE).max(1);
        let requested = self.page.or_else(|| {
            self.highlight()?;
            let at = matches.iter().position(|entry| is_highlighted(entry))?;
            Some(at / PAGE_SIZE + 1)
        });
        let number = requested.unwrap_or(1).clamp(1, pages);
        let start = (number - 1) * PAGE_SIZE;
        let rows: Vec<_> = matches
            .iter()
            .skip(start)
            .take(PAGE_SIZE)
            .copied()
            .collect();
        let (first, last) = if rows.is_empty() {
            (0, 0)
        } else {
            (start + 1, start + rows.len())
        };
        Page {
            rows,
            number,
            pages,
            matched: matches.len(),
            first,
            last,
        }
    }

    /// The query string of the same search on `page`: `?q=…&page=…`, without empty fields.
    pub(super) fn query(&self, page: usize) -> String {
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        if let Some(q) = self.search() {
            query.append_pair("q", q);
        }
        if page > 1 {
            query.append_pair("page", &page.to_string());
        }
        let query = query.finish();
        if query.is_empty() {
            query
        } else {
            format!("?{query}")
        }
    }
}

#[cfg(test)]
mod tests {
    use df_db::EntryKind;
    use uuid::Uuid;

    use super::*;

    fn entries(count: usize) -> Vec<EntryRecord> {
        (0..count)
            .map(|n| EntryRecord {
                id: Uuid::now_v7(),
                project_key: (n % 2 == 1).then(|| "DF".to_string()),
                task_key: None,
                kind: EntryKind::HttpHost {
                    host: format!("host{n}.example"),
                    port: 443,
                },
            })
            .collect()
    }

    #[test]
    fn pages_hold_25_rows_and_a_page_beyond_the_end_shows_the_last() {
        let all = entries(33);
        let view = |page| View {
            page,
            ..View::default()
        };

        let first = view(None).page(&all, |_| false);
        assert_eq!((first.rows.len(), first.number, first.pages), (25, 1, 2));
        assert_eq!((first.first, first.last, first.matched), (1, 25, 33));
        let second = view(Some(2)).page(&all, |_| false);
        assert_eq!((second.rows.len(), second.first, second.last), (8, 26, 33));
        for beyond in [Some(9), Some(0)] {
            let page = view(beyond).page(&all, |_| false);
            assert_eq!(page.number, if beyond == Some(0) { 1 } else { 2 });
        }
    }

    #[test]
    fn the_search_matches_the_target_or_the_scope_ignoring_case() {
        let all = entries(33);
        let search = |q: &str| {
            View {
                q: q.into(),
                ..View::default()
            }
            .page(&all, |_| false)
        };

        assert_eq!(search("HOST7.").matched, 1);
        assert_eq!(search("project df").matched, 16);
        let none = search("nothing-here");
        assert_eq!(
            (none.matched, none.pages, none.first, none.last),
            (0, 1, 0, 0)
        );
        assert!(none.rows.is_empty());
    }

    #[test]
    fn a_highlight_without_a_page_opens_the_page_of_its_first_row() {
        let all = entries(33);
        let view = View {
            highlight: "host29.example:443".into(),
            ..View::default()
        };
        let is_it = |entry: &EntryRecord| matches!(&entry.kind, EntryKind::HttpHost { host, .. } if host == "host29.example");

        assert_eq!(view.page(&all, is_it).number, 2);
        let explicit = View {
            page: Some(1),
            ..view
        };
        assert_eq!(explicit.page(&all, is_it).number, 1);
    }

    #[test]
    fn the_query_keeps_the_search_and_leaves_out_the_first_page() {
        let view = View {
            q: " github ".into(),
            ..View::default()
        };

        assert_eq!(view.query(1), "?q=github");
        assert_eq!(view.query(2), "?q=github&page=2");
        assert_eq!(View::default().query(1), "");
    }
}
