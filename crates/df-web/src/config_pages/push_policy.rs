//! The "Security" tab of the project form: the project's push policy and hidden paths
//! (iter2-push-checks design D4, `specs/push-checks/spec.md`, "Per-project push policy").

use df_core::glob::{Glob, GlobError, hidden_glob, protected_glob};
use df_db::push_checks::{CheckMode, PushPolicy};
use maud::{Markup, html};
use serde::Deserialize;

use crate::components::FormErrors;
use crate::components::forms::INPUT_CLASS;

/// Fields of the tab, so a re-rendered form opens the tab with the error.
pub(crate) const FIELDS: [&str; 9] = [
    "protected_globs",
    "hidden_globs",
    "protected_mode",
    "secret_mode",
    "size_mode",
    "max_blob_mib",
    "linear_history",
    "branch_rewrite_policy",
    "agent_branch_globs",
];

const MIB: u64 = 1024 * 1024;

/// Largest size limit the form takes, in MiB.
const MAX_BLOB_MIB: u64 = 1024;

const MODE_ERROR: &str = "Choose Ask for approval or Reject.";

/// Fewest rows of the globs textarea.
const MIN_GLOB_ROWS: usize = 4;

/// The push policy fields of the project form, as submitted.
#[derive(Debug, Default, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub(crate) struct PushPolicyForm {
    /// One glob per line.
    protected_globs: String,
    /// One glob per line.
    hidden_globs: String,
    /// The modes are missing in a form without the tab.
    protected_mode: Option<String>,
    secret_mode: Option<String>,
    size_mode: Option<String>,
    max_blob_mib: String,
    /// `on` while the switch is on; missing otherwise.
    linear_history: String,
}

impl PushPolicyForm {
    pub(crate) fn from_policy(policy: &PushPolicy) -> Self {
        let mode = |mode: CheckMode| Some(mode.as_str().to_owned());
        Self {
            protected_globs: policy.protected_globs.join("\n"),
            hidden_globs: policy.hidden_globs.join("\n"),
            protected_mode: mode(policy.protected_mode),
            secret_mode: mode(policy.secret_mode),
            size_mode: mode(policy.size_mode),
            max_blob_mib: policy.max_blob_bytes.div_ceil(MIB).to_string(),
            linear_history: if policy.linear_history { "on" } else { "" }.to_owned(),
        }
    }
}

/// The policy of `form`; `None` if the form has no push check fields (the stored policy stays).
pub(crate) fn validate(form: &PushPolicyForm) -> Result<Option<PushPolicy>, FormErrors> {
    if form.protected_mode.is_none() && form.secret_mode.is_none() && form.size_mode.is_none() {
        return Ok(None);
    }
    let mut errors = FormErrors::default();
    let protected_globs = globs(
        &mut errors,
        &GlobsField::PROTECTED,
        &form.protected_globs,
        protected_glob,
    );
    let hidden_globs = globs(
        &mut errors,
        &GlobsField::HIDDEN,
        &form.hidden_globs,
        hidden_glob,
    );
    let protected_mode = mode(
        &mut errors,
        "protected_mode",
        form.protected_mode.as_deref(),
    );
    let secret_mode = mode(&mut errors, "secret_mode", form.secret_mode.as_deref());
    let size_mode = mode(&mut errors, "size_mode", form.size_mode.as_deref());
    let max_blob_mib = form
        .max_blob_mib
        .trim()
        .parse::<u64>()
        .ok()
        .filter(|mib| (1..=MAX_BLOB_MIB).contains(mib));
    if max_blob_mib.is_none() {
        errors.add(
            "max_blob_mib",
            format!("Enter a whole number of MiB from 1 to {MAX_BLOB_MIB}."),
        );
    }
    errors.into_result(Some(PushPolicy {
        protected_globs,
        protected_mode,
        hidden_globs,
        secret_mode,
        size_mode,
        max_blob_bytes: max_blob_mib.unwrap_or_default() * MIB,
        linear_history: form.linear_history == "on",
    }))
}

