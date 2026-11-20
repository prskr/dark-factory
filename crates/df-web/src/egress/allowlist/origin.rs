//! Where a dialog or an action on a rule came from (egress-rule-editing design D4): the rules page
//! with its search and page, or the Egress tab of a project. The answer re-renders that region.

use maud::{Markup, html};
use serde::Deserialize;

use super::paging::View;

/// What the answer of a dialog or an action renders.
pub(in crate::egress) enum ReturnTo {
    /// `#config-list` of the rules page, with the search and the page the user had.
    Allowlist(View),
    /// `#project-egress` of the project with this key.
    Project(String),
}

/// The form fields (`return`, `q`, `page`) and query parameters that carry the context.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub(in crate::egress) struct Origin {
    /// `project:<KEY>` for the project tab; empty for the rules page.
    #[serde(rename = "return")]
    target: String,
    q: String,
    page: String,
}

impl Origin {
    /// The region the answer renders. An empty project key is the rules page.
    pub(in crate::egress) fn resolve(&self) -> ReturnTo {
        match self.project_key() {
            Some(key) => ReturnTo::Project(key.to_string()),
            None => ReturnTo::Allowlist(View {
                q: self.q.clone(),
                page: self.page.parse().ok(),
                highlight: String::new(),
            }),
        }
    }

    /// The key of the project whose tab the request came from.
    pub(in crate::egress) fn project_key(&self) -> Option<&str> {
        self.target
            .strip_prefix("project:")
            .filter(|k| !k.is_empty())
    }

    /// The query string that carries a project tab context: `?return=project:DF`.
    pub(in crate::egress) fn project_query(key: &str) -> String {
        format!("?return=project:{key}")
    }

    /// The hidden fields that carry the context through a dialog.
    pub(in crate::egress) fn hidden_fields(&self) -> Markup {
        html! {
            @for (name, value) in [("return", &self.target), ("q", &self.q), ("page", &self.page)] {
                @if !value.is_empty() {
                    input type="hidden" name=(name) value=(value);
                }
            }
        }
    }
}
