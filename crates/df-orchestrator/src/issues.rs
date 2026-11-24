//! The `issue.create` outbox handler (`chat-issue-tools`): creates the issue of an approved
//! `create-issue` request from the stored payload and tells the task the number or the forge's
//! error. The forge is never called in the decision's transaction (spec `agent-requests`).

use std::fmt::Display;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use df_core::{ForgeClient, IntegrationError, NewIssue, RequestId};
use df_db::create_issue::{
    CREATE_ISSUE, ISSUE_CREATE_KIND, IssueJob, IssueOutcome, issue_payload, outcome_recorded,
    record_issue_outcome,
};
use df_db::outbox::{JobError, OutboxHandler, RetryPolicy};
use df_db::requests::{RequestState, get_request};
use serde_json::Value;
use sqlx::PgPool;

use crate::pr::forge_retry_policy;

/// One forge call per attempt.
const LEASE: Duration = Duration::from_mins(1);

/// Runs [`ISSUE_CREATE_KIND`] jobs.
pub struct IssueCreateHandler {
    pool: PgPool,
    forge: Arc<dyn ForgeClient>,
    retry_base: Duration,
}

impl IssueCreateHandler {
    pub fn new(pool: PgPool, forge: Arc<dyn ForgeClient>, retry_base: Duration) -> Self {
        Self {
            pool,
            forge,
            retry_base,
        }
    }

    /// Records `outcome` of the job's request, if it is still to be recorded.
    async fn record(&self, request_id: RequestId, outcome: &IssueOutcome) -> Result<(), JobError> {
        record_issue_outcome(&self.pool, request_id, outcome)
            .await
            .map(drop)
            .map_err(retry)
    }

    async fn create(&self, job: IssueJob) -> Result<(), JobError> {
        let request = get_request(&self.pool, job.request_id)
            .await
            .map_err(retry)?;
        // A request that is gone, not an approved `create-issue`, or done already creates nothing.
        let Some(request) = request.filter(|r| {
            r.approval_type.as_deref() == Some(CREATE_ISSUE)
                && r.state == RequestState::Approved
                && !outcome_recorded(r)
        }) else {
            return Ok(());
        };
        let payload = issue_payload(&request.payload)
            .map_err(|e| JobError::GiveUp(format!("invalid create-issue payload: {e}")))?;
        let Some(task) = df_db::get_task(&self.pool, request.task_id)
            .await
            .map_err(retry)?
        else {
            return Ok(());
        };
        let new = NewIssue {
            title: payload.title,
            body: payload.body,
            labels: payload.labels,
        };
        let outcome = match self.forge.create_issue(&task.project_id, &new).await {
            Ok(issue) => IssueOutcome::Created {
                number: issue.id.number,
                url: issue.url,
            },
            Err(e @ (IntegrationError::RateLimited(_) | IntegrationError::Transport(_))) => {
                return Err(retry(e));
            }
            Err(e) => IssueOutcome::Failed {
                error: e.to_string(),
            },
        };
        self.record(request.id, &outcome).await
    }
}

#[async_trait]
impl OutboxHandler for IssueCreateHandler {
    fn kind(&self) -> &'static str {
        ISSUE_CREATE_KIND
    }

    fn retry_policy(&self) -> RetryPolicy {
        forge_retry_policy(self.retry_base, LEASE)
    }

    async fn handle(&self, payload: &Value) -> Result<(), JobError> {
        let job: IssueJob = serde_json::from_value(payload.clone())
            .map_err(|e| JobError::GiveUp(format!("invalid issue.create job: {e}")))?;
        self.create(job).await
    }

    /// The forge stayed down until the give-up age: the request records the last error and the
    /// task gets the same message as for an error that is not retried.
    async fn expired(&self, payload: &Value, last_error: &str) -> Result<(), JobError> {
        let job = match serde_json::from_value::<IssueJob>(payload.clone()) {
            Ok(job) => job,
            Err(e) => {
                // `handle` gives an unparsable payload up as well; there is no request to tell.
                tracing::warn!(error = %e, "expired issue.create job with an invalid payload");
                return Ok(());
            }
        };
        let outcome = IssueOutcome::Failed {
            error: last_error.to_string(),
        };
        // Does nothing when an outcome is recorded already.
        self.record(job.request_id, &outcome).await
    }
}

fn retry(error: impl Display) -> JobError {
    JobError::Retry(error.to_string())
}

#[cfg(test)]
mod tests;
