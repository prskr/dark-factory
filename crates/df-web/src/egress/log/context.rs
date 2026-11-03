//! What the rows and graph nodes need besides the event: the project hosts for the icons and the
//! deny rules that label denied hosts. Both are read when a viewer connects (a deny rule created
//! later shows after the stream reconnects).

use crate::components::ProjectHosts;

/// The per-connection data of a viewer.
#[derive(Debug, Default, Clone)]
pub(in crate::egress) struct Context {
    pub(super) projects: ProjectHosts,
    /// The patterns of the deny rules.
    deny: Vec<String>,
}

impl Context {
    pub(in crate::egress) fn new(projects: ProjectHosts, deny: Vec<String>) -> Self {
        Self { projects, deny }
    }

    /// The first deny rule pattern that matches `host`.
    pub(super) fn deny_rule_for(&self, host: &str) -> Option<&str> {
        self.deny
            .iter()
            .map(String::as_str)
            .find(|pattern| df_core::host::pattern_matches(pattern, host))
    }
}

/// Link to the rules page with the deny rule `pattern` highlighted.
pub(super) fn deny_rule_href(pattern: &str) -> String {
    super::toast::highlight_href(&format!("{pattern}:any"))
}
