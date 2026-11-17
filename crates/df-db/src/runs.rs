//! Run rows: lookup, creation for queued tasks and one function per state transition (design D2,
//! D4; `db-write-ownership` design decision 3).
//!
//! The state lists of the queries come from [`RunState`] constants and are bound as arrays, so a
//! new state is added in one place.

use chrono::{DateTime, TimeDelta, Utc};
use df_core::config::RunsConfig;
use df_core::task_transition::{RunEnded, TaskEvent};
use df_core::{Principal, RunId, RunOutcome, RunState, TaskId, TaskStatus};
use sqlx::{PgExecutor, PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::lifecycle_events::{self, Event, EventSinks};
use crate::notify::{Notification, publish};
use crate::pool::DbError;
use crate::task_events::apply_task_event;
use crate::transcript::RunTaskLock;

/// Time a stopped run gets to report before its sandbox is torn down (design D1).
pub const STOP_TIMEOUT_SECONDS: u64 = 120;

/// Key of the instance-wide advisory lock that serializes the slot accounting of
/// [`start_queued_runs`] across replicas.
const START_LOCK_KEY: i64 = 0x4446_5253; // "DFRS"

/// States of a run whose runtime has not opened its stream yet.
const UNCONNECTED: &[&str] = &[
    RunState::Pending.as_str(),
    RunState::Provisioning.as_str(),
    RunState::Connecting.as_str(),
];

/// States of a run that is up and not over: the wall-clock budget applies.
const CONNECTED: &[&str] = &[
    RunState::Preparing.as_str(),
    RunState::Running.as_str(),
    RunState::AwaitingInput.as_str(),
    RunState::Finalizing.as_str(),
];

/// States of a run whose sandbox is requested but did not start yet.
const NOT_STARTED: &[&str] = &[RunState::Pending.as_str(), RunState::Provisioning.as_str()];

/// Longest interval a deadline query binds (about 100 years); a longer one means "no deadline".
const MAX_INTERVAL_SECONDS: i64 = 3_155_760_000;

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

struct RunRecord {
    id: Uuid,
    task_id: Uuid,
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
    type Error = DbError;

    fn try_from(r: RunRecord) -> Result<Self, DbError> {
        Ok(Self {
            id: RunId::from_uuid(r.id),
            task_id: TaskId::from_uuid(r.task_id),
            number: r.number,
            state: r.state.parse()?,
            outcome: r.outcome.map(|o| o.parse()).transpose()?,
            detail: r.detail,
            pod_ip: r.pod_ip,
            last_persisted_seq: u64::try_from(r.last_persisted_seq)
                .map_err(|_negative| DbError::InvalidData("negative last_persisted_seq".into()))?,
            harness_session_id: r.harness_session_id,
            started_at: r.started_at,
            connected_at: r.connected_at,
            stop_requested_at: r.stop_requested_at,
        })
    }
}

/// Loads a run by id.
pub async fn get_run(
    executor: impl PgExecutor<'_>,
    run_id: RunId,
) -> Result<Option<RunRow>, DbError> {
    sqlx::query_as!(
        RunRecord,
        r#"
        SELECT id, task_id, number, state, outcome, detail, pod_ip, last_persisted_seq,
               harness_session_id, started_at, connected_at, stop_requested_at
        FROM runs WHERE id = $1
        "#,
        run_id.as_uuid()
    )
    .fetch_optional(executor)
    .await?
    .map(RunRow::try_from)
    .transpose()
}

/// State and persisted sequence number of a run, as one read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PersistState {
    pub state: RunState,
    pub last_persisted_seq: u64,
}

