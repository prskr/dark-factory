//! ACP Agent implementation for `fake-acp-agent`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use agent_client_protocol::schema::v1::{
    AgentCapabilities, CancelNotification, Content, ContentBlock, ContentChunk, Error,
    InitializeRequest, InitializeResponse, LoadSessionRequest, LoadSessionResponse, McpServer,
    NewSessionRequest, NewSessionResponse, Plan, PlanEntry, PlanEntryPriority, PlanEntryStatus,
    PromptRequest, PromptResponse, SessionId, SessionNotification, SessionUpdate, StopReason,
    TextContent, ToolCall, ToolCallContent, ToolCallId, ToolCallStatus, ToolCallUpdate,
    ToolCallUpdateFields, UsageUpdate,
};
use agent_client_protocol::{
    Agent, Builder, Client, ConnectionTo, HandleDispatchFrom, Responder, Stdio,
};
use rmcp::ServiceExt;
use tokio::sync::{Mutex, Notify};
use uuid::Uuid;

use crate::script::{EndStep, Script, Step};

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

    pub fn default_session_dir() -> PathBuf {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        home.join(".fake-acp").join("sessions")
    }
}

/// Build an ACP Agent instance wired with scripted handlers.
#[expect(
    clippy::too_many_lines,
    reason = "split build_agent into one builder per scripted handler"
)]
pub fn build_agent(
    state: Arc<Mutex<AgentState>>,
) -> Builder<
    agent_client_protocol::role::acp::Agent,
    impl HandleDispatchFrom<
        <agent_client_protocol::role::acp::Agent as agent_client_protocol::role::Role>::Counterpart,
    >,
