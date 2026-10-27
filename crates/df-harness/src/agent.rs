//! ACP agent handlers: one `rpi_agent::Agent` per ACP session.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::broadcast;

use agent_client_protocol::schema::v1::{
    AgentCapabilities, CancelNotification, ContentBlock, ContentChunk, Error, InitializeRequest,
    InitializeResponse, NewSessionRequest, NewSessionResponse, PromptRequest, PromptResponse,
    SessionId, SessionNotification, SessionUpdate, StopReason, TextContent, ToolCall, ToolCallId,
    ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, UsageUpdate,
};
use agent_client_protocol::{Agent as AcpAgent, Client, ConnectionTo, Responder, Stdio};
use rpi_agent::{AgentBuilder, AgentEvent, AgentTool, StreamFn};
use rpi_ai::Provider;
use rpi_ai::model::Model;
use rpi_ai::providers::openai_completions::OpenAiCompletionsProvider;
use rpi_ai::types::{Api, AssistantMessageEvent, StopReason as RpiStop};
use rpi_tools::{
    ExecutionToolContext, OsExecutionEnv, create_bash_tool, create_edit_tool, create_read_tool,
    create_write_tool,
};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::Cli;

const SYSTEM_PROMPT: &str = "You are a coding agent. Work in the current directory with the \
available tools and finish the task you are given.";

const CONTEXT_WINDOW: u64 = 128_000;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// Max silence between response bytes. Total stream time is not capped, so long generations work.
const READ_TIMEOUT: Duration = Duration::from_secs(120);

fn http_client(connect: Duration, read: Duration) -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(connect)
        .read_timeout(read)
        .build()
}

/// One ACP session: the loop plus a flag that `session/cancel` sets, because a cancel during a
/// tool call ends the loop without an aborted assistant message.
#[derive(Clone)]
struct Session {
    agent: Arc<rpi_agent::Agent>,
    cancelled: Arc<AtomicBool>,
}

type Sessions = Arc<Mutex<HashMap<String, Session>>>;

