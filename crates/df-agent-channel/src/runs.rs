//! Run rows: lookup, creation for queued tasks, state changes and the run-end mapping to task
//! status (design D2, D4).

use chrono::{DateTime, Utc};
use df_core::{RunId, RunOutcome, RunState, TaskId, TaskStatus};
use df_db::{Notification, publish};
use sqlx::{FromRow, PgPool, Postgres, Transaction};

use crate::ChannelContext;
use crate::error::ChannelError;

// sqlx only accepts literal SQL, so shared fragments are macros expanded by `concat!`.
macro_rules! terminal_states {
    () => {
        "('succeeded', 'failed', 'cancelled')"
    };
}

macro_rules! run_columns {
    () => {
        "id, task_id, number, state, outcome, detail, pod_ip, last_persisted_seq, \
         harness_session_id, started_at, connected_at"
    };
}

/// A row of `runs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRow {
    pub id: RunId,
    pub task_id: TaskId,
    pub number: i64,
    pub state: RunState,
    pub outcome: Option<RunOutcome>,
    pub detail: String,
    pub pod_ip: Option<String>,
    pub last_persisted_seq: u64,
    pub harness_session_id: String,
    pub started_at: DateTime<Utc>,
    /// First `Hello` of the run's runtime; `None` until it connected.
    pub connected_at: Option<DateTime<Utc>>,
}

impl RunRow {
    /// Start of the wall-clock budget: when the runtime connected, else when the run was created.
    pub fn budget_start(&self) -> DateTime<Utc> {
        self.connected_at.unwrap_or(self.started_at)
    }
}

#[derive(FromRow)]
struct RunRecord {
    id: uuid::Uuid,
    task_id: uuid::Uuid,
    number: i64,
    state: String,
    outcome: Option<String>,
    detail: String,
    pod_ip: Option<String>,
    last_persisted_seq: i64,
    harness_session_id: String,
    started_at: DateTime<Utc>,
    connected_at: Option<DateTime<Utc>>,
}

impl TryFrom<RunRecord> for RunRow {
    type Error = ChannelError;

    fn try_from(r: RunRecord) -> Result<Self, ChannelError> {
        Ok(Self {
            id: RunId::from_uuid(r.id),
            task_id: TaskId::from_uuid(r.task_id),
            number: r.number,
            state: r.state.parse()?,
            outcome: r.outcome.map(|o| o.parse()).transpose()?,
            detail: r.detail,
            pod_ip: r.pod_ip,
            last_persisted_seq: u64::try_from(r.last_persisted_seq)
                .map_err(|_| ChannelError::InvalidData("negative last_persisted_seq".into()))?,
            harness_session_id: r.harness_session_id,
            started_at: r.started_at,
            connected_at: r.connected_at,
        })
    }
}

/// Loads a run by id.
pub async fn get_run(pool: &PgPool, run_id: RunId) -> Result<Option<RunRow>, ChannelError> {
    sqlx::query_as::<_, RunRecord>(concat!(
        "SELECT ",
        run_columns!(),
        " FROM runs WHERE id = $1"
    ))
    .bind(run_id.as_uuid())
    .fetch_optional(pool)
    .await?
    .map(RunRow::try_from)
    .transpose()
}

/// Loads all non-terminal runs; rows that cannot be read are logged and skipped.
pub async fn list_active_runs(pool: &PgPool) -> Result<Vec<RunRow>, ChannelError> {
    Ok(sqlx::query_as::<_, RunRecord>(concat!(
        "SELECT ",
        run_columns!(),
        " FROM runs WHERE state NOT IN ",
        terminal_states!(),
        " ORDER BY started_at"
    ))
    .fetch_all(pool)
    .await?
    .into_iter()
    .filter_map(|record| {
        let id = record.id;
        RunRow::try_from(record)
            .inspect_err(
                |e| tracing::error!(error = %e, run_id = %id, "skipping unreadable run row"),
            )
            .ok()
    })
    .collect())
}

