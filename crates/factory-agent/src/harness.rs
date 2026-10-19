//! ACP harness runner and session manager.
//!
//! Follows `specs/acp-harness/spec.md`, `design/agent-run.md` §5.2, and ADR-0005.

use std::collections::HashMap;
use std::path::Path;

use agent_client_protocol::ConnectionTo;
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    ClientCapabilities, ContentBlock as AcpContentBlock, InitializeRequest, LoadSessionRequest,
    McpServer, McpServerStdio, NewSessionRequest, PermissionOptionKind, RequestPermissionOutcome,
    RequestPermissionRequest, RequestPermissionResponse, SelectedPermissionOutcome, SessionId,
    TextContent as AcpTextContent,
};
use df_proto::agent::v1::{Assign, Budgets, Continuation, HarnessCredentials, Prompt, RunOutcome};
use df_proto::transcript::v1::{
    Event, Notice, NoticeLevel, StopReason, TurnStarted, UserMessage,
    content_block::Kind as ContentBlockKind, event::Kind as EventKind,
};

/// Async I/O compatibility wrapper for Tokio AsyncRead into futures AsyncRead.
pub struct TokioAsyncReadCompat<T>(pub T);

impl<T: tokio::io::AsyncRead + Unpin> futures::AsyncRead for TokioAsyncReadCompat<T> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let mut read_buf = tokio::io::ReadBuf::new(buf);
        match std::pin::Pin::new(&mut self.0).poll_read(cx, &mut read_buf) {
            std::task::Poll::Ready(Ok(())) => std::task::Poll::Ready(Ok(read_buf.filled().len())),
            std::task::Poll::Ready(Err(e)) => std::task::Poll::Ready(Err(e)),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

/// Async I/O compatibility wrapper for Tokio AsyncWrite into futures AsyncWrite.
pub struct TokioAsyncWriteCompat<T>(pub T);

impl<T: tokio::io::AsyncWrite + Unpin> futures::AsyncWrite for TokioAsyncWriteCompat<T> {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_close(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.0).poll_shutdown(cx)
    }
}

/// Automatically approves permission requests from the ACP harness.
///
/// Selects the first `AllowOnce` or `AllowAlways` option, or the first available option.
pub fn approve_permission(req: &RequestPermissionRequest) -> RequestPermissionResponse {
    let chosen_id = req
        .options
        .iter()
        .find(|o| {
            matches!(
                o.kind,
                PermissionOptionKind::AllowOnce | PermissionOptionKind::AllowAlways
            )
        })
        .or_else(|| req.options.first())
        .map(|o| o.option_id.clone());

    if let Some(opt_id) = chosen_id {
        RequestPermissionResponse::new(RequestPermissionOutcome::Selected(
            SelectedPermissionOutcome::new(opt_id),
        ))
    } else {
        RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled)
    }
}

/// Prepares the harness environment and writes `/data/home` settings.
///
/// Secrets from `HarnessCredentials` are injected exclusively into the returned
/// environment map and are NEVER written to disk under `/data`.
pub fn prepare_harness_environment(
    assign: &Assign,
    credentials: Option<&HarnessCredentials>,
    home_dir: &Path,
) -> anyhow::Result<HashMap<String, String>> {
    let mut env = HashMap::new();

    // 1. Proxy environment passthrough
    for &key in crate::git::PROXY_ENV_VARS {
        if let Ok(val) = std::env::var(key) {
            env.insert(key.to_string(), val);
        }
    }

    // 2. Set HOME
    env.insert("HOME".to_string(), home_dir.to_string_lossy().to_string());

    // 3. Harness settings & system prompt
    if let Some(harness) = &assign.harness {
        let claude_dir = home_dir.join(".claude");

        if let Some(df_proto::agent::v1::harness::Kind::Acp(acp)) = &harness.kind {
            if !acp.settings_json.is_empty() {
                crate::fs_util::create_real_dir(&claude_dir)?;
                crate::fs_util::write_fresh(&claude_dir.join("settings.json"), &acp.settings_json)?;
            }
            for (k, v) in &acp.env {
                env.insert(k.clone(), v.clone());
            }
        }

        if !harness.system_prompt.is_empty() {
            crate::fs_util::create_real_dir(&claude_dir)?;
            crate::fs_util::write_fresh(&claude_dir.join("CLAUDE.md"), &harness.system_prompt)?;
        }
    }

    // 4. Inject credentials into environment ONLY (never write to disk)
    if let Some(creds) = credentials {
        for (k, v) in &creds.env {
            env.insert(k.clone(), v.clone());
        }
    }

    Ok(env)
}

