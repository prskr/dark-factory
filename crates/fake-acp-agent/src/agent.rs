//! ACP Agent implementation for `fake-acp-agent`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use agent_client_protocol::schema::v1::{
    AgentCapabilities, CancelNotification, Content, ContentBlock, ContentChunk, Error,
    InitializeRequest, InitializeResponse, LoadSessionRequest, LoadSessionResponse, McpServer,
    McpServerStdio, NewSessionRequest, NewSessionResponse, Plan, PlanEntry, PlanEntryPriority,
    PlanEntryStatus, PromptRequest, PromptResponse, SessionId, SessionNotification, SessionUpdate,
    StopReason, TextContent, ToolCall, ToolCallContent, ToolCallId, ToolCallStatus, ToolCallUpdate,
    ToolCallUpdateFields, UsageUpdate,
};
use agent_client_protocol::{
    Agent, Builder, Client, ConnectionTo, HandleDispatchFrom, Responder, Stdio,
};
use rmcp::ServiceExt;
use tokio::sync::{Mutex, Notify};
use uuid::Uuid;

use crate::script::{
    EndStep, FetchStep, McpStep, PlanStep, Script, Step, ToolCallStep, Turn, UsageStep,
};

/// Pause between the attempts of a `fetch` step.
const FETCH_RETRY: Duration = Duration::from_secs(1);
/// Limit of one attempt of a `fetch` step.
const FETCH_ATTEMPT: Duration = Duration::from_secs(10);

/// Tracks active session state.
#[derive(Debug, Clone)]
pub struct SessionState {
    pub session_id: SessionId,
    pub cwd: PathBuf,
    pub is_loaded: bool,
    pub turn_index: usize,
    pub mcp_servers: Vec<McpServer>,
}

/// Shared state across all agent request handlers.
#[derive(Debug)]
pub struct AgentState {
    pub script: Script,
    pub sessions: HashMap<String, SessionState>,
    pub cancel_notify: Arc<Notify>,
    pub cancelled: Arc<AtomicBool>,
    pub session_dir: PathBuf,
}

impl AgentState {
    pub fn new(script: Script, session_dir: PathBuf) -> Self {
        Self {
            script,
            sessions: HashMap::new(),
            cancel_notify: Arc::new(Notify::new()),
            cancelled: Arc::new(AtomicBool::new(false)),
            session_dir,
        }
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "stable fallback dir when HOME is unset; a tempfile dir would be deleted, sessions outlive the process"
    )]
    pub fn default_session_dir() -> PathBuf {
        let home = std::env::var_os("HOME").map_or_else(std::env::temp_dir, PathBuf::from);
        home.join(".fake-acp").join("sessions")
    }
}

/// Build an ACP Agent instance wired with scripted handlers.
pub fn build_agent(
    state: Arc<Mutex<AgentState>>,
) -> Builder<
    Agent,
    impl HandleDispatchFrom<<Agent as agent_client_protocol::role::Role>::Counterpart>,
