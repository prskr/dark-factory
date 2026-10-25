//! Filters of the traffic page, as query parameters of the page and its event stream.

use df_proto::gateway::v1::{FlowEvent, Verdict};
use serde::Deserialize;

/// The *Advanced* filters, by parameter name (the page's script counts the same ones).
pub(in crate::egress) const ADVANCED_FIELDS: [&str; 2] = ["persona", "task"];
/// The choices of the *Rows* select: how many rows the table keeps.
pub(in crate::egress) const ROW_LIMITS: [usize; 6] = [100, 250, 500, 1000, 2500, 5000];
pub(in crate::egress) const DEFAULT_ROW_LIMIT: usize = 500;

/// The verdict segments of the filter bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(in crate::egress) enum VerdictFilter {
    #[default]
    All,
    /// Denied connections (`Verdict::Denied`; "blocked" is only the label).
    Blocked,
    /// Everything that was not denied, open tunnels included.
    Allowed,
}

impl VerdictFilter {
    pub(in crate::egress) const fn param(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Blocked => "blocked",
            Self::Allowed => "allowed",
        }
    }

    /// The segment a `verdict` parameter names.
    fn from_param(param: &str) -> Option<Self> {
        [Self::All, Self::Blocked, Self::Allowed]
            .into_iter()
            .find(|verdict| verdict.param() == param)
    }

    fn matches(self, event: &FlowEvent) -> bool {
        match self {
            Self::All => true,
            Self::Blocked => is_blocked(event),
            Self::Allowed => !is_blocked(event),
        }
    }
}

/// Whether the connection was refused. Everything else, open tunnels included, counts as allowed.
pub(in crate::egress) fn is_blocked(event: &FlowEvent) -> bool {
    event.verdict() == Verdict::Denied
}

/// Filters of the log. `denied` and `host` are the parameters of the earlier page and still
/// work: `denied=1` selects *Blocked*, `host` is the search. Unknown parameters (the dropped
/// `sandbox` and `project` filters) are ignored.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub(in crate::egress) struct Filter {
    /// Legacy: any value selects *Blocked*.
    pub(in crate::egress) denied: String,
    /// `all`, `blocked` or `allowed`; anything else is ignored.
    #[serde(deserialize_with = "lenient_verdict")]
    pub(in crate::egress) verdict: Option<VerdictFilter>,
    /// Substring of the destination host.
    pub(in crate::egress) q: String,
    /// Task key (`DF-7`), any case.
    pub(in crate::egress) task: String,
    /// Persona id, chosen by name in the filter bar.
    pub(in crate::egress) persona: String,
    /// Rows the table keeps, one of [`ROW_LIMITS`]; anything else is the default. A view
    /// setting, not a filter: it is in the URL but does not count as a filter in use.
    #[serde(deserialize_with = "lenient_rows")]
    pub(in crate::egress) rows: Option<usize>,
    /// Legacy name of `q`.
    pub(in crate::egress) host: String,
}

impl Filter {
    pub(in crate::egress) fn verdict_filter(&self) -> VerdictFilter {
        self.verdict.unwrap_or(if self.denied.is_empty() {
            VerdictFilter::All
        } else {
            VerdictFilter::Blocked
        })
    }

    pub(in crate::egress) fn search(&self) -> &str {
        let q = self.q.trim();
        if q.is_empty() { self.host.trim() } else { q }
    }

    /// The *Advanced* filters in use (the search and the verdict are in the bar itself).
    pub(in crate::egress) fn advanced_count(&self) -> usize {
        [&self.persona, &self.task]
            .into_iter()
            .filter(|value| !value.trim().is_empty())
            .count()
    }

    pub(in crate::egress) fn row_limit(&self) -> usize {
        self.rows.unwrap_or(DEFAULT_ROW_LIMIT)
    }

