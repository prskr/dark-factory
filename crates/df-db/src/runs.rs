//! Run rows: lookup, creation for queued tasks and one function per state transition (design D2,
//! D4; `db-write-ownership` design decision 3).
//!
//! The state lists of the queries come from [`RunState`] constants and are bound as arrays, so a
//! new state is added in one place.

use df_core::config::RunsConfig;
use df_core::run_transition::{FinalizeReason, RunEnd, RunEvent, STOP_TIMEOUT_SECONDS};
use df_core::task_transition::TaskEvent;
use df_core::{Principal, RunId, RunOutcome, RunState, TaskId, TaskKind, TaskStatus};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::{PgExecutor, PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::lifecycle_events::EventSinks;
use crate::notify::{Notification, publish};
use crate::pool::DbError;
use crate::run_events::{RunEnv, apply_run_event};
use crate::task_events::apply_task_event;
use crate::task_views::status_list;
use crate::transcript::RunTaskLock;

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
    pub started_at: Timestamp,
    /// First `Hello` of the run's runtime; `None` until it connected.
    pub connected_at: Option<Timestamp>,
    /// When a board member stopped the run (design D1); `None` for a run nobody stopped.
    pub stop_requested_at: Option<Timestamp>,
    /// Why the run is `finalizing`; `None` for a run that is not.
    pub finalize_reason: Option<FinalizeReason>,
}

impl RunRow {
    /// Start of the wall-clock budget: when the runtime connected, else when the run was created.
    pub fn budget_start(&self) -> Timestamp {
        self.connected_at.unwrap_or(self.started_at)
    }
}

