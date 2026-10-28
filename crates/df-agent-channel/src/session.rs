//! One connected agent stream: handshake, reliable persistence with cumulative acks, prompt
//! delivery and off-stream handling of tool calls and credential requests (design D3).
//!
//! A session keeps no state that the database does not have, so a reconnect to any replica
//! produces the same commands.

use std::collections::HashSet;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use df_core::{RunOutcome, RunState, TaskStatus};
use df_db::{Notification, PgBlobStore, publish};
use df_proto::agent::v1::RunOutcome as WireOutcome;
use df_proto::agent::v1::{
    AgentMessage, Assign, Cancel, ControlMessage, CredentialRequest, HarnessCredentials, HelloAck,
    PushReport, Release, RunPhase, RunReport, ToolCallRequest, agent_message, control_message,
};
use df_proto::transcript::v1::{Event, ToolCallStatus, ToolKind, ToolOrigin, event};
use futures::{FutureExt, Stream, StreamExt};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;
use tokio::time::Instant;
use tonic::Status;

use crate::assign::{RunContext, build_assign, load_run_context, pending_prompts};
use crate::error::ChannelError;
use crate::metrics::Observation;
use crate::runs::{
    EndedRun, RunEnd, RunRow, end_run_tx, finish_run, get_run, has_undelivered_message, lock_task,
    mark_connected, set_idle_tx, set_run_state_tx,
};
use crate::scheduler::cancel_deadline;
use crate::{ChannelContext, credentials, pr, tools, transcript};

/// Time the runtime has to send `Hello` after the stream was accepted.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
/// Messages persisted in one transaction.
const MAX_BATCH: usize = 50;
/// Optional protocol features this control plane supports.
const CAPABILITIES: &[&str] = &["transcript.images"];
/// Time granted to in-flight tool calls and credential requests when the stream has ended.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(60);
/// Time granted to the forge work after a push report or the end of a run.
const FORGE_TIMEOUT: Duration = Duration::from_secs(60);
/// Longest timer armed for a deadline; `tokio::time::sleep` panics on durations it cannot add.
const MAX_TIMER: Duration = Duration::from_secs(365 * 24 * 60 * 60);

type Outbound = mpsc::Sender<Result<ControlMessage, Status>>;

/// Serves one authenticated stream until the runtime or the control plane closes it.
#[tracing::instrument(name = "agent.session", skip_all, fields(run_id = %run.id))]
pub async fn run<S>(
    ctx: ChannelContext,
    run: RunRow,
    mut inbound: S,
    out: Outbound,
    stop: watch::Receiver<bool>,
) -> Result<(), Status>
where
    S: Stream<Item = Result<AgentMessage, Status>> + Unpin,
{
    match tokio::time::timeout(HANDSHAKE_TIMEOUT, inbound.next()).await {
        Err(_) => return Err(Status::deadline_exceeded("no Hello received")),
        Ok(None) => return Ok(()),
        Ok(Some(Err(status))) => {
            tracing::warn!(error = %status, run_id = %run.id, "stream failed before Hello");
            return Err(wire_status(status));
        }
        Ok(Some(Ok(AgentMessage {
            body: Some(agent_message::Body::Hello(_)),
            ..
        }))) => {}
        Ok(Some(Ok(_))) => {
            return Err(Status::failed_precondition(
                "the first message must be Hello",
            ));
        }
    }

    let run = get_run(&ctx.pool, run.id)
        .await?
        .filter(|r| !r.state.is_terminal())
        .ok_or_else(|| Status::unauthenticated("run is not active"))?;
    let mut rc = load_run_context(&ctx.pool, run).await?;
    // A run that connected before has had a stream.
    let _connection = ctx
        .metrics
        .connection(&rc.project.key, rc.run.connected_at.is_some());

    let credentials = match credentials::harness_credentials(&ctx, &rc).await {
        Ok(c) => c,
        Err(missing) => {
            let detail = missing.to_string();
            finish_run(
                &ctx,
                &RunEnd::failure(rc.run.id, RunOutcome::Failed, &detail),
            )
            .await?;
            return Err(Status::failed_precondition(detail));
        }
    };
    // The runtime is up: stop the connect timeout clock and start the wall-clock budget.
    rc.run = mark_connected(&ctx.pool, rc.run.id)
        .await?
        .ok_or_else(|| Status::unauthenticated("run is not active"))?;

    let assign = build_assign(&ctx, &rc).await?;
    let hello_ack = HelloAck {
        run_id: rc.run.id.to_string(),
        last_persisted_seq: rc.run.last_persisted_seq,
        capabilities: CAPABILITIES.iter().map(ToString::to_string).collect(),
    };
    let busy = busy_on_connect(&ctx.pool, rc.run.id, rc.task.id).await?;
    let mut session = Session {
        ctx,
        rc: Arc::new(rc),
        out,
        sent_prompts: HashSet::new(),
        busy,
        tasks: JoinSet::new(),
        ended: false,
        idle_at: None,
        done_cancel_sent: false,
        release_sent: false,
    };
    let result = session
        .serve(inbound, hello_ack, credentials, assign, stop)
        .await;
    session.drain().await;
    result
}

struct Session {
    ctx: ChannelContext,
    rc: Arc<RunContext>,
    out: Outbound,
    sent_prompts: HashSet<String>,
    /// The runtime is in a turn: persisted `TurnStarted` without a later `TurnEnded`. Queued
    /// messages are held back meanwhile (design D1 of `ui-polish`).
    busy: bool,
    /// Tool call and credential handlers; awaited when the stream ends (see [`Session::drain`]).
    tasks: JoinSet<()>,
    /// Set once this session persisted the run's `RunReport`.
    ended: bool,
    /// When the idle run is to be released: last persisted `TurnEnded` plus
    /// `runs.keepAliveSeconds`. `None` unless the run is `awaiting_input` and no prompt is in
    /// flight.
    idle_at: Option<Instant>,
    /// This session sent `Cancel` because the task was marked done.
    done_cancel_sent: bool,
    /// This session sent `Release`.
    release_sent: bool,
}

/// What a persisted batch asks the session to do afterwards.
#[derive(Default)]
struct Effects {
    tool_calls: Vec<ToolCallRequest>,
    credentials: Vec<CredentialRequest>,
    pushes: Vec<PushReport>,
    run_end: Option<(RunReport, EndedRun)>,
}

struct Persisted {
    ack: Option<u64>,
    effects: Effects,
    /// Turn state after the batch if it persisted a turn event.
    busy: Option<bool>,
    /// The batch persisted a `TurnEnded`.
    turn_ended: bool,
    /// The batch changed whether the run is idle (idle or running phase, turn event).
    idle_changed: bool,
    /// `(expected, got)` if the batch contained a sequence gap.
    gap: Option<(u64, u64)>,
    /// What the committed messages say happened; recorded as metrics by the session.
    observations: Vec<Observation>,
}