/// Creates a `pending` run for up to `limit` queued tasks without an active run and sets those
/// tasks to `running`. Concurrent callers never pick the same task (`FOR UPDATE SKIP LOCKED`; the
/// partial unique index on `runs` is the final guard).
pub async fn start_queued_runs(pool: &PgPool, limit: i64) -> Result<Vec<RunRow>, ChannelError> {
    let mut tx = pool.begin().await?;
    let task_ids: Vec<uuid::Uuid> = sqlx::query_scalar(concat!(
        "SELECT t.id FROM tasks t
         WHERE t.status = 'queued'
           AND NOT EXISTS (
               SELECT 1 FROM runs r WHERE r.task_id = t.id AND r.state NOT IN ",
        terminal_states!(),
        ")
         ORDER BY t.created_at
         LIMIT $1
         FOR UPDATE OF t SKIP LOCKED"
    ))
    .bind(limit)
    .fetch_all(&mut *tx)
    .await?;

    let mut started = Vec::with_capacity(task_ids.len());
    for task_id in task_ids {
        let record = sqlx::query_as::<_, RunRecord>(concat!(
            "INSERT INTO runs (id, task_id, number)
             VALUES ($1, $2, (SELECT COALESCE(MAX(number), 0) + 1 FROM runs WHERE task_id = $2))
             RETURNING ",
            run_columns!()
        ))
        .bind(RunId::new().as_uuid())
        .bind(task_id)
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query("UPDATE tasks SET status = 'running', updated_at = now() WHERE id = $1")
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
        let run = RunRow::try_from(record)?;
        notify_run(&mut tx, &run).await?;
        publish(
            &mut *tx,
            &Notification::TaskChanged {
                task_id: run.task_id,
            },
        )
        .await?;
        started.push(run);
    }
    tx.commit().await?;
    Ok(started)
}

/// Moves a non-terminal run to `state` (and optionally records the pod IP). Returns whether the
/// run was changed.
pub async fn set_run_state(
    pool: &PgPool,
    run_id: RunId,
    state: RunState,
    pod_ip: Option<&str>,
) -> Result<bool, ChannelError> {
    let mut tx = pool.begin().await?;
    let changed = set_run_state_tx(&mut tx, run_id, state, pod_ip).await?;
    tx.commit().await?;
    Ok(changed)
}

/// Transactional variant of [`set_run_state`].
pub async fn set_run_state_tx(
    tx: &mut Transaction<'_, Postgres>,
    run_id: RunId,
    state: RunState,
    pod_ip: Option<&str>,
) -> Result<bool, ChannelError> {
    let task_id: Option<uuid::Uuid> = sqlx::query_scalar(concat!(
        "UPDATE runs SET state = $2, pod_ip = COALESCE($3, pod_ip)
         WHERE id = $1 AND state NOT IN ",
        terminal_states!(),
        " AND state <> $2
         RETURNING task_id"
    ))
    .bind(run_id.as_uuid())
    .bind(state.as_str())
    .bind(pod_ip)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(task_id) = task_id else {
        return Ok(false);
    };
    publish(
        &mut **tx,
        &Notification::RunChanged {
            run_id,
            task_id: TaskId::from_uuid(task_id),
        },
    )
    .await?;
    Ok(true)
}

/// Applies the runtime's idle phases: `idle` moves the run to `awaiting_input` and the task to
/// `waiting` (unless a message is undelivered, then the task stays `running` and the session
/// forwards it), otherwise back to `running` for both. A `finalizing` or terminal run is never
/// moved: once the control plane decided to end it, the runtime cannot reopen it.
///
/// The caller holds the task row lock (see `queue_message`).
pub async fn set_idle_tx(
    tx: &mut Transaction<'_, Postgres>,
    run_id: RunId,
    task_id: TaskId,
    idle: bool,
) -> Result<(), ChannelError> {
    let state = if idle {
        RunState::AwaitingInput
    } else {
        RunState::Running
    };
    let changed: Option<uuid::Uuid> = sqlx::query_scalar(
        "UPDATE runs SET state = $2
         WHERE id = $1 AND state NOT IN ('finalizing', 'succeeded', 'failed', 'cancelled')
           AND state <> $2
         RETURNING task_id",
    )
    .bind(run_id.as_uuid())
    .bind(state.as_str())
    .fetch_optional(&mut **tx)
    .await?;
    if changed.is_none() {
        return Ok(());
    }
    // `waiting` -> `running` is the idle task being woken by the next turn.
    let (from, to) = if idle {
        (TaskStatus::Running, TaskStatus::Waiting)
    } else {
        (TaskStatus::Waiting, TaskStatus::Running)
    };
    sqlx::query(
        "UPDATE tasks SET status = $3, updated_at = now()
         WHERE id = $1 AND status = $2
           AND ($4 = false OR NOT EXISTS (
               SELECT 1 FROM task_messages WHERE task_id = $1 AND delivered_at IS NULL))",
    )
    .bind(task_id.as_uuid())
    .bind(from.as_str())
    .bind(to.as_str())
    .bind(idle)
    .execute(&mut **tx)
    .await?;
    publish(&mut **tx, &Notification::RunChanged { run_id, task_id }).await?;
    publish(&mut **tx, &Notification::TaskChanged { task_id }).await?;
    Ok(())
}

/// Final state of a run, from a `RunReport` or from the control plane's own failure handling.
#[derive(Debug, Clone)]
pub struct RunEnd {
    pub run_id: RunId,
    pub outcome: RunOutcome,
    pub detail: String,
    pub retry_after: Option<DateTime<Utc>>,
    pub harness_session_id: String,
    pub task_branch_sha: String,
    pub wip_sha: String,
    pub diverged: bool,
}

impl RunEnd {
    /// An end caused by the control plane (timeout, lost or failed sandbox).
    pub fn failure(run_id: RunId, outcome: RunOutcome, detail: impl Into<String>) -> Self {
        Self {
            run_id,
            outcome,
            detail: detail.into(),
            retry_after: None,
            harness_session_id: String::new(),
            task_branch_sha: String::new(),
            wip_sha: String::new(),
            diverged: false,
        }
    }
}

/// Result of [`end_run`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndedRun {
    pub task_id: TaskId,
    pub run_state: RunState,
    pub task_status: TaskStatus,
}

