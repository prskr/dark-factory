//! Core run driver and execution engine.
//!
//! Follows `specs/acp-harness/spec.md`, `specs/workspace-lifecycle/spec.md`,
//! and `design/agent-run.md` §6.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use agent_client_protocol::schema::v1::{
    CancelNotification, ContentBlock, PromptRequest, PromptResponse, RequestPermissionRequest,
    RequestPermissionResponse, SessionId, SessionNotification, SessionUpdate,
};
use agent_client_protocol::{ByteStreams, Client, ConnectionTo, Responder};
use df_proto::agent::v1::{
    AgentMessage, Assign, ControlMessage, HarnessCredentials, PhaseChanged, Prompt, PushReport,
    PushStatus, RunOutcome, RunPhase, RunReport, RuntimeOperation, RuntimeTiming, ToolCallResult,
    Workspace, agent_message, control_message,
};
use df_proto::transcript::v1::{Notice, NoticeLevel, StopReason, UsageSource};
use tokio::sync::{Mutex, Notify, broadcast, mpsc, oneshot};

use crate::git::Git;
use crate::harness::{
    BudgetTracker, PromptQueue, RateLimitInfo, RunOutcomeDecision, SessionSetupResult,
    TokioAsyncReadCompat, TokioAsyncWriteCompat, TurnOutcomeInput, approve_permission,
    build_acp_prompt_blocks, determine_run_outcome, parse_rate_limit, prepare_harness_environment,
    setup_session,
};
use crate::outbox::Outbox;
use crate::runtime_handler::DriverRuntimeHandler;
use crate::runtime_socket::RuntimeSocketServer;
use crate::transcript::{
    TranscriptTranslator, acp_to_proto_stop_reason, notice_event, truncate_utf8,
};
use crate::usage::SessionLogTracker;
use crate::workspace::{FinalizeParams, PrepareResult, finalize_workspace, push_after_turn};

/// Maximum bytes of a variable-size field in the log: a tool call's raw input or a chunk's text.
pub(crate) const LOG_FIELD_MAX_BYTES: usize = 512;

/// How long the harness gets to exit on its own once the ACP connection is closed.
const HARNESS_EXIT_GRACE: Duration = Duration::from_secs(5);
/// How long to wait for the harness to answer a prompt after it was sent a `session/cancel`.
const CANCEL_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);
/// Stands in for "no wall-clock budget": a timer that effectively never fires (one year).
const NO_DEADLINE: Duration = Duration::from_hours(8760);

/// Logs that a prompt reached the runtime (id only, never the content).
pub(crate) fn log_prompt_received(p: &Prompt) {
    tracing::info!(prompt_id = %p.prompt_id, "prompt received");
}

