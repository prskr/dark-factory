//! The `pr.mark_draft` outbox handler (`iter3-webhooks` design D5): marks a task's pull request
//! as draft again once a message moved the task out of `in_review` or `completed`.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use df_core::{ForgeClient, PullRequestUpdate, TaskStatus};
use df_db::catalog::get_project_by_key;
use df_db::outbox::{JobError, OutboxHandler, RetryPolicy};
use df_db::tasks::{MARK_DRAFT_KIND, MarkDraftJob, get_task};
use serde_json::Value;
use sqlx::PgPool;

use super::lock_task_pr;

/// Retries while the forge is unavailable; after an hour the dispatcher gives up with a warning.
const RETRY_POLICY: RetryPolicy = RetryPolicy {
    base: Duration::from_secs(30),
    cap: Duration::from_mins(10),
    give_up_after: Duration::from_hours(1),
    lease: Duration::from_secs(30),
};

/// Runs [`MARK_DRAFT_KIND`] jobs.
pub struct MarkDraftHandler {
    pool: PgPool,
    forge: Arc<dyn ForgeClient>,
}

impl MarkDraftHandler {
    pub fn new(pool: PgPool, forge: Arc<dyn ForgeClient>) -> Self {
        Self { pool, forge }
    }
}

#[async_trait]
impl OutboxHandler for MarkDraftHandler {
    fn kind(&self) -> &'static str {
        MARK_DRAFT_KIND
    }

    fn retry_policy(&self) -> RetryPolicy {
        RETRY_POLICY
    }

    async fn handle(&self, payload: &Value) -> Result<(), JobError> {
        let job: MarkDraftJob = serde_json::from_value(payload.clone())
            .map_err(|e| JobError::GiveUp(format!("invalid mark-draft job: {e}")))?;
        // Held until the update is done: a `pr.sync` job marking the PR ready runs before or
        // after the status check and update below, never between them (design D3).
        let _lock = lock_task_pr(&self.pool, job.task_id)
            .await
            .map_err(|e| JobError::Retry(format!("lock of task `{}`: {e}", job.task_id)))?;
        let task = get_task(&self.pool, job.task_id)
            .await
            .map_err(|e| JobError::Retry(format!("task `{}`: {e}", job.task_id)))?;
        // A deleted task or one without a pull request has nothing to mark.
        let Some((task, number)) = task.and_then(|t| t.pr_number.map(|n| (t, n))) else {
            return Ok(());
        };
        // The task finished again (a merge also completes it) before the job ran.
        if matches!(
            task.status,
            TaskStatus::InReview | TaskStatus::Completed | TaskStatus::Cancelled
        ) {
            return Ok(());
        }
        let number = u64::try_from(number)
            .map_err(|e| JobError::GiveUp(format!("invalid pull request number {number}: {e}")))?;
        let project = get_project_by_key(&self.pool, task.key.project().as_ref())
            .await
            .map_err(|e| JobError::Retry(format!("project of task `{}`: {e}", task.id)))?;
        let Some(project) = project else {
            return Ok(());
        };
        let open = self
            .forge
            .find_open_pull_request(
                &task.project_id,
                &task.key.task_branch(),
                &project.base_branch,
            )
            .await
            .map_err(|e| JobError::Retry(format!("PR #{number}: {e}")))?;
        // A closed or merged PR is left as it is, and so is one that is a draft already.
        if !open.is_some_and(|pr| pr.id.number == number && !pr.draft) {
            return Ok(());
        }
        let draft = PullRequestUpdate {
            draft: Some(true),
            ..PullRequestUpdate::default()
        };
        self.forge
            .update_pull_request(&task.project_id, number, draft)
            .await
            .map(|_| ())
            .map_err(|e| JobError::Retry(format!("PR #{number}: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use df_core::testing::ForgeCall;
    use df_core::{IntegrationError, NewPullRequest, RepoRef};

    use super::*;
    use crate::testing::Env;

    /// A task whose pull request is open and ready for review on the fake forge.
    async fn env_with_pr() -> Env {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool).await;
        let pr = env
            .forge
            .create_pull_request(
                &env.project_id,
                NewPullRequest {
                    repo: RepoRef {
                        owner: "peter".into(),
                        name: "dark-factory".into(),
                    },
                    head: "factory/task/DF-1".into(),
                    base: "main".into(),
                    title: "Fix login".into(),
                    body: String::new(),
                    draft: false,
                },
            )
            .await
            .unwrap();
        sqlx::query("UPDATE tasks SET pr_number = $2 WHERE id = $1")
            .bind(env.task.id.as_uuid())
            .bind(i64::try_from(pr.id.number).unwrap())
            .execute(&env.ctx.pool)
            .await
            .unwrap();
        env
    }

    fn handler(env: &Env) -> MarkDraftHandler {
        MarkDraftHandler::new(env.ctx.pool.clone(), Arc::new(env.forge.clone()))
    }

    fn job(env: &Env) -> Value {
        serde_json::json!({ "task_id": env.task.id })
    }

    async fn is_draft(env: &Env) -> bool {
        let pr = env
            .forge
            .find_open_pull_request(&env.project_id, "factory/task/DF-1", "main")
            .await
            .unwrap()
            .unwrap();
        pr.draft
    }

    #[tokio::test]
    async fn mark_draft_turns_the_pull_request_into_a_draft() {
        let env = env_with_pr().await;

        handler(&env).handle(&job(&env)).await.unwrap();
        assert!(is_draft(&env).await);
    }

    #[tokio::test]
    async fn mark_draft_retries_when_the_forge_fails() {
        let env = env_with_pr().await;
        env.forge
            .push_update_pull_request_result(Err(IntegrationError::Transport("down".into())));

        let failed = handler(&env).handle(&job(&env)).await;
        assert!(matches!(failed, Err(JobError::Retry(_))), "{failed:?}");
        assert!(!is_draft(&env).await);

        handler(&env).handle(&job(&env)).await.unwrap();
        assert!(is_draft(&env).await);
    }

    #[tokio::test]
    async fn mark_draft_without_a_pull_request_does_nothing() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool).await;

        handler(&env).handle(&job(&env)).await.unwrap();
        assert!(env.forge.calls().is_empty());
    }

    fn updates(env: &Env) -> usize {
        env.forge
            .calls()
            .iter()
            .filter(|c| matches!(c, ForgeCall::UpdatePullRequest { .. }))
            .count()
    }

    #[tokio::test]
    async fn mark_draft_leaves_the_pull_request_of_a_task_that_finished_again() {
        for status in ["in_review", "completed", "cancelled"] {
            let env = env_with_pr().await;
            sqlx::query("UPDATE tasks SET status = $2 WHERE id = $1")
                .bind(env.task.id.as_uuid())
                .bind(status)
                .execute(&env.ctx.pool)
                .await
                .unwrap();

            handler(&env).handle(&job(&env)).await.unwrap();
            assert_eq!(updates(&env), 0, "{status}");
            assert!(!is_draft(&env).await, "{status}");
        }
    }

    #[tokio::test]
    async fn mark_draft_leaves_a_closed_pull_request_alone() {
        let env = env_with_pr().await;
        env.forge.push_find_open_pull_request_result(Ok(None));

        handler(&env).handle(&job(&env)).await.unwrap();
        assert_eq!(updates(&env), 0);
    }

    #[tokio::test]
    async fn mark_draft_retries_when_the_open_pull_request_lookup_fails() {
        let env = env_with_pr().await;
        env.forge
            .push_find_open_pull_request_result(Err(IntegrationError::Transport("down".into())));

        let failed = handler(&env).handle(&job(&env)).await;
        assert!(matches!(failed, Err(JobError::Retry(_))), "{failed:?}");
        assert_eq!(updates(&env), 0);
    }

    #[tokio::test]
    async fn mark_draft_skips_a_pull_request_that_is_a_draft_already() {
        let env = env_with_pr().await;
        handler(&env).handle(&job(&env)).await.unwrap();

        handler(&env).handle(&job(&env)).await.unwrap();
        assert_eq!(updates(&env), 1);
    }

    #[tokio::test]
    async fn mark_draft_waits_for_the_task_lock_and_then_drafts_the_pull_request() {
        let env = env_with_pr().await;
        let lock = lock_task_pr(&env.ctx.pool, env.task.id).await.unwrap();
        let (handler, job) = (handler(&env), job(&env));
        let running = tokio::spawn(async move { handler.handle(&job).await });

        crate::pr::sync::tests::wait_for_queued_advisory_lock(&env.ctx.pool).await;
        assert!(!running.is_finished());
        assert_eq!(updates(&env), 0);
        // The ready update of a `pr.sync` job finished meanwhile.
        lock.commit().await.unwrap();

        running.await.unwrap().unwrap();
        assert!(is_draft(&env).await);
    }

    #[tokio::test]
    async fn mark_draft_gives_up_on_an_invalid_payload() {
        let env = env_with_pr().await;

        let result = handler(&env).handle(&serde_json::json!({})).await;
        assert!(matches!(result, Err(JobError::GiveUp(_))), "{result:?}");
    }
}