/// Run state for a finished run (design D4).
pub const fn run_state_for(outcome: RunOutcome) -> RunState {
    match outcome {
        RunOutcome::Completed
        | RunOutcome::EndTurn
        | RunOutcome::Blocked
        | RunOutcome::LimitReached
        | RunOutcome::RateLimited => RunState::Succeeded,
        RunOutcome::Cancelled => RunState::Cancelled,
        RunOutcome::Failed
        | RunOutcome::ProvisioningFailed
        | RunOutcome::ConnectTimeout
        | RunOutcome::SandboxLost => RunState::Failed,
    }
}

/// Task status for a finished run, before the "undelivered prompts" override. `completed` is
/// reserved for a human (mark done), so the agent's `completed` outcome puts the task in review.
pub const fn task_status_for(outcome: RunOutcome) -> TaskStatus {
    match outcome {
        RunOutcome::Completed => TaskStatus::InReview,
        RunOutcome::EndTurn | RunOutcome::Cancelled => TaskStatus::Waiting,
        RunOutcome::Blocked => TaskStatus::Blocked,
        RunOutcome::LimitReached => TaskStatus::LimitReached,
        RunOutcome::RateLimited => TaskStatus::RateLimited,
        RunOutcome::Failed
        | RunOutcome::ProvisioningFailed
        | RunOutcome::ConnectTimeout
        | RunOutcome::SandboxLost => TaskStatus::Failed,
    }
}

const fn keeps_status_on_pending_prompts(outcome: RunOutcome) -> bool {
    matches!(
        outcome,
        RunOutcome::Failed
            | RunOutcome::ProvisioningFailed
            | RunOutcome::ConnectTimeout
            | RunOutcome::SandboxLost
            | RunOutcome::RateLimited
    )
}