/// Writes the session log event for an outbound message (see `specs/logging/spec.md`).
///
/// Never logs tool output, tool content, prompt content or credentials; chunk text is DEBUG only.
pub(crate) fn log_outbound(body: &agent_message::Body) {
    use df_proto::transcript::v1::event::Kind;
    match body {
        agent_message::Body::TranscriptEvent(ev) => {
            let turn_id = ev.turn_id.as_str();
            match &ev.kind {
                Some(Kind::TurnStarted(t)) => {
                    tracing::info!(turn_id, prompts = t.prompt_ids.len(), "turn started");
                }
                Some(Kind::UserMessage(m)) => {
                    tracing::info!(turn_id, prompt_id = %m.prompt_id, "user message delivered");
                }
                Some(Kind::AgentMessageChunk(c)) => {
                    let (text, cut) = chunk_text(c.content.as_ref());
                    tracing::debug!(
                        turn_id,
                        message_id = %c.message_id,
                        text,
                        text_truncated = cut.then_some(true),
                        "agent message chunk"
                    );
                }
                Some(Kind::AgentThoughtChunk(c)) => {
                    let (text, cut) = chunk_text(c.content.as_ref());
                    tracing::debug!(
                        turn_id,
                        message_id = %c.message_id,
                        text,
                        text_truncated = cut.then_some(true),
                        "agent thought chunk"
                    );
                }
                Some(Kind::ToolCall(c)) => {
                    let (input, cut) = truncate_utf8(&c.raw_input_json, LOG_FIELD_MAX_BYTES);
                    tracing::info!(
                        turn_id,
                        tool_call_id = %c.tool_call_id,
                        title = %c.title,
                        kind = ?c.kind(),
                        origin = ?c.origin(),
                        input,
                        input_truncated = cut.then_some(true),
                        "tool call"
                    );
                }
                Some(Kind::ToolCallUpdate(u)) if u.status.is_some() => {
                    tracing::info!(turn_id, tool_call_id = %u.tool_call_id, status = ?u.status(), "tool call status changed");
                }
                Some(Kind::ToolCallUpdate(u)) => {
                    tracing::debug!(turn_id, tool_call_id = %u.tool_call_id, "tool call updated");
                }
                Some(Kind::Plan(p)) => {
                    tracing::info!(turn_id, entries = p.entries.len(), "plan updated");
                }
                Some(Kind::TurnEnded(t)) => {
                    let output_tokens: u64 = t.usage.iter().map(|u| u.output_tokens).sum();
                    tracing::info!(
                        turn_id,
                        stop_reason = ?t.stop_reason(),
                        output_tokens,
                        detail = (!t.detail.is_empty()).then_some(t.detail.as_str()),
                        "turn ended"
                    );
                }
                Some(Kind::Notice(n)) => match n.level() {
                    NoticeLevel::Warning | NoticeLevel::Error => {
                        tracing::warn!(code = %n.code, message = %n.message, "notice");
                    }
                    _ => tracing::info!(code = %n.code, message = %n.message, "notice"),
                },
                None => {}
            }
        }
        agent_message::Body::PushReport(r) => {
            tracing::info!(r#ref = %r.r#ref, status = ?r.status(), sha = %r.sha, "push report");
        }
        agent_message::Body::RunReport(r) => {
            tracing::info!(outcome = ?r.outcome(), detail = %r.detail, "run finished");
        }
        _ => {}
    }
}

/// Parameters for running an agent run.
pub struct RunDriverParams {
    pub assign: Assign,
    pub credentials: Option<HarnessCredentials>,
    pub initial_prompts: Vec<Prompt>,
    /// True if a `Cancel` arrived before `Assign`; the run ends as cancelled without a turn.
    pub cancel_requested: bool,
    pub repo_dir: PathBuf,
    pub home_dir: PathBuf,
    pub runtime_dir: PathBuf,
    pub hooks_path: PathBuf,
    pub mcp_command: PathBuf,
    pub runtime_sock_path: PathBuf,
    pub git: Git,
    pub outbox: Arc<Mutex<Outbox>>,
    pub inbound_rx: mpsc::Receiver<ControlMessage>,
    pub outbound_tx: mpsc::Sender<AgentMessage>,
}

/// Result of completing an agent run.
#[derive(Debug, Clone)]
pub struct RunExecutionResult {
    pub outcome: RunOutcome,
    pub run_report: RunReport,
    pub push_reports: Vec<PushReport>,
    pub notices: Vec<Notice>,
}

/// Runs the complete agent run lifecycle: runtime socket and inbound loop, workspace prepare,
/// harness start, the turns, harness stop and workspace finalize.
pub async fn run_session(params: RunDriverParams) -> anyhow::Result<RunExecutionResult> {
    let shared = RunShared::new(params.cancel_requested);
    for p in params.initial_prompts {
        log_prompt_received(&p);
        shared.prompt_queue.lock().await.push(p);
    }
    let mut collected = Collected::default();
    let Some(workspace) = params.assign.workspace.clone() else {
        return Ok(collected
            .fail(
                &params.outbound_tx,
                "missing workspace in Assign".to_string(),
                String::new(),
            )
            .await);
    };

    let git = params.git;
    let inbound_rx = params.inbound_rx;
    let ctx = Arc::new(RunContext {
        assign: params.assign,
        workspace,
        credentials: params.credentials,
        repo_dir: params.repo_dir,
        home_dir: params.home_dir,
        runtime_dir: params.runtime_dir,
        hooks_path: params.hooks_path,
        mcp_command: params.mcp_command,
        runtime_sock_path: params.runtime_sock_path,
        git,
        outbox: params.outbox,
        outbound_tx: params.outbound_tx,
    });

    let _socket_guard = match start_runtime_socket(&ctx, &shared) {
        Ok(guard) => guard,
        Err(detail) => {
            return Ok(collected
                .fail(&ctx.outbound_tx, detail, String::new())
                .await);
        }
    };
    let (cancel_tx, cancel_rx) = broadcast::channel::<String>(16);
    let _inbound_guard = spawn_inbound_loop(inbound_rx, &shared, cancel_tx, &ctx.outbound_tx);

    // A cancel that arrived before Assign: don't clone, push or start the harness
    if shared.cancelled.load(Ordering::SeqCst) {
        return Ok(end_run_early(
            &ctx.outbound_tx,
            RunOutcome::Cancelled,
            "Run cancelled by control plane before the workspace was prepared".to_string(),
            String::new(),
            collected.push_reports,
            collected.notices,
        )
        .await);
    }

    let prepare_res = match prepare_phase(&ctx, &mut collected).await {
        Ok(res) => res,
        Err(detail) => {
            return Ok(collected
                .fail(&ctx.outbound_tx, detail, String::new())
                .await);
        }
    };
    let HarnessProcess {
        mut child,
        transport,
        started,
    } = match spawn_harness(&ctx).await {
        Ok(process) => process,
        Err(detail) => {
            return Ok(collected
                .fail(&ctx.outbound_tx, detail, String::new())
                .await);
        }
    };

    let driver_result =
        drive_session(&ctx, &shared, cancel_rx, &prepare_res, transport, started).await;
    stop_harness(&mut child).await;

    // `push_after_turn` keeps the outbox state current, so it is right even after a driver error
    let outbox_last_pushed_sha = ctx.outbox.lock().await.run_state().last_pushed_sha.clone();
    let loop_outcome = match driver_result {
        Ok(out) => out,
        Err(e) => TurnLoopOutcome {
            session_id: String::new(),
            last_pushed_sha: outbox_last_pushed_sha,
            end: LoopEnd::new(RunOutcome::Failed, format!("Run driver error: {e}")),
            notices: Vec::new(),
        },
    };

    collected
        .push_reports
        .extend(shared.push_reports.lock().await.drain(..));
    collected.notices.extend(loop_outcome.notices.clone());
    Ok(finalize_run(&ctx, collected, prepare_res.diverged, loop_outcome).await)
}

/// Helper RAII guard that aborts a task when dropped (Finding 10).
struct AbortOnDrop<T>(Option<tokio::task::JoinHandle<T>>);

impl<T> AbortOnDrop<T> {
    fn new(h: tokio::task::JoinHandle<T>) -> Self {
        Self(Some(h))
    }
}

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        if let Some(h) = self.0.take() {
            h.abort();
        }
    }
}

/// Sends a message without a seq (the outbox assigns it). A closed channel means the connection
/// manager is gone, which the run can't act on, so the result is ignored.
async fn send_body(outbound_tx: &mpsc::Sender<AgentMessage>, body: agent_message::Body) {
    log_outbound(&body);
    outbound_tx
        .send(AgentMessage {
            seq: 0,
            body: Some(body),
        })
        .await
        .ok();
}

/// Reports how long `operation` took, measured from `started` (`RuntimeTiming`). Reported in whole
/// milliseconds, at least 1: the control plane rejects a zero duration as implausible.
async fn send_timing(
    outbound_tx: &mpsc::Sender<AgentMessage>,
    operation: RuntimeOperation,
    started: Instant,
    ok: bool,
) {
    let duration_ms = u64::try_from(started.elapsed().as_millis())
        .unwrap_or(u64::MAX)
        .max(1);
    let timing = RuntimeTiming {
        operation: operation as i32,
        duration_ms,
        ok,
    };
    send_body(outbound_tx, agent_message::Body::RuntimeTiming(timing)).await;
}

