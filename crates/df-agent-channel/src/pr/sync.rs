//! The `pr.sync` outbox handler (`pr-forge-outbox` design D1): opens the task's draft PR, marks
//! it ready for review and posts the completion summary of a run. Every step is idempotent, so a
//! retry or a second run of the job leaves the same state.

use std::fmt::Display;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use df_core::{
    ForgeClient, IntegrationError, NewPullRequest, PullRequest, RunId, TaskId, TaskStatus,
};
use df_db::TaskRow;
use df_db::outbox::{JobError, OutboxHandler, RetryPolicy};
use df_db::tasks::{PR_SYNC_KIND, PrSyncJob, get_task};
use df_db::transcript::{append_control_event, forge_event, lock_run_and_task};
use df_db::{Notification, publish};
use df_proto::transcript::v1::{
    ForgeActivity, PullRequestOpened, PullRequestReady, forge_activity,
};
use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction};

use super::{lock_task_pr, repo_ref};
use crate::error::ChannelError;

/// Retries while the forge is unavailable; after an hour the dispatcher gives up with a warning.
/// The lease covers up to five forge calls at the default plugin call timeout of 30 s.
const RETRY_POLICY: RetryPolicy = RetryPolicy {
    base: Duration::from_secs(30),
    cap: Duration::from_mins(10),
    give_up_after: Duration::from_hours(1),
    lease: Duration::from_mins(3),
};

/// Runs [`PR_SYNC_KIND`] jobs.
pub struct PrSyncHandler {
    pool: PgPool,
    forge: Arc<dyn ForgeClient>,
}

impl PrSyncHandler {
    pub fn new(pool: PgPool, forge: Arc<dyn ForgeClient>) -> Self {
        Self { pool, forge }
    }

    async fn sync(&self, job: PrSyncJob) -> Result<(), JobError> {
        // A deleted or cancelled task, or one without a project, has nothing to sync.
        let Some(task) = get_task(&self.pool, job.task_id).await.map_err(retry)? else {
            return Ok(());
        };
        if task.status == TaskStatus::Cancelled {
            return Ok(());
        }
        let Some(project) = self.project(&task).await.map_err(retry)? else {
            return Ok(());
        };

        // Open the PR; continue with the one the database has afterwards.
        let mut recorded = RecordedPr::of_task(&task).map_err(retry)?;
        if job.open && recorded.is_none() {
            recorded = self
                .ensure_draft(&task, &project, job.run_id)
                .await
                .map_err(retry)?;
        }
        let Some(ready_at) = job.ready else {
            return Ok(());
        };
        let Some(pr) = recorded else {
            // No PR to mark. Wait for the PR creation an earlier run's push still retries.
            if !job.open && self.open_job_pending(&task).await.map_err(retry)? {
                return Err(retry("the pull request of the task is not opened yet"));
            }
            return Ok(());
        };

        // Mark ready and post the summary of the run. The factory never changes an attached PR.
        if !pr.attached {
            self.mark_ready(&task, &pr, ready_at, job.run_id)
                .await
                .map_err(retry)?;
        }
        if let Some(run_id) = job.run_id {
            self.post_summary(&task, pr.number, run_id)
                .await
                .map_err(retry)?;
        }
        Ok(())
    }