/// Locks the task row, the lock `queue_message` takes. What the caller decides afterwards cannot
/// miss a message queued concurrently.
///
/// Lock order: run rows first, then the task row. A transaction that needs both locks its
/// run row(s) before calling this (`persist_batch`, `end_run_tx`, `Session::start_finalizing`,
/// `df_db::mark_task_done`), otherwise two of them can deadlock.
pub async fn lock_task(
    tx: &mut Transaction<'_, Postgres>,
    task_id: TaskId,
) -> Result<(), ChannelError> {
    sqlx::query("SELECT 1 FROM tasks WHERE id = $1 FOR UPDATE")
        .bind(task_id.as_uuid())
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Whether a message of the task is still undelivered.
pub async fn has_undelivered_message(
    tx: &mut Transaction<'_, Postgres>,
    task_id: TaskId,
) -> Result<bool, ChannelError> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM task_messages WHERE task_id = $1 AND delivered_at IS NULL)",
    )
    .bind(task_id.as_uuid())
    .fetch_one(&mut **tx)
    .await?)
}

/// Ends a non-terminal run and sets the task status. Returns `None` if the run was already
/// terminal (another replica or an earlier report ended it). The caller tears the sandbox down.
pub async fn end_run(pool: &PgPool, end: &RunEnd) -> Result<Option<EndedRun>, ChannelError> {
    let mut tx = pool.begin().await?;
    let ended = end_run_tx(&mut tx, end).await?;
    tx.commit().await?;
    Ok(ended)
}

/// Transactional variant of [`end_run`].
pub async fn end_run_tx(
    tx: &mut Transaction<'_, Postgres>,
    end: &RunEnd,
) -> Result<Option<EndedRun>, ChannelError> {
    let run_state = run_state_for(end.outcome);
    let task_id: Option<uuid::Uuid> = sqlx::query_scalar(concat!(
        "UPDATE runs SET state = $2, outcome = $3, detail = $4, retry_after = $5,
             harness_session_id = CASE WHEN $6 <> '' THEN $6 ELSE harness_session_id END,
             task_branch_sha = $7, wip_sha = $8, ended_at = now()
         WHERE id = $1 AND state NOT IN ",
        terminal_states!(),
        "
         RETURNING task_id"
    ))
    .bind(end.run_id.as_uuid())
    .bind(run_state.as_str())
    .bind(end.outcome.as_str())
    .bind(&end.detail)
    .bind(end.retry_after)
    .bind(&end.harness_session_id)
    .bind(&end.task_branch_sha)
    .bind(&end.wip_sha)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(task_id) = task_id else {
        return Ok(None);
    };
    let task_id = TaskId::from_uuid(task_id);

    // `queue_message` locks the task row before it inserts. Waiting for that lock makes a message
    // queued concurrently visible below, instead of the task becoming `waiting` with it stranded.
    lock_task(tx, task_id).await?;
    let pending_prompts = has_undelivered_message(tx, task_id).await?;
    let current: TaskStatus =
        sqlx::query_scalar::<_, String>("SELECT status FROM tasks WHERE id = $1")
            .bind(task_id.as_uuid())
            .fetch_one(&mut **tx)
            .await?
            .parse()?;
    let task_status = if pending_prompts && !keeps_status_on_pending_prompts(end.outcome) {
        TaskStatus::Queued
    } else if current == TaskStatus::Completed {
        // A human marked the task done; the end of its run must not reopen it.
        TaskStatus::Completed
    } else {
        task_status_for(end.outcome)
    };

    sqlx::query(
        "UPDATE tasks SET status = $2,
             harness_session_id = CASE WHEN $3 <> '' THEN $3 ELSE harness_session_id END,
             diverged = diverged OR $4, updated_at = now()
         WHERE id = $1",
    )
    .bind(task_id.as_uuid())
    .bind(task_status.as_str())
    .bind(&end.harness_session_id)
    .bind(end.diverged)
    .execute(&mut **tx)
    .await?;

    publish(
        &mut **tx,
        &Notification::RunChanged {
            run_id: end.run_id,
            task_id,
        },
    )
    .await?;
    publish(&mut **tx, &Notification::TaskChanged { task_id }).await?;
    if task_status == TaskStatus::Queued {
        publish(&mut **tx, &Notification::TaskMessageQueued { task_id }).await?;
    }
    Ok(Some(EndedRun {
        task_id,
        run_state,
        task_status,
    }))
}

