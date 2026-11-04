//! Pull request lifecycle: draft PR on the first push ahead of base, ready for review on
//! completion, divergence flag (`specs/pull-request-lifecycle/spec.md`, design D7).
//!
//! All functions are best effort towards the forge: a failure is logged and never fails the run.
//! Opening the PR is retried by the next trigger (push report or run end); at the end of a
//! completed run there is no next trigger, so each step there runs even if the one before failed.

use df_core::{IntegrationError, NewPullRequest, PullRequest, RepoRef, TaskId, TaskStatus};
use df_db::transcript::{append_control_event, forge_event, lock_run_and_task};
use df_db::{Notification, publish};
use df_proto::agent::v1::{PushReport, PushStatus, PushTarget, WorkspaceState};
use df_proto::transcript::v1::{
    ForgeActivity, PullRequestOpened, PullRequestReady, forge_activity,
};
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
    let mut pr = match RecordedPr::of_task(&current.task) {
        Ok(pr) => pr,
        Err(e) => {
            tracing::warn!(error = %e, task_id = %rc.task.id, "task has an invalid pull request number");
            return;
        }
    };
    if pr.is_none() && (pushed || completed) {
        pr = try_ensure_draft(ctx, &current).await.unwrap_or_else(|e| {
            tracing::warn!(error = %e, task_id = %rc.task.id, "pull request creation failed; retrying on next trigger");
            None
        });
    }
    // A cancelled task keeps its PR as it is: neither ready nor a summary.
    if !completed || current.task.status == TaskStatus::Cancelled {
        return;
    }
    let Some(RecordedPr { number, url }) = pr else {
        return;
    };
    match ctx
        .forge
        .mark_pull_request_ready(&rc.project.id, number)
        .await
    {
        Ok(_) => {
            if let Err(e) = record_ready(&ctx.pool, rc, number, url).await {
                tracing::warn!(error = %e, task_id = %rc.task.id, "failed to record the ready pull request");
            }
        }
        // A completed run has no later trigger: still post the summary.
        Err(e) => {
            tracing::warn!(error = %e, task_id = %rc.task.id, "failed to mark the pull request ready");
        }
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

/// The pull request recorded on a task.
struct RecordedPr {
    number: u64,
    /// Empty if the task row has none.
    url: String,
}

impl RecordedPr {
    /// The PR of the task row, if it has one. A negative stored number is `InvalidData`.
    fn of_task(task: &df_db::TaskRow) -> Result<Option<Self>, ChannelError> {
        task.pr_number
            .map(|number| Self::from_row(number, task.pr_url.clone()))
            .transpose()
    }

    fn from_row(number: i64, url: Option<String>) -> Result<Self, ChannelError> {
        let number = u64::try_from(number).map_err(|_negative| {
            ChannelError::InvalidData("stored pull request number out of range".into())
        })?;
        Ok(Self {
            number,
            url: url.unwrap_or_default(),
        })
    }
}

/// Returns the task's PR, opening or finding and recording it if `current` (a freshly loaded
/// context) has none.
async fn try_ensure_draft(
    ctx: &ChannelContext,
    current: &RunContext,
) -> Result<Option<RecordedPr>, ChannelError> {
    if let Some(recorded) = RecordedPr::of_task(&current.task)? {
        return Ok(Some(recorded));
    }
    // Serialize per task, also across control-plane replicas: the lock is held until the PR is
    // recorded, so a second caller sees the PR the first one opened. It is released when `tx`
    // ends, on every path including a dropped future. The hold is bounded by the forge calls'
    // plugin timeouts; a waiting caller holds one pooled connection meanwhile.
    let mut tx = ctx.pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("task-pr:{}", current.task.id))
        .execute(&mut *tx)
        .await?;
    let (recorded, url, status): (Option<i64>, Option<String>, String) =
        sqlx::query_as("SELECT pr_number, pr_url, status FROM tasks WHERE id = $1")
            .bind(current.task.id.as_uuid())
            .fetch_one(&mut *tx)
            .await?;
    if let Some(number) = recorded {
        return RecordedPr::from_row(number, url).map(Some);
    }
    let (head, base) = (current.task_branch(), current.project.base_branch.clone());
    let project = current.project.id;
    let existing = ctx
        .forge
        .find_open_pull_request(&project, &head, &base)
        .await?;
    // A board member marked the task done or cancelled it without a PR: a push of its stopped
    // run or the retry at run end must not open one. A PR opened before (its recording was
    // lost) is still recorded.
    let closed = matches!(
        status.parse::<TaskStatus>().map_err(df_db::DbError::from)?,
        TaskStatus::Completed | TaskStatus::Cancelled
    );
    let pr = if let Some(pr) = existing {
        pr
    } else if closed {
        return Ok(None);
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
    record_pr(&mut tx, current, &pr).await?;
    tx.commit().await?;
    Ok(Some(RecordedPr {
        number: pr.id.number,
        url: pr.url,
    }))
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

/// Records `pr` on the task and, if it was not recorded yet, writes the forge activity to the
/// transcript of the run that triggered it. Takes the run and task locks before the task update
/// (design D3), the order of the runtime's batch writer.
async fn record_pr(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    rc: &RunContext,
    pr: &PullRequest,
) -> Result<(), ChannelError> {
    let task_id = rc.task.id;
    let number = i64::try_from(pr.id.number).map_err(|_out_of_range| {
        ChannelError::InvalidData("pull request number out of range".into())
    })?;
    let lock = lock_run_and_task(tx, rc.run.id, task_id).await?;
    let updated = sqlx::query(
        "UPDATE tasks SET pr_number = $2, pr_url = $3, updated_at = now()
         WHERE id = $1 AND pr_number IS NULL",
    )
    .bind(task_id.as_uuid())
    .bind(number)
    .bind(&pr.url)
    .execute(&mut **tx)
    .await?;
    if updated.rows_affected() > 0 {
        publish(&mut **tx, &Notification::TaskChanged { task_id }).await?;
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
    Ok(())
}

/// Writes the "ready for review" forge activity of `number` to the run's transcript, once.
async fn record_ready(
    pool: &PgPool,
    rc: &RunContext,
    number: u64,
    url: String,
) -> Result<(), ChannelError> {
    let ready = forge_event(ForgeActivity {
        kind: Some(forge_activity::Kind::PullRequestReady(PullRequestReady {
            number,
            url,
        })),
    });
    let mut tx = pool.begin().await?;
    let lock = lock_run_and_task(&mut tx, rc.run.id, rc.task.id).await?;
    append_control_event(&mut tx, &lock, &format!("pr-ready:{number}"), &ready).await?;
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
    fn a_negative_stored_pull_request_number_is_invalid_data() {
        let error = RecordedPr::from_row(-1, None).err().unwrap();
        assert!(matches!(error, ChannelError::InvalidData(_)));
        assert_eq!(RecordedPr::from_row(7, None).unwrap().number, 7);
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
    async fn no_pull_request_is_opened_for_a_task_marked_done() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        sqlx::query("UPDATE tasks SET status = 'completed' WHERE id = $1")
            .bind(rc.task.id.as_uuid())
            .execute(&pool)
            .await
            .unwrap();

        on_push_report(&env.ctx, &rc, &push(2, PushStatus::Ok)).await;
        on_run_end(&env.ctx, &rc, false, Some(&pushed_workspace())).await;

        assert!(creates(&env).is_empty());
        assert_eq!(pr_number(&pool, rc.task.id).await, None);
    }

    #[tokio::test]
    async fn no_pull_request_is_opened_for_a_cancelled_task() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        sqlx::query("UPDATE tasks SET status = 'cancelled' WHERE id = $1")
            .bind(rc.task.id.as_uuid())
            .execute(&pool)
            .await
            .unwrap();

        on_push_report(&env.ctx, &rc, &push(2, PushStatus::Ok)).await;
        on_run_end(&env.ctx, &rc, true, Some(&pushed_workspace())).await;

        assert!(creates(&env).is_empty());
        assert_eq!(pr_number(&pool, rc.task.id).await, None);
    }

    #[tokio::test]
    async fn an_existing_pr_of_a_cancelled_task_is_recorded_without_creating_one() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        sqlx::query("UPDATE tasks SET status = 'cancelled' WHERE id = $1")
            .bind(rc.task.id.as_uuid())
            .execute(&pool)
            .await
            .unwrap();
        env.forge
            .push_find_open_pull_request_result(Ok(Some(pull_request(7))));

        on_push_report(&env.ctx, &rc, &push(2, PushStatus::Ok)).await;

        assert!(creates(&env).is_empty());
        assert_eq!(pr_number(&pool, rc.task.id).await, Some(7));
    }

    #[tokio::test]
    async fn completed_run_of_a_cancelled_task_leaves_the_pr_draft_and_posts_no_comment() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        on_push_report(&env.ctx, &rc, &push(1, PushStatus::Ok)).await;
        sqlx::query(
            "UPDATE tasks SET status = 'cancelled', completion_summary = 'All done' WHERE id = $1",
        )
        .bind(rc.task.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

        on_run_end(&env.ctx, &rc, true, None).await;

        assert!(
            !env.forge.calls().iter().any(|c| matches!(
                c,
                ForgeCall::UpdatePullRequest { .. } | ForgeCall::AddIssueComment { .. }
            )),
            "{:?}",
            env.forge.calls()
        );
    }

    fn pushed_workspace() -> WorkspaceState {
        WorkspaceState {
            task_branch_pushed: true,
            ..WorkspaceState::default()
        }
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

    /// Returns once two PR lookups are in flight or an advisory lock of this database is queued.
    async fn wait_for_overlap(forge: &df_core::testing::FakeForgeClient, pool: &PgPool) {
        loop {
            let finds = forge
                .calls()
                .iter()
                .filter(|c| matches!(c, ForgeCall::FindOpenPullRequest { .. }))
                .count();
            let queued: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND NOT granted \
                 AND database = (SELECT oid FROM pg_database WHERE datname = current_database())",
            )
            .fetch_one(pool)
            .await
            .unwrap();
            if finds >= 2 || queued > 0 {
                return;
            }
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test]
    async fn concurrent_push_report_and_run_end_open_one_pull_request() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        let gate = env.forge.hold_find_open_pull_request();
        let workspace = WorkspaceState {
            task_branch_pushed: true,
            ..WorkspaceState::default()
        };
        let (ctx_a, rc_a) = (env.ctx.clone(), rc.clone());
        let push_report =
            tokio::spawn(
                async move { on_push_report(&ctx_a, &rc_a, &push(1, PushStatus::Ok)).await },
            );
        let (ctx_b, rc_b) = (env.ctx.clone(), rc.clone());
        let run_end =
            tokio::spawn(async move { on_run_end(&ctx_b, &rc_b, true, Some(&workspace)).await });

        // Wait until both are inside the PR lookup, or one of them is queued behind the other on
        // an advisory lock of this test's own database (the PostgreSQL server is shared); only
        // then let the forge answer.
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            wait_for_overlap(&env.forge, &pool),
        )
        .await
        .expect("neither two PR lookups nor a queued advisory lock showed up within 10 s");
        // Enough permits for every lookup either caller can make.
        gate.add_permits(8);
        push_report.await.unwrap();
        run_end.await.unwrap();

        assert_eq!(creates(&env).len(), 1);
        assert_eq!(pr_number(&pool, rc.task.id).await, Some(1));
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

    /// `(run, source_key, forgeActivity payload)` of every forge activity row, oldest first.
    async fn forge_events(pool: &PgPool) -> Vec<(uuid::Uuid, String, serde_json::Value)> {
        sqlx::query_as(
            "SELECT run_id, source_key, payload -> 'forgeActivity'
             FROM transcript_events WHERE kind = 'forge_activity' ORDER BY id",
        )
        .fetch_all(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn first_push_ahead_writes_one_pull_request_opened_event_in_its_run() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;

        on_push_report(&env.ctx, &rc, &push(1, PushStatus::Ok)).await;
        on_push_report(&env.ctx, &rc, &push(2, PushStatus::Ok)).await;

        let events = forge_events(&pool).await;
        assert_eq!(events.len(), 1);
        let (run, key, payload) = &events[0];
        assert_eq!(*run, rc.run.id.as_uuid());
        assert_eq!(key, "pr-opened:1");
        let opened = &payload["pullRequestOpened"];
        assert_eq!(opened["number"], "1");
        assert_eq!(opened["draft"], true);
        assert!(opened["url"].as_str().unwrap().ends_with("/1"));
    }

    #[tokio::test]
    async fn an_existing_open_pr_that_is_recorded_writes_one_event() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        env.forge
            .push_find_open_pull_request_result(Ok(Some(pull_request(7))));

        on_push_report(&env.ctx, &rc, &push(1, PushStatus::Ok)).await;

        let events = forge_events(&pool).await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1, "pr-opened:7");
    }

    #[tokio::test]
    async fn concurrent_push_report_and_run_end_write_one_opened_event() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        let workspace = WorkspaceState {
            task_branch_pushed: true,
            ..WorkspaceState::default()
        };

        let report = push(1, PushStatus::Ok);
        tokio::join!(
            on_push_report(&env.ctx, &rc, &report),
            on_run_end(&env.ctx, &rc, false, Some(&workspace)),
        );

        assert_eq!(forge_events(&pool).await.len(), 1);
    }

    #[tokio::test]
    async fn completed_run_writes_a_ready_event_with_the_url_of_a_pr_opened_at_run_end() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;

        // No push report: the PR is opened at run end and then marked ready.
        on_run_end(&env.ctx, &rc, true, Some(&pushed_workspace())).await;

        let events = forge_events(&pool).await;
        let keys: Vec<&str> = events.iter().map(|e| e.1.as_str()).collect();
        assert_eq!(keys, ["pr-opened:1", "pr-ready:1"]);
        let ready = &events[1].2["pullRequestReady"];
        assert_eq!(ready["number"], "1");
        assert!(ready["url"].as_str().unwrap().ends_with("/1"));
    }

    #[tokio::test]
    async fn marking_the_pr_ready_twice_in_one_run_writes_one_event() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        on_push_report(&env.ctx, &rc, &push(1, PushStatus::Ok)).await;
        on_run_end(&env.ctx, &rc, true, None).await;
        on_run_end(&env.ctx, &rc, true, None).await;

        let ready = forge_events(&pool)
            .await
            .into_iter()
            .filter(|e| e.1 == "pr-ready:1")
            .count();
        assert_eq!(ready, 1, "once per run and PR");
    }

    #[tokio::test]
    async fn each_completed_run_that_marks_the_pr_ready_gets_its_own_ready_event() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc1) = setup(pool.clone()).await;
        on_push_report(&env.ctx, &rc1, &push(1, PushStatus::Ok)).await;
        let mut runs = vec![rc1.run.id];
        let mut contexts = Vec::new();
        for number in [2, 4] {
            sqlx::query("UPDATE runs SET state = 'succeeded' WHERE task_id = $1")
                .bind(rc1.task.id.as_uuid())
                .execute(&pool)
                .await
                .unwrap();
            let run = env.insert_run(env.task.id, number, "running").await;
            let row = get_run(&pool, run).await.unwrap().unwrap();
            contexts.push(load_run_context(&pool, row).await.unwrap());
            runs.push(run);
            on_run_end(&env.ctx, contexts.last().unwrap(), true, None).await;
        }

        let ready: Vec<uuid::Uuid> = forge_events(&pool)
            .await
            .into_iter()
            .filter(|e| e.1 == "pr-ready:1")
            .map(|e| e.0)
            .collect();
        assert_eq!(ready, [runs[1].as_uuid(), runs[2].as_uuid()]);
    }

    #[tokio::test]
    async fn failed_ready_update_writes_no_ready_event() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        on_push_report(&env.ctx, &rc, &push(1, PushStatus::Ok)).await;
        env.forge
            .push_update_pull_request_result(Err(IntegrationError::Upstream(UpstreamError {
                status: 502,
                message: "bad gateway".into(),
            })));

        on_run_end(&env.ctx, &rc, true, None).await;

        let events = forge_events(&pool).await;
        assert!(events.iter().all(|e| e.2.get("pullRequestReady").is_none()));
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