/// Reads what a batch of messages needs from the run row: its state and `last_persisted_seq`.
/// `None` if the run does not exist.
pub async fn persist_state(
    executor: impl PgExecutor<'_>,
    run_id: RunId,
) -> Result<Option<PersistState>, DbError> {
    let Some(row) = sqlx::query!(
        "SELECT state, last_persisted_seq FROM runs WHERE id = $1",
        run_id.as_uuid()
    )
    .fetch_optional(executor)
    .await?
    else {
        return Ok(None);
    };
    Ok(Some(PersistState {
        state: row.state.parse()?,
        last_persisted_seq: u64::try_from(row.last_persisted_seq)
            .map_err(|_negative| DbError::InvalidData("negative last_persisted_seq".into()))?,
    }))
}

/// Loads all non-terminal runs; rows that cannot be read are logged and skipped.
pub async fn list_active_runs(executor: impl PgExecutor<'_>) -> Result<Vec<RunRow>, DbError> {
    Ok(sqlx::query_as!(
        RunRecord,
        r#"
        SELECT id, task_id, number, state, outcome, detail, pod_ip, last_persisted_seq,
               harness_session_id, started_at, connected_at, stop_requested_at
        FROM runs WHERE state <> ALL($1) ORDER BY started_at
        "#,
        &state_list(RunState::TERMINAL)
    )
    .fetch_all(executor)
    .await?
    .into_iter()
    .filter_map(skip_unreadable)
    .collect())
}

/// Runs in `pending` or `provisioning` that were created before `before` and that nobody
/// stopped: their provisioning was interrupted or is still waiting for the sandbox to start.
pub async fn list_unprovisioned_runs(
    executor: impl PgExecutor<'_>,
    before: DateTime<Utc>,
) -> Result<Vec<RunRow>, DbError> {
    sqlx::query_as!(
        RunRecord,
        r#"
        SELECT id, task_id, number, state, outcome, detail, pod_ip, last_persisted_seq,
               harness_session_id, started_at, connected_at, stop_requested_at
        FROM runs
        WHERE state = ANY($1) AND started_at < $2 AND stop_requested_at IS NULL
        ORDER BY started_at
        "#,
        &state_list(NOT_STARTED),
        before
    )
    .fetch_all(executor)
    .await?
    .into_iter()
    .map(RunRow::try_from)
    .collect()
}

/// The non-terminal runs that the scheduler's timeout enforcement can end at `now`; unreadable
/// rows are logged and skipped. A superset of what the caller ends: the caller keeps the final
/// decision. One OR-branch per way a run can time out:
///
/// 1. a stop request on a run that is not connected yet, at any age;
/// 2. a stop request older than [`STOP_TIMEOUT_SECONDS`], in any state;
/// 3. the connect timeout of a run that is not connected yet;
/// 4. the hard stop of a connected run: wall-clock budget plus two grace periods.
///
/// An interval that cannot be represented (a `u64::MAX` budget) is bound as `NULL`, and its
/// branch then matches nothing: no deadline, as in the caller.
pub async fn list_overdue_runs(
    executor: impl PgExecutor<'_>,
    now: DateTime<Utc>,
    cfg: &RunsConfig,
) -> Result<Vec<RunRow>, DbError> {
    let hard_stop = cfg
        .max_wall_clock_seconds
        .saturating_add(cfg.cancel_grace_seconds.saturating_mul(2));
    Ok(sqlx::query_as!(
        RunRecord,
        r#"
        SELECT id, task_id, number, state, outcome, detail, pod_ip, last_persisted_seq,
               harness_session_id, started_at, connected_at, stop_requested_at
        FROM runs
        WHERE state <> ALL($1)
          AND (
            (stop_requested_at IS NOT NULL AND state = ANY($2))
            OR (stop_requested_at IS NOT NULL AND $3::bigint IS NOT NULL
                AND stop_requested_at + $3::bigint * interval '1 second' < $4)
            OR (state = ANY($2) AND $5::bigint IS NOT NULL
                AND started_at + $5::bigint * interval '1 second' < $4)
            OR (state = ANY($6) AND $7::bigint IS NOT NULL
                AND COALESCE(connected_at, started_at) + $7::bigint * interval '1 second' < $4)
          )
        ORDER BY started_at
        "#,
        &state_list(RunState::TERMINAL),
        &state_list(UNCONNECTED),
        interval_seconds(STOP_TIMEOUT_SECONDS),
        now,
        interval_seconds(cfg.connect_timeout_seconds),
        &state_list(CONNECTED),
        interval_seconds(hard_stop),
    )
    .fetch_all(executor)
    .await?
    .into_iter()
    .filter_map(skip_unreadable)
    .collect())
}