    async fn project(&self, task: &TaskRow) -> Result<Option<Project>, sqlx::Error> {
        let row: Option<(String, String)> =
            sqlx::query_as("SELECT repo_url, base_branch FROM projects WHERE id = $1")
                .bind(task.project_id.as_uuid())
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|(repo_url, base_branch)| Project {
            repo_url,
            base_branch: task.base_branch(&base_branch).to_owned(),
        }))
    }

    /// Whether another `pr.sync` job of the task that opens the PR has not finished.
    async fn open_job_pending(&self, task: &TaskRow) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT EXISTS (
                 SELECT 1 FROM outbox
                 WHERE kind = $1 AND payload ->> 'task_id' = $2
                   AND (payload ->> 'open')::boolean
                   AND done_at IS NULL AND gave_up_at IS NULL)",
        )
        .bind(PR_SYNC_KIND)
        .bind(task.id.to_string())
        .fetch_one(&self.pool)
        .await
    }

    /// Opens (or finds) the draft PR of the task and records it; returns the PR the task has
    /// afterwards, which is not the one just created if another caller recorded a different one
    /// first.
    async fn ensure_draft(
        &self,
        task: &TaskRow,
        project: &Project,
        run_id: Option<RunId>,
    ) -> Result<Option<RecordedPr>, ChannelError> {
        // Serialize per task, also across control-plane replicas: the lock is held until the PR
        // is recorded, so a second caller sees the PR the first one opened.
        let mut tx = lock_task_pr(&self.pool, task.id).await?;
        if let Some(recorded) = RecordedPr::read(&mut tx, task.id).await? {
            return Ok(Some(recorded));
        }
        let status: String = sqlx::query_scalar("SELECT status FROM tasks WHERE id = $1")
            .bind(task.id.as_uuid())
            .fetch_one(&mut *tx)
            .await?;
        let (head, base) = (task.branch.clone(), project.base_branch.clone());
        let existing = self
            .forge
            .find_open_pull_request(&task.project_id, &head, &base)
            .await?;
        // A board member marked the task done or cancelled it without a PR: a push of its
        // stopped run must not open one. A PR opened before (its recording was lost) is still
        // recorded.
        let closed = matches!(
            status.parse::<TaskStatus>().map_err(df_db::DbError::from)?,
            TaskStatus::Completed | TaskStatus::Cancelled
        );
        // Any PR the factory did not create itself counts as found.
        let mut found = existing.is_some();
        let pr = if let Some(pr) = existing {
            pr
        } else if closed {
            return Ok(None);
        } else {
            let request = new_pull_request(task, &project.repo_url, head.clone(), base.clone())?;
            match self
                .forge
                .create_pull_request(&task.project_id, request)
                .await
            {
                Ok(pr) => pr,
                Err(IntegrationError::Conflict(_)) => {
                    found = true;
                    self.forge
                        .find_open_pull_request(&task.project_id, &head, &base)
                        .await?
                        .ok_or_else(|| {
                            ChannelError::InvalidData("conflicting pull request vanished".into())
                        })?
                }
                Err(e) => return Err(e.into()),
            }
        };
        // A PR found on the branch of an existing-branch task was opened outside the factory;
        // on the task's own branch a found PR is the factory's (a crash lost its recording).
        let attached = found && task.on_existing_branch();
        let recorded = record_pr(&mut tx, task, run_id, &pr, attached).await?;
        tx.commit().await?;
        Ok(Some(recorded))
    }

    /// Marks the PR ready, unless a message reopened the task after `ready_at`, and writes the
    /// `pr-ready:<n>` transcript event of the run. Under the per-task lock of `pr.mark_draft`, so
    /// whichever of the two runs second sees the result of the other (design D3).
    async fn mark_ready(
        &self,
        task: &TaskRow,
        pr: &RecordedPr,
        ready_at: DateTime<Utc>,
        run_id: Option<RunId>,
    ) -> Result<(), ChannelError> {
        let mut tx = lock_task_pr(&self.pool, task.id).await?;
        let reopened: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM task_messages WHERE task_id = $1 AND queued_at > $2)",
        )
        .bind(task.id.as_uuid())
        .bind(ready_at)
        .fetch_one(&mut *tx)
        .await?;
        if reopened {
            tracing::info!(task_id = %task.id, "task was reopened; the pull request stays a draft");
            return Ok(());
        }
        self.forge
            .mark_pull_request_ready(&task.project_id, pr.number)
            .await?;
        if let Some(run_id) = run_id {
            let ready = forge_event(ForgeActivity {
                kind: Some(forge_activity::Kind::PullRequestReady(PullRequestReady {
                    number: pr.number,
                    url: pr.url.clone(),
                })),
            });
            let lock = lock_run_and_task(&mut tx, run_id, task.id).await?;
            let key = format!("pr-ready:{}", pr.number);
            append_control_event(&mut tx, &lock, &key, &ready).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Posts the completion summary of `run_id` on the PR; nothing for a run without one.
    async fn post_summary(
        &self,
        task: &TaskRow,
        number: u64,
        run_id: RunId,
    ) -> Result<(), ChannelError> {
        let summary: Option<String> =
            sqlx::query_scalar("SELECT completion_summary FROM runs WHERE id = $1")
                .bind(run_id.as_uuid())
                .fetch_optional(&self.pool)
                .await?;
        match summary.filter(|s| !s.is_empty()) {
            Some(summary) => {
                self.forge
                    .add_issue_comment(&task.project_id, number, &summary)
                    .await?;
                Ok(())
            }
            None => Ok(()),
        }
    }
}

#[async_trait]
impl OutboxHandler for PrSyncHandler {
    fn kind(&self) -> &'static str {
        PR_SYNC_KIND
    }

    fn retry_policy(&self) -> RetryPolicy {
        RETRY_POLICY
    }

    async fn handle(&self, payload: &Value) -> Result<(), JobError> {
        let job: PrSyncJob = serde_json::from_value(payload.clone())
            .map_err(|e| JobError::GiveUp(format!("invalid pr.sync job: {e}")))?;
        self.sync(job).await
    }
}