> {
    let _state_init = Arc::clone(&state);
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
                let mut caps = AgentCapabilities::new();
                caps = caps.load_session(true);
                let response =
                    InitializeResponse::new(req.protocol_version).agent_capabilities(caps);
                responder.respond(response)
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: NewSessionRequest,
                        responder: Responder<NewSessionResponse>,
                        _cx: ConnectionTo<Client>| {
                let session_uuid = Uuid::now_v7().to_string();
                let session_id = SessionId::new(&*session_uuid);

                let mut st = state_new.lock().await;
                let _ = std::fs::create_dir_all(&st.session_dir);
                let _ = std::fs::write(st.session_dir.join(&session_uuid), "");

                st.sessions.insert(
                    session_uuid.clone(),
                    SessionState {
                        session_id: session_id.clone(),
                        cwd: req.cwd,
                        is_loaded: false,
                        turn_index: 0,
                        mcp_servers: req.mcp_servers,
                    },
                );

                responder.respond(NewSessionResponse::new(session_id))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: LoadSessionRequest,
                        responder: Responder<LoadSessionResponse>,
                        cx: ConnectionTo<Client>| {
                let session_id_str = req.session_id.to_string();
                let mut st = state_load.lock().await;

                let session_file = st.session_dir.join(&session_id_str);
                if !session_file.exists() && !st.sessions.contains_key(&session_id_str) {
                    return responder.respond_with_error(
                        Error::invalid_params().data("session not found"),
                    );
                }

                st.sessions.insert(
                    session_id_str.clone(),
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
                        let chunk = ContentChunk::new(ContentBlock::Text(TextContent::new(text.clone())));
                        let _ = cx.send_notification(SessionNotification::new(
                            req.session_id.clone(),
                            SessionUpdate::AgentMessageChunk(chunk),
                        ));
                    }
                }

                responder.respond(LoadSessionResponse::new())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: PromptRequest,
                        responder: Responder<PromptResponse>,
                        cx: ConnectionTo<Client>| {
                let state_prompt_task = Arc::clone(&state_prompt);
                let cx_task = cx.clone();
                cx.spawn(async move {
                    let session_id_str = req.session_id.to_string();
                    let mut st = state_prompt_task.lock().await;

                    let is_loaded = match st.sessions.get(&session_id_str) {
                        Some(s) => s.is_loaded,
                        None => {
                            return responder.respond_with_error(
                                Error::invalid_params().data("session not found"),
                            );
                        }
                    };

                    let turns = if is_loaded {
                        st.script.sessions.loaded.clone()
                    } else {
                        st.script.sessions.new.clone()
                    };

                    let cancel_notify = Arc::clone(&st.cancel_notify);
                    let cancelled = Arc::clone(&st.cancelled);
                    let session = st.sessions.get_mut(&session_id_str).unwrap();
                    let session_cwd = session.cwd.clone();
                    let session_mcp_servers = session.mcp_servers.clone();
                    let turn = if session.turn_index < turns.len() {
                        let t = turns[session.turn_index].clone();
                        session.turn_index += 1;
                        t
                    } else {
                        return responder.respond(PromptResponse::new(StopReason::EndTurn));
                    };

                    drop(st);

                    let mut stop_reason = StopReason::EndTurn;

                    for step in turn.steps {
                        match step {
                            Step::Say(text) => {
                                let chunk =
                                    ContentChunk::new(ContentBlock::Text(TextContent::new(text)));
                                let _ = cx_task.send_notification(SessionNotification::new(
                                    req.session_id.clone(),
                                    SessionUpdate::AgentMessageChunk(chunk),
                                ));
                            }
                            Step::Think(thought) => {
                                let chunk =
                                    ContentChunk::new(ContentBlock::Text(TextContent::new(thought)));
                                let _ = cx_task.send_notification(SessionNotification::new(
                                    req.session_id.clone(),
                                    SessionUpdate::AgentThoughtChunk(chunk),
                                ));
                            }
                            Step::ToolCall(tool_step) => {
                                let call_id = ToolCallId::new(&*tool_step.id);
                                let title = tool_step
                                    .title
                                    .unwrap_or_else(|| "Tool Call".to_string());
                                let mut call = ToolCall::new(call_id.clone(), title);
                                if let Some(name) = tool_step.name {
                                    call.name = Some(name);
                                }
                                let _ = cx_task.send_notification(SessionNotification::new(
                                    req.session_id.clone(),
                                    SessionUpdate::ToolCall(call),
                                ));

                                let mut update_fields = ToolCallUpdateFields::new();
                                if let Some(out) = tool_step.output {
                                    update_fields = update_fields.content(vec![
                                        ToolCallContent::Content(Content::new(ContentBlock::Text(
                                            TextContent::new(out.to_string()),
                                        ))),
                                    ]);
                                }
                                let status = match tool_step.status.as_deref() {
                                    Some("failed") => ToolCallStatus::Failed,
                                    Some("in_progress") => ToolCallStatus::InProgress,
                                    _ => ToolCallStatus::Completed,
                                };
                                update_fields = update_fields.status(status);
                                let update = ToolCallUpdate::new(call_id, update_fields);
                                let _ = cx_task.send_notification(SessionNotification::new(
                                    req.session_id.clone(),
                                    SessionUpdate::ToolCallUpdate(update),
                                ));
                            }
                            Step::Plan(plan_step) => {
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
                                let plan = Plan::new(entries);
                                let _ = cx_task.send_notification(SessionNotification::new(
                                    req.session_id.clone(),
                                    SessionUpdate::Plan(plan),
                                ));
                            }
                            Step::Usage(usage_step) => {
                                let used = usage_step.used.unwrap_or(
                                    usage_step.input_tokens.unwrap_or(0)
                                        + usage_step.output_tokens.unwrap_or(0),
                                );
                                let size = usage_step.size.unwrap_or(200_000);
                                let usage = UsageUpdate::new(used, size);
                                let _ = cx_task.send_notification(SessionNotification::new(
                                    req.session_id.clone(),
                                    SessionUpdate::UsageUpdate(usage),
                                ));
                            }
                            Step::Shell(cmd) => {
                                let output = tokio::process::Command::new("sh")
                                    .arg("-c")
                                    .arg(&cmd)
                                    .current_dir(&session_cwd)
                                    .output()
                                    .await;
                                match output {
                                    Ok(out) => {
                                        if !out.status.success() {
                                            let stderr = String::from_utf8_lossy(&out.stderr);
                                            eprintln!(
                                                "fake-acp-agent: shell command '{cmd}' exited with {}: {stderr}",
                                                out.status
                                            );
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!(
                                            "fake-acp-agent: failed to execute shell command '{cmd}': {e}"
                                        );
                                    }
                                }
                            }
                            Step::Mcp(mcp_step) => {
                                let matched_server = session_mcp_servers.iter().find_map(|s| match s {
                                    McpServer::Stdio(stdio) if stdio.name == mcp_step.server => {
                                        Some(stdio.clone())
                                    }
                                    _ => None,
                                });

                                let call_id = ToolCallId::new(&*format!("call_{}", Uuid::now_v7()));
                                let tool_title =
                                    format!("mcp__{}__{}", mcp_step.server, mcp_step.tool);
                                let mut call = ToolCall::new(call_id.clone(), tool_title);
                                call.name =
                                    Some(format!("mcp__{}__{}", mcp_step.server, mcp_step.tool));

                                let _ = cx_task.send_notification(SessionNotification::new(
                                    req.session_id.clone(),
                                    SessionUpdate::ToolCall(call),
                                ));

                                let (tool_result_text, status) = if let Some(stdio) = matched_server {
                                    let mut child_cmd = tokio::process::Command::new(&stdio.command);
                                    child_cmd.args(&stdio.args);
                                    for env in &stdio.env {
                                        child_cmd.env(&env.name, &env.value);
                                    }
                                    child_cmd.stdin(std::process::Stdio::piped());
                                    child_cmd.stdout(std::process::Stdio::piped());
                                    // Don't leave the MCP server running after the call
                                    child_cmd.kill_on_drop(true);

                                    match child_cmd.spawn() {
                                        Ok(mut child) => {
                                            let child_stdout = child.stdout.take();
                                            let child_stdin = child.stdin.take();

                                            if let (Some(stdout), Some(stdin)) =
                                                (child_stdout, child_stdin)
                                            {
                                                match ().serve((stdout, stdin)).await {
                                                    Ok(client) => {
                                                        let mut params =
                                                            rmcp::model::CallToolRequestParams::new(
                                                                mcp_step.tool.clone(),
                                                            );
                                                        if let serde_json::Value::Object(map) =
                                                            mcp_step.args
                                                        {
                                                            params = params.with_arguments(map);
                                                        }
                                                        match client.call_tool(params).await {
                                                            Ok(res) => {
                                                                let json_str =
                                                                    serde_json::to_string(&res)
                                                                        .unwrap_or_default();
                                                                (json_str, ToolCallStatus::Completed)
                                                            }
                                                            Err(e) => (
                                                                format!("Tool call failed: {e}"),
                                                                ToolCallStatus::Failed,
                                                            ),
                                                        }
                                                    }
                                                    Err(e) => (
                                                        format!(
                                                            "Failed to connect to MCP server: {e}"
                                                        ),
                                                        ToolCallStatus::Failed,
                                                    ),
                                                }
                                            } else {
                                                (
                                                    "Failed to capture stdio".to_string(),
                                                    ToolCallStatus::Failed,
                                                )
                                            }
                                        }
                                        Err(e) => (
                                            format!("Failed to spawn MCP server '{}': {e}", stdio.name),
                                            ToolCallStatus::Failed,
                                        ),
                                    }
                                } else {
                                    (
                                        format!(
                                            "MCP server '{}' not found in session",
                                            mcp_step.server
                                        ),
                                        ToolCallStatus::Failed,
                                    )
                                };

                                let mut update_fields = ToolCallUpdateFields::new();
                                update_fields = update_fields.content(vec![
                                    ToolCallContent::Content(Content::new(ContentBlock::Text(
                                        TextContent::new(tool_result_text),
                                    ))),
                                ]);
                                update_fields = update_fields.status(status);
                                let update = ToolCallUpdate::new(call_id, update_fields);
                                let _ = cx_task.send_notification(SessionNotification::new(
                                    req.session_id.clone(),
                                    SessionUpdate::ToolCallUpdate(update),
                                ));
                            }
                            Step::End(end_step) => match end_step {
                                EndStep::EndTurn => {
                                    stop_reason = StopReason::EndTurn;
                                }
                                EndStep::Cancelled => {
                                    stop_reason = StopReason::Cancelled;
                                }
                                EndStep::MaxTokens => {
                                    stop_reason = StopReason::MaxTokens;
                                }
                                EndStep::MaxTurnRequests => {
                                    stop_reason = StopReason::MaxTurnRequests;
                                }
                                EndStep::Refusal => {
                                    stop_reason = StopReason::Refusal;
                                }
                                EndStep::Crash => {
                                    std::process::exit(1);
                                }
                                EndStep::Hang => {
                                    // Register for the wakeup before checking the flag, so a cancel
                                    // arriving in between is not lost.
                                    let notified = cancel_notify.notified();
                                    tokio::pin!(notified);
                                    notified.as_mut().enable();
                                    if !cancelled.load(Ordering::SeqCst) {
                                        notified.await;
                                    }
                                    stop_reason = StopReason::Cancelled;
                                }
                                EndStep::RateLimited(rl) => {
                                    let seconds = rl.retry_after_seconds.unwrap_or(3600);
                                    return responder.respond_with_error(
                                        Error::internal_error().data(serde_json::json!({
                                            "error": "rate_limited",
                                            "retry_after_seconds": seconds,
                                            "message": format!("Rate limit exceeded. Retry after {seconds} seconds.")
                                        })),
                                    );
                                }
                            },
                        }
                    }

                    // A cancel only applies to the prompt it was sent for. Resetting here, not at the
                    // start, keeps a cancel that arrives before this task is scheduled.
                    cancelled.store(false, Ordering::SeqCst);
                    responder.respond(PromptResponse::new(stop_reason))
                })
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

/// Run the fake ACP agent over standard I/O until connection close.
pub async fn run_stdio(script: Option<Script>) -> anyhow::Result<()> {
    let script = script.unwrap_or_default();
    let session_dir = AgentState::default_session_dir();
    let state = Arc::new(Mutex::new(AgentState::new(script, session_dir)));
    let agent = build_agent(state);
    agent.connect_to(Stdio::new()).await?;
    Ok(())
}