/// A state list as the `text[]` argument of `state = ANY($n)` / `state <> ALL($n)`: `query!`
/// binds an array as `&[String]`.
pub(crate) fn state_list(states: &[&str]) -> Vec<String> {
    states.iter().map(ToString::to_string).collect()
}

fn interval_seconds(seconds: u64) -> Option<i64> {
    i64::try_from(seconds)
        .ok()
        .filter(|seconds| *seconds <= MAX_INTERVAL_SECONDS)
}

fn skip_unreadable(record: RunRecord) -> Option<RunRow> {
    let id = record.id;
    RunRow::try_from(record)
        .inspect_err(|e| tracing::error!(error = %e, run_id = %id, "skipping unreadable run row"))
        .ok()
}

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
) -> Result<Vec<RunRow>, DbError> {
    let mut tx = pool.begin().await?;
    let locked = sqlx::query_scalar!(
        r#"SELECT pg_try_advisory_xact_lock($1) AS "locked!""#,
        START_LOCK_KEY
    )
    .fetch_one(&mut *tx)
    .await?;
    // Every exit ends the transaction itself: a dropped one only queues its rollback, which a
    // pooled connection sends when it is used again, and until then the lock stays held.
    if !locked {
        tx.rollback().await?;
        return Ok(Vec::new());
    }
    let active = sqlx::query_scalar!(
        r#"SELECT count(*) AS "active!" FROM runs WHERE state <> ALL($1)"#,
        &state_list(RunState::TERMINAL)
    )
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
    let tasks = sqlx::query!(
        r#"
        SELECT t.id, t.resume_at IS NOT NULL AS "resumed!" FROM tasks t
        WHERE t.status IN ('queued', 'rate_limited')
          AND (t.resume_at IS NULL OR t.resume_at <= now())
          AND NOT EXISTS (
              SELECT 1 FROM runs r WHERE r.task_id = t.id AND r.state <> ALL($2))
        ORDER BY COALESCE(t.resume_at, t.updated_at), t.created_at
        LIMIT $1
        FOR UPDATE OF t SKIP LOCKED
        "#,
        limit.min(free),
        &state_list(RunState::TERMINAL)
    )
    .fetch_all(&mut *tx)
    .await?;

    let actor = Principal::system("run.started");
    let mut started = Vec::with_capacity(tasks.len());
    for task in tasks {
        // Without a prompt the resumed run would end idle at once.
        if task.resumed {
            crate::tasks::queue_resume_prompt_tx(&mut tx, TaskId::from_uuid(task.id)).await?;
        }
        let record = sqlx::query_as!(
            RunRecord,
            r#"
            INSERT INTO runs (id, task_id, number)
            VALUES ($1, $2, (SELECT COALESCE(MAX(number), 0) + 1 FROM runs WHERE task_id = $2))
            RETURNING id AS "id!", task_id AS "task_id!", number AS "number!", state AS "state!",
                      outcome, detail AS "detail!", pod_ip,
                      last_persisted_seq AS "last_persisted_seq!",
                      harness_session_id AS "harness_session_id!",
                      started_at AS "started_at!", connected_at, stop_requested_at
            "#,
            RunId::new().as_uuid(),
            task.id
        )
        .fetch_one(&mut *tx)
        .await?;
        let run = RunRow::try_from(record)?;
        notify_run(&mut tx, run.id, run.task_id).await?;
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
) -> Result<bool, DbError> {
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
) -> Result<bool, DbError> {
    let task_id = sqlx::query_scalar!(
        r#"
        UPDATE runs SET state = $2, pod_ip = COALESCE($3, pod_ip)
        WHERE id = $1 AND state <> ALL($4) AND state <> $2
        RETURNING task_id
        "#,
        run_id.as_uuid(),
        state.as_str(),
        pod_ip,
        &state_list(RunState::TERMINAL)
    )
    .fetch_optional(&mut **tx)
    .await?;
    let Some(task_id) = task_id else {
        return Ok(false);
    };
    notify_run(tx, run_id, TaskId::from_uuid(task_id)).await?;
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
) -> Result<(), DbError> {
    let state = if idle {
        RunState::AwaitingInput
    } else {
        RunState::Running
    };
    let changed = sqlx::query_scalar!(
        r#"
        UPDATE runs SET state = $2
        WHERE id = $1 AND state <> ALL($3) AND state <> $2
        RETURNING task_id
        "#,
        run_id.as_uuid(),
        state.as_str(),
        &state_list(RunState::NOT_COMMANDABLE)
    )
    .fetch_optional(&mut **tx)
    .await?;
    if changed.is_none() {
        return Ok(());
    }
    notify_run(tx, run_id, task_id).await?;
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

/// Locks the task row, the lock `queue_message` takes. What the caller decides afterwards cannot
/// miss a message queued concurrently.
///
/// A transaction that also locks a run row must lock the run row first, see
/// [`crate::transcript::lock_run_and_task`].
pub async fn lock_task(tx: &mut Transaction<'_, Postgres>, task_id: TaskId) -> Result<(), DbError> {
    sqlx::query!(
        "SELECT 1 AS one FROM tasks WHERE id = $1 FOR UPDATE",
        task_id.as_uuid()
    )
    .fetch_optional(&mut **tx)
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
) -> Result<bool, DbError> {
    Ok(sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM task_messages
            WHERE task_id = $1 AND delivered_at IS NULL AND discarded_at IS NULL
              AND ($2::timestamptz IS NULL OR queued_at > $2)) AS "undelivered!"
        "#,
        task_id.as_uuid(),
        after
    )
    .fetch_one(&mut **tx)
    .await?)
}

