//! Pull request lifecycle: draft PR on the first push ahead of base, ready for review on
//! completion, divergence flag (`specs/pull-request-lifecycle/spec.md`, design D7).
//!
//! All functions are best effort towards the forge: a failure is logged and never fails the run.
//! Opening the PR is retried by the next trigger (push report or run end); at the end of a
//! completed run there is no next trigger, so each step there runs even if the one before failed.

use df_core::{IntegrationError, NewPullRequest, PullRequest, PullRequestUpdate, RepoRef, TaskId};
use df_db::{Notification, publish};
use df_proto::agent::v1::{PushReport, PushStatus, PushTarget, WorkspaceState};
use sqlx::PgPool;

use crate::ChannelContext;
use crate::assign::RunContext;
use crate::error::ChannelError;

/// Handles a `PushReport` after it was persisted.
pub async fn on_push_report(ctx: &ChannelContext, rc: &RunContext, report: &PushReport) {
    if report.target() != PushTarget::TaskBranch {
        return;
    }
    match report.status() {
        PushStatus::RejectedNonFastForward => {
            if let Err(e) = mark_diverged(&ctx.pool, rc.task.id).await {
                tracing::warn!(error = %e, task_id = %rc.task.id, "failed to flag divergence");
            }
        }
        PushStatus::Ok if report.commits_ahead_of_base > 0 => ensure_draft(ctx, rc).await,
        _ => {}
    }
}

/// Handles the end of a run: retries a missing PR, and on `completed` marks it ready for review
/// and posts the completion summary.
pub async fn on_run_end(
    ctx: &ChannelContext,
    rc: &RunContext,
    completed: bool,
    workspace: Option<&WorkspaceState>,
) {
    let pushed = workspace.is_some_and(|w| w.task_branch_pushed);
    let current = match refreshed(&ctx.pool, rc).await {
        Ok(current) => current,
        Err(e) => {
            tracing::warn!(error = %e, task_id = %rc.task.id, "failed to reload task");
            return;
        }
    };
    let mut pr_number = current.task.pr_number;
    if pr_number.is_none() && (pushed || completed) {
        pr_number = try_ensure_draft(ctx, &current).await.unwrap_or_else(|e| {
            tracing::warn!(error = %e, task_id = %rc.task.id, "pull request creation failed; retrying on next trigger");
            None
        });
    }
    if !completed {
        return;
    }
    let Some(number) = pr_number.and_then(|n| u64::try_from(n).ok()) else {
        return;
    };
    let ready = PullRequestUpdate {
        draft: Some(false),
        ..PullRequestUpdate::default()
    };
    if let Err(e) = ctx
        .forge
        .update_pull_request(&rc.project.id, number, ready)
        .await
    {
        // A completed run has no later trigger: still post the summary.
        tracing::warn!(error = %e, task_id = %rc.task.id, "failed to mark the pull request ready");
    }
    match completion_summary(&ctx.pool, rc.task.id).await {
        Ok(summary) if !summary.is_empty() => {
            if let Err(e) = ctx
                .forge
                .add_issue_comment(&rc.project.id, number, &summary)
                .await
            {
                tracing::warn!(error = %e, task_id = %rc.task.id, "failed to post the completion summary");
            }
        }
        Ok(_) => {}
        Err(e) => {
            tracing::warn!(error = %e, task_id = %rc.task.id, "failed to load completion summary");
        }
    }
}

/// Opens (or finds) the task's draft PR and records it; no-op if the task already has one.
pub async fn ensure_draft(ctx: &ChannelContext, rc: &RunContext) {
    let result = match refreshed(&ctx.pool, rc).await {
        Ok(current) => try_ensure_draft(ctx, &current).await.map(|_| ()),
        Err(e) => Err(e),
    };
    if let Err(e) = result {
        tracing::warn!(error = %e, task_id = %rc.task.id, "pull request creation failed; retrying on next trigger");
    }
}