impl Session {
    async fn serve<S>(
        &mut self,
        mut inbound: S,
        hello_ack: HelloAck,
        credentials: HarnessCredentials,
        assign: Assign,
        mut stop: watch::Receiver<bool>,
    ) -> Result<(), Status>
    where
        S: Stream<Item = Result<AgentMessage, Status>> + Unpin,
    {
        // HarnessCredentials precede Assign: the runtime ignores credentials that arrive later.
        let handshake = [
            control(control_message::Body::HelloAck(hello_ack)),
            control(control_message::Body::HarnessCredentials(credentials)),
            control(control_message::Body::Assign(assign)),
        ];
        for msg in handshake {
            if !self.send(msg).await {
                return Ok(());
            }
        }
        if !self.send_new_prompts().await? {
            return Ok(());
        }
        // A run that was idle (or already released) before this stream existed: a replica that
        // takes over computes the same deadline from the persisted turn end.
        self.refresh_idle().await?;
        if !self.reconcile_finalizing().await? {
            return Ok(());
        }

        let mut notifications = self.ctx.notifier.subscribe();
        let cancel_at = cancel_deadline(self.rc.run.budget_start(), &self.ctx.config.runs);
        let cancel_in = cancel_at.map_or(MAX_TIMER, |at| {
            (at - Utc::now())
                .to_std()
                .unwrap_or_default()
                .min(MAX_TIMER)
        });
        let cancel = tokio::time::sleep(cancel_in);
        tokio::pin!(cancel);
        let mut cancel_sent = cancel_at.is_none();

        loop {
            let idle = async {
                match self.idle_at {
                    Some(at) => tokio::time::sleep_until(at).await,
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                // The control plane is shutting down: stop reading; `run` lets in-flight
                // handlers finish before the stream ends.
                () = stop_requested(&mut stop) => return Ok(()),
                msg = inbound.next() => {
                    let first = match msg {
                        Some(Ok(first)) => first,
                        Some(Err(status)) => return Err(wire_status(status)),
                        None => return Ok(()),
                    };
                    let mut batch = vec![first];
                    let mut stream_error = None;
                    // Take what is already buffered; never wait for more.
                    while batch.len() < MAX_BATCH {
                        match inbound.next().now_or_never() {
                            Some(Some(Ok(next))) => batch.push(next),
                            Some(Some(Err(status))) => {
                                stream_error = Some(status);
                                break;
                            }
                            Some(None) | None => break,
                        }
                    }
                    if !self.handle_batch(batch).await? {
                        return Ok(());
                    }
                    if let Some(status) = stream_error {
                        return Err(wire_status(status));
                    }
                }
                note = notifications.recv() => {
                    if matches!(&note, Ok(Notification::RunChanged { run_id, .. }) if *run_id == self.rc.run.id)
                        && !self.reconcile_finalizing().await?
                    {
                        return Ok(());
                    }
                    let relevant = match note {
                        Ok(Notification::TaskMessageQueued { task_id }) => task_id == self.rc.task.id,
                        Ok(Notification::Reconnected) | Err(RecvError::Lagged(_)) => true,
                        Ok(_) => false,
                        Err(RecvError::Closed) => return Ok(()),
                    };
                    if relevant && !self.send_new_prompts().await? {
                        return Ok(());
                    }
                }
                () = &mut cancel, if !cancel_sent => {
                    cancel_sent = true;
                    if !self.active().await? {
                        return Ok(());
                    }
                    // `finalizing` first: no prompt may reach a run that is being cancelled.
                    self.start_finalizing(false).await?;
                    if !self.send_cancel("wall-clock budget exceeded").await {
                        return Ok(());
                    }
                }
                () = idle => {
                    if !self.release_if_idle().await? {
                        return Ok(());
                    }
                }
                Some(_) = self.tasks.join_next() => {}
            }
        }
    }

    /// Releases the idle run at its deadline: `finalizing` first (in the transaction that checks
    /// for undelivered messages), then `Release`. Returns `false` once the peer is gone or the
    /// run ended.
    async fn release_if_idle(&mut self) -> Result<bool, ChannelError> {
        self.idle_at = None;
        if !self.active().await? {
            return Ok(false);
        }
        if !self.start_finalizing(true).await? {
            return Ok(true);
        }
        self.release_sent = true;
        Ok(self.send_release().await)
    }

    /// Moves the run to `finalizing` in a transaction that holds the task row lock (the one
    /// `queue_message` takes), so a message queued meanwhile is either seen here or sees the
    /// run `finalizing` and queues the task for the next run. With `only_idle` the move happens
    /// only for an `awaiting_input` run without an undelivered message (the idle `Release`);
    /// otherwise for any run that is not ending yet (the budget `Cancel`). Returns whether the
    /// run was moved.
    async fn start_finalizing(&self, only_idle: bool) -> Result<bool, ChannelError> {
        let mut tx = self.ctx.pool.begin().await?;
        // Lock order: run row, then task row (see `lock_task`).
        sqlx::query("SELECT 1 FROM runs WHERE id = $1 FOR UPDATE")
            .bind(self.rc.run.id.as_uuid())
            .execute(&mut *tx)
            .await?;
        lock_task(&mut tx, self.rc.task.id).await?;
        let moved = if only_idle {
            !has_undelivered_message(&mut tx, self.rc.task.id).await?
                && sqlx::query(
                    "UPDATE runs SET state = 'finalizing' WHERE id = $1 AND state = 'awaiting_input'",
                )
                .bind(self.rc.run.id.as_uuid())
                .execute(&mut *tx)
                .await?
                .rows_affected()
                    > 0
        } else {
            sqlx::query(
                "UPDATE runs SET state = 'finalizing'
                 WHERE id = $1
                   AND state NOT IN ('finalizing', 'succeeded', 'blocked', 'failed', 'cancelled')",
            )
            .bind(self.rc.run.id.as_uuid())
            .execute(&mut *tx)
            .await?
            .rows_affected()
                > 0
        };
        if moved {
            publish(
                &mut *tx,
                &Notification::RunChanged {
                    run_id: self.rc.run.id,
                    task_id: self.rc.task.id,
                },
            )
            .await?;
        }
        tx.commit().await?;
        Ok(moved)
    }

    /// Re-arms the idle timer from the database: the run is idle when it is `awaiting_input` and
    /// no prompt is in flight.
    async fn refresh_idle(&mut self) -> Result<(), ChannelError> {
        self.idle_at = None;
        if self.busy {
            return Ok(());
        }
        let awaiting = get_run(&self.ctx.pool, self.rc.run.id)
            .await?
            .is_some_and(|run| run.state == RunState::AwaitingInput);
        if !awaiting {
            return Ok(());
        }
        let Some(turn_end) = last_turn_end(&self.ctx.pool, self.rc.run.id).await? else {
            return Ok(());
        };
        self.idle_at = idle_instant(turn_end, self.ctx.config.runs.keep_alive_seconds);
        Ok(())
    }

    /// Commands for a run the control plane has moved to `finalizing`: `Cancel` when the task was
    /// marked done, `Release` when the idle deadline passed (again after a reconnect, when the
    /// first one was lost with the old stream). Returns `false` once the peer is gone or the run
    /// ended.
    async fn reconcile_finalizing(&mut self) -> Result<bool, ChannelError> {
        let Some(run) = get_run(&self.ctx.pool, self.rc.run.id).await? else {
            return Ok(false);
        };
        if run.state.is_terminal() || self.ended {
            return Ok(false);
        }
        if run.state != RunState::Finalizing {
            return Ok(true);
        }
        let status: String = sqlx::query_scalar("SELECT status FROM tasks WHERE id = $1")
            .bind(self.rc.task.id.as_uuid())
            .fetch_one(&self.ctx.pool)
            .await?;
        let status: TaskStatus = status.parse()?;
        if status == TaskStatus::Completed {
            if self.done_cancel_sent {
                return Ok(true);
            }
            self.done_cancel_sent = true;
            return Ok(self.send_cancel("task marked done").await);
        }
        // A budget `Cancel` also leaves the run `finalizing`. Once the budget deadline has passed
        // the budget timer sends it (also on a replica that takes over after the first one was
        // lost), so an idle `Release` must not follow.
        let budget_cancelled = cancel_deadline(self.rc.run.budget_start(), &self.ctx.config.runs)
            .is_some_and(|at| at <= Utc::now());
        if budget_cancelled
            || self.release_sent
            || last_turn_end(&self.ctx.pool, self.rc.run.id)
                .await?
                .is_none()
        {
            return Ok(true);
        }
        self.release_sent = true;
        Ok(self.send_release().await)
    }

    async fn send_release(&self) -> bool {
        self.send(control(control_message::Body::Release(Release {})))
            .await
    }

    async fn send_cancel(&self, reason: &str) -> bool {
        let cancel = Cancel {
            reason: reason.to_string(),
        };
        self.send(control(control_message::Body::Cancel(cancel)))
            .await
    }

    /// Lets in-flight tool calls and credential requests finish after the stream ended: their
    /// effects are recorded by `call_id` and must not be lost to a closing stream. Bounded, so a
    /// stuck forge cannot keep the session alive; whatever is left is aborted and re-sent by the
    /// runtime after a reconnect.
    async fn drain(&mut self) {
        let finish = async { while self.tasks.join_next().await.is_some() {} };
        if tokio::time::timeout(DRAIN_TIMEOUT, finish).await.is_err() {
            tracing::warn!(run_id = %self.rc.run.id, "aborting handlers still running after the stream ended");
        }
    }

    /// Whether commands may still be sent: this session has not seen the run end and the
    /// database (another replica may have ended it) agrees.
    async fn active(&self) -> Result<bool, ChannelError> {
        Ok(self.run_state().await?.is_some())
    }

    /// State of the run if this session may still command it, `None` once it ended.
    async fn run_state(&self) -> Result<Option<RunState>, ChannelError> {
        if self.ended {
            return Ok(None);
        }
        Ok(get_run(&self.ctx.pool, self.rc.run.id)
            .await?
            .map(|run| run.state)
            .filter(|state| !state.is_terminal()))
    }

    /// Runs forge work beside the stream loop, so a slow forge neither stops acks nor delays
    /// `Cancel` and prompts. Bounded by [`FORGE_TIMEOUT`]; the work is best effort anyway.
    fn spawn_forge_work(&mut self, work: impl Future<Output = ()> + Send + 'static) {
        let run_id = self.rc.run.id;
        self.tasks.spawn(async move {
            if tokio::time::timeout(FORGE_TIMEOUT, work).await.is_err() {
                tracing::warn!(run_id = %run_id, "forge work timed out");
            }
        });
    }

    /// Returns `false` once the peer is gone.
    async fn send(&self, msg: ControlMessage) -> bool {
        self.out.send(Ok(msg)).await.is_ok()
    }

    /// Sends prompts not sent on this stream yet: forwarded ones that are still undelivered and,
    /// between turns, the held-back ones (which this marks as forwarded). `false` ends the
    /// session: the peer is gone or the run ended, so a prompt must stay queued for the next run.
    async fn send_new_prompts(&mut self) -> Result<bool, ChannelError> {
        match self.run_state().await? {
            None => return Ok(false),
            // Ending (`Release`, `Cancel`): a prompt sent now could be lost with the turn it
            // interrupts. It stays queued; the run end makes the task `queued` for the next run.
            Some(RunState::Finalizing) => return Ok(true),
            Some(_) => {}
        }
        for prompt in pending_prompts(&self.ctx.pool, self.rc.task.id, self.busy).await? {
            if !self.sent_prompts.insert(prompt.prompt_id.clone()) {
                continue;
            }
            if !self
                .send(control(control_message::Body::Prompt(prompt)))
                .await
            {
                return Ok(false);
            }
            // The runtime starts a turn for it: hold back new messages until the persisted
            // `TurnEnded` clears this, not only once `TurnStarted` is persisted.
            self.busy = true;
            self.idle_at = None;
        }
        Ok(true)
    }

    /// Persists the batch, acks it, then runs its effects. `false` ends the session.
    async fn handle_batch(&mut self, batch: Vec<AgentMessage>) -> Result<bool, Status> {
        // Asked before the batch is persisted: a `RunReport` in this very batch must not
        // refuse the tool calls that precede it.
        let active = self.active().await?;
        let persisted = persist_batch(&self.ctx, &self.rc, batch).await?;
        // The batch is committed; replayed messages produced no observation.
        self.ctx
            .metrics
            .record(&self.rc.project.key, &persisted.observations);
        if let Some(ack) = persisted.ack
            && !self.send(ControlMessage { ack, body: None }).await
        {
            return Ok(false);
        }
        // The messages before a gap are committed and acked: their effects must still happen.
        self.run_effects(persisted.effects, active).await;
        if let Some(busy) = persisted.busy {
            self.busy = busy;
        }
        if persisted.turn_ended && !self.ended && !self.send_new_prompts().await? {
            return Ok(false);
        }
        if persisted.idle_changed && !self.ended {
            self.refresh_idle().await?;
        }
        if let Some((expected, got)) = persisted.gap {
            return Err(Status::data_loss(format!(
                "sequence gap: expected {expected}, got {got}"
            )));
        }
        Ok(true)
    }

    /// `active`: the run had not ended when the batch arrived. Tool calls and credential requests
    /// of an ended run are refused.
    async fn run_effects(&mut self, effects: Effects, active: bool) {
        for call in effects.tool_calls {
            let (ctx, rc, out) = (self.ctx.clone(), self.rc.clone(), self.out.clone());
            self.tasks.spawn(async move {
                let result = if active {
                    tools::execute(&ctx, &rc, call).await
                } else {
                    tools::run_ended(&call.call_id)
                };
                // The runtime re-sends unanswered calls after a reconnect.
                let _ = out
                    .send(Ok(control(control_message::Body::ToolCallResult(result))))
                    .await;
            });
        }
        for request in effects.credentials {
            let (ctx, rc, out) = (self.ctx.clone(), self.rc.clone(), self.out.clone());
            self.tasks.spawn(async move {
                let grant = if active {
                    credentials::grant(&ctx, &rc, request).await
                } else {
                    credentials::run_ended(request.request_id)
                };
                let _ = out
                    .send(Ok(control(control_message::Body::CredentialGrant(grant))))
                    .await;
            });
        }
        for push in effects.pushes {
            let (ctx, rc) = (self.ctx.clone(), self.rc.clone());
            self.spawn_forge_work(async move { pr::on_push_report(&ctx, &rc, &push).await });
        }
        if let Some((report, ended)) = effects.run_end {
            self.ended = true;
            crate::runs::teardown(&self.ctx, self.rc.run.id).await;
            let completed = report.outcome() == WireOutcome::Completed;
            let (ctx, rc) = (self.ctx.clone(), self.rc.clone());
            self.spawn_forge_work(async move {
                pr::on_run_end(&ctx, &rc, completed, report.workspace.as_ref()).await;
            });
            tracing::info!(
                run_id = %self.rc.run.id,
                task_id = %ended.task_id,
                task_status = %ended.task_status,
                "run ended"
            );
        }
    }
}

/// Resolves when a stop was requested. A dropped sender never stops a session.
async fn stop_requested(stop: &mut watch::Receiver<bool>) {
    if stop.wait_for(|stop| *stop).await.is_err() {
        std::future::pending::<()>().await;
    }
}

/// tonic reports an oversized inbound message as `OUT_OF_RANGE`; the protocol promises
/// `RESOURCE_EXHAUSTED`.
fn wire_status(status: Status) -> Status {
    if status.code() == tonic::Code::OutOfRange {
        Status::resource_exhausted(status.message().to_string())
    } else {
        status
    }
}

fn control(body: control_message::Body) -> ControlMessage {
    ControlMessage {
        ack: 0,
        body: Some(body),
    }
}

/// Stores a batch in one transaction: drops already persisted messages, stops at a gap, applies
/// the rest in order and advances `last_persisted_seq`.
#[tracing::instrument(name = "agent.persist_batch", skip_all, fields(messages = batch.len()))]
async fn persist_batch(
    ctx: &ChannelContext,
    rc: &RunContext,
    batch: Vec<AgentMessage>,
) -> Result<Persisted, ChannelError> {
    // Events a replay repeats are dropped below; don't move their blobs again. The authoritative
    // check happens under the row lock in the transaction.
    let already_persisted: i64 =
        sqlx::query_scalar("SELECT last_persisted_seq FROM runs WHERE id = $1")
            .bind(rc.run.id.as_uuid())
            .fetch_one(&ctx.pool)
            .await?;
    let already_persisted = u64::try_from(already_persisted).unwrap_or_default();
    let blobs = PgBlobStore::new(ctx.pool.clone());
    let mut prepared = std::collections::HashMap::new();
    for msg in &batch {
        if let Some(agent_message::Body::TranscriptEvent(event)) = &msg.body
            && msg.seq > already_persisted
        {
            prepared.insert(
                msg.seq,
                transcript::prepare(&blobs, msg.seq, event.clone()).await?,
            );
        }
    }

    let mut tx = ctx.pool.begin().await?;
    let stored: i64 =
        sqlx::query_scalar("SELECT last_persisted_seq FROM runs WHERE id = $1 FOR UPDATE")
            .bind(rc.run.id.as_uuid())
            .fetch_one(&mut *tx)
            .await?;
    // Event ids are the task-wide cursor of the web UI's live transcript, so they must become
    // visible in id order. The run lock alone allows another run of the same task (a lost run's
    // late batch) to commit a lower id after this one. Same lock order as `end_run`: run, task (see `lock_task`).
    lock_task(&mut tx, rc.task.id).await?;
    let start = u64::try_from(stored)
        .map_err(|_| ChannelError::InvalidData("negative last_persisted_seq".into()))?;
    let mut last = start;
    let mut effects = Effects::default();
    let mut observations = Vec::new();
    let mut gap = None;
    let mut newest_event = None;
    let mut busy = None;
    let mut turn_ended = false;
    let mut idle_changed = false;

    for msg in batch {
        if matches!(msg.body, Some(agent_message::Body::Hello(_))) || msg.seq <= last {
            continue;
        }
        if msg.seq != last + 1 {
            gap = Some((last + 1, msg.seq));
            break;
        }
        // A body this version does not know (an additive change within v1) still takes its
        // place in the sequence; skipping it would make the next message look like a gap.
        let Some(body) = msg.body else {
            tracing::warn!(run_id = %rc.run.id, seq = msg.seq, "ignoring message without a known body");
            last = msg.seq;
            continue;
        };
        match body {
            agent_message::Body::Hello(_) => {}
            agent_message::Body::PhaseChanged(change) => {
                match change.phase() {
                    RunPhase::Preparing => {
                        set_run_state_tx(&mut tx, rc.run.id, RunState::Preparing, None).await?;
                    }
                    RunPhase::Finalizing => {
                        set_run_state_tx(&mut tx, rc.run.id, RunState::Finalizing, None).await?;
                    }
                    // The task row is locked above: a message queued concurrently is either
                    // visible here or waits for this transaction.
                    RunPhase::Running => {
                        set_idle_tx(&mut tx, rc.run.id, rc.task.id, false).await?;
                        idle_changed = true;
                    }
                    RunPhase::AwaitingInput => {
                        set_idle_tx(&mut tx, rc.run.id, rc.task.id, true).await?;
                        idle_changed = true;
                    }
                    RunPhase::Unspecified => {}
                }
            }
            agent_message::Body::TranscriptEvent(raw) => {
                if let Some(event) = prepared.get(&msg.seq)
                    && let Some(id) =
                        transcript::insert(&mut tx, rc.task.id, rc.run.id, event).await?
                {
                    newest_event = newest_event.max(Some(id));
                    observations.extend(observe_event(&mut tx, rc, &raw).await?);
                    match event.kind {
                        "turn_started" => {
                            busy = Some(true);
                            idle_changed = true;
                        }
                        "turn_ended" => {
                            busy = Some(false);
                            turn_ended = true;
                            idle_changed = true;
                        }
                        _ => {}
                    }
                }
            }
            agent_message::Body::ToolCallRequest(call) => effects.tool_calls.push(call),
            agent_message::Body::CredentialRequest(request) => effects.credentials.push(request),
            agent_message::Body::PushReport(push) => {
                observations.push(Observation::Push {
                    target: push.target(),
                    status: push.status(),
                });
                effects.pushes.push(push);
            }
            agent_message::Body::RuntimeTiming(timing) => {
                // Not stored: it only takes its place in the sequence.
                observations.push(Observation::Runtime {
                    operation: timing.operation(),
                    duration: Duration::from_millis(timing.duration_ms),
                    ok: timing.ok,
                });
            }
            agent_message::Body::RunReport(report) => {
                if let Some(ended) = end_run_tx(&mut tx, &run_end(rc, &report)).await? {
                    observations.push(Observation::RunEnded {
                        outcome: report.outcome(),
                        duration: (Utc::now() - rc.run.started_at)
                            .to_std()
                            .unwrap_or_default(),
                    });
                    effects.run_end = Some((report, ended));
                }
            }
        }
        last = msg.seq;
    }

    if last > start {
        sqlx::query("UPDATE runs SET last_persisted_seq = $2 WHERE id = $1")
            .bind(rc.run.id.as_uuid())
            .bind(
                i64::try_from(last)
                    .map_err(|_| ChannelError::InvalidData("seq out of range".into()))?,
            )
            .execute(&mut *tx)
            .await?;
    }
    if let Some(id) = newest_event {
        publish(
            &mut *tx,
            &Notification::TranscriptEvent {
                task_id: rc.task.id,
                id,
            },
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Persisted {
        ack: (last > start).then_some(last),
        effects,
        busy,
        turn_ended,
        idle_changed,
        gap,
        observations,
    })
}

/// The observations a newly stored transcript event stands for: ended turns and tool calls that
/// reached a terminal status.
async fn observe_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    rc: &RunContext,
    event: &Event,
) -> Result<Option<Observation>, ChannelError> {
    let terminal = |status| matches!(status, ToolCallStatus::Completed | ToolCallStatus::Failed);
    Ok(match &event.kind {
        Some(event::Kind::TurnEnded(ended)) => Some(Observation::Turn {
            stop_reason: ended.stop_reason(),
            usage: ended.usage.clone(),
        }),
        Some(event::Kind::ToolCall(call)) if terminal(call.status()) => {
            Some(Observation::ToolCall {
                kind: call.kind(),
                origin: call.origin(),
                status: call.status(),
            })
        }
        Some(event::Kind::ToolCallUpdate(update)) if update.status.is_some() => {
            let status = update.status();
            if !terminal(status) {
                return Ok(None);
            }
            // An update names neither origin nor (usually) kind: take them from the call.
            let (kind, origin) = stored_tool_call(tx, rc, &update.tool_call_id)
                .await?
                .unwrap_or_default();
            Some(Observation::ToolCall {
                kind: update.kind.map_or(kind, |_| update.kind()),
                origin,
                status,
            })
        }
        _ => None,
    })
}

/// Kind and origin of the stored `ToolCall` event of `tool_call_id` in this run.
async fn stored_tool_call(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    rc: &RunContext,
    tool_call_id: &str,
) -> Result<Option<(ToolKind, ToolOrigin)>, ChannelError> {
    let payload: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT payload FROM transcript_events
         WHERE run_id = $1 AND kind = 'tool_call' AND payload -> 'toolCall' ->> 'toolCallId' = $2
         ORDER BY seq LIMIT 1",
    )
    .bind(rc.run.id.as_uuid())
    .bind(tool_call_id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(event) = payload.and_then(|p| serde_json::from_value::<Event>(p).ok()) else {
        return Ok(None);
    };
    Ok(match event.kind {
        Some(event::Kind::ToolCall(call)) => Some((call.kind(), call.origin())),
        _ => None,
    })
}

/// Whether the run's last persisted turn event is a `TurnStarted`.
/// Whether a new stream starts busy: a turn is open, or a prompt was forwarded but not confirmed
/// yet (the runtime starts a turn for it, its `TurnStarted` may not be persisted).
async fn busy_on_connect(
    pool: &sqlx::PgPool,
    run_id: df_core::RunId,
    task_id: df_core::TaskId,
) -> Result<bool, ChannelError> {
    Ok(turn_open(pool, run_id).await? || !pending_prompts(pool, task_id, true).await?.is_empty())
}

/// Kind (`turn_started` or `turn_ended`) and time of the run's last persisted turn event.
async fn last_turn_event(
    pool: &sqlx::PgPool,
    run_id: df_core::RunId,
) -> Result<Option<(String, DateTime<Utc>)>, ChannelError> {
    Ok(sqlx::query_as(
        "SELECT kind, occurred_at FROM transcript_events
         WHERE run_id = $1 AND kind IN ('turn_started', 'turn_ended')
         ORDER BY seq DESC LIMIT 1",
    )
    .bind(run_id.as_uuid())
    .fetch_optional(pool)
    .await?)
}

async fn turn_open(pool: &sqlx::PgPool, run_id: df_core::RunId) -> Result<bool, ChannelError> {
    Ok(last_turn_event(pool, run_id)
        .await?
        .is_some_and(|(kind, _)| kind == "turn_started"))
}

/// When the run's last persisted turn ended, if the last turn event is a `TurnEnded`.
async fn last_turn_end(
    pool: &sqlx::PgPool,
    run_id: df_core::RunId,
) -> Result<Option<DateTime<Utc>>, ChannelError> {
    Ok(last_turn_event(pool, run_id)
        .await?
        .and_then(|(kind, at)| (kind == "turn_ended").then_some(at)))
}

/// The instant `keep_alive_seconds` after `turn_end`; `None` if that is not representable.
fn idle_instant(turn_end: DateTime<Utc>, keep_alive_seconds: u64) -> Option<Instant> {
    let keep_alive = chrono::TimeDelta::try_seconds(i64::try_from(keep_alive_seconds).ok()?)?;
    let wait = (turn_end.checked_add_signed(keep_alive)? - Utc::now())
        .to_std()
        .unwrap_or_default()
        .min(MAX_TIMER);
    Some(Instant::now() + wait)
}

fn run_end(rc: &RunContext, report: &RunReport) -> RunEnd {
    let (outcome, fallback) = match report.outcome() {
        WireOutcome::Completed => (RunOutcome::Completed, ""),
        WireOutcome::EndTurn => (RunOutcome::EndTurn, ""),
        WireOutcome::Blocked => (RunOutcome::Blocked, ""),
        WireOutcome::LimitReached => (RunOutcome::LimitReached, ""),
        WireOutcome::RateLimited => (RunOutcome::RateLimited, ""),
        WireOutcome::Cancelled => (RunOutcome::Cancelled, ""),
        WireOutcome::Failed => (RunOutcome::Failed, ""),
        WireOutcome::Unspecified => (RunOutcome::Failed, "the runtime reported no outcome"),
    };
    let workspace = report.workspace.clone().unwrap_or_default();
    RunEnd {
        run_id: rc.run.id,
        outcome,
        detail: if report.detail.is_empty() {
            fallback.to_string()
        } else {
            report.detail.clone()
        },
        retry_after: report
            .retry_after
            .as_ref()
            .and_then(|t| DateTime::from_timestamp(t.seconds, u32::try_from(t.nanos).ok()?)),
        harness_session_id: report.harness_session_id.clone(),
        task_branch_sha: workspace.task_branch_sha,
        wip_sha: workspace.wip_sha,
        diverged: workspace.diverged,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::probe::Probe;
    use crate::testing::Env;
    use df_core::testing::SandboxCall;
    use df_proto::agent::v1::{PushStatus, PushTarget, RuntimeOperation, RuntimeTiming};
    use df_proto::transcript::v1 as wire;

    fn msg(seq: u64, body: agent_message::Body) -> AgentMessage {
        AgentMessage {
            seq,
            body: Some(body),
        }
    }

    fn hello() -> AgentMessage {
        msg(
            0,
            agent_message::Body::Hello(df_proto::agent::v1::Hello::default()),
        )
    }

    fn text_event() -> agent_message::Body {
        agent_message::Body::TranscriptEvent(wire::Event {
            schema_version: 1,
            occurred_at: None,
            turn_id: "t".into(),
            kind: Some(wire::event::Kind::AgentMessageChunk(
                wire::AgentMessageChunk {
                    message_id: "m".into(),
                    content: Some(wire::ContentBlock {
                        kind: Some(wire::content_block::Kind::Text(wire::TextContent {
                            text: "x".into(),
                        })),
                        hidden: false,
                    }),
                },
            )),
        })
    }

    fn report(outcome: WireOutcome) -> agent_message::Body {
        agent_message::Body::RunReport(RunReport {
            outcome: outcome as i32,
            ..RunReport::default()
        })
    }

    /// Runs a session over a fixed list of inbound messages that are all available at once, so
    /// they form a single batch.
    async fn run_session(
        env: &Env,
        run_id: df_core::RunId,
        messages: Vec<AgentMessage>,
    ) -> Result<(), Status> {
        run_session_with_output(env, run_id, messages).await.0
    }

    /// Like [`run_session`], also returning what the session sent.
    async fn run_session_with_output(
        env: &Env,
        run_id: df_core::RunId,
        messages: Vec<AgentMessage>,
    ) -> (Result<(), Status>, Vec<ControlMessage>) {
        let row = get_run(&env.ctx.pool, run_id).await.unwrap().unwrap();
        let (tx, mut rx) = mpsc::channel(64);
        let inbound = futures::stream::iter(messages.into_iter().map(Ok));
        let result = run(env.ctx.clone(), row, inbound, tx, never_stops()).await;
        let sent = std::iter::from_fn(|| rx.try_recv().ok())
            .filter_map(Result::ok)
            .collect();
        (result, sent)
    }

    fn never_stops() -> watch::Receiver<bool> {
        watch::channel(false).1
    }

    #[tokio::test]
    async fn effects_before_a_sequence_gap_still_happen() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool.clone()).await;
        let run_id = env.running_run().await;

        // RunReport (seq 1) and a message after a gap (seq 3) arrive in one batch.
        let result = run_session(
            &env,
            run_id,
            vec![
                hello(),
                msg(1, report(WireOutcome::EndTurn)),
                msg(3, text_event()),
            ],
        )
        .await;

        assert_eq!(result.unwrap_err().code(), tonic::Code::DataLoss);
        let run = get_run(&pool, run_id).await.unwrap().unwrap();
        assert_eq!(run.state, RunState::Succeeded);
        assert_eq!(run.last_persisted_seq, 1);
        assert!(
            env.provider
                .calls()
                .contains(&SandboxCall::Teardown(run_id))
        );
    }