/// Ends a non-terminal run and sets the task status. Returns `None` if the run was already
/// terminal (another replica or an earlier report ended it). The caller tears the sandbox down.
///
/// A run whose provisioning began emits `sandbox.provisioning_failed` (outcome
/// `provisioning_failed`) or `sandbox.destroyed`. `max_auto_resumes` and `resume_backoff` are
/// the resume policy for infrastructure failures (`runs.maxAutoResumes`,
/// `scheduling.resumeBackoffSeconds`).
pub async fn end_run(
    pool: &PgPool,
    end: &RunEnd,
    events: &EventSinks,
    max_auto_resumes: u32,
    resume_backoff: TimeDelta,
) -> Result<Option<EndedRun>, DbError> {
    let mut tx = pool.begin().await?;
    let ended = end_run_tx(&mut tx, end, events, max_auto_resumes, resume_backoff).await?;
    tx.commit().await?;
    Ok(ended)
}

/// Transactional variant of [`end_run`].
pub async fn end_run_tx(
    tx: &mut Transaction<'_, Postgres>,
    end: &RunEnd,
    events: &EventSinks,
    max_auto_resumes: u32,
    resume_backoff: TimeDelta,
) -> Result<Option<EndedRun>, DbError> {
    let run_state = end.outcome.run_state();
    let ended = sqlx::query_as!(
        EndedRow,
        r#"
        UPDATE runs SET state = $2, outcome = $3, detail = $4, retry_after = $5,
            harness_session_id = CASE WHEN $6 <> '' THEN $6 ELSE harness_session_id END,
            task_branch_sha = $7, wip_sha = $8, ended_at = now()
        WHERE id = $1 AND state <> ALL($9)
        RETURNING task_id AS "task_id!", stop_requested_at, provisioning_started_at,
                  sandbox_started_at, ended_at
        "#,
        end.run_id.as_uuid(),
        run_state.as_str(),
        end.outcome.as_str(),
        end.detail,
        end.retry_after,
        end.harness_session_id,
        end.task_branch_sha,
        end.wip_sha,
        &state_list(RunState::TERMINAL)
    )
    .fetch_optional(&mut **tx)
    .await?;
    let Some(ended) = ended else {
        return Ok(None);
    };
    let task_id = TaskId::from_uuid(ended.task_id);
    let stop_requested_at = ended.stop_requested_at;
    if let Some(event) = ended.sandbox_event(end) {
        lifecycle_events::enqueue(tx, events, &event).await?;
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
        max_auto_resumes,
        resume_backoff,
        now: Utc::now(),
    });
    let next = apply_task_event(tx, task_id, event, &Principal::system("run.ended")).await?;
    sqlx::query!(
        r#"
        UPDATE tasks SET
            harness_session_id = CASE WHEN $2 <> '' THEN $2 ELSE harness_session_id END,
            diverged = diverged OR $3, updated_at = now()
        WHERE id = $1
        "#,
        task_id.as_uuid(),
        end.harness_session_id,
        end.diverged
    )
    .execute(&mut **tx)
    .await?;

    notify_run(tx, end.run_id, task_id).await?;
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
) -> Result<bool, DbError> {
    let mut tx = pool.begin().await?;
    let started = sqlx::query!(
        r#"
        UPDATE runs SET state = 'connecting', pod_ip = $2,
            sandbox_started_at = COALESCE(sandbox_started_at, now())
        WHERE id = $1 AND state = ANY($3)
        RETURNING task_id, provisioning_started_at IS NOT NULL AS "provisioning_began!"
        "#,
        run_id.as_uuid(),
        pod_ip,
        &state_list(NOT_STARTED)
    )
    .fetch_optional(&mut *tx)
    .await?;
    let Some(started) = started else {
        return Ok(false);
    };
    if started.provisioning_began {
        lifecycle_events::enqueue(&mut tx, events, &Event::SandboxProvisioned { run_id }).await?;
    }
    notify_run(&mut tx, run_id, TaskId::from_uuid(started.task_id)).await?;
    tx.commit().await?;
    Ok(true)
}