/// Records that the sandbox of `run_id` is up at `pod_ip`: `pending`/`provisioning` → `connecting`.
pub async fn mark_started(
    pool: &PgPool,
    run_id: RunId,
    pod_ip: &str,
) -> Result<bool, ChannelError> {
    let mut tx = pool.begin().await?;
    let task_id: Option<uuid::Uuid> = sqlx::query_scalar(
        "UPDATE runs SET state = 'connecting', pod_ip = $2
         WHERE id = $1 AND state IN ('pending', 'provisioning')
         RETURNING task_id",
    )
    .bind(run_id.as_uuid())
    .bind(pod_ip)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(task_id) = task_id else {
        return Ok(false);
    };
    publish(
        &mut *tx,
        &Notification::RunChanged {
            run_id,
            task_id: TaskId::from_uuid(task_id),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(true)
}

/// Records the runtime's first connect and moves an early run to `preparing`. Returns the run as
/// it is now, `None` if it is already terminal.
pub async fn mark_connected(pool: &PgPool, run_id: RunId) -> Result<Option<RunRow>, ChannelError> {
    let record = sqlx::query_as::<_, RunRecord>(concat!(
        "UPDATE runs SET connected_at = COALESCE(connected_at, now()),
             state = CASE WHEN state IN ('pending', 'provisioning', 'connecting')
                          THEN 'preparing' ELSE state END
         WHERE id = $1 AND state NOT IN ",
        terminal_states!(),
        " RETURNING ",
        run_columns!()
    ))
    .bind(run_id.as_uuid())
    .fetch_optional(pool)
    .await?;
    let Some(record) = record else {
        return Ok(None);
    };
    let run = RunRow::try_from(record)?;
    publish(
        pool,
        &Notification::RunChanged {
            run_id,
            task_id: run.task_id,
        },
    )
    .await?;
    Ok(Some(run))
}

/// Ends the run and, if this call ended it, deletes its sandbox. The task volume is kept.
pub async fn finish_run(
    ctx: &ChannelContext,
    end: &RunEnd,
) -> Result<Option<EndedRun>, ChannelError> {
    let ended = end_run(&ctx.pool, end).await?;
    if ended.is_some() {
        teardown(ctx, end.run_id).await;
    }
    Ok(ended)
}

/// Deletes the sandbox of `run_id`; a failure is logged (the provider reports leftovers as `Gone`).
pub async fn teardown(ctx: &ChannelContext, run_id: RunId) {
    if let Err(e) = ctx.provider.teardown(run_id).await {
        tracing::warn!(error = %e, run_id = %run_id, "sandbox teardown failed");
    }
}

async fn notify_run(tx: &mut Transaction<'_, Postgres>, run: &RunRow) -> Result<(), ChannelError> {
    publish(
        &mut **tx,
        &Notification::RunChanged {
            run_id: run.id,
            task_id: run.task_id,
        },
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Env;

    #[test]
    fn the_agent_completing_puts_the_task_in_review() {
        assert_eq!(task_status_for(RunOutcome::Completed), TaskStatus::InReview);
        assert_eq!(run_state_for(RunOutcome::Completed), RunState::Succeeded);
    }

    #[tokio::test]
    async fn blocked_run_is_terminal_and_blocks_the_task() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool.clone()).await;
        deliver_all(&pool, env.task.id).await;
        let run_id = env.insert_run(env.task.id, 1, "finalizing").await;
        let ended = end_run(&pool, &end_with(run_id, RunOutcome::Blocked))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ended.run_state, RunState::Succeeded);
        assert_eq!(status_of(&pool, env.task.id).await, "blocked");
        // No further phase changes are accepted for it.
        let mut tx = pool.begin().await.unwrap();
        set_idle_tx(&mut tx, run_id, env.task.id, true)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let state: String = sqlx::query_scalar("SELECT state FROM runs WHERE id = $1")
            .bind(run_id.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(state, "succeeded");
    }

    #[tokio::test]
    async fn blocked_run_with_an_undelivered_message_queues_the_task() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool.clone()).await;
        // The seeded message is still undelivered.
        let run_id = env.insert_run(env.task.id, 1, "finalizing").await;
        end_run(&pool, &end_with(run_id, RunOutcome::Blocked))
            .await
            .unwrap();
        assert_eq!(status_of(&pool, env.task.id).await, "queued");
    }

    fn end_with(run_id: RunId, outcome: RunOutcome) -> RunEnd {
        RunEnd::failure(run_id, outcome, "")
    }

    async fn status_of(pool: &PgPool, task_id: TaskId) -> String {
        sqlx::query_scalar("SELECT status FROM tasks WHERE id = $1")
            .bind(task_id.as_uuid())
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn deliver_all(pool: &PgPool, task_id: TaskId) {
        sqlx::query("UPDATE task_messages SET delivered_at = now() WHERE task_id = $1")
            .bind(task_id.as_uuid())
            .execute(pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn completed_run_puts_the_task_in_review_or_queues_a_waiting_message() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool.clone()).await;
        let run_id = env.insert_run(env.task.id, 1, "running").await;
        // The seeded task has an undelivered message.
        end_run(&pool, &end_with(run_id, RunOutcome::Completed))
            .await
            .unwrap();
        assert_eq!(status_of(&pool, env.task.id).await, "queued");

        deliver_all(&pool, env.task.id).await;
        let run_id = env.insert_run(env.task.id, 2, "running").await;
        end_run(&pool, &end_with(run_id, RunOutcome::Completed))
            .await
            .unwrap();
        assert_eq!(status_of(&pool, env.task.id).await, "in_review");
    }

    #[tokio::test]
    async fn run_end_keeps_a_task_a_human_marked_done() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool.clone()).await;
        deliver_all(&pool, env.task.id).await;
        let run_id = env.insert_run(env.task.id, 1, "finalizing").await;
        sqlx::query("UPDATE tasks SET status = 'completed' WHERE id = $1")
            .bind(env.task.id.as_uuid())
            .execute(&pool)
            .await
            .unwrap();
        let ended = end_run(&pool, &end_with(run_id, RunOutcome::Cancelled))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ended.task_status, TaskStatus::Completed);
        assert_eq!(status_of(&pool, env.task.id).await, "completed");
    }

    #[tokio::test]
    async fn run_end_of_a_task_marked_done_queues_a_message_that_arrived_since() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool.clone()).await;
        // The seeded message is still undelivered.
        let run_id = env.insert_run(env.task.id, 1, "finalizing").await;
        sqlx::query("UPDATE tasks SET status = 'completed' WHERE id = $1")
            .bind(env.task.id.as_uuid())
            .execute(&pool)
            .await
            .unwrap();
        end_run(&pool, &end_with(run_id, RunOutcome::Cancelled))
            .await
            .unwrap();
        assert_eq!(status_of(&pool, env.task.id).await, "queued");
    }

    #[tokio::test]
    async fn message_queued_while_the_run_ends_is_not_stranded() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool.clone()).await;
        let run_id = env.insert_run(env.task.id, 1, "running").await;
        sqlx::query("UPDATE task_messages SET delivered_at = now() WHERE task_id = $1")
            .bind(env.task.id.as_uuid())
            .execute(&pool)
            .await
            .unwrap();

        // `queue_message`, not yet committed: the task row is locked and the message inserted.
        let mut queueing = pool.begin().await.unwrap();
        sqlx::query("UPDATE tasks SET updated_at = now() WHERE id = $1")
            .bind(env.task.id.as_uuid())
            .execute(&mut *queueing)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO task_messages (id, task_id, author, content) VALUES ($1, $2, '{}', '[]')",
        )
        .bind(uuid::Uuid::now_v7())
        .bind(env.task.id.as_uuid())
        .execute(&mut *queueing)
        .await
        .unwrap();

        let ending = tokio::spawn({
            let pool = pool.clone();
            async move {
                let end = RunEnd {
                    run_id,
                    outcome: RunOutcome::EndTurn,
                    detail: String::new(),
                    retry_after: None,
                    harness_session_id: String::new(),
                    task_branch_sha: String::new(),
                    wip_sha: String::new(),
                    diverged: false,
                };
                end_run(&pool, &end).await
            }
        });
        // The run end is now waiting for the task row.
        loop {
            let waiting: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_stat_activity WHERE wait_event_type = 'Lock'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            if waiting > 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
        queueing.commit().await.unwrap();
        ending.await.unwrap().unwrap();

        let status: String = sqlx::query_scalar("SELECT status FROM tasks WHERE id = $1")
            .bind(env.task.id.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(status, "queued");
    }
}