/// The pull request recorded on a task.
struct RecordedPr {
    number: u64,
    /// Empty if the task row has none.
    url: String,
    /// The PR was opened outside the factory.
    attached: bool,
}

impl RecordedPr {
    /// The PR of the task row, if it has one. A negative stored number is `InvalidData`.
    fn of_task(task: &TaskRow) -> Result<Option<Self>, ChannelError> {
        task.pr_number
            .map(|number| Self::from_row(number, task.pr_url.clone(), task.pr_attached))
            .transpose()
    }

    /// The PR recorded on the task right now.
    async fn read(
        tx: &mut Transaction<'_, Postgres>,
        task_id: TaskId,
    ) -> Result<Option<Self>, ChannelError> {
        let row: (Option<i64>, Option<String>, bool) =
            sqlx::query_as("SELECT pr_number, pr_url, pr_attached FROM tasks WHERE id = $1")
                .bind(task_id.as_uuid())
                .fetch_one(&mut **tx)
                .await?;
        row.0
            .map(|number| Self::from_row(number, row.1, row.2))
            .transpose()
    }

    fn from_row(number: i64, url: Option<String>, attached: bool) -> Result<Self, ChannelError> {
        let number = u64::try_from(number).map_err(|_negative| {
            ChannelError::InvalidData("stored pull request number out of range".into())
        })?;
        Ok(Self {
            number,
            url: url.unwrap_or_default(),
            attached,
        })
    }
}

/// What the forge needs to know about the project of the task.
struct Project {
    repo_url: String,
    base_branch: String,
}

fn retry(error: impl Display) -> JobError {
    JobError::Retry(error.to_string())
}

fn new_pull_request(
    task: &TaskRow,
    repo_url: &str,
    head: String,
    base: String,
) -> Result<NewPullRequest, ChannelError> {
    let default_body = if task.description.trim().is_empty() {
        format!("Task: {}", task.key)
    } else {
        format!("{}\n\nTask: {}", task.description, task.key)
    };
    Ok(NewPullRequest {
        repo: repo_ref(repo_url)?,
        head,
        base,
        title: task
            .pr_title
            .clone()
            .unwrap_or_else(|| format!("{}: {}", task.key, task.title)),
        body: task.pr_body.clone().unwrap_or(default_body),
        draft: true,
    })
}

/// Records `pr` on the task and, with a run, writes the forge activity to its transcript. If
/// another PR was recorded meanwhile (a forge without conflict detection), nothing changes and
/// the recorded one is returned (design D1). Takes the run and task locks before the task update
/// (`lock_run_and_task`).
async fn record_pr(
    tx: &mut Transaction<'_, Postgres>,
    task: &TaskRow,
    run_id: Option<RunId>,
    pr: &PullRequest,
    attached: bool,
) -> Result<RecordedPr, ChannelError> {
    let number = i64::try_from(pr.id.number).map_err(|_out_of_range| {
        ChannelError::InvalidData("pull request number out of range".into())
    })?;
    let lock = match run_id {
        Some(run_id) => Some(lock_run_and_task(tx, run_id, task.id).await?),
        None => None,
    };
    let updated = sqlx::query(
        "UPDATE tasks SET pr_number = $2, pr_url = $3, pr_attached = $4, updated_at = now()
         WHERE id = $1 AND pr_number IS NULL",
    )
    .bind(task.id.as_uuid())
    .bind(number)
    .bind(&pr.url)
    .bind(attached)
    .execute(&mut **tx)
    .await?;
    if updated.rows_affected() == 0 {
        return RecordedPr::read(tx, task.id).await?.ok_or_else(|| {
            ChannelError::InvalidData("pull request vanished from the task".into())
        });
    }
    publish(&mut **tx, &Notification::TaskChanged { task_id: task.id }).await?;
    if let Some(lock) = lock {
        let opened = forge_event(ForgeActivity {
            kind: Some(forge_activity::Kind::PullRequestOpened(PullRequestOpened {
                number: pr.id.number,
                url: pr.url.clone(),
                draft: pr.draft,
            })),
        });
        let key = format!("pr-opened:{}", pr.id.number);
        append_control_event(tx, &lock, &key, &opened).await?;
    }
    Ok(RecordedPr {
        number: pr.id.number,
        url: pr.url.clone(),
        attached,
    })
}

#[cfg(test)]
mod dispatcher_tests;
#[cfg(test)]
pub(crate) mod tests;