/// The text of a chunk cut to [`LOG_FIELD_MAX_BYTES`] and whether it was cut; empty for
/// non-text content.
fn chunk_text(block: Option<&df_proto::transcript::v1::ContentBlock>) -> (&str, bool) {
    use df_proto::transcript::v1::content_block;
    match block.and_then(|b| b.kind.as_ref()) {
        Some(content_block::Kind::Text(t)) => truncate_utf8(&t.text, LOG_FIELD_MAX_BYTES),
        _ => ("", false),
    }
}

/// Helper to emit a notice transcript event to outbound channel (Finding 28).
async fn emit_notice(outbound_tx: &mpsc::Sender<AgentMessage>, notice: Notice) {
    send_body(
        outbound_tx,
        agent_message::Body::TranscriptEvent(notice_event(notice)),
    )
    .await;
}

/// Ends the run before any turn ran: sends a `RunReport` without workspace state and returns the
/// matching result.
async fn end_run_early(
    outbound_tx: &mpsc::Sender<AgentMessage>,
    outcome: RunOutcome,
    detail: String,
    harness_session_id: String,
    push_reports: Vec<PushReport>,
    notices: Vec<Notice>,
) -> RunExecutionResult {
    let run_report = RunReport {
        outcome: outcome as i32,
        detail,
        retry_after: None,
        harness_session_id,
        workspace: None,
    };
    send_body(
        outbound_tx,
        agent_message::Body::RunReport(run_report.clone()),
    )
    .await;
    RunExecutionResult {
        outcome,
        run_report,
        push_reports,
        notices,
    }
}

/// Sends a FAILED `RunReport` and returns the matching result.
async fn fail_run(
    outbound_tx: &mpsc::Sender<AgentMessage>,
    detail: String,
    harness_session_id: String,
    push_reports: Vec<PushReport>,
    notices: Vec<Notice>,
) -> RunExecutionResult {
    end_run_early(
        outbound_tx,
        RunOutcome::Failed,
        detail,
        harness_session_id,
        push_reports,
        notices,
    )
    .await
}

