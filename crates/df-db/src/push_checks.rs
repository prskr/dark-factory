//! Push checks of the Git proxy (iter2-push-checks design D4, D5): the per-project push policy,
//! the task exceptions the policy snapshot carries, the `push-check-exception` approval type and
//! what a recorded push verdict changes on its task.

use std::collections::{BTreeSet, HashMap};
use std::str::FromStr;

use df_core::glob::hidden_glob;
use df_core::system_reason::PUSH_REJECTED;
use df_core::{Principal, RequestId, TaskId, TaskStatus};
use df_proto::gateway::v1 as pb;
use df_proto::push_reasons::{PUSH_TOO_LARGE, finding_reason};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgExecutor, PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::catalog::{Actor, audit};
use crate::git_pushes::{PushRecord, PushVerdict};
use crate::notify::{Notification, publish};
use crate::pool::DbError;
use crate::requests::{
    ApprovalType, NewRequest, Raise, RequestError, RequestKind, RequestRow, Scope,
    close_pending_of_type, raise_in, text_block,
};
use crate::tasks::{TaskRow, queue_message_tx};

/// Name of the approval type.
pub const PUSH_CHECK_EXCEPTION: &str = "push-check-exception";

/// `push.rejected` messages queued since the last accepted task-branch push after which no more
/// are queued.
pub const MAX_REJECTION_MESSAGES: i32 = 3;

/// Findings a `push.rejected` message lists.
const MAX_LISTED_FINDINGS: usize = 20;

/// Reason of a `push-check-exception` request.
const EXCEPTION_REASON: &str = "Approving lets exactly these findings pass for this task.";

/// Protected path globs of a project without a stored policy.
pub const DEFAULT_PROTECTED_GLOBS: [&str; 4] = [
    ".forgejo/workflows/**",
    ".github/workflows/**",
    ".gitea/workflows/**",
    "CODEOWNERS",
];

/// Size limit of a project without a stored policy: 5 MiB.
pub const DEFAULT_MAX_BLOB_BYTES: u64 = 5 * 1024 * 1024;

/// What a finding of a configurable check does to a task-branch push.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckMode {
    /// The push is rejected.
    Reject,
    /// The push is rejected unless the task has an exception for the finding.
    Approval,
}

impl CheckMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Reject => "reject",
            Self::Approval => "approval",
        }
    }
}

impl FromStr for CheckMode {
    type Err = DbError;

    fn from_str(s: &str) -> Result<Self, DbError> {
        match s {
            "reject" => Ok(Self::Reject),
            "approval" => Ok(Self::Approval),
            other => Err(DbError::InvalidData(format!(
                "unknown check mode `{other}`"
            ))),
        }
    }
}

/// A project's push policy. The identity check and `.dark-factory/**` are always on and not part
/// of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PushPolicy {
    pub protected_globs: Vec<String>,
    pub protected_mode: CheckMode,
    /// Paths hidden from the agent's workspace; a push touching one is always rejected.
    pub hidden_globs: Vec<String>,
    pub secret_mode: CheckMode,
    pub size_mode: CheckMode,
    pub max_blob_bytes: u64,
    pub linear_history: bool,
}

impl Default for PushPolicy {
    /// The defaults of the spec ("Per-project push policy").
    fn default() -> Self {
        Self {
            protected_globs: DEFAULT_PROTECTED_GLOBS.map(str::to_owned).to_vec(),
            protected_mode: CheckMode::Approval,
            hidden_globs: Vec::new(),
            secret_mode: CheckMode::Approval,
            size_mode: CheckMode::Approval,
            max_blob_bytes: DEFAULT_MAX_BLOB_BYTES,
            linear_history: false,
        }
    }
}

/// The stored push policies of `project_ids`, by project id. A project without a stored policy
/// is missing; it uses [`PushPolicy::default`].
pub async fn push_policies(
    executor: impl PgExecutor<'_>,
    project_ids: &[Uuid],
) -> Result<HashMap<Uuid, PushPolicy>, DbError> {
    let rows = sqlx::query!(
        "SELECT project_id, protected_globs, protected_mode, hidden_globs, secret_mode, size_mode,
                max_blob_bytes, linear_history
         FROM project_push_policies WHERE project_id = ANY($1)",
        project_ids
    )
    .fetch_all(executor)
    .await?;
    rows.into_iter()
        .map(|row| {
            let max_blob_bytes = u64::try_from(row.max_blob_bytes).map_err(|_negative| {
                DbError::InvalidData(format!("negative size limit of project {}", row.project_id))
            })?;
            let policy = PushPolicy {
                protected_globs: row.protected_globs,
                protected_mode: row.protected_mode.parse()?,
                hidden_globs: row.hidden_globs,
                secret_mode: row.secret_mode.parse()?,
                size_mode: row.size_mode.parse()?,
                max_blob_bytes,
                linear_history: row.linear_history,
            };
            Ok((row.project_id, policy))
        })
        .collect()
}