    /// Whether no filter is in use (the row limit is no filter).
    pub(in crate::egress) fn is_empty(&self) -> bool {
        self.verdict_filter() == VerdictFilter::All
            && self.search().is_empty()
            && self.advanced_count() == 0
    }

    /// `?key=value&...` of the set fields, with the legacy parameters in their current form;
    /// empty if none is set.
    pub(in crate::egress) fn query(&self) -> String {
        let verdict = self.verdict_filter();
        let rows = match self.row_limit() {
            DEFAULT_ROW_LIMIT => String::new(),
            limit => limit.to_string(),
        };
        let fields = [
            (
                "verdict",
                if verdict == VerdictFilter::All {
                    ""
                } else {
                    verdict.param()
                },
            ),
            ("q", self.search()),
            ("task", self.task.trim()),
            ("persona", self.persona.trim()),
            ("rows", &rows),
        ];
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        for (name, value) in fields {
            if !value.is_empty() {
                query.append_pair(name, value);
            }
        }
        let query = query.finish();
        if query.is_empty() {
            query
        } else {
            format!("?{query}")
        }
    }

    /// Matches everything but the verdict: what the segment counts are about.
    pub(in crate::egress) fn matches_except_verdict(&self, event: &FlowEvent) -> bool {
        let task = self.task.trim();
        let persona = self.persona.trim();
        (task.is_empty() || task.eq_ignore_ascii_case(&event.task_key))
            && (persona.is_empty() || persona == event.persona_id)
            && event
                .host
                .to_lowercase()
                .contains(&self.search().to_lowercase())
    }

    pub(in crate::egress) fn matches(&self, event: &FlowEvent) -> bool {
        self.verdict_filter().matches(event) && self.matches_except_verdict(event)
    }
}

/// Reads `rows`; a value that is not one of [`ROW_LIMITS`] is the same as none.
fn lenient_rows<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<usize>, D::Error> {
    let raw = Option::<String>::deserialize(deserializer)?;
    Ok(raw
        .and_then(|raw| raw.parse().ok())
        .filter(|rows| ROW_LIMITS.contains(rows)))
}

/// Reads `verdict`; an unknown value is the same as none, so an old or mistyped link still opens.
fn lenient_verdict<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<VerdictFilter>, D::Error> {
    let raw = Option::<String>::deserialize(deserializer)?;
    Ok(raw.as_deref().and_then(VerdictFilter::from_param))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advanced_fields_name_the_filters_that_advanced_count_counts() {
        for name in ADVANCED_FIELDS {
            let uri: axum::http::Uri = format!("/x?{name}=a").parse().unwrap();
            let filter = axum::extract::Query::<Filter>::try_from_uri(&uri)
                .unwrap()
                .0;
            assert_eq!(filter.advanced_count(), 1, "{name}");
            assert_eq!(filter.query(), format!("?{name}=a"), "{name}");
        }
        assert_eq!(Filter::default().advanced_count(), 0);
        // And the other way: all fields set count all names.
        let all = ADVANCED_FIELDS.map(|name| format!("{name}=a")).join("&");
        let uri: axum::http::Uri = format!("/x?{all}").parse().unwrap();
        let filter = axum::extract::Query::<Filter>::try_from_uri(&uri)
            .unwrap()
            .0;
        assert_eq!(filter.advanced_count(), ADVANCED_FIELDS.len());
    }

    #[test]
    fn the_task_key_matches_regardless_of_case_and_the_persona_exactly() {
        let event = FlowEvent {
            task_key: "DF-7".into(),
            persona_id: "p-1".into(),
            ..Default::default()
        };
        let task = |task: &str| Filter {
            task: task.into(),
            ..Default::default()
        };
        assert!(task("df-7").matches(&event) && task(" DF-7 ").matches(&event));
        assert!(!task("DF-70").matches(&event));
        let persona = |persona: &str| Filter {
            persona: persona.into(),
            ..Default::default()
        };
        assert!(persona("p-1").matches(&event) && !persona("p-2").matches(&event));
    }
}