/// The tab's content.
pub(crate) fn panel(values: &PushPolicyForm, errors: &FormErrors) -> Markup {
    html! {
        p class="text-[13px] text-muted" { "Checked on every push of this project's tasks, before it reaches the forge." }
        (globs_field(&GlobsField::PROTECTED, &values.protected_globs, errors))
        div class="divide-y divide-line rounded-[10px] border border-line" {
            (check_row(&Check {
                name: "protected_mode",
                title: "Protected paths",
                description: "Changes to the paths above.",
                value: values.protected_mode.as_deref(),
                error: errors.get("protected_mode"),
                input: None,
            }))
            (check_row(&Check {
                name: "secret_mode",
                title: "Secret scan",
                description: "Every added file, binary files included. Matches are masked.",
                value: values.secret_mode.as_deref(),
                error: errors.get("secret_mode"),
                input: None,
            }))
            (check_row(&Check {
                name: "size_mode",
                title: "Blob size limit",
                description: "Largest file a commit may add.",
                value: values.size_mode.as_deref(),
                error: errors.get("max_blob_mib").or(errors.get("size_mode")),
                input: Some(size_input(&values.max_blob_mib, errors.get("max_blob_mib").is_some())),
            }))
            (linear_history_row(values.linear_history == "on"))
        }
        (globs_field(&GlobsField::HIDDEN, &values.hidden_globs, errors))
        p class="text-xs text-faint" {
            "Always rejected, whatever the policy: commits whose author or committer is not the instance's bot identity, and changes to .dark-factory/**."
        }
    }
}

/// The non-blank lines of `text`, trimmed. Reports the first one `check` refuses, with its line
/// number and the reason, compiled like the Git proxy does.
pub(super) fn globs(
    errors: &mut FormErrors,
    field: &GlobsField,
    text: &str,
    check: fn(&str) -> Result<Glob, GlobError>,
) -> Vec<String> {
    let mut globs = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let glob = line.trim();
        if glob.is_empty() {
            continue;
        }
        if let Err(error) = check(glob) {
            errors.add(
                field.name,
                format!(
                    "Line {}: \"{glob}\" is not a valid glob ({}).",
                    index + 1,
                    reason(&error)
                ),
            );
            break;
        }
        globs.push(glob.to_owned());
    }
    globs
}

/// Why a glob is invalid, as a short phrase.
fn reason(error: &GlobError) -> String {
    match error {
        // `unclosed character class; missing ']'`: the part before the `;` reads as a reason.
        GlobError::Syntax(error) => {
            let kind = error.kind().to_string();
            kind.split(';').next().unwrap_or_default().to_owned()
        }
        GlobError::Alternation => error.to_string(),
    }
}

fn mode(errors: &mut FormErrors, field: &'static str, value: Option<&str>) -> CheckMode {
    if let Some(Ok(mode)) = value.map(str::parse::<CheckMode>) {
        return mode;
    }
    errors.add(field, MODE_ERROR);
    CheckMode::Approval
}

/// A textarea of globs: its field name, label and hint.
pub(super) struct GlobsField {
    name: &'static str,
    label: &'static str,
    hint: &'static str,
}

impl GlobsField {
    const PROTECTED: Self = Self {
        name: "protected_globs",
        label: "Protected paths",
        hint: "One glob per line. A commit that adds, changes, deletes or renames a matching path is a finding.",
    };

    /// The "Branches" group of the Security tab.
    pub(super) const AGENT_BRANCHES: Self = Self {
        name: "agent_branch_globs",
        label: "Agent branches",
        hint: "One glob per line. Existing branches agents may start tasks on with create_task. Board members can pick any branch in the composer.",
    };

    const HIDDEN: Self = Self {
        name: "hidden_globs",
        label: "Hidden paths",
        hint: "One glob per line, without {a,b}. Matching files are left out of the agent's workspace, and a push that changes one is always rejected. Their content stays reachable through Git history.",
    };
}