/// The push policy of project `project_id`: the stored one or the defaults.
pub async fn push_policy(pool: &PgPool, project_id: Uuid) -> Result<PushPolicy, DbError> {
    Ok(push_policies(pool, &[project_id])
        .await?
        .remove(&project_id)
        .unwrap_or_default())
}

/// Stores `policy` as the push policy of project `project_id` (key `key`) unless it is the
/// project's policy already. A change is audited (`push_policy.create` or `.update`) and
/// publishes `egress_policy`.
pub(crate) async fn save_push_policy(
    tx: &mut Transaction<'_, Postgres>,
    actor: Actor<'_>,
    project_id: Uuid,
    key: &str,
    policy: &PushPolicy,
) -> Result<(), DbError> {
    for glob in &policy.hidden_globs {
        hidden_glob(glob)
            .map_err(|error| DbError::InvalidData(format!("hidden path `{glob}`: {error}")))?;
    }
    let before = push_policies(&mut **tx, &[project_id])
        .await?
        .remove(&project_id);
    if before.clone().unwrap_or_default() == *policy {
        return Ok(());
    }
    let max_blob_bytes = i64::try_from(policy.max_blob_bytes).map_err(|_too_large| {
        DbError::InvalidData(format!("size limit {} is too large", policy.max_blob_bytes))
    })?;
    sqlx::query!(
        "INSERT INTO project_push_policies
             (project_id, protected_globs, protected_mode, hidden_globs, secret_mode, size_mode,
              max_blob_bytes, linear_history)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         ON CONFLICT (project_id) DO UPDATE SET
             protected_globs = EXCLUDED.protected_globs,
             protected_mode = EXCLUDED.protected_mode,
             hidden_globs = EXCLUDED.hidden_globs,
             secret_mode = EXCLUDED.secret_mode,
             size_mode = EXCLUDED.size_mode,
             max_blob_bytes = EXCLUDED.max_blob_bytes,
             linear_history = EXCLUDED.linear_history,
             updated_at = now()",
        project_id,
        &policy.protected_globs,
        policy.protected_mode.as_str(),
        &policy.hidden_globs,
        policy.secret_mode.as_str(),
        policy.size_mode.as_str(),
        max_blob_bytes,
        policy.linear_history
    )
    .execute(&mut **tx)
    .await?;
    audit(
        tx,
        actor,
        "push_policy",
        key,
        project_id,
        before.as_ref(),
        policy,
    )
    .await?;
    publish(&mut **tx, &Notification::EgressPolicy).await
}

/// The exception fingerprints of `task_ids`, by task id, sorted. A task without exceptions is
/// missing.
pub async fn push_check_exceptions(
    pool: &PgPool,
    task_ids: &[Uuid],
) -> Result<HashMap<Uuid, Vec<String>>, DbError> {
    let rows = sqlx::query!(
        "SELECT task_id, fingerprint FROM push_check_exceptions
         WHERE task_id = ANY($1) ORDER BY task_id, fingerprint",
        task_ids
    )
    .fetch_all(pool)
    .await?;
    let mut exceptions: HashMap<Uuid, Vec<String>> = HashMap::new();
    for row in rows {
        exceptions
            .entry(row.task_id)
            .or_default()
            .push(row.fingerprint);
    }
    Ok(exceptions)
}

/// Adds exceptions for `fingerprints` to task `task_id`, approved by `request_id`, and publishes
/// `egress_policy`. Existing exceptions stay as they are.
pub(crate) async fn add_push_check_exceptions(
    tx: &mut Transaction<'_, Postgres>,
    task_id: TaskId,
    fingerprints: &[String],
    request_id: RequestId,
) -> Result<(), DbError> {
    sqlx::query!(
        "INSERT INTO push_check_exceptions (task_id, fingerprint, request_id)
         SELECT $1, fingerprint, $3 FROM unnest($2::text[]) AS fingerprint
         ON CONFLICT (task_id, fingerprint) DO NOTHING",
        task_id.as_uuid(),
        fingerprints,
        request_id.as_uuid()
    )
    .execute(&mut **tx)
    .await?;
    publish(&mut **tx, &Notification::EgressPolicy).await
}