/// Result of setting up an ACP session.
#[derive(Debug, Clone)]
pub struct SessionSetupResult {
    pub session_id: String,
    pub is_resumed: bool,
    pub notices: Vec<Notice>,
}

/// Initializes connection and establishes ACP session (`session/load` or `session/new`).
///
/// The factory MCP server is registered in every session.
pub async fn setup_session(
    cx: &ConnectionTo<agent_client_protocol::role::acp::Agent>,
    repo_dir: &Path,
    continuation: Option<&Continuation>,
    mcp_command: &Path,
    mcp_socket_path: &Path,
) -> Result<SessionSetupResult, agent_client_protocol::Error> {
    // 1. Initialize with empty client capabilities (no filesystem, no terminal)
    let _init_res = cx
        .send_request(
            InitializeRequest::new(ProtocolVersion::V1)
                .client_capabilities(ClientCapabilities::new()),
        )
        .block_task()
        .await?;

    // 2. Build factory MCP server definition
    let factory_mcp = McpServer::Stdio(
        McpServerStdio::new("factory", mcp_command.to_path_buf()).args(vec![
            "mcp".to_string(),
            "--socket".to_string(),
            mcp_socket_path.to_string_lossy().to_string(),
        ]),
    );

    let mut notices = Vec::new();

    // 3. Check continuation for session resumption
    if let Some(cont) = continuation
        && !cont.harness_session_id.is_empty()
    {
        let load_res = cx
            .send_request(
                LoadSessionRequest::new(
                    SessionId::new(cont.harness_session_id.clone()),
                    repo_dir.to_path_buf(),
                )
                .mcp_servers(vec![factory_mcp.clone()]),
            )
            .block_task()
            .await;

        match load_res {
            Ok(_) => {
                return Ok(SessionSetupResult {
                    session_id: cont.harness_session_id.clone(),
                    is_resumed: true,
                    notices,
                });
            }
            Err(e) => {
                notices.push(Notice {
                    level: NoticeLevel::Warning as i32,
                    code: "harness.session_resume_failed".to_string(),
                    message: format!("Failed to resume session {}: {e}", cont.harness_session_id),
                });
            }
        }
    }

    // New session (first run or fallback)
    let new_res = cx
        .send_request(NewSessionRequest::new(repo_dir.to_path_buf()).mcp_servers(vec![factory_mcp]))
        .block_task()
        .await?;

    Ok(SessionSetupResult {
        session_id: new_res.session_id.to_string(),
        is_resumed: false,
        notices,
    })
}

/// Builds ACP prompt content blocks from Dark Factory prompts.
pub fn build_acp_prompt_blocks(
    prompts: &[Prompt],
    prepend_summary: Option<&str>,
) -> Vec<AcpContentBlock> {
    let mut blocks = Vec::new();

    if let Some(sum) = prepend_summary
        && !sum.trim().is_empty()
    {
        blocks.push(AcpContentBlock::Text(AcpTextContent::new(format!(
            "Context from previous run:\n{sum}"
        ))));
    }

    for p in prompts {
        for b in &p.content {
            if let Some(ContentBlockKind::Text(t)) = &b.kind {
                blocks.push(AcpContentBlock::Text(AcpTextContent::new(&t.text)));
            }
        }
    }

    blocks
}

/// Queue and delivery manager for turn-based prompts.
///
/// Implements `specs/acp-harness/spec.md`:
/// "All prompts the runtime holds when a turn starts SHALL form that turn's input.
/// For each, a `UserMessage` with its prompt id SHALL be emitted at turn start.
/// Prompts arriving during a turn SHALL wait for the next turn."
#[derive(Debug, Default)]
pub struct PromptQueue {
    queued: Vec<Prompt>,
}

