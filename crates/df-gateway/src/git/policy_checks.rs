//! The gateway's push checks (design D2/D3): [`check_push`] on every pushed ref, with the
//! project's push policy and the task's exceptions from the snapshot.

use df_proto::gateway::v1::{CheckMode, PushCheck, PushFinding};
use df_proto::push_reasons::{CHECK_FAILED, PUSH_TOO_LARGE, finding_reason};

use super::checks::{CheckPolicy, Finding, Identity, Target, Verdict, check_push, verdict};
use super::push::{CheckedPush, ChecksOutcome, PushChecks, PushObjects};
use super::refs::{Reason, RefCommand};
use super::secret_rules::{RuleMatcher, SecretRules};
use super::view::{MAX_COMMITS, TaskRefs, ViewError};

/// Longest reason of one rejected ref (design D2).
const MAX_REASON_BYTES: usize = 1024;

/// Runs the checks of design D2 with the secret rules of design D3.
pub struct PolicyChecks {
    rules: SecretRules,
}

impl PolicyChecks {
    pub fn new(rules: SecretRules) -> Self {
        Self { rules }
    }
}

impl PushChecks for PolicyChecks {
    fn check(&self, push: &CheckedPush<'_>, objects: &dyn PushObjects) -> ChecksOutcome {
        let policy = match push.policy.map(CheckPolicy::new) {
            Some(Ok(policy)) => policy,
            Some(Err(error)) => {
                tracing::error!(%error, "the project's push policy does not compile; push rejected");
                return failed(push.commands.len());
            }
            None => {
                tracing::error!("the snapshot has no push policy for the project; push rejected");
                return failed(push.commands.len());
            }
        };
        let refs = RefChecks {
            push,
            policy,
            identity: Identity {
                email: &push.route.commit_email,
            },
            secrets: RuleMatcher {
                rules: &self.rules,
                blobs: objects,
            },
            objects,
        };
        let mut outcome = ChecksOutcome::default();
        let mut only_approval = true;
        for command in push.commands {
            let reason = match refs.check(command) {
                Ok(Verdict::Forward) => None,
                Ok(Verdict::Reject {
                    findings,
                    approval_requested,
                }) => {
                    let findings: Vec<PushFinding> = findings
                        .into_iter()
                        .map(|finding| to_proto(&command.name, finding))
                        .collect();
                    let checks: Vec<&str> = findings
                        .iter()
                        .filter_map(|f| f.check().reason_code())
                        .collect();
                    tracing::info!(ref_name = %command.name, ?checks, approval_requested, "push checks rejected a ref");
                    only_approval &= approval_requested;
                    let reason = Reason::Findings(reason_text(&findings));
                    outcome.findings.extend(findings);
                    Some(reason)
                }
                Err(reason) => {
                    only_approval = false;
                    Some(reason)
                }
            };
            outcome.reasons.push(reason);
        }
        outcome.approval_requested = only_approval && outcome.reasons.iter().any(Option::is_some);
        outcome
    }
}

/// What checking one ref of a push needs.
struct RefChecks<'a> {
    push: &'a CheckedPush<'a>,
    policy: CheckPolicy,
    identity: Identity<'a>,
    secrets: RuleMatcher<'a>,
    objects: &'a dyn PushObjects,
}

impl RefChecks<'_> {
    /// The verdict over what the push of `command` adds, after the task's exceptions; `Err` if it
    /// could not be checked.
    fn check(&self, command: &RefCommand) -> Result<Verdict, Reason> {
        if command.new.is_zero() {
            return Ok(Verdict::Forward);
        }
        let target = if command.name == self.push.route.wip_branch {
            Target::Wip
        } else {
            Target::TaskBranch
        };
        let task = TaskRefs {
            task_branch: &self.push.route.task_branch,
            wip_branch: &self.push.route.wip_branch,
        };
        let view = self
            .objects
            .push_view(task, &command.new, MAX_COMMITS, self.push.interrupt)
            .map_err(|error| match error {
                ViewError::TooManyCommits(max_commits) => {
                    tracing::info!(ref_name = %command.name, max_commits, "push rejected: too many commits");
                    Reason::Check(PUSH_TOO_LARGE)
                }
                error => {
                    tracing::warn!(%error, ref_name = %command.name, "reading the pushed commits failed");
                    Reason::Check(CHECK_FAILED)
                }
            })?;
        let findings = check_push(&view, &self.policy, &self.identity, target, &self.secrets)
            .map_err(|error| {
                tracing::warn!(%error, ref_name = %command.name, "scanning the pushed blobs failed");
                Reason::Check(CHECK_FAILED)
            })?;
        Ok(verdict(findings, self.push.exceptions))
    }
}

/// Every one of `commands` commands rejected with [`CHECK_FAILED`].
fn failed(commands: usize) -> ChecksOutcome {
    ChecksOutcome {
        reasons: vec![Some(Reason::Check(CHECK_FAILED)); commands],
        ..ChecksOutcome::default()
    }
}

/// The [`finding_reason`] of every finding, joined, on one line of at most [`MAX_REASON_BYTES`].
fn reason_text(findings: &[PushFinding]) -> String {
    let mut text = findings
        .iter()
        .map(|f| finding_reason(f.check(), &f.path, f.line, &f.detail))
        .collect::<Vec<_>>()
        .join("; ")
        .replace(char::is_control, " ");
    text.truncate(text.floor_char_boundary(MAX_REASON_BYTES));
    text
}

fn to_proto(ref_name: &str, finding: Finding) -> PushFinding {
    PushFinding {
        r#ref: ref_name.to_owned(),
        check: PushCheck::from(finding.check).into(),
        mode: CheckMode::from(finding.mode).into(),
        fingerprint: finding.fingerprint,
        path: finding.path,
        line: finding.line,
        detail: finding.detail,
        secret: finding.secret,
    }
}

#[cfg(test)]
mod tests;