/// A check of the Git proxy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PushCheck {
    SecretScan,
    Identity,
    ProtectedPath,
    BlobTooLarge,
    MergeCommit,
    HiddenPath,
}

impl PushCheck {
    /// The check's `df:` rejection reason without the prefix, e.g. `secret-scan`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SecretScan => "secret-scan",
            Self::Identity => "identity",
            Self::ProtectedPath => "protected-path",
            Self::BlobTooLarge => "blob-too-large",
            Self::MergeCommit => "merge-commit",
            Self::HiddenPath => "hidden-path",
        }
    }
}

impl From<PushCheck> for pb::PushCheck {
    fn from(check: PushCheck) -> Self {
        match check {
            PushCheck::SecretScan => Self::SecretScan,
            PushCheck::Identity => Self::Identity,
            PushCheck::ProtectedPath => Self::ProtectedPath,
            PushCheck::BlobTooLarge => Self::BlobTooLarge,
            PushCheck::MergeCommit => Self::MergeCommit,
            PushCheck::HiddenPath => Self::HiddenPath,
        }
    }
}

/// A finding of the push checks as the Git proxy records it. Never contains a matched secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushFinding {
    /// Full name of the ref whose added commits produced the finding.
    pub ref_name: String,
    pub check: PushCheck,
    pub mode: CheckMode,
    /// Key of an exception; `None` for identity and merge commit findings.
    pub fingerprint: Option<String>,
    /// Empty for identity and merge commit findings.
    pub path: String,
    pub line: Option<u32>,
    /// Masked, human-readable detail.
    pub detail: String,
    /// What a secret scan finding matched.
    pub secret: Option<MaskedSecret>,
}

impl PushFinding {
    /// The Git proxy's rejection reason of this finding.
    fn reason(&self) -> String {
        finding_reason(self.check.into(), &self.path, self.line, &self.detail)
    }
}

/// A secret a push check found, without the secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaskedSecret {
    /// Id of the rule that matched.
    pub rule: String,
    /// The match with all but its first characters masked.
    pub masked: String,
}

/// A finding of a `push-check-exception` request, as its payload `{findings: [...]}` stores it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExceptionFinding {
    pub check: PushCheck,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// The rule of a secret.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    /// The masked match of a secret or the size of a blob; empty for a protected path.
    #[serde(default)]
    pub preview: String,
    pub fingerprint: String,
}

impl ExceptionFinding {
    /// The request's view of `finding`; `None` for a finding without a fingerprint, which no
    /// exception can pass.
    pub fn from_finding(finding: &PushFinding) -> Option<Self> {
        let (rule, preview) = match (&finding.secret, finding.check) {
            (Some(secret), _) => (Some(secret.rule.clone()), secret.masked.clone()),
            (None, PushCheck::ProtectedPath | PushCheck::HiddenPath) => (None, String::new()),
            (None, _) => (None, finding.detail.clone()),
        };
        Some(Self {
            check: finding.check,
            path: finding.path.clone(),
            line: finding.line,
            rule,
            preview,
            fingerprint: finding.fingerprint.clone()?,
        })
    }
}

/// The findings of a `push-check-exception` request; none if its payload has no valid list.
pub fn request_findings(payload: &Value) -> Vec<ExceptionFinding> {
    payload
        .get("findings")
        .and_then(|findings| serde_json::from_value(findings.clone()).ok())
        .unwrap_or_default()
}

/// The `push-check-exception` approval type: approving creates task-scoped exceptions for exactly
/// the request's findings.
pub struct PushCheckException;

#[async_trait::async_trait]
impl ApprovalType for PushCheckException {
    fn name(&self) -> &'static str {
        PUSH_CHECK_EXCEPTION
    }

    /// Only the control plane raises these requests, from the Git proxy's push records.
    fn validate(&self, _task: &TaskRow, _payload: &Value) -> Result<Value, RequestError> {
        Err(RequestError::Invalid(format!(
            "`{PUSH_CHECK_EXCEPTION}` requests are raised by the Git proxy, not by agents"
        )))
    }

    /// The sorted set of the finding fingerprints, hashed.
    fn fingerprint(&self, payload: &Value) -> String {
        let findings = request_findings(payload);
        let set: BTreeSet<&str> = findings.iter().map(|f| f.fingerprint.as_str()).collect();
        let joined = set.into_iter().collect::<Vec<_>>().join("\n");
        hex::encode(Sha256::digest(joined.as_bytes()))
    }

    fn scopes(&self) -> &'static [Scope] {
        &[Scope::Task]
    }

    async fn apply(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        request: &RequestRow,
        _scope: Scope,
        _decider: Actor<'_>,
    ) -> Result<(), RequestError> {
        let fingerprints: Vec<String> = request_findings(&request.payload)
            .into_iter()
            .map(|finding| finding.fingerprint)
            .filter(|fingerprint| !fingerprint.is_empty())
            .collect();
        if fingerprints.is_empty() {
            return Err(RequestError::ApplyFailed(
                "the request lists no findings".into(),
            ));
        }
        add_push_check_exceptions(tx, request.task_id, &fingerprints, request.id).await?;
        Ok(())
    }

    fn describe(&self, request: &RequestRow) -> String {
        match request_findings(&request.payload).len() {
            1 => "Push needs approval: 1 finding".to_string(),
            n => format!("Push needs approval: {n} findings"),
        }
    }
}

