//! The "Branches" group of the project's Security tab: the branch rewrite policy, who merges
//! subtask pull requests and the agent branches (`configuration-management`, "Projects"; design
//! D10, D11).

use df_core::{RewritePolicy, SubtaskMerge};
use df_sandbox_contract::glob::protected_glob;
use maud::{Markup, html};

use super::push_policy::{GlobsField, globs, globs_field};
use crate::components::FormErrors;
use crate::components::forms::select_field_with_hint;

const REWRITE_HINT: &str = "With lease, agents may rebase their task branch; commits pushed by others in the meantime are never overwritten by the runtime.";

const SUBTASK_MERGE_HINT: &str = "Human: board members merge the pull requests of subtasks. Parent agent: the agent of the parent task may also merge them into its own task branch.";

/// The group's content: the rewrite policy select, the subtask merge select, then the agent
/// branches textarea.
pub(super) fn group(
    rewrite_policy: &str,
    subtask_merge: &str,
    agent_branch_globs: &str,
    errors: &FormErrors,
) -> Markup {
    html! {
        (select_field_with_hint("Branch rewrites", "branch_rewrite_policy", &policy_options(),
            rewrite_policy, None, Some(REWRITE_HINT), errors))
        (select_field_with_hint("Subtask merges", "subtask_merge", &subtask_merge_options(),
            subtask_merge, None, Some(SUBTASK_MERGE_HINT), errors))
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

/// The subtask merge policy of the form; an empty value (a form without the field) is the default.
pub(super) fn parse_subtask_merge(errors: &mut FormErrors, value: &str) -> SubtaskMerge {
    if value.is_empty() {
        return SubtaskMerge::default();
    }
    value.parse().unwrap_or_else(|_unknown| {
        errors.add("subtask_merge", "Choose who merges subtask pull requests.");
        SubtaskMerge::default()
    })
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

fn subtask_merge_options() -> Vec<(String, String)> {
    SubtaskMerge::ALL
        .iter()
        .map(|value| {
            let label = match value {
                SubtaskMerge::Human => "Human (default)",
                SubtaskMerge::ParentAgent => "Parent agent",
            };
            (value.as_str().to_owned(), label.to_owned())
        })
        .collect()
}