/// A `runs` row as `query_as!` returns it. `timestamptz` columns arrive as `jiff_sqlx::Timestamp`
/// (`sqlx.toml`); `query_as!` converts a NOT NULL field with `.into()`, so those are plain
/// `jiff::Timestamp`. `Option<jiff::Timestamp>: From<Option<jiff_sqlx::Timestamp>>` does not exist,
/// so nullable fields keep the wrapper and `TryFrom` unwraps them with `.map(Into::into)`; the
/// other records of this crate follow the same split.
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
    started_at: Timestamp,
    connected_at: Option<jiff_sqlx::Timestamp>,
    stop_requested_at: Option<jiff_sqlx::Timestamp>,
    finalize_reason: Option<String>,
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
            connected_at: r.connected_at.map(Into::into),
            stop_requested_at: r.stop_requested_at.map(Into::into),
            finalize_reason: r.finalize_reason.map(|r| r.parse()).transpose()?,
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
               harness_session_id, started_at, connected_at, stop_requested_at, finalize_reason
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
               harness_session_id, started_at, connected_at, stop_requested_at, finalize_reason
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
    before: Timestamp,
) -> Result<Vec<RunRow>, DbError> {
    sqlx::query_as!(
        RunRecord,
        r#"
        SELECT id, task_id, number, state, outcome, detail, pod_ip, last_persisted_seq,
               harness_session_id, started_at, connected_at, stop_requested_at, finalize_reason
        FROM runs
        WHERE state = ANY($1) AND started_at < $2 AND stop_requested_at IS NULL
        ORDER BY started_at
        "#,
        &state_list(NOT_STARTED),
        before.to_sqlx()
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
    now: Timestamp,
    cfg: &RunsConfig,
) -> Result<Vec<RunRow>, DbError> {
    let hard_stop = cfg
        .max_wall_clock_seconds
        .saturating_add(cfg.cancel_grace_seconds.saturating_mul(2));
    Ok(sqlx::query_as!(
        RunRecord,
        r#"
        SELECT id, task_id, number, state, outcome, detail, pod_ip, last_persisted_seq,
               harness_session_id, started_at, connected_at, stop_requested_at, finalize_reason
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
        now.to_sqlx(),
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
    max_concurrent_chats: u32,
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
    // Tasks and chats have a budget each.
    let active = sqlx::query!(
        r#"SELECT t.kind AS "kind!", count(*) AS "active!" FROM runs r
           JOIN tasks t ON t.id = r.task_id WHERE r.state <> ALL($1) GROUP BY t.kind"#,
        &state_list(RunState::TERMINAL)
    )
    .fetch_all(&mut *tx)
    .await?;
    let active_of = |kind: TaskKind| {
        active
            .iter()
            .find(|row| row.kind == kind.as_str())
            .map_or(0, |row| row.active)
    };
    let free_tasks = i64::from(max_concurrent).saturating_sub(active_of(TaskKind::Task));
    let free_chats = i64::from(max_concurrent_chats).saturating_sub(active_of(TaskKind::Chat));
    let mut tasks = Vec::new();
    for (kind, free) in [(TaskKind::Task, free_tasks), (TaskKind::Chat, free_chats)] {
        if free > 0 {
            tasks.extend(due_tasks(&mut tx, kind, limit.min(free)).await?);
        }
    }
    if free_chats <= 0 {
        release_idle_chat_run(&mut tx).await?;
    }
    if tasks.is_empty() {
        // Releasing a run is published, so the transaction commits.
        tx.commit().await?;
        return Ok(Vec::new());
    }

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
                      started_at AS "started_at!", connected_at, stop_requested_at,
                      finalize_reason
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

/// A due task as [`due_tasks`] reads it. `resumed`: the task starts from its resume time (rate
/// limit, infrastructure failure).
struct DueTask {
    id: Uuid,
    resumed: bool,
}

/// The oldest due tasks of `kind` without an active run, locked, skipping archived chats: a
/// message queued on an archived chat starts nothing until it is unarchived. A task is due when
/// its status waits for resume and its resume time is unset or past.
///
/// ponytail: `updated_at` approximates the time a task became queued; add a status change
/// column if other writes to a queued task make the order drift.
async fn due_tasks(
    tx: &mut Transaction<'_, Postgres>,
    kind: TaskKind,
    limit: i64,
) -> Result<Vec<DueTask>, DbError> {
    Ok(sqlx::query_as!(
        DueTask,
        r#"
        SELECT t.id, t.resume_at IS NOT NULL AS "resumed!" FROM tasks t
        WHERE t.kind = $4
          AND t.archived_at IS NULL
          AND t.status = ANY($3)
          AND (t.resume_at IS NULL OR t.resume_at <= now())
          AND NOT EXISTS (
              SELECT 1 FROM runs r WHERE r.task_id = t.id AND r.state <> ALL($2))
        ORDER BY COALESCE(t.resume_at, t.updated_at), t.created_at
        LIMIT $1
        FOR UPDATE OF t SKIP LOCKED
        "#,
        limit,
        &state_list(RunState::TERMINAL),
        &status_list(TaskStatus::WAITS_FOR_RESUME),
        kind.as_str()
    )
    .fetch_all(&mut **tx)
    .await?)
}

/// A chat is due and every chat slot is taken: asks the session of the chat run that has waited
/// longest after a turn to release it (`ReleaseIdle`). Nothing happens without an idle chat run.
/// The released chat keeps its status; the next pass starts the queued chat in the freed slot.
async fn release_idle_chat_run(tx: &mut Transaction<'_, Postgres>) -> Result<(), DbError> {
    if due_tasks(tx, TaskKind::Chat, 1).await?.is_empty() {
        return Ok(());
    }
    let idle = sqlx::query_scalar!(
        r#"
        SELECT r.id FROM runs r JOIN tasks t ON t.id = r.task_id
        WHERE t.kind = 'chat' AND r.state = 'awaiting_input' AND r.stop_requested_at IS NULL
        ORDER BY r.idle_since, r.started_at
        LIMIT 1
        "#
    )
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(run_id) = idle {
        let run_id = RunId::from_uuid(run_id);
        publish(&mut **tx, &Notification::ReleaseIdle { run_id }).await?;
    }
    Ok(())
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
    idle: bool,
) -> Result<(), DbError> {
    let event = if idle {
        RunEvent::TurnEnded
    } else {
        RunEvent::TurnStarted
    };
    apply_run_event(tx, run_id, event, &RunEnv::none()).await?;
    Ok(())
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
    after: Option<Timestamp>,
) -> Result<bool, DbError> {
    Ok(sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM task_messages
            WHERE task_id = $1 AND undelivered
              AND ($2::timestamptz IS NULL OR queued_at > $2)) AS "undelivered!"
        "#,
        task_id.as_uuid(),
        after.map(ToSqlx::to_sqlx)
    )
    .fetch_one(&mut **tx)
    .await?)
}

/// Ends a non-terminal run and sets the task status. Returns `None` if the run was already
/// terminal (another replica or an earlier report ended it). The caller tears the sandbox down.
///
/// A run whose provisioning began emits `sandbox.provisioning_failed` (outcome
/// `provisioning_failed`) or `sandbox.destroyed`. `env` carries the lifecycle sinks and the
/// resume policy for infrastructure failures (`runs.maxAutoResumes`,
/// `scheduling.resumeBackoffSeconds`).
pub async fn end_run(
    pool: &PgPool,
    end: &RunEnd,
    env: &RunEnv<'_>,
) -> Result<Option<EndedRun>, DbError> {
    let mut tx = pool.begin().await?;
    let ended = end_run_tx(&mut tx, end, env).await?;
    tx.commit().await?;
    Ok(ended)
}

/// Transactional variant of [`end_run`].
pub async fn end_run_tx(
    tx: &mut Transaction<'_, Postgres>,
    end: &RunEnd,
    env: &RunEnv<'_>,
) -> Result<Option<EndedRun>, DbError> {
    apply_end(tx, end.run_id, RunEvent::Ended(end.clone()), env).await
}

/// Ends the run if it is overdue at `now` on the row as it is now (under its lock), by
/// [`df_core::run_transition::overdue`]; `None` if it is not (it moved since it was listed) or was
/// ended already. The caller tears the sandbox down when the run ended.
pub async fn end_overdue(
    pool: &PgPool,
    run_id: RunId,
    now: Timestamp,
    limits: &RunsConfig,
    env: &RunEnv<'_>,
) -> Result<Option<EndedRun>, DbError> {
    let event = RunEvent::Overdue {
        now,
        limits: limits.clone(),
    };
    let mut tx = pool.begin().await?;
    let ended = apply_end(&mut tx, run_id, event, env).await?;
    tx.commit().await?;
    Ok(ended)
}

async fn apply_end(
    tx: &mut Transaction<'_, Postgres>,
    run_id: RunId,
    event: RunEvent,
    env: &RunEnv<'_>,
) -> Result<Option<EndedRun>, DbError> {
    let applied = apply_run_event(tx, run_id, event, env).await?;
    Ok(applied
        .filter(|applied| applied.changed)
        .and_then(|applied| {
            Some(EndedRun {
                task_id: applied.task_id,
                run_state: applied.state,
                task_status: applied.task_status?,
            })
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
    let event = RunEvent::SandboxStarted {
        pod_ip: pod_ip.to_string(),
    };
    apply_in_tx(pool, events, run_id, event).await
}

/// Records the runtime's first connect and moves an early run to `preparing`. Returns the run as
/// it is now, `None` if it is already terminal.
pub async fn mark_connected(pool: &PgPool, run_id: RunId) -> Result<Option<RunRow>, DbError> {
    let mut tx = pool.begin().await?;
    apply_run_event(&mut tx, run_id, RunEvent::Connected, &RunEnv::none()).await?;
    let run = get_run(&mut *tx, run_id)
        .await?
        .filter(|run| !run.state.is_terminal());
    tx.commit().await?;
    Ok(run)
}

/// The scheduler picks `run_id`: `pending` → `provisioning`, sets `provisioning_started_at` and
/// emits `sandbox.provisioning`. Returns whether the run was moved.
pub async fn begin_provisioning(
    pool: &PgPool,
    events: &EventSinks,
    run_id: RunId,
) -> Result<bool, DbError> {
    apply_in_tx(pool, events, run_id, RunEvent::ProvisioningStarted).await
}

/// Moves the run to `finalizing` for `reason` in a transaction that holds the run and task row
/// locks (`lock`, the lock `queue_message` takes), so a message queued meanwhile is either seen
/// here or sees the run `finalizing` and queues the task for the next run. Returns whether the
/// run was moved: [`FinalizeReason::Idle`] only moves an `awaiting_input` run without an
/// undelivered message, [`FinalizeReason::Budget`] any run that is not ending.
pub async fn start_finalizing(
    tx: &mut Transaction<'_, Postgres>,
    lock: &RunTaskLock,
    reason: FinalizeReason,
) -> Result<bool, DbError> {
    let applied = apply_run_event(
        tx,
        lock.run_id(),
        RunEvent::Finalize(reason),
        &RunEnv::none(),
    )
    .await?;
    Ok(applied.is_some_and(|applied| applied.changed))
}

/// Applies `event` in a transaction of its own; whether it changed the run.
async fn apply_in_tx(
    pool: &PgPool,
    events: &EventSinks,
    run_id: RunId,
    event: RunEvent,
) -> Result<bool, DbError> {
    let mut tx = pool.begin().await?;
    let applied = apply_run_event(&mut tx, run_id, event, &RunEnv::events_only(events)).await?;
    tx.commit().await?;
    Ok(applied.is_some_and(|applied| applied.changed))
}

/// The task's run that has not ended, if it has one.
pub(crate) async fn active_run_of_task(
    executor: impl PgExecutor<'_>,
    task_id: TaskId,
) -> Result<Option<RunId>, DbError> {
    Ok(sqlx::query_scalar!(
        "SELECT id FROM runs WHERE task_id = $1 AND state <> ALL($2)",
        task_id.as_uuid(),
        &state_list(RunState::TERMINAL)
    )
    .fetch_optional(executor)
    .await?
    .map(RunId::from_uuid))
}

/// The run of the task that waits for the next message in its keep-alive window.
pub(crate) async fn idle_run_of_task(
    executor: impl PgExecutor<'_>,
    task_id: TaskId,
) -> Result<Option<RunId>, DbError> {
    Ok(sqlx::query_scalar!(
        "SELECT id FROM runs WHERE task_id = $1 AND state = $2",
        task_id.as_uuid(),
        RunState::AwaitingInput.as_str()
    )
    .fetch_optional(executor)
    .await?
    .map(RunId::from_uuid))
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

async fn notify_run(
    tx: &mut Transaction<'_, Postgres>,
    run_id: RunId,
    task_id: TaskId,
) -> Result<(), DbError> {
    publish(&mut **tx, &Notification::RunChanged { run_id, task_id }).await
}

#[cfg(test)]
mod tests;
