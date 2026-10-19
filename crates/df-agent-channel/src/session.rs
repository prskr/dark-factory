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
use df_core::{RunOutcome, RunState};
use df_db::{Notification, PgBlobStore, publish};
use df_proto::agent::v1::RunOutcome as WireOutcome;
use df_proto::agent::v1::{
    AgentMessage, Assign, Cancel, ControlMessage, CredentialRequest, HarnessCredentials, HelloAck,
    PushReport, RunPhase, RunReport, ToolCallRequest, agent_message, control_message,
};
use futures::{FutureExt, Stream, StreamExt};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;
use tonic::Status;

use crate::assign::{RunContext, build_assign, load_run_context, pending_prompts};
use crate::error::ChannelError;
use crate::runs::{
    EndedRun, RunEnd, RunRow, end_run_tx, finish_run, get_run, mark_connected, set_run_state_tx,
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
    let mut session = Session {
        ctx,
        rc: Arc::new(rc),
        out,
        sent_prompts: HashSet::new(),
        tasks: JoinSet::new(),
        ended: false,
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
    /// Tool call and credential handlers; awaited when the stream ends (see [`Session::drain`]).
    tasks: JoinSet<()>,
    /// Set once this session persisted the run's `RunReport`.
    ended: bool,
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
    /// `(expected, got)` if the batch contained a sequence gap.
    gap: Option<(u64, u64)>,
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
                    let cancel = control(control_message::Body::Cancel(Cancel {
                        reason: "wall-clock budget exceeded".to_string(),
                    }));
                    if !self.send(cancel).await {
                        return Ok(());
                    }
                }
                Some(_) = self.tasks.join_next() => {}
            }
        }
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
        if self.ended {
            return Ok(false);
        }
        Ok(get_run(&self.ctx.pool, self.rc.run.id)
            .await?
            .is_some_and(|run| !run.state.is_terminal()))
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

    /// Sends prompts not sent on this stream yet. `false` ends the session: the peer is gone or
    /// the run ended, so a prompt must stay queued for the next run.
    async fn send_new_prompts(&mut self) -> Result<bool, ChannelError> {
        if !self.active().await? {
            return Ok(false);
        }
        for prompt in pending_prompts(&self.ctx.pool, self.rc.task.id).await? {
            if self.sent_prompts.insert(prompt.prompt_id.clone())
                && !self
                    .send(control(control_message::Body::Prompt(prompt)))
                    .await
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Persists the batch, acks it, then runs its effects. `false` ends the session.
    async fn handle_batch(&mut self, batch: Vec<AgentMessage>) -> Result<bool, Status> {
        // Asked before the batch is persisted: a `RunReport` in this very batch must not
        // refuse the tool calls that precede it.
        let active = self.active().await?;
        let persisted = persist_batch(&self.ctx, &self.rc, batch).await?;
        if let Some(ack) = persisted.ack
            && !self.send(ControlMessage { ack, body: None }).await
        {
            return Ok(false);
        }
        // The messages before a gap are committed and acked: their effects must still happen.
        self.run_effects(persisted.effects, active).await;
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
    let start = u64::try_from(stored)
        .map_err(|_| ChannelError::InvalidData("negative last_persisted_seq".into()))?;
    let mut last = start;
    let mut effects = Effects::default();
    let mut gap = None;
    let mut newest_event = None;

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
                let state = match change.phase() {
                    RunPhase::Preparing => Some(RunState::Preparing),
                    RunPhase::Running => Some(RunState::Running),
                    RunPhase::Finalizing => Some(RunState::Finalizing),
                    RunPhase::AwaitingInput | RunPhase::Unspecified => None,
                };
                if let Some(state) = state {
                    set_run_state_tx(&mut tx, rc.run.id, state, None).await?;
                }
            }
            agent_message::Body::TranscriptEvent(_) => {
                if let Some(event) = prepared.get(&msg.seq)
                    && let Some(id) =
                        transcript::insert(&mut tx, rc.task.id, rc.run.id, event).await?
                {
                    newest_event = newest_event.max(Some(id));
                }
            }
            agent_message::Body::ToolCallRequest(call) => effects.tool_calls.push(call),
            agent_message::Body::CredentialRequest(request) => effects.credentials.push(request),
            agent_message::Body::PushReport(push) => effects.pushes.push(push),
            agent_message::Body::RunReport(report) => {
                if let Some(ended) = end_run_tx(&mut tx, &run_end(rc, &report)).await? {
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
        gap,
    })
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
    use crate::testing::Env;
    use df_core::testing::SandboxCall;
    use df_proto::transcript::v1 as wire;
    use sqlx::PgPool;

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
        let row = get_run(&env.ctx.pool, run_id).await.unwrap().unwrap();
        let (tx, _rx) = mpsc::channel(64);
        let inbound = futures::stream::iter(messages.into_iter().map(Ok));
        run(env.ctx.clone(), row, inbound, tx, never_stops()).await
    }

    fn never_stops() -> watch::Receiver<bool> {
        watch::channel(false).1
    }

    #[sqlx::test(migrator = "df_db::MIGRATOR")]
    async fn effects_before_a_sequence_gap_still_happen(pool: PgPool) {
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

    #[sqlx::test(migrator = "df_db::MIGRATOR")]
    async fn tool_calls_finish_after_the_stream_ended(pool: PgPool) {
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
            tasks: JoinSet::new(),
            ended: false,
        };
        (session, rx)
    }

    #[sqlx::test(migrator = "df_db::MIGRATOR")]
    async fn a_hanging_forge_does_not_block_the_stream(pool: PgPool) {
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

    #[sqlx::test(migrator = "df_db::MIGRATOR")]
    async fn tool_calls_and_credentials_are_refused_after_the_run_ended(pool: PgPool) {
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

    #[sqlx::test(migrator = "df_db::MIGRATOR")]
    async fn a_stream_error_before_hello_is_reported(pool: PgPool) {
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

    #[sqlx::test(migrator = "df_db::MIGRATOR")]
    async fn a_tool_call_in_flight_at_shutdown_stores_its_result(pool: PgPool) {
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

    #[sqlx::test(migrator = "df_db::MIGRATOR")]
    async fn no_prompts_are_sent_once_the_run_ended(pool: PgPool) {
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
}
