//! The `create-issue` approval type (`chat-issue-tools`, spec `agent-requests`).
//!
//! The agent proposes an issue; the stored payload (title, body, labels) is fixed once raised.
//! Approving queues an `issue.create` outbox job in the decision's transaction, so the forge is
//! never called under the database lock. The job creates the issue from the stored payload and
//! records the outcome here with [`record_issue_outcome`], once per approval.

use std::collections::BTreeSet;

use df_core::system_reason::ISSUE_CREATED;
use df_core::{Principal, RequestId};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};

use crate::catalog::Actor;
use crate::outbox::enqueue;
use crate::pool::DbError;
use crate::requests::{ApprovalType, RequestError, RequestRow, Scope, lock_request};
use crate::tasks::{TaskRow, queue_message_tx, text_block};

/// Name of the approval type.
pub const CREATE_ISSUE: &str = "create-issue";
/// Outbox kind of the job that creates the issue; payload [`IssueJob`].
pub const ISSUE_CREATE_KIND: &str = "issue.create";
/// Longest title, in characters.
pub const MAX_TITLE_CHARS: usize = 256;
/// Longest body, in characters.
pub const MAX_BODY_CHARS: usize = 65_535;
/// Most labels of an issue.
pub const MAX_LABELS: usize = 20;
/// Longest label name, in characters.
pub const MAX_LABEL_CHARS: usize = 50;

/// The stored payload of a `create-issue` request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssuePayload {
    pub title: String,
    pub body: String,
    pub labels: Vec<String>,
}

/// Payload of an [`ISSUE_CREATE_KIND`] job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueJob {
    pub request_id: RequestId,
}

/// What the job reported for a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssueOutcome {
    Created { number: u64, url: String },
    Failed { error: String },
}

/// The issue an approved request created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedIssue {
    pub number: u64,
    pub url: String,
}

/// The issue the job recorded for an approved `create-issue` request, if it has.
pub fn created_issue(request: &RequestRow) -> Option<CreatedIssue> {
    let decision = request.decision.as_ref()?;
    Some(CreatedIssue {
        number: decision.get("issue_number")?.as_u64()?,
        url: decision.get("issue_url")?.as_str()?.to_string(),
    })
}

/// Whether the job recorded an outcome (an issue or an error) for the request already.
pub fn outcome_recorded(request: &RequestRow) -> bool {
    request
        .decision
        .as_ref()
        .is_some_and(|d| d.get("issue_number").is_some() || d.get("issue_error").is_some())
}

/// The forge's error the job recorded when it could not create the issue of the request.
pub fn issue_error(request: &RequestRow) -> Option<&str> {
    request.decision.as_ref()?.get("issue_error")?.as_str()
}

/// The payload of a stored `create-issue` request.
pub fn issue_payload(payload: &Value) -> Result<IssuePayload, RequestError> {
    serde_json::from_value(payload.clone())
        .map_err(|e| RequestError::Invalid(format!("not a create-issue payload: {e}")))
}

/// The `create-issue` approval type.
pub struct CreateIssue;

#[async_trait::async_trait]
impl ApprovalType for CreateIssue {
    fn name(&self) -> &'static str {
        CREATE_ISSUE
    }

    fn validate(&self, _task: &TaskRow, payload: &Value) -> Result<Value, RequestError> {
        validate_payload(payload).map(|issue| json!(issue))
    }

    /// A hash of title, body and the sorted labels.
    fn fingerprint(&self, payload: &Value) -> String {
        let Ok(issue) = issue_payload(payload) else {
            return payload.to_string();
        };
        let labels: BTreeSet<&str> = issue.labels.iter().map(String::as_str).collect();
        let mut hash = Sha256::new();
        for part in [issue.title.as_str(), issue.body.as_str()]
            .into_iter()
            .chain(labels)
        {
            // The length keeps `("ab", "c")` apart from `("a", "bc")`.
            hash.update(part.len().to_le_bytes());
            hash.update(part.as_bytes());
        }
        hex::encode(hash.finalize())
    }

    /// One scope: this request only.
    fn scopes(&self) -> &'static [Scope] {
        &[Scope::Task]
    }

    /// Queues the job that creates the issue; the forge is not called here.
    async fn apply(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        request: &RequestRow,
        _scope: Scope,
        _decider: Actor<'_>,
    ) -> Result<(), RequestError> {
        let job = json!(IssueJob {
            request_id: request.id
        });
        enqueue(tx, ISSUE_CREATE_KIND, &job).await?;
        Ok(())
    }

    fn describe(&self, request: &RequestRow) -> String {
        let title = issue_payload(&request.payload).map_or_default(|p| p.title);
        format!("Create issue \"{title}\"")
    }
}