/// Records the runtime's first connect and moves an early run to `preparing`. Returns the run as
/// it is now, `None` if it is already terminal.
pub async fn mark_connected(pool: &PgPool, run_id: RunId) -> Result<Option<RunRow>, DbError> {
    let record = sqlx::query_as!(
        RunRecord,
        r#"
        UPDATE runs SET connected_at = COALESCE(connected_at, now()),
            state = CASE WHEN state = ANY($2) THEN 'preparing' ELSE state END
        WHERE id = $1 AND state <> ALL($3)
        RETURNING id AS "id!", task_id AS "task_id!", number AS "number!", state AS "state!",
                  outcome, detail AS "detail!", pod_ip,
                  last_persisted_seq AS "last_persisted_seq!",
                  harness_session_id AS "harness_session_id!",
                  started_at AS "started_at!", connected_at, stop_requested_at
        "#,
        run_id.as_uuid(),
        &state_list(UNCONNECTED),
        &state_list(RunState::TERMINAL)
    )
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
) -> Result<bool, DbError> {
    let mut tx = pool.begin().await?;
    let task_id = sqlx::query_scalar!(
        r#"
        UPDATE runs SET state = 'provisioning',
            provisioning_started_at = COALESCE(provisioning_started_at, now())
        WHERE id = $1 AND state = 'pending'
        RETURNING task_id
        "#,
        run_id.as_uuid()
    )
    .fetch_optional(&mut *tx)
    .await?;
    let Some(task_id) = task_id else {
        return Ok(false);
    };
    lifecycle_events::enqueue(&mut tx, events, &Event::SandboxProvisioning { run_id }).await?;
    notify_run(&mut tx, run_id, TaskId::from_uuid(task_id)).await?;
    tx.commit().await?;
    Ok(true)
}