    #[tokio::test]
    async fn transcript_writes_wait_for_the_task_lock() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool.clone()).await;
        let run_id = env.running_run().await;
        let row = get_run(&pool, run_id).await.unwrap().unwrap();
        let rc = load_run_context(&pool, row).await.unwrap();

        // A late batch of another run of the same task (e.g. a run marked lost while its sandbox
        // still sends) has taken an event id but not committed yet. If this run's batch committed
        // first, a reader already past its id would never see the other one.
        let earlier = env.insert_run(env.task.id, 2, "failed").await;
        let mut other = pool.begin().await.unwrap();
        sqlx::query(
            "INSERT INTO transcript_events (task_id, run_id, seq, occurred_at, kind, payload)
             VALUES ($1, $2, 1, now(), 'agent_message_chunk', '{}')",
        )
        .bind(env.task.id.as_uuid())
        .bind(earlier.as_uuid())
        .execute(&mut *other)
        .await
        .unwrap();
        let ctx = env.ctx.clone();
        let write =
            tokio::spawn(async move { persist_batch(&ctx, &rc, vec![msg(1, text_event())]).await });

        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let waiting: i64 = sqlx::query_scalar(
                    "SELECT count(*) FROM pg_stat_activity
                     WHERE datname = current_database() AND wait_event_type = 'Lock'",
                )
                .fetch_one(&pool)
                .await
                .unwrap();
                if waiting > 0 {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("the batch did not wait for the task lock");
        assert!(!write.is_finished());

        other.commit().await.unwrap();
        write.await.unwrap().unwrap();
        let ids: Vec<(i64, uuid::Uuid)> =
            sqlx::query_as("SELECT id, run_id FROM transcript_events ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        let runs: Vec<_> = ids.iter().map(|(_, run)| *run).collect();
        assert_eq!(runs, [earlier.as_uuid(), run_id.as_uuid()]);
    }

    #[tokio::test]
    async fn tool_calls_finish_after_the_stream_ended() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool.clone()).await;
        let run_id = env.running_run().await;
        let call = ToolCallRequest {
            call_id: uuid::Uuid::now_v7().to_string(),
            tool_name: "report_progress".into(),
            arguments_json: r#"{"summary":"almost done"}"#.into(),
        };

        // The stream ends right after the request, as when the runtime closes after RunReport.
        run_session(
            &env,
            run_id,
            vec![hello(), msg(1, agent_message::Body::ToolCallRequest(call))],
        )
        .await
        .unwrap();

        let line: String = sqlx::query_scalar("SELECT status_line FROM tasks WHERE id = $1")
            .bind(env.task.id.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(line, "almost done");
        let stored: i64 =
            sqlx::query_scalar("SELECT count(*) FROM tool_calls WHERE result IS NOT NULL")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(stored, 1);
    }

    fn phase(phase: RunPhase) -> agent_message::Body {
        agent_message::Body::PhaseChanged(df_proto::agent::v1::PhaseChanged {
            phase: phase as i32,
            detail: String::new(),
        })
    }

    async fn state_and_status(env: &Env, run_id: df_core::RunId) -> (RunState, String) {
        let run = get_run(&env.ctx.pool, run_id).await.unwrap().unwrap();
        let status = sqlx::query_scalar("SELECT status FROM tasks WHERE id = $1")
            .bind(env.task.id.as_uuid())
            .fetch_one(&env.ctx.pool)
            .await
            .unwrap();
        (run.state, status)
    }

    /// A run in `state` of a task that is `running`, without undelivered messages.
    async fn run_of_running_task(env: &Env, state: &str) -> df_core::RunId {
        let run_id = env.insert_run(env.task.id, 1, state).await;
        env.bind_pod(run_id);
        sqlx::query("UPDATE tasks SET status = 'running' WHERE id = $1")
            .bind(env.task.id.as_uuid())
            .execute(&env.ctx.pool)
            .await
            .unwrap();
        sqlx::query("UPDATE task_messages SET delivered_at = now() WHERE task_id = $1")
            .bind(env.task.id.as_uuid())
            .execute(&env.ctx.pool)
            .await
            .unwrap();
        run_id
    }

    #[tokio::test]
    async fn the_idle_phase_makes_the_run_awaiting_input_and_the_task_waiting() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool.clone()).await;
        let run_id = run_of_running_task(&env, "running").await;

        run_session(
            &env,
            run_id,
            vec![hello(), msg(1, phase(RunPhase::AwaitingInput))],
        )
        .await
        .unwrap();
        assert_eq!(
            state_and_status(&env, run_id).await,
            (RunState::AwaitingInput, "waiting".to_string())
        );

        run_session(
            &env,
            run_id,
            vec![hello(), msg(2, phase(RunPhase::Running))],
        )
        .await
        .unwrap();
        assert_eq!(
            state_and_status(&env, run_id).await,
            (RunState::Running, "running".to_string())
        );
    }

    #[tokio::test]
    async fn the_idle_phase_keeps_the_task_running_while_a_message_is_undelivered() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool.clone()).await;
        let run_id = run_of_running_task(&env, "running").await;
        let prompt_id = queue_human(&env, "one more thing").await;

        let (result, sent) = run_session_with_output(
            &env,
            run_id,
            vec![hello(), msg(1, phase(RunPhase::AwaitingInput))],
        )
        .await;
        result.unwrap();
        assert_eq!(
            state_and_status(&env, run_id).await,
            (RunState::AwaitingInput, "running".to_string())
        );
        // The session forwards the message to the idle run.
        let prompts: Vec<_> = sent
            .into_iter()
            .filter_map(|c| match c.body {
                Some(control_message::Body::Prompt(p)) => Some(p.prompt_id),
                _ => None,
            })
            .collect();
        assert_eq!(prompts, [prompt_id]);
    }

    #[tokio::test]
    async fn idle_phases_do_not_reopen_a_finalizing_run() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool.clone()).await;
        let run_id = run_of_running_task(&env, "finalizing").await;
        sqlx::query("UPDATE tasks SET status = 'completed' WHERE id = $1")
            .bind(env.task.id.as_uuid())
            .execute(&pool)
            .await
            .unwrap();

        run_session(
            &env,
            run_id,
            vec![hello(), msg(1, phase(RunPhase::Running))],
        )
        .await
        .unwrap();
        assert_eq!(
            state_and_status(&env, run_id).await,
            (RunState::Finalizing, "completed".to_string())
        );
    }

    /// Persists a `TurnEnded` of the run that occurred at `at`.
    async fn persist_turn_end(env: &Env, run_id: df_core::RunId, at: DateTime<Utc>) {
        sqlx::query(
            "INSERT INTO transcript_events (task_id, run_id, seq, occurred_at, kind, payload)
             VALUES ($1, $2, 1, $3, 'turn_ended', '{}')",
        )
        .bind(env.task.id.as_uuid())
        .bind(run_id.as_uuid())
        .bind(at)
        .execute(&env.ctx.pool)
        .await
        .unwrap();
    }

    fn with_keep_alive(env: &mut Env, seconds: u64) {
        env.ctx.config = Arc::new(crate::ChannelConfig {
            runs: df_core::config::RunsConfig {
                keep_alive_seconds: seconds,
                ..Default::default()
            },
            ..Default::default()
        });
    }

    /// Serves `run_id` on a stream that stays open and returns the time the first control
    /// message accepted by `wanted` was sent, with the run state at that moment. Fails if none
    /// comes within 30 s.
    async fn serve_until(
        env: &Env,
        run_id: df_core::RunId,
        wanted: impl Fn(&control_message::Body) -> bool,
    ) -> (DateTime<Utc>, RunState) {
        let row = get_run(&env.ctx.pool, run_id).await.unwrap().unwrap();
        let (to_session, inbound) = mpsc::channel(8);
        let (tx, mut rx) = mpsc::channel(64);
        to_session.send(Ok(hello())).await.unwrap();
        let ctx = env.ctx.clone();
        let session = tokio::spawn(async move {
            run(
                ctx,
                row,
                tokio_stream::wrappers::ReceiverStream::new(inbound),
                tx,
                never_stops(),
            )
            .await
        });
        let found = tokio::time::timeout(std::time::Duration::from_secs(30), async {
            while let Some(Ok(control)) = rx.recv().await {
                if control.body.as_ref().is_some_and(&wanted) {
                    return Some(Utc::now());
                }
            }
            None
        })
        .await
        .expect("the command in time")
        .expect("the stream ended without the command");
        let (state, _) = state_and_status(env, run_id).await;
        drop(to_session);
        session.await.unwrap().unwrap();
        (found, state)
    }

    /// Serves an idle run whose turn ended at `turn_end` and returns when `Release` was sent,
    /// with the run state at that moment. The deadline is wall-clock (a persisted timestamp plus
    /// the keep-alive), so these tests wait in real time; the keep-alive is at most 1 s.
    async fn wait_for_release(
        keep_alive: u64,
        turn_end: impl FnOnce() -> DateTime<Utc>,
    ) -> (DateTime<Utc>, RunState) {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let mut env = Env::new(pool.clone()).await;
        with_keep_alive(&mut env, keep_alive);
        let run_id = run_of_running_task(&env, "awaiting_input").await;
        persist_turn_end(&env, run_id, turn_end()).await;
        serve_until(&env, run_id, |b| {
            matches!(b, control_message::Body::Release(_))
        })
        .await
    }

    /// `persist_batch` holds the run row and then wants the task row; `start_finalizing` must
    /// wait for the run row first instead of holding the task row, or both deadlock.
    #[tokio::test]
    async fn start_finalizing_takes_the_run_lock_before_the_task_lock() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool.clone()).await;
        let run_id = run_of_running_task(&env, "awaiting_input").await;
        let (session, _rx) = session_for(&env, run_id).await;
        let mut other = pool.begin().await.unwrap();
        sqlx::query("SELECT 1 FROM runs WHERE id = $1 FOR UPDATE")
            .bind(run_id.as_uuid())
            .execute(&mut *other)
            .await
            .unwrap();
        let task_id = env.task.id;
        let finalizing = tokio::spawn(async move { session.start_finalizing(false).await });
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let waiting: i64 = sqlx::query_scalar(
                    "SELECT count(*) FROM pg_stat_activity
                     WHERE datname = current_database() AND wait_event_type = 'Lock'",
                )
                .fetch_one(&pool)
                .await
                .unwrap();
                if waiting > 0 {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("start_finalizing did not wait for the run lock");

        // Without the run lock first, `start_finalizing` now holds the task row: deadlock.
        lock_task(&mut other, task_id).await.unwrap();
        other.commit().await.unwrap();
        assert!(finalizing.await.unwrap().unwrap());
    }

    #[tokio::test]
    async fn a_budget_cancel_is_never_followed_by_a_release() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let mut env = Env::new(pool.clone()).await;
        env.ctx.config = Arc::new(crate::ChannelConfig {
            runs: df_core::config::RunsConfig {
                max_wall_clock_seconds: 0,
                cancel_grace_seconds: 0,
                ..Default::default()
            },
            ..Default::default()
        });
        let run_id = run_of_running_task(&env, "awaiting_input").await;
        persist_turn_end(&env, run_id, Utc::now()).await;
        let (mut session, mut rx) = session_for(&env, run_id).await;

        // The session that sent the budget `Cancel` sees its own `finalizing` run.
        assert!(session.start_finalizing(false).await.unwrap());
        assert!(session.reconcile_finalizing().await.unwrap());
        // A replica that takes over later sees the same.
        let (mut other, mut other_rx) = session_for(&env, run_id).await;
        assert!(other.reconcile_finalizing().await.unwrap());

        assert!(rx.try_recv().is_err() && other_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn the_budget_cancel_finalizes_the_run_first_and_no_prompt_follows() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let mut env = Env::new(pool.clone()).await;
        env.ctx.config = Arc::new(crate::ChannelConfig {
            runs: df_core::config::RunsConfig {
                max_wall_clock_seconds: 0,
                cancel_grace_seconds: 0,
                ..Default::default()
            },
            ..Default::default()
        });
        let run_id = run_of_running_task(&env, "running").await;

        // The run is `finalizing` when `Cancel` goes out.
        let (_, state) = serve_until(&env, run_id, |b| {
            matches!(b, control_message::Body::Cancel(_))
        })
        .await;
        assert_eq!(state, RunState::Finalizing);

        // A message queued afterwards is not forwarded to the cancelled run.
        let (mut session, mut rx) = session_for(&env, run_id).await;
        queue_human(&env, "too late").await;
        assert!(session.send_new_prompts().await.unwrap());
        assert!(sent_prompt_ids(&mut rx).is_empty());
        assert_eq!(state_and_status(&env, run_id).await.1, "queued");
    }

    #[tokio::test]
    async fn the_idle_run_is_released_one_keep_alive_after_its_turn_end() {
        let turn_end = Utc::now();
        let (released_at, state) = wait_for_release(1, || turn_end).await;
        assert!(released_at - turn_end >= chrono::Duration::seconds(1));
        // `finalizing` is set before `Release` is sent.
        assert_eq!(state, RunState::Finalizing);
    }

    #[tokio::test]
    async fn a_keep_alive_of_zero_releases_right_after_the_turn() {
        let turn_end = Utc::now();
        let (released_at, state) = wait_for_release(0, || turn_end).await;
        assert!(released_at - turn_end < chrono::Duration::seconds(30));
        assert_eq!(state, RunState::Finalizing);
    }

    #[tokio::test]
    async fn a_new_session_after_the_deadline_releases_after_the_handshake() {
        let (_, state) =
            wait_for_release(600, || Utc::now() - chrono::Duration::seconds(3600)).await;
        assert_eq!(state, RunState::Finalizing);
    }

    #[tokio::test]
    async fn a_run_with_a_message_in_flight_is_not_released() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let mut env = Env::new(pool.clone()).await;
        with_keep_alive(&mut env, 0);
        let run_id = run_of_running_task(&env, "awaiting_input").await;
        persist_turn_end(&env, run_id, Utc::now()).await;
        let (mut session, mut rx) = session_for(&env, run_id).await;

        // A prompt was forwarded and its turn has not been confirmed yet.
        session.busy = true;
        session.refresh_idle().await.unwrap();
        assert!(session.idle_at.is_none());

        // The message was queued after the last forwarding: the deadline does not release.
        session.busy = false;
        queue_human(&env, "wait for me").await;
        assert!(session.release_if_idle().await.unwrap());
        assert!(rx.try_recv().is_err());
        assert_eq!(
            state_and_status(&env, run_id).await.0,
            RunState::AwaitingInput
        );
    }

    #[tokio::test]
    async fn a_message_for_an_idle_run_starts_no_run_and_is_forwarded_at_once() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool.clone()).await;
        let run_id = run_of_running_task(&env, "awaiting_input").await;
        sqlx::query("UPDATE tasks SET status = 'waiting' WHERE id = $1")
            .bind(env.task.id.as_uuid())
            .execute(&pool)
            .await
            .unwrap();
        let (mut session, mut rx) = session_for(&env, run_id).await;

        let prompt_id = queue_human(&env, "and one more thing").await;

        assert_eq!(state_and_status(&env, run_id).await.1, "running");
        assert!(
            crate::runs::start_queued_runs(&pool, 10)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(session.send_new_prompts().await.unwrap());
        assert_eq!(sent_prompt_ids(&mut rx), [prompt_id]);
    }

    #[tokio::test]
    async fn marking_the_task_done_cancels_its_finalizing_run_once() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool.clone()).await;
        let run_id = run_of_running_task(&env, "awaiting_input").await;
        sqlx::query("UPDATE tasks SET status = 'waiting' WHERE id = $1")
            .bind(env.task.id.as_uuid())
            .execute(&pool)
            .await
            .unwrap();
        let (mut session, mut rx) = session_for(&env, run_id).await;

        // Nothing to do while the run is idle.
        assert!(session.reconcile_finalizing().await.unwrap());
        assert!(rx.try_recv().is_err());

        df_db::mark_task_done(
            &pool,
            env.task.id,
            df_core::Principal::human(df_core::UserId::new(), "Ada"),
        )
        .await
        .unwrap();
        assert!(session.reconcile_finalizing().await.unwrap());
        assert!(session.reconcile_finalizing().await.unwrap());

        let sent: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok())
            .filter_map(|m| m.ok().and_then(|c| c.body))
            .collect();
        assert!(
            matches!(sent.as_slice(), [control_message::Body::Cancel(_)]),
            "{sent:?}"
        );
    }

    #[tokio::test]
    async fn no_prompt_is_sent_to_a_finalizing_run_and_its_end_queues_the_task() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool.clone()).await;
        let run_id = run_of_running_task(&env, "finalizing").await;
        let (mut session, mut rx) = session_for(&env, run_id).await;

        queue_human(&env, "too late").await;
        assert!(session.send_new_prompts().await.unwrap());
        assert!(sent_prompt_ids(&mut rx).is_empty());

        let end = RunEnd::failure(run_id, RunOutcome::EndTurn, "");
        crate::runs::end_run(&pool, &end).await.unwrap();
        assert_eq!(state_and_status(&env, run_id).await.1, "queued");
    }

    async fn session_for(
        env: &Env,
        run_id: df_core::RunId,
    ) -> (Session, mpsc::Receiver<Result<ControlMessage, Status>>) {
        let row = get_run(&env.ctx.pool, run_id).await.unwrap().unwrap();
        let rc = load_run_context(&env.ctx.pool, row).await.unwrap();
        let (tx, rx) = mpsc::channel(64);
        let session = Session {
            ctx: env.ctx.clone(),
            rc: Arc::new(rc),
            out: tx,
            sent_prompts: HashSet::new(),
            busy: false,
            tasks: JoinSet::new(),
            ended: false,
            idle_at: None,
            done_cancel_sent: false,
            release_sent: false,
        };
        (session, rx)
    }

    #[tokio::test]
    async fn a_hanging_forge_does_not_block_the_stream() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool).await;
        let run_id = env.running_run().await;
        env.forge.hang_find_open_pull_request();
        let (mut session, _rx) = session_for(&env, run_id).await;
        let push = agent_message::Body::PushReport(PushReport {
            target: df_proto::agent::v1::PushTarget::TaskBranch as i32,
            status: df_proto::agent::v1::PushStatus::Ok as i32,
            commits_ahead_of_base: 1,
            ..PushReport::default()
        });

        let handled = tokio::time::timeout(
            Duration::from_secs(10),
            session.handle_batch(vec![msg(1, push)]),
        )
        .await;

        assert!(handled.is_ok(), "the stream loop waited for the forge");
    }

    #[tokio::test]
    async fn tool_calls_and_credentials_are_refused_after_the_run_ended() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool.clone()).await;
        let run_id = env.running_run().await;
        let (mut session, mut rx) = session_for(&env, run_id).await;
        // The scheduler ended the run (hard stop); the stream is still open.
        sqlx::query("UPDATE runs SET state = 'failed' WHERE id = $1")
            .bind(run_id.as_uuid())
            .execute(&pool)
            .await
            .unwrap();
        let call = ToolCallRequest {
            call_id: uuid::Uuid::now_v7().to_string(),
            tool_name: "report_progress".into(),
            arguments_json: r#"{"summary":"too late"}"#.into(),
        };
        let credential = CredentialRequest {
            request_id: "r1".into(),
            kind: None,
        };

        let batch = vec![
            msg(1, agent_message::Body::ToolCallRequest(call)),
            msg(2, agent_message::Body::CredentialRequest(credential)),
        ];
        assert!(session.handle_batch(batch).await.unwrap());
        session.drain().await;

        let mut refused = (false, false);
        while let Ok(Ok(control)) = rx.try_recv() {
            match control.body {
                Some(control_message::Body::ToolCallResult(r)) => refused.0 = r.is_error,
                Some(control_message::Body::CredentialGrant(g)) => {
                    refused.1 = matches!(
                        g.result,
                        Some(df_proto::agent::v1::credential_grant::Result::DeniedReason(
                            _
                        ))
                    );
                }
                _ => {}
            }
        }
        assert_eq!(refused, (true, true));
        let line: String = sqlx::query_scalar("SELECT status_line FROM tasks WHERE id = $1")
            .bind(env.task.id.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_ne!(line, "too late");
    }

    #[tokio::test]
    async fn a_stream_error_before_hello_is_reported() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool).await;
        let run_id = env.running_run().await;
        let row = get_run(&env.ctx.pool, run_id).await.unwrap().unwrap();
        let inbound = futures::stream::iter([Err(Status::out_of_range("too large"))]);
        let (tx, _rx) = mpsc::channel(1);

        let status = run(env.ctx.clone(), row, inbound, tx, never_stops())
            .await
            .unwrap_err();

        assert_eq!(status.code(), tonic::Code::ResourceExhausted);
    }

    #[tokio::test]
    async fn a_tool_call_in_flight_at_shutdown_stores_its_result() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool.clone()).await;
        let run_id = env.running_run().await;
        sqlx::query("UPDATE tasks SET issue_number = 42 WHERE id = $1")
            .bind(env.task.id.as_uuid())
            .execute(&pool)
            .await
            .unwrap();
        let forge_reply = env.forge.hold_issue_comments();
        let call = ToolCallRequest {
            call_id: uuid::Uuid::now_v7().to_string(),
            tool_name: "comment_on_issue".into(),
            arguments_json: r#"{"body":"hello"}"#.into(),
        };
        let row = get_run(&pool, run_id).await.unwrap().unwrap();
        let (inbound_tx, inbound_rx) = mpsc::channel(8);
        let (out_tx, _out_rx) = mpsc::channel(64);
        let (stop_tx, stop_rx) = watch::channel(false);
        let ctx = env.ctx.clone();
        let session = tokio::spawn(run(
            ctx,
            row,
            tokio_stream::wrappers::ReceiverStream::new(inbound_rx),
            out_tx,
            stop_rx,
        ));
        inbound_tx.send(Ok(hello())).await.unwrap();
        inbound_tx
            .send(Ok(msg(1, agent_message::Body::ToolCallRequest(call))))
            .await
            .unwrap();
        // The forge call is under way.
        while env.forge.calls().is_empty() {
            tokio::task::yield_now().await;
        }

        stop_tx.send_replace(true);
        forge_reply.add_permits(1);
        session.await.unwrap().unwrap();

        let stored: i64 =
            sqlx::query_scalar("SELECT count(*) FROM tool_calls WHERE result IS NOT NULL")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(stored, 1);
    }

    #[tokio::test]
    async fn no_prompts_are_sent_once_the_run_ended() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool.clone()).await;
        let run_id = env.running_run().await;

        // Active run: the assignment prompt goes out.
        let (mut session, mut rx) = session_for(&env, run_id).await;
        assert!(session.send_new_prompts().await.unwrap());
        assert!(rx.try_recv().is_ok());

        // Ended in this session.
        let (mut session, mut rx) = session_for(&env, run_id).await;
        session.ended = true;
        assert!(!session.send_new_prompts().await.unwrap());
        assert!(rx.try_recv().is_err());

        // Ended elsewhere (another replica): the database says so.
        sqlx::query("UPDATE runs SET state = 'succeeded' WHERE id = $1")
            .bind(run_id.as_uuid())
            .execute(&pool)
            .await
            .unwrap();
        let (mut session, mut rx) = session_for(&env, run_id).await;
        assert!(!session.send_new_prompts().await.unwrap());
        assert!(rx.try_recv().is_err());
    }
    fn turn_event(kind: wire::event::Kind) -> agent_message::Body {
        agent_message::Body::TranscriptEvent(wire::Event {
            schema_version: 1,
            occurred_at: None,
            turn_id: "t".into(),
            kind: Some(kind),
        })
    }

    fn turn_started() -> agent_message::Body {
        turn_event(wire::event::Kind::TurnStarted(wire::TurnStarted::default()))
    }

    fn turn_ended() -> agent_message::Body {
        turn_event(wire::event::Kind::TurnEnded(wire::TurnEnded::default()))
    }

    /// Queues a board member's message and returns its prompt id.
    async fn queue_human(env: &Env, text: &str) -> String {
        let content = vec![wire::ContentBlock {
            kind: Some(wire::content_block::Kind::Text(wire::TextContent {
                text: text.into(),
            })),
            hidden: false,
        }];
        let author = df_core::Principal::human(df_core::UserId::new(), "Ada");
        df_db::queue_message(&env.ctx.pool, env.task.id, author, content)
            .await
            .unwrap()
            .id
            .to_string()
    }

    fn sent_prompt_ids(rx: &mut mpsc::Receiver<Result<ControlMessage, Status>>) -> Vec<String> {
        let mut ids = Vec::new();
        while let Ok(Ok(control)) = rx.try_recv() {
            if let Some(control_message::Body::Prompt(p)) = control.body {
                ids.push(p.prompt_id);
            }
        }
        ids
    }

    async fn forwarded(env: &Env, prompt_id: &str) -> bool {
        sqlx::query_scalar::<_, bool>(
            "SELECT forwarded_at IS NOT NULL FROM task_messages WHERE id = $1::uuid",
        )
        .bind(prompt_id)
        .fetch_one(&env.ctx.pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn busy_follows_the_persisted_turn_events_of_the_run() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool.clone()).await;
        let run_id = env.running_run().await;
        let (mut session, mut rx) = session_for(&env, run_id).await;
        assert!(!turn_open(&pool, run_id).await.unwrap());
        assert!(session.send_new_prompts().await.unwrap());
        sent_prompt_ids(&mut rx);

        session
            .handle_batch(vec![msg(1, turn_started())])
            .await
            .unwrap();
        // A reconnect during the turn derives the same state.
        assert!(session.busy);
        assert!(turn_open(&pool, run_id).await.unwrap());

        session
            .handle_batch(vec![msg(2, turn_ended())])
            .await
            .unwrap();
        assert!(!session.busy);
        assert!(!turn_open(&pool, run_id).await.unwrap());
    }

    #[tokio::test]
    async fn a_message_queued_during_a_turn_is_sent_when_the_turn_ends() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool).await;
        let run_id = env.running_run().await;
        let (mut session, mut rx) = session_for(&env, run_id).await;
        assert!(session.send_new_prompts().await.unwrap());
        let assignment = sent_prompt_ids(&mut rx);
        assert_eq!(assignment.len(), 1);
        session
            .handle_batch(vec![msg(1, turn_started())])
            .await
            .unwrap();

        let queued = queue_human(&env, "also check logout").await;
        assert!(session.send_new_prompts().await.unwrap());
        assert!(sent_prompt_ids(&mut rx).is_empty());
        assert!(!forwarded(&env, &queued).await);

        session
            .handle_batch(vec![msg(2, turn_ended())])
            .await
            .unwrap();
        assert_eq!(sent_prompt_ids(&mut rx), vec![queued.clone()]);
        assert!(forwarded(&env, &queued).await);
    }

    #[tokio::test]
    async fn a_message_queued_before_the_turn_start_is_persisted_is_held_back() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool).await;
        let run_id = env.running_run().await;
        let (mut session, mut rx) = session_for(&env, run_id).await;
        assert!(session.send_new_prompts().await.unwrap());
        sent_prompt_ids(&mut rx);

        // The runtime is in the turn for the assignment, its `TurnStarted` not persisted yet.
        let queued = queue_human(&env, "quick follow-up").await;
        assert!(session.send_new_prompts().await.unwrap());

        assert!(sent_prompt_ids(&mut rx).is_empty());
        assert!(!forwarded(&env, &queued).await);
    }

    #[tokio::test]
    async fn a_message_on_an_idle_run_is_forwarded_at_once() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool).await;
        let run_id = env.running_run().await;
        let (mut session, mut rx) = session_for(&env, run_id).await;
        assert!(session.send_new_prompts().await.unwrap());
        sent_prompt_ids(&mut rx);
        let batch = vec![msg(1, turn_started()), msg(2, turn_ended())];
        session.handle_batch(batch).await.unwrap();

        let queued = queue_human(&env, "next").await;
        assert!(session.send_new_prompts().await.unwrap());

        assert_eq!(sent_prompt_ids(&mut rx), vec![queued.clone()]);
        assert!(forwarded(&env, &queued).await);
    }

    #[tokio::test]
    async fn a_forwarded_undelivered_message_is_sent_again_on_reconnect() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool).await;
        let run_id = env.running_run().await;
        let (mut first, mut first_rx) = session_for(&env, run_id).await;
        assert!(first.send_new_prompts().await.unwrap());
        let sent = sent_prompt_ids(&mut first_rx);

        // The new stream is busy (turn open) yet repeats what was forwarded before.
        let (mut second, mut second_rx) = session_for(&env, run_id).await;
        second.busy = true;
        assert!(second.send_new_prompts().await.unwrap());

        assert_eq!(sent_prompt_ids(&mut second_rx), sent);
    }

    #[tokio::test]
    async fn a_reconnect_before_the_turn_start_is_persisted_keeps_new_messages_held_back() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool.clone()).await;
        let run_id = env.running_run().await;
        let (mut first, mut first_rx) = session_for(&env, run_id).await;
        assert!(first.send_new_prompts().await.unwrap());
        let sent = sent_prompt_ids(&mut first_rx);
        drop(first);
        let queued = queue_human(&env, "quick follow-up").await;

        // The runtime is in the turn for the assignment; no `TurnStarted` persisted.
        let (mut second, mut second_rx) = session_for(&env, run_id).await;
        second.busy = busy_on_connect(&pool, run_id, env.task.id).await.unwrap();
        assert!(second.send_new_prompts().await.unwrap());

        assert_eq!(sent_prompt_ids(&mut second_rx), sent);
        assert!(!forwarded(&env, &queued).await);
    }

    #[tokio::test]
    async fn a_run_that_ends_during_a_turn_keeps_the_message_held_back() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool.clone()).await;
        let run_id = env.running_run().await;
        let (mut session, mut rx) = session_for(&env, run_id).await;
        assert!(session.send_new_prompts().await.unwrap());
        sent_prompt_ids(&mut rx);
        let batch = vec![msg(1, turn_started()), msg(2, report(WireOutcome::EndTurn))];
        session.handle_batch(batch).await.unwrap();
        let queued = queue_human(&env, "too late").await;

        assert!(!session.send_new_prompts().await.unwrap());

        assert!(sent_prompt_ids(&mut rx).is_empty());
        assert!(!forwarded(&env, &queued).await);
    }

    // --- metrics (`specs/telemetry/spec.md`) ---

    /// An environment whose channel records into the returned probe.
    async fn metrics_env() -> (Env, Probe) {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let mut env = Env::new(pool).await;
        let probe = Probe::new();
        env.ctx.metrics = Arc::new(crate::Metrics::new(&probe.meter()));
        (env, probe)
    }

    fn timing(operation: RuntimeOperation, duration_ms: u64) -> agent_message::Body {
        agent_message::Body::RuntimeTiming(RuntimeTiming {
            operation: operation as i32,
            duration_ms,
            ok: true,
        })
    }

    fn turn_ended_with_usage() -> agent_message::Body {
        turn_event(wire::event::Kind::TurnEnded(wire::TurnEnded {
            stop_reason: wire::StopReason::EndTurn as i32,
            usage: vec![wire::Usage {
                model_id: "claude-sonnet-5-5".into(),
                input_tokens: 100,
                output_tokens: 20,
                ..wire::Usage::default()
            }],
            ..wire::TurnEnded::default()
        }))
    }

    fn tool_call(status: wire::ToolCallStatus) -> agent_message::Body {
        turn_event(wire::event::Kind::ToolCall(wire::ToolCall {
            tool_call_id: "call-1".into(),
            kind: wire::ToolKind::Execute as i32,
            origin: wire::ToolOrigin::Harness as i32,
            status: status as i32,
            ..wire::ToolCall::default()
        }))
    }

    fn tool_call_update(status: wire::ToolCallStatus) -> agent_message::Body {
        turn_event(wire::event::Kind::ToolCallUpdate(wire::ToolCallUpdate {
            tool_call_id: "call-1".into(),
            status: Some(status as i32),
            ..wire::ToolCallUpdate::default()
        }))
    }

    fn push_report() -> agent_message::Body {
        agent_message::Body::PushReport(PushReport {
            target: PushTarget::TaskBranch as i32,
            status: PushStatus::Ok as i32,
            ..PushReport::default()
        })
    }

    #[tokio::test]
    async fn a_runtime_timing_takes_its_place_in_the_sequence_without_a_row() {
        let (env, probe) = metrics_env().await;
        let run_id = env.running_run().await;

        run_session(
            &env,
            run_id,
            vec![
                hello(),
                msg(1, timing(RuntimeOperation::Turn, 1500)),
                msg(2, text_event()),
            ],
        )
        .await
        .unwrap();

        let run = get_run(&env.ctx.pool, run_id).await.unwrap().unwrap();
        assert_eq!(run.last_persisted_seq, 2);
        let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM transcript_events")
            .fetch_one(&env.ctx.pool)
            .await
            .unwrap();
        assert_eq!(rows, 1, "only the text event is stored");
        assert_eq!(
            probe.total(
                "df.agent.runtime.duration",
                &[("operation", "turn"), ("project", "DF")]
            ),
            1.0
        );
    }

    #[tokio::test]
    async fn every_persisted_message_is_recorded_once() {
        let (env, probe) = metrics_env().await;
        let run_id = env.running_run().await;
        let (mut session, _rx) = session_for(&env, run_id).await;
        let batch = vec![
            msg(1, tool_call(wire::ToolCallStatus::InProgress)),
            msg(2, tool_call_update(wire::ToolCallStatus::Completed)),
            msg(3, turn_ended_with_usage()),
            msg(4, push_report()),
            msg(5, timing(RuntimeOperation::Push, 40)),
            msg(6, report(WireOutcome::Completed)),
        ];

        session.handle_batch(batch.clone()).await.unwrap();
        // A reconnect replays the batch.
        session.handle_batch(batch).await.unwrap();

        let project = ("project", "DF");
        let call = [
            ("kind", "execute"),
            ("origin", "harness"),
            ("status", "completed"),
            project,
        ];
        assert_eq!(probe.total("df.agent.tool_calls", &[]), 1.0);
        assert_eq!(probe.total("df.agent.tool_calls", &call), 1.0);
        assert_eq!(
            probe.total("df.agent.turns", &[("stop_reason", "end_turn"), project]),
            1.0
        );
        assert_eq!(
            probe.total("df.agent.tokens", &[("token_type", "input"), project]),
            100.0
        );
        assert_eq!(
            probe.total("df.agent.tokens", &[("token_type", "output"), project]),
            20.0
        );
        assert_eq!(
            probe.total(
                "df.agent.pushes",
                &[("target", "task_branch"), ("status", "ok"), project]
            ),
            1.0
        );
        assert_eq!(
            probe.total("df.agent.runtime.duration", &[("operation", "push")]),
            1.0
        );
        assert_eq!(
            probe.total("df.runs.ended", &[("outcome", "completed"), project]),
            1.0
        );
        assert_eq!(
            probe.total("df.run.duration", &[("outcome", "completed")]),
            1.0
        );
    }

    #[tokio::test]
    async fn a_rolled_back_batch_records_nothing_until_it_is_persisted() {
        let (env, probe) = metrics_env().await;
        let run_id = env.running_run().await;
        let (mut session, _rx) = session_for(&env, run_id).await;
        // The last statement of every batch fails, after the messages were applied.
        sqlx::raw_sql(
            "CREATE FUNCTION refuse() RETURNS trigger LANGUAGE plpgsql AS
               $$ BEGIN RAISE EXCEPTION 'refused'; END $$;
             CREATE TRIGGER refuse BEFORE UPDATE OF last_persisted_seq ON runs
               FOR EACH ROW EXECUTE FUNCTION refuse();",
        )
        .execute(&env.ctx.pool)
        .await
        .unwrap();
        let batch = vec![msg(1, report(WireOutcome::Completed))];

        assert!(session.handle_batch(batch.clone()).await.is_err());
        assert_eq!(probe.total("df.runs.ended", &[]), 0.0);

        sqlx::raw_sql("DROP TRIGGER refuse ON runs")
            .execute(&env.ctx.pool)
            .await
            .unwrap();
        session.handle_batch(batch).await.unwrap();
        assert_eq!(
            probe.total("df.runs.ended", &[("outcome", "completed")]),
            1.0
        );
    }

    #[tokio::test]
    async fn streams_are_counted_and_reconnects_noticed() {
        let (env, probe) = metrics_env().await;
        let run_id = env.running_run().await;

        run_session(&env, run_id, vec![hello()]).await.unwrap();
        assert_eq!(probe.total("df.agent.reconnects", &[]), 0.0);
        run_session(&env, run_id, vec![hello()]).await.unwrap();

        assert_eq!(
            probe.total("df.agent.reconnects", &[("project", "DF")]),
            1.0
        );
        assert_eq!(probe.total("df.agent.connections", &[]), 0.0);
    }
}