/// Checks a raised payload against the limits and returns it in the form it is stored in: trimmed
/// title, body as given, trimmed labels without duplicates.
pub fn validate_payload(payload: &Value) -> Result<IssuePayload, RequestError> {
    let invalid = |what: String| RequestError::Invalid(what);
    let title = payload
        .get("title")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("`title` is required".into()))?
        .trim();
    if title.is_empty() || title.chars().count() > MAX_TITLE_CHARS || title.contains(['\n', '\r']) {
        return Err(invalid(format!(
            "the title must be one line of 1 to {MAX_TITLE_CHARS} characters"
        )));
    }
    let body = match payload.get("body") {
        None | Some(Value::Null) => "",
        Some(body) => body
            .as_str()
            .ok_or_else(|| invalid("`body` must be a string".into()))?,
    };
    if body.chars().count() > MAX_BODY_CHARS {
        return Err(invalid(format!(
            "the body must have at most {MAX_BODY_CHARS} characters"
        )));
    }
    let mut labels: Vec<String> = Vec::new();
    for label in label_values(payload)? {
        let label = label.trim();
        if label.is_empty() || label.chars().count() > MAX_LABEL_CHARS {
            return Err(invalid(format!(
                "a label must have 1 to {MAX_LABEL_CHARS} characters"
            )));
        }
        if !labels.iter().any(|l| l == label) {
            labels.push(label.to_string());
        }
    }
    if labels.len() > MAX_LABELS {
        return Err(invalid(format!("at most {MAX_LABELS} labels")));
    }
    Ok(IssuePayload {
        title: title.to_string(),
        body: body.to_string(),
        labels,
    })
}

fn label_values(payload: &Value) -> Result<Vec<&str>, RequestError> {
    match payload.get("labels") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(labels)) => labels
            .iter()
            .map(|l| {
                l.as_str()
                    .ok_or_else(|| RequestError::Invalid("`labels` must be strings".into()))
            })
            .collect(),
        Some(_) => Err(RequestError::Invalid("`labels` must be a list".into())),
    }
}

/// Records what the job did for `request_id` and tells the task in a hidden message, in one
/// transaction. Returns `false` and changes nothing if an outcome is recorded already, so a
/// request creates at most one issue per approval.
pub async fn record_issue_outcome(
    pool: &PgPool,
    request_id: RequestId,
    outcome: &IssueOutcome,
) -> Result<bool, DbError> {
    let mut tx = pool.begin().await?;
    let Some((task_id, request)) = lock_request(&mut tx, request_id).await? else {
        return Ok(false);
    };
    if outcome_recorded(&request) {
        return Ok(false);
    }
    let (recorded, message) = match outcome {
        IssueOutcome::Created { number, url } => (
            json!({ "issue_number": number, "issue_url": url }),
            format!("Issue #{number} created: {url}"),
        ),
        IssueOutcome::Failed { error } => (
            json!({ "issue_error": error }),
            format!("The issue could not be created: {error}"),
        ),
    };
    sqlx::query!(
        "UPDATE agent_requests SET decision = COALESCE(decision, '{}'::jsonb) || $2 WHERE id = $1",
        request_id.as_uuid(),
        recorded
    )
    .execute(&mut *tx)
    .await?;
    queue_message_tx(
        &mut tx,
        task_id,
        Principal::system(ISSUE_CREATED),
        vec![text_block(message, true)],
        None,
    )
    .await?;
    crate::notify::publish(
        &mut *tx,
        &crate::notify::Notification::RequestChanged {
            task_id,
            request_id,
        },
    )
    .await?;
    tx.commit().await?;
    Ok(true)
}

#[cfg(test)]
mod tests;