/// Moves the run to `finalizing` in a transaction that holds the run and task row locks (`lock`,
/// the lock `queue_message` takes), so a message queued meanwhile is either seen here or sees the
/// run `finalizing` and queues the task for the next run. With `only_idle` the move happens only
/// for an `awaiting_input` run without an undelivered message (the idle `Release`); otherwise for
/// any run that is commandable (the budget `Cancel`). Returns whether the run was moved.
pub async fn start_finalizing(
    tx: &mut Transaction<'_, Postgres>,
    lock: &RunTaskLock,
    only_idle: bool,
) -> Result<bool, DbError> {
    let (run_id, task_id) = (lock.run_id(), lock.task_id());
    let moved = if only_idle {
        !has_undelivered_message(tx, task_id, None).await?
            && sqlx::query!(
                "UPDATE runs SET state = 'finalizing' WHERE id = $1 AND state = 'awaiting_input'",
                run_id.as_uuid()
            )
            .execute(&mut **tx)
            .await?
            .rows_affected()
                > 0
    } else {
        sqlx::query!(
            "UPDATE runs SET state = 'finalizing' WHERE id = $1 AND state <> ALL($2)",
            run_id.as_uuid(),
            &state_list(RunState::NOT_COMMANDABLE)
        )
        .execute(&mut **tx)
        .await?
        .rows_affected()
            > 0
    };
    if moved {
        notify_run(tx, run_id, task_id).await?;
    }
    Ok(moved)
}

/// Records that the messages up to `seq` of the run are persisted.
pub async fn set_last_persisted_seq(
    executor: impl PgExecutor<'_>,
    run_id: RunId,
    seq: u64,
) -> Result<(), DbError> {
    let seq = i64::try_from(seq)
        .map_err(|_out_of_range| DbError::InvalidData("seq out of range".into()))?;
    sqlx::query!(
        "UPDATE runs SET last_persisted_seq = $2 WHERE id = $1",
        run_id.as_uuid(),
        seq
    )
    .execute(executor)
    .await?;
    Ok(())
}

/// Stores the summary the agent gave when it completed the task.
pub async fn set_completion_summary(
    executor: impl PgExecutor<'_>,
    run_id: RunId,
    summary: &str,
) -> Result<(), DbError> {
    sqlx::query!(
        "UPDATE runs SET completion_summary = $2 WHERE id = $1",
        run_id.as_uuid(),
        summary
    )
    .execute(executor)
    .await?;
    Ok(())
}

/// The completion summary of the run; `None` if the run does not exist, empty if it has none.
pub async fn completion_summary(
    executor: impl PgExecutor<'_>,
    run_id: RunId,
) -> Result<Option<String>, DbError> {
    Ok(sqlx::query_scalar!(
        "SELECT completion_summary FROM runs WHERE id = $1",
        run_id.as_uuid()
    )
    .fetch_optional(executor)
    .await?)
}

/// The harness session of the newest run of the task before run `number` that has one.
pub async fn previous_harness_session_id(
    executor: impl PgExecutor<'_>,
    task_id: TaskId,
    number: i64,
) -> Result<Option<String>, DbError> {
    Ok(sqlx::query_scalar!(
        "SELECT harness_session_id FROM runs
         WHERE task_id = $1 AND number < $2 AND harness_session_id <> ''
         ORDER BY number DESC LIMIT 1",
        task_id.as_uuid(),
        number
    )
    .fetch_optional(executor)
    .await?)
}

/// What [`end_run_tx`] reads back from the run it ended.
struct EndedRow {
    task_id: Uuid,
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

async fn notify_run(
    tx: &mut Transaction<'_, Postgres>,
    run_id: RunId,
    task_id: TaskId,
) -> Result<(), DbError> {
    publish(&mut **tx, &Notification::RunChanged { run_id, task_id }).await
}

#[cfg(test)]
mod tests;