pub async fn run_stdio(cli: Cli) -> anyhow::Result<()> {
    let model = Arc::new(model(&cli));
    let provider = Arc::new(OpenAiCompletionsProvider::with_models_without_env_api_key(
        "df-endpoint",
        Some(cli.api_key),
        http_client(CONNECT_TIMEOUT, READ_TIMEOUT)?,
        vec![(*model).clone()],
    ));
    let sessions: Sessions = Arc::default();

    let (s_new, s_prompt, s_cancel) = (sessions.clone(), sessions.clone(), sessions);
    let (m_new, p_new) = (model.clone(), provider);

    AcpAgent
        .builder()
        .name("df-harness")
        .on_receive_request(
            async move |req: InitializeRequest,
                        responder: Responder<InitializeResponse>,
                        _cx: ConnectionTo<Client>| {
                responder.respond(
                    InitializeResponse::new(req.protocol_version)
                        .agent_capabilities(AgentCapabilities::new()),
                )
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: NewSessionRequest,
                        responder: Responder<NewSessionResponse>,
                        _cx: ConnectionTo<Client>| {
                let id = Uuid::now_v7().to_string();
                let mcp_tools =
                    match crate::mcp::connect(&req.mcp_servers, crate::mcp::SETUP_TIMEOUT).await {
                        Ok(t) => t,
                        Err(e) => {
                            return responder.respond_with_error(
                                Error::internal_error().data(format!("mcp server failed: {e:#}")),
                            );
                        }
                    };
                let agent = build_agent(&m_new, p_new.clone(), req.cwd, mcp_tools);
                s_new.lock().await.insert(
                    id.clone(),
                    Session {
                        agent: Arc::new(agent),
                        cancelled: Arc::default(),
                    },
                );
                responder.respond(NewSessionResponse::new(SessionId::new(&*id)))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: PromptRequest,
                        responder: Responder<PromptResponse>,
                        cx: ConnectionTo<Client>| {
                let session = s_prompt.lock().await.get(&*req.session_id.0).cloned();
                let Some(session) = session else {
                    return responder
                        .respond_with_error(Error::invalid_params().data("session not found"));
                };
                let cx_task = cx.clone();
                cx.spawn(async move {
                    let text = req
                        .prompt
                        .iter()
                        .filter_map(|b| match b {
                            ContentBlock::Text(t) => Some(t.text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    match run_prompt(&session, &req.session_id, &cx_task, text, CONTEXT_WINDOW)
                        .await
                    {
                        Ok(stop) => responder.respond(PromptResponse::new(stop)),
                        Err(e) => responder.respond_with_error(e),
                    }
                })
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_notification(
            async move |note: CancelNotification, _cx: ConnectionTo<Client>| {
                if let Some(s) = s_cancel.lock().await.get(&*note.session_id.0) {
                    s.cancelled.store(true, Ordering::SeqCst);
                    s.agent.abort();
                }
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .connect_to(Stdio::new())
        .await?;
    Ok(())
}

fn model(cli: &Cli) -> Model {
    let mut m = Model::new(
        cli.model.clone(),
        cli.model.clone(),
        Api::OpenaiCompletions,
        "df-endpoint",
        cli.base_url.clone(),
    );
    m.reasoning = cli.reasoning;
    m.context_window = CONTEXT_WINDOW;
    m.max_tokens = 16_384;
    m
}

fn build_agent(
    model: &Model,
    provider: Arc<OpenAiCompletionsProvider>,
    cwd: PathBuf,
    mcp_tools: Vec<Arc<dyn AgentTool>>,
) -> rpi_agent::Agent {
    let env = Arc::new(OsExecutionEnv::with_cwd(cwd));
    let ctx = ExecutionToolContext::new(env.clone(), Some(env));
    let stream_fn: StreamFn = rpi_agent::stream_fn(move |m, c, o| {
        let (p, m, c, o) = (provider.clone(), m.clone(), c.clone(), o.clone());
        // ponytail: rpi's StreamFn is synchronous, so bridge into async here.
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(async move { p.stream_simple(&m, &c, &o).await })
        })
    });
    AgentBuilder::new()
        .model(model.clone())
        .system_prompt(SYSTEM_PROMPT)
        .tools(
            [
                create_bash_tool(&ctx, None),
                create_read_tool(&ctx, None),
                create_write_tool(&ctx),
                create_edit_tool(&ctx),
            ]
            .into_iter()
            .chain(mcp_tools)
            .collect(),
        )
        .stream_fn(stream_fn)
        .build()
        .expect("builder has model and stream fn")
}

/// Forward loop events as ACP updates until the run settles, then map the stop reason.
async fn run_prompt(
    state: &Session,
    session: &SessionId,
    cx: &ConnectionTo<Client>,
    text: String,
    context_window: u64,
) -> Result<StopReason, Error> {
    let agent = &state.agent;
    state.cancelled.store(false, Ordering::SeqCst);
    let mut rx = agent.subscribe();
    let send = |update| {
        // Best-effort: the client may already be gone.
        let _ = cx.send_notification(SessionNotification::new(session.clone(), update));
    };
    let chunk = |s: String| ContentChunk::new(ContentBlock::Text(TextContent::new(s)));
    let run = agent.prompt(text);
    tokio::pin!(run);
    let mut last = RpiStop::Stop;
    let mut failure = None;
    let mut on_event = |ev: AgentEvent| match ev {
        AgentEvent::MessageUpdate {
            assistant_message_event: AssistantMessageEvent::TextDelta { delta, .. },
            ..
        } => send(SessionUpdate::AgentMessageChunk(chunk(delta))),
        AgentEvent::MessageUpdate {
            assistant_message_event: AssistantMessageEvent::ThinkingDelta { delta, .. },
            ..
        } => send(SessionUpdate::AgentThoughtChunk(chunk(delta))),
        AgentEvent::ToolExecutionStart {
            tool_call_id,
            tool_name,
            ..
        } => send(SessionUpdate::ToolCall(ToolCall::new(
            ToolCallId::new(&*tool_call_id),
            tool_name,
        ))),
        AgentEvent::ToolExecutionEnd {
            tool_call_id,
            is_error,
            ..
        } => {
            let status = if is_error {
                ToolCallStatus::Failed
            } else {
                ToolCallStatus::Completed
            };
            send(SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                ToolCallId::new(&*tool_call_id),
                ToolCallUpdateFields::new().status(status),
            )));
        }
        AgentEvent::TurnEnd {
            message: rpi_agent::AgentMessage::Assistant(m),
            ..
        } => {
            send(SessionUpdate::UsageUpdate(UsageUpdate::new(
                m.usage.total_tokens.max(0) as u64,
                context_window,
            )));
            last = m.stop_reason;
            failure = m.error_message;
        }
        _ => {}
    };
    // Run to completion (never drop `run`: it owns the agent's cleanup). Events are broadcast
    // before `run` returns, so draining the channel afterwards sees all of them.
    let res = loop {
        tokio::select! {
            res = &mut run => break res,
            ev = rx.recv() => match ev {
                Ok(ev) => on_event(ev),
                Err(broadcast::error::RecvError::Lagged(_)) => {} // dropped events, keep going
                Err(broadcast::error::RecvError::Closed) => break (&mut run).await,
            },
        }
    };
    while let Ok(ev) = rx.try_recv() {
        on_event(ev);
    }
    if let Err(e) = res {
        last = RpiStop::Error;
        failure.get_or_insert_with(|| e.to_string());
    }
    if state.cancelled.load(Ordering::SeqCst) {
        return Ok(StopReason::Cancelled);
    }
    if last == RpiStop::Error {
        return Err(provider_error(failure.unwrap_or_default()));
    }
    Ok(match last {
        RpiStop::Aborted => StopReason::Cancelled,
        RpiStop::Length => StopReason::MaxTokens,
        _ => StopReason::EndTurn,
    })
}

/// Shape the factory runtime parses (`parse_rate_limit` in factory-agent): structured
/// `rate_limited` data for HTTP 429, a plain internal error otherwise.
fn provider_error(message: String) -> Error {
    let lower = message.to_ascii_lowercase();
    if lower.contains("429") || lower.contains("too many requests") {
        Error::internal_error()
            .data(serde_json::json!({"error": "rate_limited", "message": message}))
    } else {
        Error::internal_error().data(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn silent_endpoint_hits_the_read_timeout() -> anyhow::Result<()> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://{}/", listener.local_addr()?);
        // Accept and never answer.
        tokio::spawn(async move {
            let _held = listener.accept().await;
            std::future::pending::<()>().await;
        });
        let client = http_client(Duration::from_secs(5), Duration::from_millis(200))?;
        let err = client.get(url).send().await.unwrap_err();
        assert!(err.is_timeout(), "{err}");
        Ok(())
    }
}
