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
                   AND state NOT IN ('finalizing', 'succeeded', 'failed', 'cancelled')",
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
#[expect(
    clippy::too_many_lines,
    reason = "split persist_batch into dedupe, gap check and apply steps"
)]
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
mod tests;