/// The task of a pushing run.
#[derive(Debug, Clone)]
pub(crate) struct PushingTask {
    pub(crate) id: TaskId,
    /// `refs/heads/<tasks.branch>`.
    pub(crate) task_branch: String,
}

/// What a stored final verdict of a push changes on the task (design D5), in the `RecordPush`
/// transaction.
pub(crate) async fn react_to_push(
    tx: &mut Transaction<'_, Postgres>,
    task: &PushingTask,
    record: &PushRecord,
) -> Result<(), DbError> {
    match record.verdict {
        PushVerdict::Accepted => {
            let accepted = record
                .refs
                .iter()
                .any(|r| r.name == task.task_branch && r.verdict == PushVerdict::Accepted);
            if accepted {
                push_accepted(tx, task.id).await?;
            }
            Ok(())
        }
        PushVerdict::Rejected => {
            let findings: Vec<&PushFinding> = record
                .findings
                .iter()
                .filter(|finding| finding.ref_name == task.task_branch)
                .collect();
            let rejection = if !findings.is_empty() {
                Rejection::Findings {
                    findings,
                    approval_requested: record.approval_requested,
                }
            } else if record
                .refs
                .iter()
                .any(|r| r.name == task.task_branch && r.reason == PUSH_TOO_LARGE)
            {
                Rejection::Reason(PUSH_TOO_LARGE)
            } else {
                return Ok(());
            };
            push_rejected(tx, task, rejection).await
        }
        PushVerdict::Forwarding | PushVerdict::UpstreamFailed | PushVerdict::Unknown => Ok(()),
    }
}

/// Resets the feedback counter and closes the pending exception requests: they are obsolete.
async fn push_accepted(tx: &mut Transaction<'_, Postgres>, task_id: TaskId) -> Result<(), DbError> {
    sqlx::query!(
        "UPDATE tasks SET push_rejections_since_accept = 0 WHERE id = $1",
        task_id.as_uuid()
    )
    .execute(&mut **tx)
    .await?;
    let actor = Principal::system("push.accepted");
    close_pending_of_type(tx, task_id, PUSH_CHECK_EXCEPTION, "push-accepted", &actor).await?;
    Ok(())
}

/// Why the task branch of a push was rejected, as far as the agent is told.
enum Rejection<'a> {
    Findings {
        findings: Vec<&'a PushFinding>,
        approval_requested: bool,
    },
    /// A `df:` reason without findings.
    Reason(&'static str),
}

/// What became of the approval of rejected findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Approval {
    Requested,
    /// The findings need approval, but the task has too many pending requests.
    TooManyPending,
    NotPossible,
}

