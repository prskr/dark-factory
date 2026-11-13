//! The "Branches" group of the project's Security tab: the branch rewrite policy and the agent
//! branches (`configuration-management`, "Projects"; design D10, D11).

use df_core::RewritePolicy;
use df_core::glob::protected_glob;
use maud::{Markup, html};

use super::push_policy::{GlobsField, globs, globs_field};
use crate::components::FormErrors;
use crate::components::forms::select_field_with_hint;

const REWRITE_HINT: &str = "With lease, agents may rebase their task branch; commits pushed by others in the meantime are never overwritten by the runtime.";

/// The group's content: the rewrite policy select, then the agent branches textarea.
pub(super) fn group(rewrite_policy: &str, agent_branch_globs: &str, errors: &FormErrors) -> Markup {
    html! {
        (select_field_with_hint("Branch rewrites", "branch_rewrite_policy", &policy_options(),
            rewrite_policy, None, Some(REWRITE_HINT), errors))
        (globs_field(&GlobsField::AGENT_BRANCHES, agent_branch_globs, errors))
    }
}

/// The agent branch globs of the textarea `text`, or the defaults for a form without the field.
pub(super) fn parse_agent_branches(errors: &mut FormErrors, text: Option<&str>) -> Vec<String> {
    match text {
        Some(text) => globs(errors, &GlobsField::AGENT_BRANCHES, text, protected_glob),
        None => df_core::default_agent_branch_globs(),
    }
}

fn policy_options() -> Vec<(String, String)> {
    RewritePolicy::ALL
        .iter()
        .map(|policy| {
            let label = match policy {
                RewritePolicy::Never => "Never",
                RewritePolicy::WithLease => "With lease (default)",
            };
            (policy.as_str().to_owned(), label.to_owned())
        })
        .collect()
}