impl PromptQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues an incoming prompt. Duplicate control messages are filtered earlier, by
    /// `CommandFilter`; the queue does not deduplicate.
    pub fn push(&mut self, prompt: Prompt) {
        self.queued.push(prompt);
    }

    /// Returns true if there are queued prompts waiting for the next turn.
    pub fn has_queued(&self) -> bool {
        !self.queued.is_empty()
    }

    /// Number of queued prompts waiting for the next turn.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.queued.len()
    }

    /// True if queue is empty.
    pub fn is_empty(&self) -> bool {
        self.queued.is_empty()
    }

    /// Drains all prompts currently held to form the next turn's input.
    ///
    /// Returns the drained prompts and the `TurnStarted` + `UserMessage` transcript events.
    pub fn start_turn(&mut self, turn_id: &str) -> (Vec<Prompt>, Vec<Event>) {
        let prompts: Vec<Prompt> = self.queued.drain(..).collect();
        let prompt_ids: Vec<String> = prompts.iter().map(|p| p.prompt_id.clone()).collect();

        let now = chrono::Utc::now().into();
        let mut events = Vec::new();

        // 1. TurnStarted event
        events.push(Event {
            schema_version: 1,
            occurred_at: Some(now),
            turn_id: turn_id.to_string(),
            kind: Some(EventKind::TurnStarted(TurnStarted {
                prompt_ids: prompt_ids.clone(),
            })),
        });

        // 2. UserMessage event per consumed prompt
        for p in &prompts {
            events.push(Event {
                schema_version: 1,
                occurred_at: Some(now),
                turn_id: turn_id.to_string(),
                kind: Some(EventKind::UserMessage(UserMessage {
                    prompt_id: p.prompt_id.clone(),
                    author: p.author.clone(),
                    content: p.content.clone(),
                })),
            });
        }

        (prompts, events)
    }
}

/// Extracted rate limit details from an ACP adapter error or payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateLimitInfo {
    pub retry_after_seconds: Option<u64>,
    pub detail: String,
}

/// Extracts a numeric "retry after <seconds>" value from a string message.
pub fn extract_retry_after(text: &str) -> Option<u64> {
    let lower = text.to_ascii_lowercase();
    for pattern in [
        "retry after ",
        "retry-after: ",
        "retry-after ",
        "retry_after: ",
        "retry_after ",
    ] {
        if let Some(pos) = lower.find(pattern) {
            let remainder = &lower[pos + pattern.len()..];
            let num_str: String = remainder
                .trim_start()
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if let Ok(sec) = num_str.parse::<u64>() {
                return Some(sec);
            }
        }
    }
    None
}

/// Parses rate limit information from an ACP error if present.
pub fn parse_rate_limit(err: &agent_client_protocol::Error) -> Option<RateLimitInfo> {
    // 1. Check error data JSON payload
    if let Some(data) = err.data.as_ref() {
        let is_rate_limited = data
            .get("error")
            .and_then(|v| v.as_str())
            .map(|s| s == "rate_limited" || s.contains("rate_limit"))
            .unwrap_or(false);

        if is_rate_limited {
            let retry_sec = data
                .get("retry_after_seconds")
                .or_else(|| data.get("retry_after"))
                .and_then(|v| v.as_u64());
            let detail = data
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or(&err.message)
                .to_string();
            return Some(RateLimitInfo {
                retry_after_seconds: retry_sec,
                detail,
            });
        }
    }

    // 2. Check error message text
    let lower_msg = err.message.to_ascii_lowercase();
    // Last resort when the adapter gave no structured data. Phrases are anchored so that a mere
    // mention of "rate limit" (e.g. in a tool failure about documentation) doesn't match.
    let is_rate_limit = lower_msg.starts_with("rate limit")
        || lower_msg.starts_with("rate_limit")
        || lower_msg.contains("rate limit exceeded")
        || lower_msg.contains("rate limit reached")
        || lower_msg.contains("rate limited")
        || lower_msg.contains("rate_limited")
        || lower_msg.contains("too many requests")
        || lower_msg.contains("status 429")
        || lower_msg.contains("status: 429")
        || lower_msg.contains("status: \"429\"")
        || lower_msg.contains("code 429")
        || lower_msg.contains("code: 429")
        || lower_msg.contains("code: \"429\"")
        || lower_msg.contains("http 429")
        || lower_msg.contains("http/1.1 429")
        || lower_msg.contains("http/2 429")
        || lower_msg.contains("\"status\": 429")
        || lower_msg.contains("\"code\": 429");

    if is_rate_limit {
        let retry_sec = extract_retry_after(&err.message);
        return Some(RateLimitInfo {
            retry_after_seconds: retry_sec,
            detail: err.message.clone(),
        });
    }

    None
}