/// The textarea of the globs, high enough to show every line (at least [`MIN_GLOB_ROWS`]).
pub(super) fn globs_field(field: &GlobsField, value: &str, errors: &FormErrors) -> Markup {
    let name = field.name;
    let error = errors.get(name);
    let describedby = if error.is_some() {
        format!("{name}-error {name}-hint")
    } else {
        format!("{name}-hint")
    };
    let rows = value.lines().count().max(MIN_GLOB_ROWS);
    html! {
        div class="space-y-1.5" {
            label for=(name) class="block text-sm font-medium text-muted" { (field.label) }
            textarea id=(name) name=(name) rows=(rows) spellcheck="false"
                class="block w-full rounded-[10px] border border-line-strong bg-panel px-3 py-2.5 font-mono text-base text-ink aria-invalid:border-danger md:text-[13px]"
                aria-invalid=[error.map(|_| "true")] aria-describedby=(describedby) { (value) }
            @if let Some(error) = error {
                p id={(name) "-error"} class="text-sm text-danger" role="alert" { (error) }
            }
            p id={(name) "-hint"} class="text-xs text-faint" { (field.hint) }
        }
    }
}

/// A configurable check: title and description on the left, its mode on the right.
struct Check<'a> {
    name: &'static str,
    title: &'a str,
    description: &'a str,
    value: Option<&'a str>,
    error: Option<&'a str>,
    /// A control left of the mode (the size limit).
    input: Option<Markup>,
}

fn check_row(check: &Check<'_>) -> Markup {
    let name = check.name;
    let describedby = match check.error {
        Some(_) => format!("{name}-desc {name}-error"),
        None => format!("{name}-desc"),
    };
    html! {
        div class="flex items-center gap-4 px-4 py-3" {
            div class="min-w-0 flex-1 space-y-0.5" {
                p id={(name) "-title"} class="text-sm font-medium text-ink" { (check.title) }
                p id={(name) "-desc"} class="text-xs text-faint" { (check.description) }
                @if let Some(error) = check.error {
                    p id={(name) "-error"} class="text-sm text-danger" role="alert" { (error) }
                }
            }
            @if let Some(input) = &check.input { (input) }
            div class="w-45 shrink-0" {
                select id=(name) name=(name) class=(INPUT_CLASS) aria-labelledby={(name) "-title"}
                    aria-describedby=(describedby) aria-invalid=[check.error.map(|_| "true")] {
                    option value="approval" selected[check.value == Some("approval")] { "Ask for approval" }
                    option value="reject" selected[check.value == Some("reject")] { "Reject" }
                }
            }
        }
    }
}

/// The size limit in whole MiB, with the unit inside the field.
fn size_input(value: &str, invalid: bool) -> Markup {
    html! {
        div class="relative w-24 shrink-0" {
            input id="max_blob_mib" name="max_blob_mib" type="text" inputmode="numeric" value=(value)
                aria-label="Blob size limit in MiB" aria-describedby=[invalid.then_some("size_mode-error")]
                aria-invalid=[invalid.then_some("true")]
                class={(INPUT_CLASS) " pr-12 font-mono aria-invalid:border-danger"};
            span aria-hidden="true" class="pointer-events-none absolute inset-y-0 right-3 flex items-center text-[13px] text-faint" { "MiB" }
        }
    }
}

fn linear_history_row(on: bool) -> Markup {
    html! {
        div class="flex items-center gap-4 px-4 py-3" {
            div class="min-w-0 flex-1 space-y-0.5" {
                p id="linear_history-title" class="text-sm font-medium text-ink" { "Linear history" }
                p id="linear_history-desc" class="text-xs text-faint" { "Reject merge commits." }
            }
            label class="inline-flex min-h-11 shrink-0 cursor-pointer items-center lg:min-h-10" {
                span class="relative h-5 w-9" {
                    input id="linear_history" name="linear_history" type="checkbox" role="switch" value="on" checked[on]
                        aria-labelledby="linear_history-title" aria-describedby="linear_history-desc" class="peer sr-only";
                    span aria-hidden="true" class="absolute inset-0 rounded-full bg-line-strong transition-colors peer-checked:bg-accent-text peer-focus-visible:outline-2 peer-focus-visible:outline-offset-2 peer-focus-visible:outline-accent-text" {}
                    span aria-hidden="true" class="absolute top-0.5 left-0.5 size-4 rounded-full bg-panel transition-transform peer-checked:translate-x-4" {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
