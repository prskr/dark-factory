//! Run rows: lookup, creation for queued tasks, state changes and the run-end mapping to task
//! status (design D2, D4).

use chrono::{DateTime, Utc};
use df_core::task_transition::{RunEnded, TaskEvent};
use df_core::{Principal, RunId, RunOutcome, RunState, TaskId, TaskStatus};
use df_db::lifecycle_events::{self, Event, EventSinks};
use df_db::{Notification, apply_task_event, publish};
use sqlx::{FromRow, PgPool, Postgres, Transaction};

use crate::error::ChannelError;
use crate::{ChannelConfig, ChannelContext};

// sqlx only accepts literal SQL, so shared fragments are macros expanded by `concat!`.
macro_rules! terminal_states {
    () => {
        "('succeeded', 'failed', 'cancelled')"
    };
}
pub(crate) use terminal_states;

macro_rules! run_columns {
    () => {
        "id, task_id, number, state, outcome, detail, pod_ip, last_persisted_seq, \
         harness_session_id, started_at, connected_at, stop_requested_at"
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
    /// When a board member stopped the run (design D1); `None` for a run nobody stopped.
    pub stop_requested_at: Option<DateTime<Utc>>,
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
    stop_requested_at: Option<DateTime<Utc>>,
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
            last_persisted_seq: u64::try_from(r.last_persisted_seq).map_err(|_negative| {
                ChannelError::InvalidData("negative last_persisted_seq".into())
            })?,
            harness_session_id: r.harness_session_id,
            started_at: r.started_at,
            connected_at: r.connected_at,
            stop_requested_at: r.stop_requested_at,
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

/// Runs in `pending` or `provisioning` that were created before `before` and that nobody
/// stopped: their provisioning was interrupted or is still waiting for the sandbox to start.
pub async fn list_unprovisioned_runs(
    pool: &PgPool,
    before: DateTime<Utc>,
) -> Result<Vec<RunRow>, ChannelError> {
    sqlx::query_as::<_, RunRecord>(concat!(
        "SELECT ",
        run_columns!(),
        " FROM runs
          WHERE state IN ('pending', 'provisioning') AND started_at < $1
            AND stop_requested_at IS NULL
          ORDER BY started_at"
    ))
    .bind(before)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(RunRow::try_from)
    .collect()
}

/// Key of the instance-wide advisory lock that serializes the slot accounting of
/// [`start_queued_runs`] across replicas.
const START_LOCK_KEY: i64 = 0x4446_5253; // "DFRS"

/// Creates a `pending` run for up to `limit` due tasks without an active run and sets those
/// tasks to `running`, as long as fewer than `max_concurrent` runs are non-terminal. A task is due
/// when it is `queued` or `rate_limited` and its resume time is unset or past; the oldest due
/// time starts first and starting clears the resume time.
///
/// Counting the active runs, selecting the tasks and inserting the runs happen under an
/// instance-wide advisory lock; a caller that doesn't get it starts nothing this time, so two
/// replicas cannot both fill the last slot. The per-task `FOR UPDATE SKIP LOCKED` and the partial
/// unique index on `runs` stay the guard against two runs of one task.
pub async fn start_queued_runs(
    pool: &PgPool,
    limit: i64,
    max_concurrent: u32,
) -> Result<Vec<RunRow>, ChannelError> {
    let mut tx = pool.begin().await?;
    let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock($1)")
        .bind(START_LOCK_KEY)
        .fetch_one(&mut *tx)
        .await?;
    // Every exit ends the transaction itself: a dropped one only queues its rollback, which a
    // pooled connection sends when it is used again, and until then the lock stays held.
    if !locked {
        tx.rollback().await?;
        return Ok(Vec::new());
    }
    let active: i64 = sqlx::query_scalar(concat!(
        "SELECT count(*) FROM runs WHERE state NOT IN ",
        terminal_states!()
    ))
    .fetch_one(&mut *tx)
    .await?;
    let free = i64::from(max_concurrent).saturating_sub(active);
    if free <= 0 {
        tx.rollback().await?;
        return Ok(Vec::new());
    }
    // ponytail: `updated_at` approximates the time a task became queued; add a status change
    // column if other writes to a queued task make the order drift.
    // `resumed`: the task starts from its resume time (rate limit, infrastructure failure).
    let tasks: Vec<(uuid::Uuid, bool)> = sqlx::query_as(concat!(
        "SELECT t.id, t.resume_at IS NOT NULL FROM tasks t
         WHERE t.status IN ('queued', 'rate_limited')
           AND (t.resume_at IS NULL OR t.resume_at <= now())
           AND NOT EXISTS (
               SELECT 1 FROM runs r WHERE r.task_id = t.id AND r.state NOT IN ",
        terminal_states!(),
        ")
         ORDER BY COALESCE(t.resume_at, t.updated_at), t.created_at
         LIMIT $1
         FOR UPDATE OF t SKIP LOCKED"
    ))
    .bind(limit.min(free))
    .fetch_all(&mut *tx)
    .await?;

    let actor = Principal::system("run.started");
    let mut started = Vec::with_capacity(tasks.len());
    for (task_id, resumed) in tasks {
        // Without a prompt the resumed run would end idle at once.
        if resumed {
            df_db::tasks::queue_resume_prompt_tx(&mut tx, TaskId::from_uuid(task_id)).await?;
        }
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
        let run = RunRow::try_from(record)?;
        notify_run(&mut tx, &run).await?;
        // The task row is locked by the `SELECT` above.
        apply_task_event(&mut tx, run.task_id, TaskEvent::RunStarted, &actor).await?;
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
    publish(&mut **tx, &Notification::RunChanged { run_id, task_id }).await?;
    let event = if idle {
        TaskEvent::TurnEnded
    } else {
        TaskEvent::TurnStarted
    };
    apply_task_event(tx, task_id, event, &Principal::system("run.turn")).await?;
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
    /// The runtime pushed the task branch (`WorkspaceState`).
    pub task_branch_pushed: bool,
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
            task_branch_pushed: false,
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

/// Locks the task row, the lock `queue_message` takes. What the caller decides afterwards cannot
/// miss a message queued concurrently.
///
/// A transaction that also locks a run row must lock the run row first, see
/// [`df_db::transcript::lock_run_and_task`].
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

/// Whether a message of the task is still undelivered (discarded ones are not). With `after`,
/// only messages queued after that time count: for a run a board member stopped, the messages
/// queued before the stop wait for the next message (design D1).
pub async fn has_undelivered_message(
    tx: &mut Transaction<'_, Postgres>,
    task_id: TaskId,
    after: Option<DateTime<Utc>>,
) -> Result<bool, ChannelError> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS (
             SELECT 1 FROM task_messages
             WHERE task_id = $1 AND delivered_at IS NULL AND discarded_at IS NULL
               AND ($2::timestamptz IS NULL OR queued_at > $2))",
    )
    .bind(task_id.as_uuid())
    .bind(after)
    .fetch_one(&mut **tx)
    .await?)
}

/// Ends a non-terminal run and sets the task status. Returns `None` if the run was already
/// terminal (another replica or an earlier report ended it). The caller tears the sandbox down.
///
/// A run whose provisioning began emits `sandbox.provisioning_failed` (outcome
/// `provisioning_failed`) or `sandbox.destroyed`.
pub async fn end_run(
    pool: &PgPool,
    end: &RunEnd,
    config: &ChannelConfig,
) -> Result<Option<EndedRun>, ChannelError> {
    let mut tx = pool.begin().await?;
    let ended = end_run_tx(&mut tx, end, config).await?;
    tx.commit().await?;
    Ok(ended)
}

/// Transactional variant of [`end_run`].
pub async fn end_run_tx(
    tx: &mut Transaction<'_, Postgres>,
    end: &RunEnd,
    config: &ChannelConfig,
) -> Result<Option<EndedRun>, ChannelError> {
    let run_state = run_state_for(end.outcome);
    let ended: Option<EndedRow> = sqlx::query_as(concat!(
        "UPDATE runs SET state = $2, outcome = $3, detail = $4, retry_after = $5,
             harness_session_id = CASE WHEN $6 <> '' THEN $6 ELSE harness_session_id END,
             task_branch_sha = $7, wip_sha = $8, ended_at = now()
         WHERE id = $1 AND state NOT IN ",
        terminal_states!(),
        "
         RETURNING task_id, stop_requested_at, provisioning_started_at, sandbox_started_at,
             ended_at"
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
    let Some(ended) = ended else {
        return Ok(None);
    };
    let task_id = TaskId::from_uuid(ended.task_id);
    let stop_requested_at = ended.stop_requested_at;
    if let Some(event) = ended.sandbox_event(end) {
        lifecycle_events::enqueue(tx, &config.events, &event).await?;
    }

    // The applier locks the task row, as `queue_message` does before it inserts. Waiting for
    // that lock makes a message queued concurrently visible to the rules, instead of the task
    // becoming `waiting` with it stranded.
    let event = TaskEvent::RunEnded(RunEnded {
        run_id: end.run_id,
        outcome: end.outcome,
        detail: end.detail.clone(),
        retry_after: end.retry_after,
        task_branch_pushed: end.task_branch_pushed,
        stop_requested_at,
        max_auto_resumes: config.runs.max_auto_resumes,
        now: Utc::now(),
    });
    let next = apply_task_event(tx, task_id, event, &Principal::system("run.ended")).await?;
    sqlx::query(
        "UPDATE tasks SET harness_session_id = CASE WHEN $2 <> '' THEN $2 ELSE harness_session_id END,
             diverged = diverged OR $3, updated_at = now()
         WHERE id = $1",
    )
    .bind(task_id.as_uuid())
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
    Ok(Some(EndedRun {
        task_id,
        run_state,
        task_status: next.status,
    }))
}

/// Records that the sandbox of `run_id` is up at `pod_ip`: `pending`/`provisioning` → `connecting`,
/// sets `sandbox_started_at` and, if its provisioning began, emits `sandbox.provisioned`.
pub async fn mark_started(
    pool: &PgPool,
    events: &EventSinks,
    run_id: RunId,
    pod_ip: &str,
) -> Result<bool, ChannelError> {
    let mut tx = pool.begin().await?;
    let started: Option<(uuid::Uuid, bool)> = sqlx::query_as(
        "UPDATE runs SET state = 'connecting', pod_ip = $2,
             sandbox_started_at = COALESCE(sandbox_started_at, now())
         WHERE id = $1 AND state IN ('pending', 'provisioning')
         RETURNING task_id, provisioning_started_at IS NOT NULL",
    )
    .bind(run_id.as_uuid())
    .bind(pod_ip)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((task_id, provisioning_began)) = started else {
        return Ok(false);
    };
    if provisioning_began {
        lifecycle_events::enqueue(&mut tx, events, &Event::SandboxProvisioned { run_id }).await?;
    }
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

/// The scheduler picks `run_id`: `pending` → `provisioning`, sets `provisioning_started_at` and
/// emits `sandbox.provisioning`. Returns whether the run was moved.
pub async fn begin_provisioning(
    pool: &PgPool,
    events: &EventSinks,
    run_id: RunId,
) -> Result<bool, ChannelError> {
    let mut tx = pool.begin().await?;
    let task_id: Option<uuid::Uuid> = sqlx::query_scalar(
        "UPDATE runs SET state = 'provisioning',
             provisioning_started_at = COALESCE(provisioning_started_at, now())
         WHERE id = $1 AND state = 'pending'
         RETURNING task_id",
    )
    .bind(run_id.as_uuid())
    .fetch_optional(&mut *tx)
    .await?;
    let Some(task_id) = task_id else {
        return Ok(false);
    };
    lifecycle_events::enqueue(&mut tx, events, &Event::SandboxProvisioning { run_id }).await?;
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

/// Ends the run and, if this call ended it, deletes its sandbox. The task volume is kept.
pub async fn finish_run(
    ctx: &ChannelContext,
    end: &RunEnd,
) -> Result<Option<EndedRun>, ChannelError> {
    let ended = end_run(&ctx.pool, end, &ctx.config).await?;
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

/// What `end_run_tx` reads back from the run it ended.
#[derive(sqlx::FromRow)]
struct EndedRow {
    task_id: uuid::Uuid,
    stop_requested_at: Option<DateTime<Utc>>,
    provisioning_started_at: Option<DateTime<Utc>>,
    sandbox_started_at: Option<DateTime<Utc>>,
    ended_at: Option<DateTime<Utc>>,
}

impl EndedRow {
    /// The closing sandbox event of a run whose provisioning began (design D2 pairing).
    fn sandbox_event(&self, end: &RunEnd) -> Option<Event> {
        self.provisioning_started_at?;
        if end.outcome == RunOutcome::ProvisioningFailed {
            return Some(Event::SandboxProvisioningFailed {
                run_id: end.run_id,
                error: end.detail.clone(),
            });
        }
        Some(Event::SandboxDestroyed {
            run_id: end.run_id,
            outcome: end.outcome,
            started_at: self.sandbox_started_at,
            ended_at: self.ended_at.unwrap_or_else(Utc::now),
        })
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
mod tests;