/// Tracks budget constraints (wall clock, output tokens, turns) for an active run.
#[derive(Debug, Clone)]
pub struct BudgetTracker {
    start_time: std::time::Instant,
    max_wall_clock: Option<std::time::Duration>,
    max_output_tokens: Option<u64>,
    max_turns: Option<u32>,
    turns_completed: u32,
    total_output_tokens: u64,
}

impl BudgetTracker {
    pub fn new(budgets: Option<&Budgets>) -> Self {
        let (max_wall_clock, max_output_tokens, max_turns) = if let Some(b) = budgets {
            let wall_clock = b.max_wall_clock.as_ref().map(|d| {
                std::time::Duration::from_secs(d.seconds as u64)
                    + std::time::Duration::from_nanos(d.nanos as u64)
            });
            (wall_clock, b.max_output_tokens, b.max_turns)
        } else {
            (None, None, None)
        };

        Self {
            start_time: std::time::Instant::now(),
            max_wall_clock,
            max_output_tokens,
            max_turns,
            turns_completed: 0,
            total_output_tokens: 0,
        }
    }

    /// True if wall clock duration has exceeded `Assign.budgets.max_wall_clock`.
    pub fn is_wall_clock_exceeded(&self) -> bool {
        if let Some(limit) = self.max_wall_clock {
            self.start_time.elapsed() >= limit
        } else {
            false
        }
    }

    /// Remaining duration before wall clock budget expires.
    pub fn remaining_wall_clock(&self) -> Option<std::time::Duration> {
        self.max_wall_clock.map(|limit| {
            let elapsed = self.start_time.elapsed();
            if elapsed >= limit {
                std::time::Duration::ZERO
            } else {
                limit - elapsed
            }
        })
    }

    /// Records completed turn with its consumed output tokens.
    pub fn record_turn(&mut self, output_tokens: u64) {
        self.turns_completed += 1;
        self.total_output_tokens += output_tokens;
    }

    pub fn turns_completed(&self) -> u32 {
        self.turns_completed
    }

    pub fn total_output_tokens(&self) -> u64 {
        self.total_output_tokens
    }

    /// True if cumulative output tokens exceed `Assign.budgets.max_output_tokens`.
    pub fn is_output_tokens_exceeded(&self) -> bool {
        if let Some(limit) = self.max_output_tokens {
            self.total_output_tokens >= limit
        } else {
            false
        }
    }

    /// True if `turns_completed` reached or exceeded `Assign.budgets.max_turns`.
    pub fn is_turn_limit_reached(&self) -> bool {
        if let Some(limit) = self.max_turns {
            self.turns_completed >= limit
        } else {
            false
        }
    }
}

/// Decision whether to continue the run with a new turn or finalize with an outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunOutcomeDecision {
    ContinueNextTurn,
    Finalize {
        outcome: RunOutcome,
        detail: String,
        retry_after: Option<std::time::Duration>,
    },
}

/// Inputs considered when determining the outcome after a turn.
#[derive(Debug, Clone, Default)]
pub struct TurnOutcomeInput {
    pub cancelled: bool,
    pub end_run_after_turn: bool,
    pub rate_limited: Option<RateLimitInfo>,
    pub wall_clock_exceeded: bool,
    pub output_tokens_exceeded: bool,
    pub stop_reason: Option<StopReason>,
    pub harness_crashed: bool,
    pub has_queued_prompts: bool,
    pub turn_limit_reached: bool,
}