/// Returns the number of the task's PR, opening or finding and recording it if `current` (a
/// freshly loaded context) has none.
async fn try_ensure_draft(
    ctx: &ChannelContext,
    current: &RunContext,
) -> Result<Option<i64>, ChannelError> {
    if current.task.pr_number.is_some() {
        return Ok(current.task.pr_number);
    }
    let (head, base) = (current.task_branch(), current.project.base_branch.clone());
    let project = current.project.id;
    let pr = if let Some(pr) = ctx
        .forge
        .find_open_pull_request(&project, &head, &base)
        .await?
    {
        pr
    } else {
        let request = new_pull_request(current, head.clone(), base.clone())?;
        match ctx.forge.create_pull_request(&project, request).await {
            Ok(pr) => pr,
            Err(IntegrationError::Conflict(_)) => ctx
                .forge
                .find_open_pull_request(&project, &head, &base)
                .await?
                .ok_or_else(|| {
                    ChannelError::InvalidData("conflicting pull request vanished".into())
                })?,
            Err(e) => return Err(e.into()),
        }
    };
    record_pr(&ctx.pool, current.task.id, &pr).await?;
    Ok(i64::try_from(pr.id.number).ok())
}

fn new_pull_request(
    rc: &RunContext,
    head: String,
    base: String,
) -> Result<NewPullRequest, ChannelError> {
    let task = &rc.task;
    let default_body = if task.description.trim().is_empty() {
        format!("Task: {}", task.key)
    } else {
        format!("{}\n\nTask: {}", task.description, task.key)
    };
    Ok(NewPullRequest {
        repo: repo_ref(&rc.project.repo_url)?,
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

/// Owner and name from the last two path segments of a repository URL.
pub fn repo_ref(repo_url: &str) -> Result<RepoRef, ChannelError> {
    let path = repo_url.split_once("://").map_or(repo_url, |(_, rest)| {
        rest.split_once('/').map_or("", |(_, p)| p)
    });
    let mut segments = path.trim_matches('/').rsplit('/');
    let name = segments.next().map(|n| n.strip_suffix(".git").unwrap_or(n));
    match (segments.next(), name) {
        (Some(owner), Some(name)) if !owner.is_empty() && !name.is_empty() => Ok(RepoRef {
            owner: owner.to_string(),
            name: name.to_string(),
        }),
        _ => Err(ChannelError::InvalidData(format!(
            "repository URL `{repo_url}` has no owner/name path"
        ))),
    }
}

async fn record_pr(pool: &PgPool, task_id: TaskId, pr: &PullRequest) -> Result<(), ChannelError> {
    let number = i64::try_from(pr.id.number).map_err(|_out_of_range| {
        ChannelError::InvalidData("pull request number out of range".into())
    })?;
    let mut tx = pool.begin().await?;
    let updated = sqlx::query(
        "UPDATE tasks SET pr_number = $2, pr_url = $3, updated_at = now()
         WHERE id = $1 AND pr_number IS NULL",
    )
    .bind(task_id.as_uuid())
    .bind(number)
    .bind(&pr.url)
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() > 0 {
        publish(&mut *tx, &Notification::TaskChanged { task_id }).await?;
    }
    tx.commit().await?;
    Ok(())
}

async fn mark_diverged(pool: &PgPool, task_id: TaskId) -> Result<(), ChannelError> {
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE tasks SET diverged = TRUE, updated_at = now() WHERE id = $1")
        .bind(task_id.as_uuid())
        .execute(&mut *tx)
        .await?;
    publish(&mut *tx, &Notification::TaskChanged { task_id }).await?;
    tx.commit().await?;
    Ok(())
}

async fn completion_summary(pool: &PgPool, task_id: TaskId) -> Result<String, ChannelError> {
    Ok(
        sqlx::query_scalar("SELECT completion_summary FROM tasks WHERE id = $1")
            .bind(task_id.as_uuid())
            .fetch_one(pool)
            .await?,
    )
}

/// The context with the task row as it is now (PR number, stored title and body).
async fn refreshed(pool: &PgPool, rc: &RunContext) -> Result<RunContext, ChannelError> {
    let task = df_db::get_task(pool, rc.task.id)
        .await?
        .ok_or_else(|| ChannelError::InvalidData(format!("task {} is missing", rc.task.id)))?;
    Ok(RunContext { task, ..rc.clone() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assign::load_run_context;
    use crate::runs::get_run;
    use crate::testing::Env;
    use df_core::testing::ForgeCall;
    use df_core::{PullRequestRef, PullRequestState, UpstreamError};
    use sqlx::PgPool;

    async fn setup(pool: PgPool) -> (Env, RunContext) {
        let env = Env::new(pool.clone()).await;
        let run = env.insert_run(env.task.id, 1, "running").await;
        let rc = load_run_context(&pool, get_run(&pool, run).await.unwrap().unwrap())
            .await
            .unwrap();
        (env, rc)
    }

    fn push(ahead: u32, status: PushStatus) -> PushReport {
        PushReport {
            target: PushTarget::TaskBranch as i32,
            status: status as i32,
            commits_ahead_of_base: ahead,
            ..PushReport::default()
        }
    }

    async fn pr_number(pool: &PgPool, task: TaskId) -> Option<i64> {
        sqlx::query_scalar("SELECT pr_number FROM tasks WHERE id = $1")
            .bind(task.as_uuid())
            .fetch_one(pool)
            .await
            .unwrap()
    }

    fn creates(env: &Env) -> Vec<NewPullRequest> {
        env.forge
            .calls()
            .into_iter()
            .filter_map(|c| match c {
                ForgeCall::CreatePullRequest { request, .. } => Some(request),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn repo_ref_takes_owner_and_name() {
        let r = repo_ref("https://git.example.org/peter/dark-factory.git").unwrap();
        assert_eq!(
            (r.owner.as_str(), r.name.as_str()),
            ("peter", "dark-factory")
        );
        assert!(repo_ref("https://git.example.org/").is_err());
    }

    #[tokio::test]
    async fn first_push_ahead_opens_a_draft_with_default_title_and_body() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        on_push_report(&env.ctx, &rc, &push(2, PushStatus::Ok)).await;

        let created = creates(&env);
        assert_eq!(created.len(), 1);
        assert!(created[0].draft);
        assert_eq!(
            (created[0].head.as_str(), created[0].base.as_str()),
            ("factory/task/DF-1", "main")
        );
        assert_eq!(created[0].title, "DF-1: Fix login");
        assert!(created[0].body.ends_with("Task: DF-1"));
        assert_eq!(pr_number(&pool, rc.task.id).await, Some(1));

        // A second push does not open another PR.
        on_push_report(&env.ctx, &rc, &push(3, PushStatus::Ok)).await;
        assert_eq!(creates(&env).len(), 1);
    }

    #[tokio::test]
    async fn push_without_commits_ahead_opens_nothing() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        on_push_report(&env.ctx, &rc, &push(0, PushStatus::Ok)).await;
        assert!(creates(&env).is_empty());
        assert_eq!(pr_number(&pool, rc.task.id).await, None);
    }

    #[tokio::test]
    async fn stored_title_and_body_are_used() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        sqlx::query(
            "UPDATE tasks SET pr_title = 'Better title', pr_body = 'Better body' WHERE id = $1",
        )
        .bind(rc.task.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
        on_push_report(&env.ctx, &rc, &push(1, PushStatus::Ok)).await;
        assert_eq!(creates(&env)[0].title, "Better title");
        assert_eq!(creates(&env)[0].body, "Better body");
    }

    #[tokio::test]
    async fn existing_open_pr_is_recorded_instead_of_creating_a_second() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        env.forge
            .push_find_open_pull_request_result(Ok(Some(pull_request(7))));
        on_push_report(&env.ctx, &rc, &push(1, PushStatus::Ok)).await;
        assert!(creates(&env).is_empty());
        assert_eq!(pr_number(&pool, rc.task.id).await, Some(7));
    }

    #[tokio::test]
    async fn creation_conflict_records_the_existing_pr() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        env.forge.push_find_open_pull_request_result(Ok(None));
        env.forge
            .push_create_pull_request_result(Err(IntegrationError::Conflict("exists".into())));
        env.forge
            .push_find_open_pull_request_result(Ok(Some(pull_request(9))));
        on_push_report(&env.ctx, &rc, &push(1, PushStatus::Ok)).await;
        assert_eq!(pr_number(&pool, rc.task.id).await, Some(9));
    }

    #[tokio::test]
    async fn failed_creation_is_retried_on_the_next_push() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        env.forge.push_find_open_pull_request_result(Ok(None));
        env.forge
            .push_create_pull_request_result(Err(IntegrationError::Upstream(UpstreamError {
                status: 502,
                message: "bad gateway".into(),
            })));
        on_push_report(&env.ctx, &rc, &push(1, PushStatus::Ok)).await;
        assert_eq!(pr_number(&pool, rc.task.id).await, None);

        on_push_report(&env.ctx, &rc, &push(1, PushStatus::Ok)).await;
        assert!(pr_number(&pool, rc.task.id).await.is_some());
    }

    #[tokio::test]
    async fn completed_run_marks_the_pr_ready_and_posts_the_summary() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        on_push_report(&env.ctx, &rc, &push(1, PushStatus::Ok)).await;
        sqlx::query("UPDATE tasks SET completion_summary = 'All done' WHERE id = $1")
            .bind(rc.task.id.as_uuid())
            .execute(&pool)
            .await
            .unwrap();

        on_run_end(&env.ctx, &rc, true, None).await;

        let calls = env.forge.calls();
        assert!(calls.iter().any(|c| matches!(c,
            ForgeCall::UpdatePullRequest { number: 1, changes, .. } if changes.draft == Some(false))));
        assert!(calls.iter().any(|c| matches!(c,
            ForgeCall::AddIssueComment { number: 1, body, .. } if body == "All done")));
    }

    #[tokio::test]
    async fn summary_is_posted_even_if_marking_the_pr_ready_failed() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        on_push_report(&env.ctx, &rc, &push(1, PushStatus::Ok)).await;
        sqlx::query("UPDATE tasks SET completion_summary = 'All done' WHERE id = $1")
            .bind(rc.task.id.as_uuid())
            .execute(&pool)
            .await
            .unwrap();
        env.forge
            .push_update_pull_request_result(Err(IntegrationError::Upstream(UpstreamError {
                status: 502,
                message: "bad gateway".into(),
            })));

        on_run_end(&env.ctx, &rc, true, None).await;

        assert!(env.forge.calls().iter().any(|c| matches!(c,
            ForgeCall::AddIssueComment { number: 1, body, .. } if body == "All done")));
    }

    #[tokio::test]
    async fn run_end_retries_a_missing_pr_when_the_branch_was_pushed() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        let workspace = WorkspaceState {
            task_branch_pushed: true,
            ..WorkspaceState::default()
        };
        on_run_end(&env.ctx, &rc, false, Some(&workspace)).await;
        assert_eq!(creates(&env).len(), 1);
    }

    #[tokio::test]
    async fn non_fast_forward_push_flags_divergence() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        on_push_report(&env.ctx, &rc, &push(0, PushStatus::RejectedNonFastForward)).await;
        let diverged: bool = sqlx::query_scalar("SELECT diverged FROM tasks WHERE id = $1")
            .bind(rc.task.id.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(diverged);
    }

    fn pull_request(number: u64) -> PullRequest {
        PullRequest {
            id: PullRequestRef {
                repo: RepoRef {
                    owner: "peter".into(),
                    name: "dark-factory".into(),
                },
                number,
            },
            url: format!("https://git.example.org/peter/dark-factory/pulls/{number}"),
            head: "factory/task/DF-1".into(),
            base: "main".into(),
            title: "t".into(),
            body: "b".into(),
            draft: true,
            state: PullRequestState::Open,
            head_sha: String::new(),
        }
    }
}