> {
    let state_new = Arc::clone(&state);
    let state_load = Arc::clone(&state);
    let state_prompt = Arc::clone(&state);
    let state_cancel = Arc::clone(&state);

    Agent
        .builder()
        .name("fake-acp-agent")
        .on_receive_request(
            async move |req: InitializeRequest,
                        responder: Responder<InitializeResponse>,
                        _cx: ConnectionTo<Client>| {
                handle_initialize(req, responder)
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: NewSessionRequest,
                        responder: Responder<NewSessionResponse>,
                        _cx: ConnectionTo<Client>| {
                handle_new_session(&state_new, req, responder).await
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: LoadSessionRequest,
                        responder: Responder<LoadSessionResponse>,
                        cx: ConnectionTo<Client>| {
                handle_load_session(&state_load, req, responder, &cx).await
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: PromptRequest,
                        responder: Responder<PromptResponse>,
                        cx: ConnectionTo<Client>| {
                let state = Arc::clone(&state_prompt);
                let cx_task = cx.clone();
                cx.spawn(async move { handle_prompt(&state, req, responder, &cx_task).await })
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_notification(
            async move |_cancel: CancelNotification, _cx: ConnectionTo<Client>| {
                let st = state_cancel.lock().await;
                st.cancelled.store(true, Ordering::SeqCst);
                st.cancel_notify.notify_waiters();
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
}

fn handle_initialize(
    req: InitializeRequest,
    responder: Responder<InitializeResponse>,
) -> Result<(), Error> {
    let caps = AgentCapabilities::new().load_session(true);
    responder.respond(InitializeResponse::new(req.protocol_version).agent_capabilities(caps))
}

async fn handle_new_session(
    state: &Mutex<AgentState>,
    req: NewSessionRequest,
    responder: Responder<NewSessionResponse>,
) -> Result<(), Error> {
    let session_uuid = Uuid::now_v7().to_string();
    let session_id = SessionId::new(&*session_uuid);

    let mut st = state.lock().await;
    // The session file only marks the session as existing; the fake agent does not need it.
    std::fs::create_dir_all(&st.session_dir).ok();
    std::fs::write(st.session_dir.join(&session_uuid), "").ok();

    st.sessions.insert(
        session_uuid,
        SessionState {
            session_id: session_id.clone(),
            cwd: req.cwd,
            is_loaded: false,
            turn_index: 0,
            mcp_servers: req.mcp_servers,
        },
    );

    responder.respond(NewSessionResponse::new(session_id))
}

async fn handle_load_session(
    state: &Mutex<AgentState>,
    req: LoadSessionRequest,
    responder: Responder<LoadSessionResponse>,
    cx: &ConnectionTo<Client>,
) -> Result<(), Error> {
    let session_id_str = req.session_id.to_string();
    let mut st = state.lock().await;

    let session_file = st.session_dir.join(&session_id_str);
    if !session_file.exists() && !st.sessions.contains_key(&session_id_str) {
        return responder.respond_with_error(Error::invalid_params().data("session not found"));
    }

    st.sessions.insert(
        session_id_str,
        SessionState {
            session_id: req.session_id.clone(),
            cwd: req.cwd,
            is_loaded: true,
            turn_index: 0,
            mcp_servers: req.mcp_servers,
        },
    );

    // Like real adapters, replay the history before answering the load. The script
    // has no recording of it, so the `Say` steps of the first-run turns stand in.
    for step in st.script.sessions.new.iter().flat_map(|turn| &turn.steps) {
        if let Step::Say(text) = step {
            send_update(cx, &req.session_id, text_chunk_update(text.clone(), false));
        }
    }

    responder.respond(LoadSessionResponse::new())
}

/// What a scripted turn needs besides its steps.
struct TurnContext {
    session_id: SessionId,
    cwd: PathBuf,
    mcp_servers: Vec<McpServer>,
    cancel_notify: Arc<Notify>,
    cancelled: Arc<AtomicBool>,
}

/// How a step affects the rest of the turn.
enum StepOutcome {
    Continue,
    /// The turn ends with this stop reason.
    Stop(StopReason),
    /// The prompt fails with a rate-limit error.
    RateLimited {
        retry_after_seconds: u64,
    },
}

async fn handle_prompt(
    state: &Mutex<AgentState>,
    req: PromptRequest,
    responder: Responder<PromptResponse>,
    cx: &ConnectionTo<Client>,
) -> Result<(), Error> {
    let (turn, ctx) = match take_next_turn(state, &req.session_id).await {
        NextTurn::Missing => {
            return responder.respond_with_error(Error::invalid_params().data("session not found"));
        }
        NextTurn::Exhausted => {
            return responder.respond(PromptResponse::new(StopReason::EndTurn));
        }
        NextTurn::Turn(turn, ctx) => (turn, ctx),
    };

    let mut stop_reason = StopReason::EndTurn;
    for step in turn.steps {
        match play_step(step, &ctx, cx).await {
            StepOutcome::Continue => {}
            StepOutcome::Stop(reason) => stop_reason = reason,
            StepOutcome::RateLimited {
                retry_after_seconds,
            } => {
                return responder.respond_with_error(Error::internal_error().data(
                    serde_json::json!({
                        "error": "rate_limited",
                        "retry_after_seconds": retry_after_seconds,
                        "message": format!(
                            "Rate limit exceeded. Retry after {retry_after_seconds} seconds."
                        )
                    }),
                ));
            }
        }
    }

    // A cancel only applies to the prompt it was sent for. Resetting here, not at the
    // start, keeps a cancel that arrives before this task is scheduled.
    ctx.cancelled.store(false, Ordering::SeqCst);
    responder.respond(PromptResponse::new(stop_reason))
}

/// The next scripted turn of a session.
enum NextTurn {
    /// The session is unknown.
    Missing,
    /// The script has no turn left for the session.
    Exhausted,
    Turn(Turn, TurnContext),
}

/// Takes the next scripted turn of the session (the first-run or the loaded script) and advances
/// its turn index.
async fn take_next_turn(state: &Mutex<AgentState>, session_id: &SessionId) -> NextTurn {
    let session_id_str = session_id.to_string();
    let mut st = state.lock().await;

    let Some(is_loaded) = st.sessions.get(&session_id_str).map(|s| s.is_loaded) else {
        return NextTurn::Missing;
    };
    let turns = if is_loaded {
        st.script.sessions.loaded.clone()
    } else {
        st.script.sessions.new.clone()
    };
    let cancel_notify = Arc::clone(&st.cancel_notify);
    let cancelled = Arc::clone(&st.cancelled);
    let Some(session) = st.sessions.get_mut(&session_id_str) else {
        return NextTurn::Missing;
    };
    let Some(turn) = turns.get(session.turn_index).cloned() else {
        return NextTurn::Exhausted;
    };
    session.turn_index += 1;
    NextTurn::Turn(
        turn,
        TurnContext {
            session_id: session_id.clone(),
            cwd: session.cwd.clone(),
            mcp_servers: session.mcp_servers.clone(),
            cancel_notify,
            cancelled,
        },
    )
}

/// Sends a session update to the client; a closed connection is not an error for the script.
fn send_update(cx: &ConnectionTo<Client>, session_id: &SessionId, update: SessionUpdate) {
    cx.send_notification(SessionNotification::new(session_id.clone(), update))
        .ok();
}

/// A text chunk of the agent's message, or of its thoughts if `thought`.
fn text_chunk_update(text: String, thought: bool) -> SessionUpdate {
    let chunk = ContentChunk::new(ContentBlock::Text(TextContent::new(text)));
    if thought {
        SessionUpdate::AgentThoughtChunk(chunk)
    } else {
        SessionUpdate::AgentMessageChunk(chunk)
    }
}

/// Plays one step of a turn.
async fn play_step(step: Step, ctx: &TurnContext, cx: &ConnectionTo<Client>) -> StepOutcome {
    match step {
        Step::Say(text) => send_update(cx, &ctx.session_id, text_chunk_update(text, false)),
        Step::Think(thought) => send_update(cx, &ctx.session_id, text_chunk_update(thought, true)),
        Step::ToolCall(tool_step) => play_tool_call(tool_step, ctx, cx),
        Step::Plan(plan_step) => play_plan(plan_step, ctx, cx),
        Step::Usage(usage_step) => play_usage(&usage_step, ctx, cx),
        Step::Shell(cmd) => run_shell(&cmd, &ctx.cwd).await,
        Step::Fetch(fetch) => play_fetch(&fetch, ctx, cx).await,
        Step::Mcp(mcp_step) => play_mcp(mcp_step, ctx, cx).await,
        Step::End(end_step) => return play_end(end_step, ctx).await,
    }
    StepOutcome::Continue
}

fn play_tool_call(tool_step: ToolCallStep, ctx: &TurnContext, cx: &ConnectionTo<Client>) {
    let call_id = ToolCallId::new(&*tool_step.id);
    let title = tool_step.title.unwrap_or_else(|| "Tool Call".to_string());
    let mut call = ToolCall::new(call_id.clone(), title);
    if let Some(name) = tool_step.name {
        call.name = Some(name);
    }
    send_update(cx, &ctx.session_id, SessionUpdate::ToolCall(call));

    let mut update_fields = ToolCallUpdateFields::new();
    if let Some(out) = tool_step.output {
        update_fields = update_fields.content(vec![text_content(out.to_string())]);
    }
    let status = match tool_step.status.as_deref() {
        Some("failed") => ToolCallStatus::Failed,
        Some("in_progress") => ToolCallStatus::InProgress,
        _ => ToolCallStatus::Completed,
    };
    let update = ToolCallUpdate::new(call_id, update_fields.status(status));
    send_update(cx, &ctx.session_id, SessionUpdate::ToolCallUpdate(update));
}

fn text_content(text: String) -> ToolCallContent {
    ToolCallContent::Content(Content::new(ContentBlock::Text(TextContent::new(text))))
}

fn play_plan(plan_step: PlanStep, ctx: &TurnContext, cx: &ConnectionTo<Client>) {
    let entries = plan_step
        .entries
        .into_iter()
        .map(|e| {
            let status = match e.status.as_str() {
                "completed" => PlanEntryStatus::Completed,
                "in_progress" => PlanEntryStatus::InProgress,
                _ => PlanEntryStatus::Pending,
            };
            PlanEntry::new(e.content, PlanEntryPriority::Medium, status)
        })
        .collect();
    send_update(cx, &ctx.session_id, SessionUpdate::Plan(Plan::new(entries)));
}

fn play_usage(usage_step: &UsageStep, ctx: &TurnContext, cx: &ConnectionTo<Client>) {
    let used = usage_step
        .used
        .unwrap_or(usage_step.input_tokens.unwrap_or(0) + usage_step.output_tokens.unwrap_or(0));
    let size = usage_step.size.unwrap_or(200_000);
    let usage = UsageUpdate::new(used, size);
    send_update(cx, &ctx.session_id, SessionUpdate::UsageUpdate(usage));
}

/// Runs a shell command in the session's working directory; failures are only logged.
#[expect(
    clippy::print_stderr,
    reason = "the fake agent reports failures on stderr, the harness captures it"
)]
async fn run_shell(cmd: &str, cwd: &Path) {
    let output = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(cwd)
        .output()
        .await;
    match output {
        Ok(out) if !out.status.success() => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            eprintln!(
                "fake-acp-agent: shell command '{cmd}' exited with {}: {stderr}",
                out.status
            );
        }
        Ok(_) => {}
        Err(e) => {
            eprintln!("fake-acp-agent: failed to execute shell command '{cmd}': {e}");
        }
    }
}

/// Plays a `fetch` step and says its outcome: `Fetched <url>.` or `Could not fetch <url>: <error>`.
async fn play_fetch(fetch: &FetchStep, ctx: &TurnContext, cx: &ConnectionTo<Client>) {
    let text = match fetch_until_ok(fetch, ctx).await {
        Ok(()) => format!("Fetched {}.", fetch.url),
        Err(error) => format!("Could not fetch {}: {error}", fetch.url),
    };
    send_update(cx, &ctx.session_id, text_chunk_update(text, false));
}

/// GETs the URL until it succeeds, the timeout has passed or the turn is cancelled; the error
/// is the one of the last attempt.
async fn fetch_until_ok(fetch: &FetchStep, ctx: &TurnContext) -> Result<(), String> {
    let deadline = tokio::time::Instant::now()
        .checked_add(Duration::from_secs(fetch.timeout_seconds))
        .ok_or_else(|| format!("timeout_seconds {} is too large", fetch.timeout_seconds))?;
    loop {
        let limit = fetch_attempt_limit(deadline, tokio::time::Instant::now());
        let error = match fetch_once(&fetch.url, limit).await {
            Ok(()) => return Ok(()),
            Err(error) => error,
        };
        if ctx.cancelled.load(Ordering::SeqCst) {
            return Err(format!("cancelled ({error})"));
        }
        if tokio::time::Instant::now() + FETCH_RETRY > deadline {
            return Err(error);
        }
        if sleep_unless_cancelled(FETCH_RETRY, &ctx.cancel_notify, &ctx.cancelled).await {
            return Err(format!("cancelled ({error})"));
        }
    }
}

/// Limit of the next attempt of a `fetch` step: the time left until `deadline`, at most
/// [`FETCH_ATTEMPT`] and at least a second.
pub(crate) fn fetch_attempt_limit(
    deadline: tokio::time::Instant,
    now: tokio::time::Instant,
) -> Duration {
    deadline
        .saturating_duration_since(now)
        .clamp(Duration::from_secs(1), FETCH_ATTEMPT)
}

/// Sleeps for `duration` unless the turn is or gets cancelled; returns whether it was.
pub(crate) async fn sleep_unless_cancelled(
    duration: Duration,
    cancel_notify: &Notify,
    cancelled: &AtomicBool,
) -> bool {
    // Register for the wakeup before checking the flag, so a cancel in between is not lost.
    let notified = cancel_notify.notified();
    tokio::pin!(notified);
    notified.as_mut().enable();
    if cancelled.load(Ordering::SeqCst) {
        return true;
    }
    tokio::select! {
        () = notified => true,
        () = tokio::time::sleep(duration) => false,
    }
}

async fn fetch_once(url: &str, limit: Duration) -> Result<(), String> {
    let output = tokio::process::Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--fail",
            "--output",
            "/dev/null",
        ])
        .args(["--max-time", &limit.as_secs().to_string(), "--url", url])
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| format!("cannot run curl: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

/// Reports a tool call to a stdio MCP server of the session and its result.
async fn play_mcp(mcp_step: McpStep, ctx: &TurnContext, cx: &ConnectionTo<Client>) {
    let matched_server = ctx.mcp_servers.iter().find_map(|s| match s {
        McpServer::Stdio(stdio) if stdio.name == mcp_step.server => Some(stdio.clone()),
        _ => None,
    });

    let call_id = ToolCallId::new(&*format!("call_{}", Uuid::now_v7()));
    let tool_title = format!("mcp__{}__{}", mcp_step.server, mcp_step.tool);
    let mut call = ToolCall::new(call_id.clone(), tool_title.clone());
    call.name = Some(tool_title);
    send_update(cx, &ctx.session_id, SessionUpdate::ToolCall(call));

    let (tool_result_text, status) = match matched_server {
        Some(stdio) => call_mcp_tool(&stdio, &mcp_step).await,
        None => (
            format!("MCP server '{}' not found in session", mcp_step.server),
            ToolCallStatus::Failed,
        ),
    };

    let update_fields = ToolCallUpdateFields::new()
        .content(vec![text_content(tool_result_text)])
        .status(status);
    let update = ToolCallUpdate::new(call_id, update_fields);
    send_update(cx, &ctx.session_id, SessionUpdate::ToolCallUpdate(update));
}

/// Starts the MCP server, calls the tool of `mcp_step` and returns the result text and status.
async fn call_mcp_tool(stdio: &McpServerStdio, mcp_step: &McpStep) -> (String, ToolCallStatus) {
    let mut child_cmd = tokio::process::Command::new(&stdio.command);
    child_cmd.args(&stdio.args);
    for env in &stdio.env {
        child_cmd.env(&env.name, &env.value);
    }
    child_cmd.stdin(std::process::Stdio::piped());
    child_cmd.stdout(std::process::Stdio::piped());
    // Don't leave the MCP server running after the call
    child_cmd.kill_on_drop(true);

    let mut child = match child_cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            return (
                format!("Failed to spawn MCP server '{}': {e}", stdio.name),
                ToolCallStatus::Failed,
            );
        }
    };
    let (Some(stdout), Some(stdin)) = (child.stdout.take(), child.stdin.take()) else {
        return (
            "Failed to capture stdio".to_string(),
            ToolCallStatus::Failed,
        );
    };
    let client = match ().serve((stdout, stdin)).await {
        Ok(client) => client,
        Err(e) => {
            return (
                format!("Failed to connect to MCP server: {e}"),
                ToolCallStatus::Failed,
            );
        }
    };
    let mut params = rmcp::model::CallToolRequestParams::new(mcp_step.tool.clone());
    if let serde_json::Value::Object(map) = &mcp_step.args {
        params = params.with_arguments(map.clone());
    }
    match client.call_tool(params).await {
        Ok(res) => (
            serde_json::to_string(&res).unwrap_or_default(),
            ToolCallStatus::Completed,
        ),
        Err(e) => (format!("Tool call failed: {e}"), ToolCallStatus::Failed),
    }
}

/// Plays an `end` step: the stop reason of the turn, a crash, a hang until cancelled, or a rate
/// limit error.
async fn play_end(end_step: EndStep, ctx: &TurnContext) -> StepOutcome {
    match end_step {
        EndStep::EndTurn => StepOutcome::Stop(StopReason::EndTurn),
        EndStep::Cancelled => StepOutcome::Stop(StopReason::Cancelled),
        EndStep::MaxTokens => StepOutcome::Stop(StopReason::MaxTokens),
        EndStep::MaxTurnRequests => StepOutcome::Stop(StopReason::MaxTurnRequests),
        EndStep::Refusal => StepOutcome::Stop(StopReason::Refusal),
        EndStep::Crash => std::process::exit(1),
        EndStep::Hang => {
            // Register for the wakeup before checking the flag, so a cancel
            // arriving in between is not lost.
            let notified = ctx.cancel_notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if !ctx.cancelled.load(Ordering::SeqCst) {
                notified.await;
            }
            StepOutcome::Stop(StopReason::Cancelled)
        }
        EndStep::RateLimited(rl) => StepOutcome::RateLimited {
            retry_after_seconds: rl.retry_after_seconds.unwrap_or(3600),
        },
    }
}

/// Run the fake ACP agent over standard I/O until connection close.
pub async fn run_stdio(script: Option<Script>) -> anyhow::Result<()> {
    let script = script.unwrap_or_default();
    let session_dir = AgentState::default_session_dir();
    let state = Arc::new(Mutex::new(AgentState::new(script, session_dir)));
    let agent = build_agent(state);
    agent.connect_to(Stdio::new()).await?;
    Ok(())
}