/// Determines the run outcome after a turn per `specs/acp-harness/spec.md`:
///
/// "After each turn the runtime SHALL continue with queued prompts if any;
/// otherwise it SHALL finalize with outcome:
/// `COMPLETED` if a tool result requested ending the run;
/// `RATE_LIMITED` (with retry time if known) if the provider rate-limited;
/// `LIMIT_REACHED` if a budget or max-token/turn stop reason was hit;
/// `CANCELLED` on `Cancel`;
/// `FAILED` if the harness crashed;
/// `END_TURN` otherwise."
pub fn determine_run_outcome(input: &TurnOutcomeInput) -> RunOutcomeDecision {
    // 1. Cancellation from control plane
    if input.cancelled {
        return RunOutcomeDecision::Finalize {
            outcome: RunOutcome::Cancelled,
            detail: "Run cancelled by control plane".to_string(),
            retry_after: None,
        };
    }

    // 2. complete_task called during turn
    // "WHEN a ToolCallResult with end_run_after_turn arrived during the turn
    //  THEN after the turn the run finalizes with COMPLETED, even if prompts are queued"
    if input.end_run_after_turn {
        return RunOutcomeDecision::Finalize {
            outcome: RunOutcome::Completed,
            detail: "Task completed".to_string(),
            retry_after: None,
        };
    }

    // 3. Provider rate limit
    // "WHEN the harness reports a provider rate limit
    //  THEN TurnEnded.stop_reason is RATE_LIMITED and the run finalizes with RUN_OUTCOME_RATE_LIMITED"
    if let Some(rl) = &input.rate_limited {
        let retry_dur = rl.retry_after_seconds.map(std::time::Duration::from_secs);
        return RunOutcomeDecision::Finalize {
            outcome: RunOutcome::RateLimited,
            detail: rl.detail.clone(),
            retry_after: retry_dur,
        };
    }

    // 4. Budgets: wall clock or output tokens or StopReason::MaxTokens / MaxTurnRequests
    if input.wall_clock_exceeded {
        return RunOutcomeDecision::Finalize {
            outcome: RunOutcome::LimitReached,
            detail: "Wall clock budget exceeded".to_string(),
            retry_after: None,
        };
    }

    if input.output_tokens_exceeded {
        return RunOutcomeDecision::Finalize {
            outcome: RunOutcome::LimitReached,
            detail: "Output token budget exceeded".to_string(),
            retry_after: None,
        };
    }

    if matches!(
        input.stop_reason,
        Some(StopReason::MaxTokens) | Some(StopReason::MaxTurnRequests)
    ) {
        return RunOutcomeDecision::Finalize {
            outcome: RunOutcome::LimitReached,
            detail: "Stop reason limit reached".to_string(),
            retry_after: None,
        };
    }

    // 5. Harness crashed / error
    if input.harness_crashed {
        return RunOutcomeDecision::Finalize {
            outcome: RunOutcome::Failed,
            detail: "Harness crashed or exited with error".to_string(),
            retry_after: None,
        };
    }

    // 6. Next turn or end of run
    if input.has_queued_prompts {
        // "WHEN max_turns is 3 and a fourth turn would start
        //  THEN the run finalizes with LIMIT_REACHED instead"
        if input.turn_limit_reached {
            return RunOutcomeDecision::Finalize {
                outcome: RunOutcome::LimitReached,
                detail: "Turn budget reached".to_string(),
                retry_after: None,
            };
        }
        RunOutcomeDecision::ContinueNextTurn
    } else {
        // "WHEN the turn ends normally and no prompts are queued
        //  THEN the run finalizes with END_TURN"
        RunOutcomeDecision::Finalize {
            outcome: RunOutcome::EndTurn,
            detail: "Turn ended normally with no queued prompts".to_string(),
            retry_after: None,
        }
    }
}