/// Raises the exception request if a human may approve the findings of the task branch, and
/// queues the `push.rejected` message unless one is undelivered or the cap is reached. Nothing
/// for a completed task.
async fn push_rejected(
    tx: &mut Transaction<'_, Postgres>,
    task: &PushingTask,
    rejection: Rejection<'_>,
) -> Result<(), DbError> {
    let state = sqlx::query!(
        "SELECT status, push_rejections_since_accept FROM tasks WHERE id = $1 FOR UPDATE",
        task.id.as_uuid()
    )
    .fetch_one(&mut **tx)
    .await?;
    if state.status.parse::<TaskStatus>()? == TaskStatus::Completed {
        return Ok(());
    }
    let text = match rejection {
        Rejection::Findings {
            findings,
            approval_requested,
        } => {
            let approval = if approval_requested {
                raise_exception_request(tx, task.id, &findings).await?
            } else {
                Approval::NotPossible
            };
            feedback(&task.task_branch, &findings, approval)
        }
        Rejection::Reason(reason) => reason_feedback(&task.task_branch, reason),
    };
    if state.push_rejections_since_accept >= MAX_REJECTION_MESSAGES
        || undelivered_feedback(tx, task.id).await?
    {
        return Ok(());
    }
    queue_message_tx(
        tx,
        task.id,
        Principal::system(PUSH_REJECTED),
        vec![text_block(text)],
    )
    .await?;
    sqlx::query!(
        "UPDATE tasks SET push_rejections_since_accept = push_rejections_since_accept + 1
         WHERE id = $1",
        task.id.as_uuid()
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Raises the `push-check-exception` request for the approval-mode `findings`, or finds the
/// identical pending one.
async fn raise_exception_request(
    tx: &mut Transaction<'_, Postgres>,
    task_id: TaskId,
    findings: &[&PushFinding],
) -> Result<Approval, DbError> {
    let findings: Vec<ExceptionFinding> = findings
        .iter()
        .filter(|finding| finding.mode == CheckMode::Approval)
        .filter_map(|finding| ExceptionFinding::from_finding(finding))
        .collect();
    if findings.is_empty() {
        return Ok(Approval::NotPossible);
    }
    let payload = json!({ "findings": findings });
    let actor = Principal::system(PUSH_REJECTED);
    let raised = raise_in(
        tx,
        NewRequest {
            from: Raise {
                task_id,
                run_id: None,
                actor: &actor,
                blocking: false,
            },
            kind: RequestKind::Approval,
            approval_type: Some(PUSH_CHECK_EXCEPTION.to_string()),
            fingerprint: PushCheckException.fingerprint(&payload),
            payload,
            reason: EXCEPTION_REASON,
        },
    )
    .await;
    match raised {
        Ok(_) => Ok(Approval::Requested),
        Err(RequestError::TooManyPending) => {
            tracing::warn!(task = %task_id, "too many pending requests; no push-check-exception request raised");
            Ok(Approval::TooManyPending)
        }
        Err(RequestError::Db(error)) => Err(error),
        Err(error) => Err(DbError::InvalidData(error.to_string())),
    }
}

/// Whether a `push.rejected` message of the task waits for delivery.
async fn undelivered_feedback(
    tx: &mut Transaction<'_, Postgres>,
    task_id: TaskId,
) -> Result<bool, DbError> {
    Ok(sqlx::query_scalar!(
        r#"SELECT EXISTS (
             SELECT 1 FROM task_messages
             WHERE task_id = $1 AND delivered_at IS NULL
               AND author->>'kind' = 'system' AND author->>'reason' = $2
           ) AS "exists!""#,
        task_id.as_uuid(),
        PUSH_REJECTED
    )
    .fetch_one(&mut **tx)
    .await?)
}

/// The `push.rejected` message: the masked findings and what to do.
fn feedback(task_branch: &str, findings: &[&PushFinding], approval: Approval) -> String {
    let branch = branch_name(task_branch);
    let mut lines = vec![format!(
        "The Git proxy rejected your push to {branch} because of these findings:"
    )];
    lines.extend(
        findings
            .iter()
            .take(MAX_LISTED_FINDINGS)
            .map(|finding| format!("- {}", finding.reason())),
    );
    if let Some(more) = findings
        .len()
        .checked_sub(MAX_LISTED_FINDINGS)
        .filter(|n| *n > 0)
    {
        lines.push(format!("- and {more} more"));
    }
    lines.push(String::new());
    lines.push(match approval {
        Approval::Requested => "A human was asked to approve these findings (push-check-exception request). If they approve, the same commits pass on the next push after your next turn. If a finding is a mistake, remove it from your unpushed commits instead (amend or rebase them).",
        Approval::TooManyPending => "These findings need a human's approval, but no request was raised because the task has too many pending requests. Remove them from your unpushed commits (amend or rebase them), or wait until a human answered the pending requests; the runtime pushes again after your next turn.",
        Approval::NotPossible => "These findings cannot be approved. Remove them from your unpushed commits (amend or rebase them); the runtime pushes again after your next turn.",
    }.to_string());
    lines.join("\n")
}

/// The `push.rejected` message of a rejection without findings: the `df:` reason.
fn reason_feedback(task_branch: &str, reason: &str) -> String {
    format!(
        "The Git proxy rejected your push to {}: {reason}",
        branch_name(task_branch)
    )
}

fn branch_name(task_branch: &str) -> &str {
    task_branch
        .strip_prefix("refs/heads/")
        .unwrap_or(task_branch)
}

#[cfg(test)]
mod tests;