/// Waits for the next cancel signal, or forever once the channel is gone.
async fn recv_cancel(
    rx: &mut Option<broadcast::Receiver<String>>,
) -> Result<String, broadcast::error::RecvError> {
    match rx {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

/// What ended the idle wait after a normal turn end.
#[derive(Debug, PartialEq, Eq)]
enum IdleWake {
    Prompt,
    Cancel,
    Release,
    BudgetEnded,
}

/// What can end the idle wait besides the cancel broadcast and the budget.
struct IdleSignals<'a> {
    prompt_queue: &'a Mutex<PromptQueue>,
    /// Notified when a prompt was queued or `Release` arrived.
    wake: &'a Notify,
    released: &'a AtomicBool,
    cancelled: &'a AtomicBool,
}

/// Waits (harness session kept up) until a prompt is queued, `Cancel` or `Release` arrives, or
/// the wall-clock budget (`remaining`) ends. Nothing else ends the wait, in particular not a lost
/// control plane stream: the connection manager reconnects underneath the run.
///
/// Cancel-safe: the only side effect is the `cancelled` flag, which is idempotent.
async fn wait_for_input(
    signals: IdleSignals<'_>,
    cancel_rx: &mut Option<broadcast::Receiver<String>>,
    remaining: Option<Duration>,
) -> IdleWake {
    let IdleSignals {
        prompt_queue,
        wake,
        released,
        cancelled,
    } = signals;
    let budget = tokio::time::sleep(remaining.unwrap_or(NO_DEADLINE));
    tokio::pin!(budget);
    loop {
        // Register for wakeups before checking the state, so none is lost in between.
        let notified = wake.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();

        if cancelled.load(Ordering::SeqCst) {
            return IdleWake::Cancel;
        }
        if prompt_queue.lock().await.has_queued() {
            return IdleWake::Prompt;
        }
        if released.load(Ordering::SeqCst) {
            return IdleWake::Release;
        }
        tokio::select! {
            () = &mut notified => {}
            recv = recv_cancel(cancel_rx) => match recv {
                Ok(_reason) => {
                    cancelled.store(true, Ordering::SeqCst);
                    return IdleWake::Cancel;
                }
                Err(broadcast::error::RecvError::Closed) => *cancel_rx = None,
                Err(broadcast::error::RecvError::Lagged(_)) => {}
            },
            () = &mut budget => return IdleWake::BudgetEnded,
        }
    }
}

type PendingToolCalls = Arc<Mutex<HashMap<String, oneshot::Sender<ToolCallResult>>>>;
type HarnessTransport = ByteStreams<
    TokioAsyncWriteCompat<tokio::process::ChildStdin>,
    TokioAsyncReadCompat<tokio::process::ChildStdout>,
>;
type AcpAgent = agent_client_protocol::role::acp::Agent;

/// Values that every phase of a run reads and none changes.
struct RunContext {
    assign: Assign,
    workspace: Workspace,
    credentials: Option<HarnessCredentials>,
    repo_dir: PathBuf,
    home_dir: PathBuf,
    runtime_dir: PathBuf,
    hooks_path: PathBuf,
    mcp_command: PathBuf,
    runtime_sock_path: PathBuf,
    /// Hardened Git for the workspace, trusting the instance CA.
    git: Git,
    outbox: Arc<Mutex<Outbox>>,
    outbound_tx: mpsc::Sender<AgentMessage>,
}

/// State shared by the inbound control loop, the runtime socket handler and the turn loop.
#[derive(Clone)]
struct RunShared {
    pending_tool_calls: PendingToolCalls,
    end_run_requested: Arc<AtomicBool>,
    /// A blocking request of the current turn (`ToolCallResult.await_input_after_turn`).
    await_input_requested: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    prompt_queue: Arc<Mutex<PromptQueue>>,
    /// Wakes the idle wait: a prompt was queued or `Release` arrived.
    input_wake: Arc<Notify>,
    /// Sticky: a `Release` that arrives before the wait starts still ends it.
    released: Arc<AtomicBool>,
    /// Reports of completed turns; shared so they survive a driver error.
    push_reports: Arc<Mutex<Vec<PushReport>>>,
}

impl RunShared {
    fn new(cancel_requested: bool) -> Self {
        Self {
            pending_tool_calls: Arc::new(Mutex::new(HashMap::new())),
            end_run_requested: Arc::new(AtomicBool::new(false)),
            await_input_requested: Arc::new(AtomicBool::new(false)),
            cancelled: Arc::new(AtomicBool::new(cancel_requested)),
            prompt_queue: Arc::new(Mutex::new(PromptQueue::new())),
            input_wake: Arc::new(Notify::new()),
            released: Arc::new(AtomicBool::new(false)),
            push_reports: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

/// Notices and push reports gathered so far, reported with the result of the run.
#[derive(Default)]
struct Collected {
    notices: Vec<Notice>,
    push_reports: Vec<PushReport>,
}

impl Collected {
    /// Ends the run as failed with what was gathered so far.
    async fn fail(
        self,
        outbound_tx: &mpsc::Sender<AgentMessage>,
        detail: String,
        harness_session_id: String,
    ) -> RunExecutionResult {
        fail_run(
            outbound_tx,
            detail,
            harness_session_id,
            self.push_reports,
            self.notices,
        )
        .await
    }
}

/// Binds the runtime socket (factory tools for the harness MCP server). The socket is served
/// until the returned guard is dropped.
fn start_runtime_socket(ctx: &RunContext, shared: &RunShared) -> Result<AbortOnDrop<()>, String> {
    let handler = Arc::new(DriverRuntimeHandler::new(
        ctx.assign.tools.clone(),
        ctx.outbound_tx.clone(),
        Arc::clone(&shared.pending_tool_calls),
    ));
    let server = RuntimeSocketServer::bind(&ctx.runtime_sock_path, handler)
        .map_err(|e| format!("Failed to bind runtime socket: {e}"))?;
    Ok(AbortOnDrop::new(server.spawn()))
}

/// Starts the loop that applies control messages. It runs from the start of the run so that
/// messages arriving during the workspace prepare are applied.
fn spawn_inbound_loop(
    mut inbound_rx: mpsc::Receiver<ControlMessage>,
    shared: &RunShared,
    cancel_tx: broadcast::Sender<String>,
    outbound_tx: &mpsc::Sender<AgentMessage>,
) -> AbortOnDrop<()> {
    let shared = shared.clone();
    let outbound_tx = outbound_tx.clone();
    AbortOnDrop::new(tokio::spawn(async move {
        while let Some(msg) = inbound_rx.recv().await {
            handle_control_message(msg, &shared, &cancel_tx, &outbound_tx).await;
        }
    }))
}

/// Applies one control message to the shared run state.
async fn handle_control_message(
    msg: ControlMessage,
    shared: &RunShared,
    cancel_tx: &broadcast::Sender<String>,
    outbound_tx: &mpsc::Sender<AgentMessage>,
) {
    match msg.body {
        Some(control_message::Body::Prompt(p)) => {
            log_prompt_received(&p);
            shared.prompt_queue.lock().await.push(p);
            shared.input_wake.notify_one();
        }
        Some(control_message::Body::Release(_)) => {
            shared.released.store(true, Ordering::SeqCst);
            shared.input_wake.notify_one();
        }
        Some(control_message::Body::Cancel(c)) => {
            shared.cancelled.store(true, Ordering::SeqCst);
            cancel_tx.send(c.reason).ok(); // receiver may be gone
        }
        Some(control_message::Body::ToolCallResult(res)) => {
            if res.end_run_after_turn {
                shared.end_run_requested.store(true, Ordering::SeqCst);
            }
            if res.await_input_after_turn {
                shared.await_input_requested.store(true, Ordering::SeqCst);
            }
            if let Some(tx) = shared.pending_tool_calls.lock().await.remove(&res.call_id) {
                tx.send(res).ok(); // receiver may be gone
            }
        }
        // HarnessCredentials must precede Assign; the harness environment is already built
        Some(control_message::Body::HarnessCredentials(_)) => {
            emit_notice(
                outbound_tx,
                Notice {
                    level: NoticeLevel::Warning as i32,
                    code: "runtime.harness_credentials_late".to_string(),
                    message: "HarnessCredentials arrived after Assign and were ignored".to_string(),
                },
            )
            .await;
        }
        _ => {}
    }
}

/// Clones or syncs the workspace and reports its notices and push report. The error is the
/// failure detail of the run.
async fn prepare_phase(
    ctx: &RunContext,
    collected: &mut Collected,
) -> Result<PrepareResult, String> {
    let started = Instant::now();
    let result = crate::workspace::prepare_workspace(
        &ctx.git,
        &ctx.repo_dir,
        &ctx.workspace,
        &ctx.hooks_path,
        ctx.git.ssl_ca_info.as_deref(),
    )
    .await;
    send_timing(
        &ctx.outbound_tx,
        RuntimeOperation::WorkspacePrepare,
        started,
        result.is_ok(),
    )
    .await;
    let res = result.map_err(|e| format!("Workspace preparation failed: {e}"))?;

    collected.notices.extend(res.notices.clone());
    for n in &res.notices {
        emit_notice(&ctx.outbound_tx, n.clone()).await;
    }
    if let Some(pr) = res.push_report.clone() {
        collected.push_reports.push(pr.clone());
        send_body(&ctx.outbound_tx, agent_message::Body::PushReport(pr)).await;
    }
    Ok(res)
}

/// The running harness subprocess and its ACP transport.
struct HarnessProcess {
    child: tokio::process::Child,
    transport: HarnessTransport,
    /// When the start began; the `HarnessStart` timing ends once the ACP session is set up.
    started: Instant,
}

/// Command and arguments of the harness from `Assign`; defaults to the fake agent.
fn harness_command(assign: &Assign) -> (String, Vec<String>) {
    let acp_command = assign.harness.as_ref().and_then(|h| match &h.kind {
        Some(df_proto::agent::v1::harness::Kind::Acp(acp)) => acp.command.split_first(),
        _ => None,
    });
    match acp_command {
        Some((cmd, args)) => (cmd.clone(), args.to_vec()),
        None => ("fake-acp-agent".to_string(), Vec::new()),
    }
}

/// Builds the harness environment (credentials injected in memory only) and spawns the harness.
/// The error is the failure detail of the run.
async fn spawn_harness(ctx: &RunContext) -> Result<HarnessProcess, String> {
    let mut env = prepare_harness_environment(&ctx.assign, ctx.credentials.as_ref(), &ctx.home_dir)
        .map_err(|e| format!("Harness environment preparation failed: {e}"))?;
    env.insert(
        "DF_RUNTIME_SOCK".to_string(),
        ctx.runtime_sock_path.to_string_lossy().to_string(),
    );
    env.insert(
        "DF_RUN_DIR".to_string(),
        ctx.runtime_dir.to_string_lossy().to_string(),
    );

    let (harness_cmd, harness_args) = harness_command(&ctx.assign);
    let started = Instant::now();
    let mut child = match tokio::process::Command::new(&harness_cmd)
        .args(&harness_args)
        .envs(&env)
        .current_dir(&ctx.repo_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            send_timing(
                &ctx.outbound_tx,
                RuntimeOperation::HarnessStart,
                started,
                false,
            )
            .await;
            return Err(format!(
                "Failed to spawn harness process '{harness_cmd}': {e}"
            ));
        }
    };
    tracing::info!(harness = %harness_cmd, "harness started");
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        child.kill().await.ok();
        return Err("Failed to capture harness stdio".to_string());
    };
    Ok(HarnessProcess {
        child,
        transport: ByteStreams::new(TokioAsyncWriteCompat(stdin), TokioAsyncReadCompat(stdout)),
        started,
    })
}

/// Stops the harness subprocess: the ACP connection (and with it the harness' stdin) is closed
/// by now, so it gets time to flush its session state before it is killed.
async fn stop_harness(child: &mut tokio::process::Child) {
    if tokio::time::timeout(HARNESS_EXIT_GRACE, child.wait())
        .await
        .is_err()
    {
        child.kill().await.ok();
    }
}

/// What the turn loop hands to the finalize phase.
struct TurnLoopOutcome {
    session_id: String,
    last_pushed_sha: Option<String>,
    end: LoopEnd,
    notices: Vec<Notice>,
}

/// Why the turn loop ended.
struct LoopEnd {
    outcome: RunOutcome,
    detail: String,
    retry_after: Option<Duration>,
}

impl LoopEnd {
    fn new(outcome: RunOutcome, detail: impl Into<String>) -> Self {
        Self {
            outcome,
            detail: detail.into(),
            retry_after: None,
        }
    }
}

/// Connects the ACP client to the harness, sets up the harness session and runs the turns.
async fn drive_session(
    ctx: &Arc<RunContext>,
    shared: &RunShared,
    cancel_rx: broadcast::Receiver<String>,
    prepare: &PrepareResult,
    transport: HarnessTransport,
    harness_started: Instant,
) -> Result<TurnLoopOutcome, agent_client_protocol::Error> {
    let translator = Arc::new(Mutex::new(TranscriptTranslator::new("")));
    // `session/load` replays the whole conversation as updates before it answers. That history
    // is in the transcript already, so updates are dropped until the session is set up.
    let session_ready = Arc::new(AtomicBool::new(false));

    let notif_translator = Arc::clone(&translator);
    let notif_ready = Arc::clone(&session_ready);
    let notif_tx = ctx.outbound_tx.clone();
    let client = Client
        .builder()
        .on_receive_notification(
            async move |notification: SessionNotification, _cx: ConnectionTo<AcpAgent>| {
                if notif_ready.load(Ordering::SeqCst) {
                    forward_session_update(&notif_tx, &notif_translator, notification.update).await;
                }
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |req: RequestPermissionRequest,
                        responder: Responder<RequestPermissionResponse>,
                        _cx: ConnectionTo<AcpAgent>| {
                responder.respond(approve_permission(&req))
            },
            agent_client_protocol::on_receive_request!(),
        );

    let last_pushed_sha = record_initial_pushed_sha(ctx, prepare).await;
    let ctx = Arc::clone(ctx);
    let shared = shared.clone();
    client
        .connect_with(transport, async move |cx| {
            let setup = setup_harness_session(&cx, &ctx, &session_ready, harness_started).await?;
            let notices = setup.notices.clone();
            let mut turns = Turns::new(ctx, shared, translator, cancel_rx, last_pushed_sha, setup);
            let end = turns.run(&cx).await;
            Ok(TurnLoopOutcome {
                session_id: turns.session_id,
                last_pushed_sha: turns.last_pushed_sha,
                end,
                notices,
            })
        })
        .await
}

/// Translates a session update of the harness into a transcript event and sends it.
async fn forward_session_update(
    outbound_tx: &mpsc::Sender<AgentMessage>,
    translator: &Mutex<TranscriptTranslator>,
    update: SessionUpdate,
) {
    // `UsageUpdate` is context-window occupancy, not per-turn tokens, so it is ignored;
    // per-turn usage comes from the session log.
    let mut translator = translator.lock().await;
    let event = match update {
        SessionUpdate::AgentMessageChunk(chunk) => translator.translate_agent_message_chunk(&chunk),
        SessionUpdate::AgentThoughtChunk(chunk) => translator.translate_agent_thought_chunk(&chunk),
        SessionUpdate::ToolCall(call) => translator.translate_tool_call(&call),
        SessionUpdate::ToolCallUpdate(update) => translator.translate_tool_call_update(&update),
        SessionUpdate::Plan(plan) => translator.translate_plan(&plan),
        _ => return,
    };
    drop(translator);
    send_body(outbound_tx, agent_message::Body::TranscriptEvent(event)).await;
}

/// Commits up to the returned SHA are known to be on the remote. After a failed prepare push or
/// on a diverged branch HEAD isn't, so nothing is assumed to be pushed. The outbox is updated.
async fn record_initial_pushed_sha(ctx: &RunContext, prepare: &PrepareResult) -> Option<String> {
    let sha = if prepare.push_failed || prepare.diverged {
        None
    } else {
        let ob = ctx.outbox.lock().await;
        ob.run_state()
            .last_pushed_sha
            .clone()
            .or_else(|| Some(prepare.head_sha.clone()))
    };
    if let Err(error) = ctx.outbox.lock().await.set_last_pushed_sha(sha.clone()) {
        tracing::warn!(error = %error, "failed to persist the last pushed sha");
    }
    sha
}

/// Establishes the ACP session, reports its notices and persists the session id in the outbox.
async fn setup_harness_session(
    cx: &ConnectionTo<AcpAgent>,
    ctx: &RunContext,
    session_ready: &AtomicBool,
    harness_started: Instant,
) -> Result<SessionSetupResult, agent_client_protocol::Error> {
    let result = setup_session(
        cx,
        &ctx.repo_dir,
        ctx.assign.continuation.as_ref(),
        &ctx.mcp_command,
        &ctx.runtime_sock_path,
    )
    .await;
    send_timing(
        &ctx.outbound_tx,
        RuntimeOperation::HarnessStart,
        harness_started,
        result.is_ok(),
    )
    .await;
    let setup = result?;
    // The dispatch loop runs each handler before it routes the next message, so the replay has
    // been dropped by the time the load response resolved.
    session_ready.store(true, Ordering::SeqCst);

    tracing::info!(session_id = %setup.session_id, resumed = setup.is_resumed, "harness session ready");
    for n in &setup.notices {
        emit_notice(&ctx.outbound_tx, n.clone()).await;
    }
    // Persist harness session id in outbox (Finding 22)
    if let Err(error) = ctx
        .outbox
        .lock()
        .await
        .set_harness_session_id(Some(setup.session_id.clone()))
    {
        tracing::warn!(error = %error, "failed to persist the harness session id");
    }
    Ok(setup)
}

/// How a turn ended, as input for the run outcome.
struct TurnResult {
    rate_limited: Option<RateLimitInfo>,
    harness_crashed: bool,
    stop_reason: Option<StopReason>,
    wall_clock_exceeded: bool,
}

/// The turn loop of one harness session.
struct Turns {
    ctx: Arc<RunContext>,
    shared: RunShared,
    translator: Arc<Mutex<TranscriptTranslator>>,
    cancel_rx: Option<broadcast::Receiver<String>>,
    last_pushed_sha: Option<String>,
    session_id: String,
    is_resumed: bool,
    budget: BudgetTracker,
    session_log: SessionLogTracker,
    turn_index: u32,
}

impl Turns {
    fn new(
        ctx: Arc<RunContext>,
        shared: RunShared,
        translator: Arc<Mutex<TranscriptTranslator>>,
        cancel_rx: broadcast::Receiver<String>,
        last_pushed_sha: Option<String>,
        setup: SessionSetupResult,
    ) -> Self {
        let budget = BudgetTracker::new(ctx.assign.budgets.as_ref());
        let session_log = SessionLogTracker::new(&ctx.home_dir, &ctx.repo_dir, &setup.session_id);
        Self {
            ctx,
            shared,
            translator,
            cancel_rx: Some(cancel_rx),
            last_pushed_sha,
            session_id: setup.session_id,
            is_resumed: setup.is_resumed,
            budget,
            session_log,
            turn_index: 0,
        }
    }

    /// Runs turns until the run ends: a budget, a cancel, an empty queue, a release or the
    /// outcome of a turn.
    async fn run(&mut self, cx: &ConnectionTo<AcpAgent>) -> LoopEnd {
        loop {
            if let Some(end) = self.pre_turn_end().await {
                return end;
            }
            let turn = self.run_turn(cx).await;
            let input = TurnOutcomeInput {
                cancelled: self.shared.cancelled.load(Ordering::SeqCst),
                end_run_after_turn: self.shared.end_run_requested.load(Ordering::SeqCst),
                rate_limited: turn.rate_limited,
                wall_clock_exceeded: turn.wall_clock_exceeded,
                output_tokens_exceeded: self.budget.is_output_tokens_exceeded(),
                stop_reason: turn.stop_reason,
                harness_crashed: turn.harness_crashed,
                has_queued_prompts: self.shared.prompt_queue.lock().await.has_queued(),
                turn_limit_reached: self.budget.is_turn_limit_reached(),
            };
            match determine_run_outcome(&input) {
                RunOutcomeDecision::ContinueNextTurn => {}
                RunOutcomeDecision::AwaitInput => {
                    if let Some(end) = self.idle_until_next_prompt().await {
                        return end;
                    }
                }
                RunOutcomeDecision::Finalize {
                    outcome,
                    detail,
                    retry_after,
                } => {
                    return LoopEnd {
                        outcome,
                        detail,
                        retry_after,
                    };
                }
            }
        }
    }

    /// The pre-turn checks: budgets, cancel and an empty prompt queue end the run.
    async fn pre_turn_end(&self) -> Option<LoopEnd> {
        let (outcome, detail) = if self.budget.is_wall_clock_exceeded() {
            (RunOutcome::LimitReached, "Wall clock budget exceeded")
        } else if self.budget.is_turn_limit_reached() {
            (RunOutcome::LimitReached, "Turn budget reached")
        } else if self.shared.cancelled.load(Ordering::SeqCst) {
            (RunOutcome::Cancelled, "Run cancelled by control plane")
        } else if self.shared.prompt_queue.lock().await.is_empty() {
            (
                RunOutcome::EndTurn,
                "Turn ended normally with no queued prompts",
            )
        } else {
            return None;
        };
        Some(LoopEnd::new(outcome, detail))
    }

    /// Runs one turn: sends the queued prompts, waits for the answer, records how the turn ended
    /// and pushes new commits.
    async fn run_turn(&mut self, cx: &ConnectionTo<AcpAgent>) -> TurnResult {
        let blocks = self.begin_turn().await;
        let turn_started = Instant::now();
        let (prompt_res, wall_clock_exceeded) = self.await_prompt(cx, blocks).await;
        send_timing(
            &self.ctx.outbound_tx,
            RuntimeOperation::Turn,
            turn_started,
            prompt_res.is_ok(),
        )
        .await;
        let mut turn = self.record_turn_end(prompt_res).await;
        turn.wall_clock_exceeded = wall_clock_exceeded;
        self.push_new_commits().await;
        turn
    }

    /// Takes the queued prompts into a new turn and returns the ACP prompt blocks.
    async fn begin_turn(&mut self) -> Vec<ContentBlock> {
        self.shared
            .await_input_requested
            .store(false, Ordering::SeqCst);
        self.turn_index += 1;
        let turn_id = format!("turn_{}", self.turn_index);
        self.translator.lock().await.set_turn_id(&turn_id);

        let (prompts, start_events) = self.shared.prompt_queue.lock().await.start_turn(&turn_id);
        for ev in start_events {
            send_body(
                &self.ctx.outbound_tx,
                agent_message::Body::TranscriptEvent(ev),
            )
            .await;
        }
        let prepend_summary = if self.turn_index == 1 && !self.is_resumed {
            self.ctx
                .assign
                .continuation
                .as_ref()
                .map(|c| c.summary.as_str())
        } else {
            None
        };
        self.session_log.mark_turn_start();
        build_acp_prompt_blocks(&prompts, prepend_summary)
    }

    /// Sends the prompt and waits for the response, a cancel or the wall-clock deadline. A cancel
    /// or the deadline sends `session/cancel` and waits a short time for the response. The flag is
    /// true if the deadline ended the wait.
    async fn await_prompt(
        &mut self,
        cx: &ConnectionTo<AcpAgent>,
        blocks: Vec<ContentBlock>,
    ) -> (Result<PromptResponse, agent_client_protocol::Error>, bool) {
        let prompt_fut = cx
            .send_request(PromptRequest::new(
                SessionId::new(&*self.session_id),
                blocks,
            ))
            .block_task();
        tokio::pin!(prompt_fut);

        let remaining_wall_clock = self.budget.remaining_wall_clock().unwrap_or(NO_DEADLINE);
        let deadline = tokio::time::sleep(remaining_wall_clock);
        tokio::pin!(deadline);

        // A closed cancel channel only stops the cancel branch; the deadline stays active.
        let mut wall_clock_exceeded = false;
        let mut needs_cancel = false;
        let mut prompt_res = None;
        while prompt_res.is_none() && !needs_cancel {
            tokio::select! {
                res = &mut prompt_fut => {
                    prompt_res = Some(res);
                }
                recv_res = recv_cancel(&mut self.cancel_rx) => match recv_res {
                    Ok(_reason) => {
                        self.shared.cancelled.store(true, Ordering::SeqCst);
                        needs_cancel = true;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        self.cancel_rx = None;
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        needs_cancel = self.shared.cancelled.load(Ordering::SeqCst);
                    }
                },
                () = &mut deadline => {
                    wall_clock_exceeded = true;
                    needs_cancel = true;
                }
            }
        }

        let res = if let Some(res) = prompt_res {
            res
        } else {
            cx.send_notification(CancelNotification::new(SessionId::new(&*self.session_id)))
                .ok();
            if let Ok(res) = tokio::time::timeout(CANCEL_RESPONSE_TIMEOUT, &mut prompt_fut).await {
                res
            } else {
                let mut err = agent_client_protocol::Error::internal_error();
                err.message = "timed out waiting for prompt response after cancel".into();
                Err(err)
            }
        };
        (res, wall_clock_exceeded)
    }

    /// Records usage and the `TurnEnded` event of the turn.
    async fn record_turn_end(
        &mut self,
        prompt_res: Result<PromptResponse, agent_client_protocol::Error>,
    ) -> TurnResult {
        let mut turn = TurnResult {
            rate_limited: None,
            harness_crashed: false,
            stop_reason: None,
            wall_clock_exceeded: false,
        };
        let (stop_reason, usage, usage_source, detail) = match prompt_res {
            Ok(resp) => {
                let proto_stop_reason = acp_to_proto_stop_reason(resp.stop_reason);
                turn.stop_reason = Some(proto_stop_reason);
                // Ordering: the ACP dispatch loop runs each notification handler to completion
                // before it routes the next message, so every chunk of this turn was already
                // queued to `outbound_tx` before `prompt_res` resolved. TurnEnded cannot overtake
                // them.
                let (usage, usage_source) = self.session_log.parse_turn_usage();
                self.budget
                    .record_turn(usage.iter().map(|u| u.output_tokens).sum());
                (proto_stop_reason, usage, usage_source, String::new())
            }
            Err(e) => {
                let (stop_reason, detail) = if let Some(rl) = parse_rate_limit(&e) {
                    let detail = rl.detail.clone();
                    turn.rate_limited = Some(rl);
                    (StopReason::RateLimited, detail)
                } else {
                    turn.harness_crashed = true;
                    (StopReason::Error, e.message)
                };
                (stop_reason, Vec::new(), UsageSource::Unspecified, detail)
            }
        };
        let event = self
            .translator
            .lock()
            .await
            .translate_turn_ended_with_stop_reason(stop_reason, usage, usage_source, &detail);
        send_body(
            &self.ctx.outbound_tx,
            agent_message::Body::TranscriptEvent(event),
        )
        .await;
        turn
    }

    /// Pushes the task branch after a turn if commits were created.
    async fn push_new_commits(&mut self) {
        let tx = &self.ctx.outbound_tx;
        let push_started = Instant::now();
        let push_result = push_after_turn(
            &self.ctx.git,
            &self.ctx.workspace,
            &mut self.last_pushed_sha,
        )
        .await;
        // Nothing to push is not an operation worth a sample.
        if let Some(ok) = match &push_result {
            Ok(Some(pr)) => Some(pr.status() == PushStatus::Ok),
            Ok(None) => None,
            Err(_) => Some(false),
        } {
            send_timing(tx, RuntimeOperation::Push, push_started, ok).await;
        }
        if let Ok(Some(pr)) = push_result {
            if let Err(error) = self
                .ctx
                .outbox
                .lock()
                .await
                .set_last_pushed_sha(self.last_pushed_sha.clone())
            {
                tracing::warn!(error = %error, "failed to persist the last pushed sha");
            }
            self.shared.push_reports.lock().await.push(pr.clone());
            send_body(tx, agent_message::Body::PushReport(pr)).await;
        }
    }

    /// Waits (harness session kept up) for the next prompt after a normal turn end. `None` means
    /// a prompt arrived; otherwise the run ends.
    async fn idle_until_next_prompt(&mut self) -> Option<LoopEnd> {
        send_phase(&self.ctx.outbound_tx, RunPhase::AwaitingInput).await;
        let woke = wait_for_input(
            IdleSignals {
                prompt_queue: &self.shared.prompt_queue,
                wake: &self.shared.input_wake,
                released: &self.shared.released,
                cancelled: &self.shared.cancelled,
            },
            &mut self.cancel_rx,
            self.budget.remaining_wall_clock(),
        )
        .await;
        let (outcome, detail) = match woke {
            IdleWake::Prompt => {
                send_phase(&self.ctx.outbound_tx, RunPhase::Running).await;
                return None;
            }
            IdleWake::Cancel => (RunOutcome::Cancelled, "Run cancelled by control plane"),
            IdleWake::Release if self.shared.await_input_requested.load(Ordering::SeqCst) => (
                RunOutcome::Blocked,
                "Nobody answered the blocking question in time",
            ),
            IdleWake::Release => (RunOutcome::EndTurn, "Released while waiting for input"),
            // No work was cut off, so this is not LimitReached.
            IdleWake::BudgetEnded => (
                RunOutcome::EndTurn,
                "Wall clock budget ended while waiting for input",
            ),
        };
        Some(LoopEnd::new(outcome, detail))
    }
}

/// Reports a phase change of the run.
async fn send_phase(outbound_tx: &mpsc::Sender<AgentMessage>, phase: RunPhase) {
    send_body(
        outbound_tx,
        agent_message::Body::PhaseChanged(PhaseChanged {
            phase: phase as i32,
            detail: String::new(),
        }),
    )
    .await;
}

/// Finalizes the workspace and sends the push reports, notices and the `RunReport`.
async fn finalize_run(
    ctx: &RunContext,
    mut collected: Collected,
    diverged: bool,
    mut loop_outcome: TurnLoopOutcome,
) -> RunExecutionResult {
    let finalize_params = FinalizeParams {
        repo_dir: &ctx.repo_dir,
        runtime_dir: &ctx.runtime_dir,
        workspace: &ctx.workspace,
        run_id: &ctx.assign.run_id,
        outcome: loop_outcome.end.outcome,
        detail: &loop_outcome.end.detail,
        harness_session_id: &loop_outcome.session_id,
        hooks_path: &ctx.hooks_path,
        ssl_ca_info: ctx.git.ssl_ca_info.as_deref(),
        wip_size_cap_bytes: 10 * 1024 * 1024,
        diverged,
    };
    let started = Instant::now();
    let result = finalize_workspace(
        &ctx.git,
        &finalize_params,
        &mut loop_outcome.last_pushed_sha,
    )
    .await;
    send_timing(
        &ctx.outbound_tx,
        RuntimeOperation::Finalize,
        started,
        result.is_ok(),
    )
    .await;
    let finalize_res = match result {
        Ok(res) => res,
        Err(e) => {
            return collected
                .fail(
                    &ctx.outbound_tx,
                    format!("Workspace finalization failed: {e}"),
                    loop_outcome.session_id,
                )
                .await;
        }
    };

    if let Err(error) = ctx
        .outbox
        .lock()
        .await
        .set_last_pushed_sha(loop_outcome.last_pushed_sha.clone())
    {
        tracing::warn!(error = %error, "failed to persist the last pushed sha");
    }

    let run_report = final_run_report(finalize_res.run_report, &loop_outcome);
    collected
        .push_reports
        .extend(finalize_res.push_reports.clone());
    for pr in finalize_res.push_reports {
        send_body(&ctx.outbound_tx, agent_message::Body::PushReport(pr)).await;
    }
    collected.notices.extend(finalize_res.notices.clone());
    for n in finalize_res.notices {
        emit_notice(&ctx.outbound_tx, n).await;
    }
    send_body(
        &ctx.outbound_tx,
        agent_message::Body::RunReport(run_report.clone()),
    )
    .await;

    RunExecutionResult {
        outcome: loop_outcome.end.outcome,
        run_report,
        push_reports: collected.push_reports,
        notices: collected.notices,
    }
}

/// The workspace report of the finalize step completed with the outcome of the turn loop.
fn final_run_report(mut run_report: RunReport, loop_outcome: &TurnLoopOutcome) -> RunReport {
    run_report.outcome = loop_outcome.end.outcome as i32;
    run_report.detail.clone_from(&loop_outcome.end.detail);
    run_report
        .harness_session_id
        .clone_from(&loop_outcome.session_id);
    if let Some(retry_dur) = loop_outcome.end.retry_after {
        let retry_timestamp = chrono::Utc::now()
            + chrono::Duration::from_std(retry_dur).unwrap_or(chrono::Duration::hours(1));
        run_report.retry_after = Some(df_proto::pbjson_types::Timestamp {
            seconds: retry_timestamp.timestamp(),
            nanos: 0,
        });
    }
    run_report
}
