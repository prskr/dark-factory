//! Core run driver and execution engine.
//!
//! Follows `specs/acp-harness/spec.md`, `specs/workspace-lifecycle/spec.md`,
//! and `design/agent-run.md` §6.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use agent_client_protocol::schema::v1::{
    CancelNotification, PromptRequest, RequestPermissionRequest, RequestPermissionResponse,
    SessionId, SessionNotification, SessionUpdate,
};
use agent_client_protocol::{ByteStreams, Client, ConnectionTo, Responder};
use df_proto::agent::v1::{
    AgentMessage, Assign, ControlMessage, CredentialGrant, CredentialRequest, GitCredentialRequest,
    HarnessCredentials, Prompt, PushReport, RunOutcome, RunReport, ToolCallRequest, ToolCallResult,
    ToolDescriptor, agent_message, control_message, credential_grant, credential_request,
};
use df_proto::transcript::v1::{Notice, NoticeLevel, StopReason, UsageSource};
use tokio::sync::{Mutex, broadcast, mpsc, oneshot};
use uuid::Uuid;

use crate::connection::CREDENTIAL_REQUEST_TIMEOUT;
use crate::git::Git;
use crate::harness::{
    BudgetTracker, PromptQueue, RunOutcomeDecision, TokioAsyncReadCompat, TokioAsyncWriteCompat,
    TurnOutcomeInput, approve_permission, build_acp_prompt_blocks, determine_run_outcome,
    parse_rate_limit, prepare_harness_environment, setup_session,
};
use crate::outbox::Outbox;
use crate::runtime_socket::{RuntimeHandler, RuntimeSocketServer};
use crate::transcript::{TranscriptTranslator, acp_to_proto_stop_reason, notice_event};
use crate::usage::SessionLogTracker;
use crate::workspace::{FinalizeParams, finalize_workspace, push_after_turn};

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
    let _ = outbound_tx
        .send(AgentMessage {
            seq: 0,
            body: Some(body),
        })
        .await;
}

/// Helper to emit a notice transcript event to outbound channel (Finding 28).
async fn emit_notice(outbound_tx: &mpsc::Sender<AgentMessage>, notice: Notice) {
    send_body(
        outbound_tx,
        agent_message::Body::TranscriptEvent(notice_event(notice)),
    )
    .await;
}

/// Checks if a credential request target matches the assigned workspace repository URL (Finding 4).
pub fn matches_repo_url(repo_url: &str, protocol: &str, host: &str, path: &str) -> bool {
    let clean_req_path = path
        .trim_matches('/')
        .strip_suffix(".git")
        .unwrap_or(path.trim_matches('/'));

    if let Some((url_proto, rest)) = repo_url.split_once("://") {
        if !url_proto.eq_ignore_ascii_case(protocol) {
            return false;
        }
        if url_proto.eq_ignore_ascii_case("file") {
            let url_path = rest
                .trim_matches('/')
                .strip_suffix(".git")
                .unwrap_or(rest.trim_matches('/'));
            return url_path == clean_req_path;
        }
        let (url_host, url_path) = match rest.split_once('/') {
            Some((h, p)) => (h, p),
            None => (rest, ""),
        };
        // Drop `user[:password]@` from the authority; git sends only the host (and port)
        let url_host = url_host.rsplit_once('@').map_or(url_host, |(_, h)| h);
        if !url_host.eq_ignore_ascii_case(host) {
            return false;
        }
        let clean_url_path = url_path
            .trim_matches('/')
            .strip_suffix(".git")
            .unwrap_or(url_path.trim_matches('/'));
        // Git sends the path because the runtime sets `credential.useHttpPath`; a request
        // without one can't be tied to the assigned repository.
        clean_url_path == clean_req_path
    } else if protocol.eq_ignore_ascii_case("file") {
        let clean_url_path = repo_url
            .trim_matches('/')
            .strip_suffix(".git")
            .unwrap_or(repo_url.trim_matches('/'));
        clean_url_path == clean_req_path
    } else {
        false
    }
}

/// Runtime handler backing `/run/factory/runtime.sock` for MCP and Git credentials.
pub struct DriverRuntimeHandler {
    repo_url: String,
    tools: Vec<ToolDescriptor>,
    outbound_tx: mpsc::Sender<AgentMessage>,
    pending_tool_calls: Arc<Mutex<HashMap<String, oneshot::Sender<ToolCallResult>>>>,
    pending_cred_requests: Arc<Mutex<HashMap<String, oneshot::Sender<CredentialGrant>>>>,
    git_credentials: Arc<Mutex<Option<(String, String)>>>,
    /// Unix time (seconds) at which the cached credentials expire, if the grant said so.
    git_credentials_expiry: Mutex<Option<i64>>,
}

impl DriverRuntimeHandler {
    pub fn new(
        repo_url: String,
        tools: Vec<ToolDescriptor>,
        outbound_tx: mpsc::Sender<AgentMessage>,
        pending_tool_calls: Arc<Mutex<HashMap<String, oneshot::Sender<ToolCallResult>>>>,
        pending_cred_requests: Arc<Mutex<HashMap<String, oneshot::Sender<CredentialGrant>>>>,
        git_credentials: Arc<Mutex<Option<(String, String)>>>,
    ) -> Self {
        Self {
            repo_url,
            tools,
            outbound_tx,
            pending_tool_calls,
            pending_cred_requests,
            git_credentials,
            git_credentials_expiry: Mutex::new(None),
        }
    }
}

#[tonic::async_trait]
impl RuntimeHandler for DriverRuntimeHandler {
    async fn handle_git_credential(
        &self,
        protocol: &str,
        host: &str,
        path: &str,
    ) -> (Option<String>, Option<String>, Option<String>) {
        // Enforce repository URL match to prevent token leakage (Finding 4)
        if !matches_repo_url(&self.repo_url, protocol, host, path) {
            return (
                None,
                None,
                Some(format!(
                    "Credential request denied: target '{protocol}://{host}/{path}' does not match repository '{}'",
                    self.repo_url
                )),
            );
        }

        let expired =
            self.git_credentials_expiry.lock().await.is_some_and(|at| {
                at <= chrono::Utc::now().timestamp() + CREDENTIAL_EXPIRY_MARGIN_SECS
            });
        if expired {
            *self.git_credentials.lock().await = None;
        } else if let Some((u, p)) = self.git_credentials.lock().await.as_ref() {
            return (Some(u.clone()), Some(p.clone()), None);
        }

        // Request credentials from control plane (Finding 4)
        let request_id = Uuid::now_v7().to_string();
        let (tx, rx) = oneshot::channel();
        self.pending_cred_requests
            .lock()
            .await
            .insert(request_id.clone(), tx);

        let req = CredentialRequest {
            request_id: request_id.clone(),
            kind: Some(credential_request::Kind::Git(GitCredentialRequest {
                protocol: protocol.to_string(),
                host: host.to_string(),
                path: path.to_string(),
            })),
        };

        let msg = AgentMessage {
            seq: 0,
            body: Some(agent_message::Body::CredentialRequest(req)),
        };

        if self.outbound_tx.send(msg).await.is_err() {
            self.pending_cred_requests.lock().await.remove(&request_id);
            return (None, None, Some("outbound channel closed".to_string()));
        }

        match tokio::time::timeout(CREDENTIAL_REQUEST_TIMEOUT, rx).await {
            Ok(Ok(grant)) => match grant.result {
                Some(credential_grant::Result::Git(cred)) => {
                    *self.git_credentials.lock().await =
                        Some((cred.username.clone(), cred.password.clone()));
                    *self.git_credentials_expiry.lock().await =
                        cred.expires_at.as_ref().map(|t| t.seconds);
                    (Some(cred.username), Some(cred.password), None)
                }
                Some(credential_grant::Result::DeniedReason(reason)) => (
                    None,
                    None,
                    Some(format!("Credential request denied: {reason}")),
                ),
                None => (
                    None,
                    None,
                    Some("Empty credential grant received".to_string()),
                ),
            },
            Ok(Err(_)) => (
                None,
                None,
                Some("Credential request channel closed".to_string()),
            ),
            Err(_) => {
                self.pending_cred_requests.lock().await.remove(&request_id);
                (None, None, Some("Credential request timed out".to_string()))
            }
        }
    }

    async fn handle_git_credential_erase(&self, protocol: &str, host: &str, path: &str) {
        // Git rejected the cached credential (for example an expired token): ask again next time
        if matches_repo_url(&self.repo_url, protocol, host, path) {
            *self.git_credentials.lock().await = None;
            *self.git_credentials_expiry.lock().await = None;
        }
    }

    async fn handle_mcp_list_tools(&self) -> Vec<ToolDescriptor> {
        self.tools.clone()
    }

    async fn handle_mcp_call_tool(
        &self,
        name: &str,
        arguments_json: &str,
    ) -> Result<ToolCallResult, String> {
        let call_id = Uuid::now_v7().to_string();
        let (tx, rx) = oneshot::channel();

        self.pending_tool_calls
            .lock()
            .await
            .insert(call_id.clone(), tx);

        let req = ToolCallRequest {
            call_id: call_id.clone(),
            tool_name: name.to_string(),
            arguments_json: arguments_json.to_string(),
        };

        let msg = AgentMessage {
            seq: 0,
            body: Some(agent_message::Body::ToolCallRequest(req)),
        };

        if self.outbound_tx.send(msg).await.is_err() {
            self.pending_tool_calls.lock().await.remove(&call_id);
            return Err("outbound channel closed".to_string());
        }

        // `end_run_after_turn` is recorded by the inbound loop before the result is delivered here
        match tokio::time::timeout(TOOL_CALL_TIMEOUT, rx).await {
            Ok(result) => result.map_err(|_| "tool call was dropped or aborted".to_string()),
            Err(_) => {
                self.pending_tool_calls.lock().await.remove(&call_id);
                Err(format!("tool call timed out after {TOOL_CALL_TIMEOUT:?}"))
            }
        }
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
    pub git_credentials: Option<(String, String)>,
}

/// Result of completing an agent run.
#[derive(Debug, Clone)]
pub struct RunExecutionResult {
    pub outcome: RunOutcome,
    pub run_report: RunReport,
    pub push_reports: Vec<PushReport>,
    pub notices: Vec<Notice>,
}

/// Cached credentials are refreshed this long before their stated expiry.
const CREDENTIAL_EXPIRY_MARGIN_SECS: i64 = 30;
/// Upper bound for a factory tool call to be answered by the control plane.
const TOOL_CALL_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// How long the harness gets to exit on its own once the ACP connection is closed.
const HARNESS_EXIT_GRACE: Duration = Duration::from_secs(5);
/// How long to wait for the harness to answer a prompt after it was sent a `session/cancel`.
const CANCEL_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

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

/// Runs the complete agent run lifecycle.
pub async fn run_session(params: RunDriverParams) -> anyhow::Result<RunExecutionResult> {
    let mut collected_notices = Vec::new();
    let mut collected_push_reports = Vec::new();

    let credential_helper_cmd = format!("{} git-credential", params.mcp_command.to_string_lossy());
    let git = params
        .git
        .clone()
        .with_credential_helper(&credential_helper_cmd)
        .with_runtime_sock(&params.runtime_sock_path);

    // Shared state across driver and runtime socket server
    let pending_tool_calls: Arc<Mutex<HashMap<String, oneshot::Sender<ToolCallResult>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let pending_cred_requests: Arc<Mutex<HashMap<String, oneshot::Sender<CredentialGrant>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let end_run_requested = Arc::new(AtomicBool::new(false));
    let cancelled = Arc::new(AtomicBool::new(params.cancel_requested));
    let (cancel_broadcast_tx, cancel_broadcast_rx) = broadcast::channel::<String>(16);

    let prompt_queue = Arc::new(Mutex::new(PromptQueue::new()));
    for p in params.initial_prompts {
        prompt_queue.lock().await.push(p);
    }

    let git_creds = Arc::new(Mutex::new(params.git_credentials));

    // 1. Prepare workspace
    let workspace = match params.assign.workspace.clone() {
        Some(w) => w,
        None => {
            return Ok(fail_run(
                &params.outbound_tx,
                "missing workspace in Assign".to_string(),
                String::new(),
                collected_push_reports,
                collected_notices,
            )
            .await);
        }
    };

    // Start runtime socket server with repository-scoped credential protection (Finding 4)
    let handler = Arc::new(DriverRuntimeHandler::new(
        workspace.repo_url.clone(),
        params.assign.tools.clone(),
        params.outbound_tx.clone(),
        Arc::clone(&pending_tool_calls),
        Arc::clone(&pending_cred_requests),
        Arc::clone(&git_creds),
    ));

    let socket_server = match RuntimeSocketServer::bind(&params.runtime_sock_path, handler) {
        Ok(s) => s,
        Err(e) => {
            return Ok(fail_run(
                &params.outbound_tx,
                format!("Failed to bind runtime socket: {e}"),
                String::new(),
                collected_push_reports,
                collected_notices,
            )
            .await);
        }
    };
    let socket_handle = socket_server.spawn();
    let _socket_guard = AbortOnDrop::new(socket_handle);

    // 2. Inbound message loop task. It runs from here on so that credential grants requested by
    // git during the workspace prepare (clone, fetch, push) are delivered.
    let pending_calls_inbound = Arc::clone(&pending_tool_calls);
    let pending_cred_inbound = Arc::clone(&pending_cred_requests);
    let prompt_queue_inbound = Arc::clone(&prompt_queue);
    let cancelled_inbound = Arc::clone(&cancelled);
    let end_run_inbound = Arc::clone(&end_run_requested);
    let outbound_tx_inbound = params.outbound_tx.clone();
    let mut inbound_rx = params.inbound_rx;

    let inbound_task = tokio::spawn(async move {
        while let Some(msg) = inbound_rx.recv().await {
            match msg.body {
                Some(control_message::Body::Prompt(p)) => {
                    prompt_queue_inbound.lock().await.push(p);
                }
                Some(control_message::Body::Cancel(c)) => {
                    cancelled_inbound.store(true, Ordering::SeqCst);
                    let _ = cancel_broadcast_tx.send(c.reason);
                }
                Some(control_message::Body::ToolCallResult(res)) => {
                    if res.end_run_after_turn {
                        end_run_inbound.store(true, Ordering::SeqCst);
                    }
                    if let Some(tx) = pending_calls_inbound.lock().await.remove(&res.call_id) {
                        let _ = tx.send(res);
                    }
                }
                Some(control_message::Body::CredentialGrant(grant)) => {
                    if let Some(tx) = pending_cred_inbound.lock().await.remove(&grant.request_id) {
                        let _ = tx.send(grant);
                    }
                }
                // HarnessCredentials must precede Assign; the harness environment is already built
                Some(control_message::Body::HarnessCredentials(_)) => {
                    emit_notice(
                        &outbound_tx_inbound,
                        Notice {
                            level: NoticeLevel::Warning as i32,
                            code: "runtime.harness_credentials_late".to_string(),
                            message: "HarnessCredentials arrived after Assign and were ignored"
                                .to_string(),
                        },
                    )
                    .await;
                }
                _ => {}
            }
        }
    });
    let _inbound_guard = AbortOnDrop::new(inbound_task);

    // A cancel that arrived before Assign: don't clone, push or start the harness
    if cancelled.load(Ordering::SeqCst) {
        return Ok(end_run_early(
            &params.outbound_tx,
            RunOutcome::Cancelled,
            "Run cancelled by control plane before the workspace was prepared".to_string(),
            String::new(),
            collected_push_reports,
            collected_notices,
        )
        .await);
    }

    let prepare_res = match crate::workspace::prepare_workspace(
        &git,
        &params.repo_dir,
        &workspace,
        &params.hooks_path,
        &credential_helper_cmd,
    )
    .await
    {
        Ok(res) => res,
        Err(e) => {
            return Ok(fail_run(
                &params.outbound_tx,
                format!("Workspace preparation failed: {e}"),
                String::new(),
                collected_push_reports,
                collected_notices,
            )
            .await);
        }
    };

    collected_notices.extend(prepare_res.notices.clone());
    for n in &prepare_res.notices {
        emit_notice(&params.outbound_tx, n.clone()).await;
    }

    if let Some(pr) = prepare_res.push_report.clone() {
        collected_push_reports.push(pr.clone());
        send_body(&params.outbound_tx, agent_message::Body::PushReport(pr)).await;
    }

    // 3. Prepare harness environment (credentials injected in-memory only)
    let mut harness_env = match prepare_harness_environment(
        &params.assign,
        params.credentials.as_ref(),
        &params.home_dir,
    ) {
        Ok(env) => env,
        Err(e) => {
            return Ok(fail_run(
                &params.outbound_tx,
                format!("Harness environment preparation failed: {e}"),
                String::new(),
                collected_push_reports,
                collected_notices,
            )
            .await);
        }
    };

    harness_env.insert(
        "DF_RUNTIME_SOCK".to_string(),
        params.runtime_sock_path.to_string_lossy().to_string(),
    );
    harness_env.insert(
        "DF_RUN_DIR".to_string(),
        params.runtime_dir.to_string_lossy().to_string(),
    );

    // 4. Spawn harness process
    let (harness_cmd, harness_args) =
        match params.assign.harness.as_ref().and_then(|h| match &h.kind {
            Some(df_proto::agent::v1::harness::Kind::Acp(acp)) => acp.command.split_first(),
            _ => None,
        }) {
            Some((cmd, args)) => (cmd.clone(), args.to_vec()),
            None => ("fake-acp-agent".to_string(), Vec::new()),
        };

    let mut cmd = tokio::process::Command::new(&harness_cmd);
    cmd.args(&harness_args)
        .envs(&harness_env)
        .current_dir(&params.repo_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return Ok(fail_run(
                &params.outbound_tx,
                format!("Failed to spawn harness process '{harness_cmd}': {e}"),
                String::new(),
                collected_push_reports,
                collected_notices,
            )
            .await);
        }
    };
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill().await;
        return Ok(fail_run(
            &params.outbound_tx,
            "Failed to capture harness stdio".to_string(),
            String::new(),
            collected_push_reports,
            collected_notices,
        )
        .await);
    };

    let transport = ByteStreams::new(TokioAsyncWriteCompat(stdin), TokioAsyncReadCompat(stdout));

    // 5. Connect ACP Client and run session turns
    let outbound_tx_acp = params.outbound_tx.clone();
    let translator_mutex = Arc::new(Mutex::new(TranscriptTranslator::new("")));
    let translator_notif = Arc::clone(&translator_mutex);

    let client = Client
        .builder()
        .on_receive_notification(
            async move |notification: SessionNotification,
                        _cx: ConnectionTo<agent_client_protocol::role::acp::Agent>| {
                // `UsageUpdate` is context-window occupancy, not per-turn tokens, so it is ignored;
                // per-turn usage comes from the session log.
                let event = match notification.update {
                    SessionUpdate::AgentMessageChunk(chunk) => Some(
                        translator_notif
                            .lock()
                            .await
                            .translate_agent_message_chunk(&chunk),
                    ),
                    SessionUpdate::AgentThoughtChunk(chunk) => Some(
                        translator_notif
                            .lock()
                            .await
                            .translate_agent_thought_chunk(&chunk),
                    ),
                    SessionUpdate::ToolCall(call) => {
                        Some(translator_notif.lock().await.translate_tool_call(&call))
                    }
                    SessionUpdate::ToolCallUpdate(update) => Some(
                        translator_notif
                            .lock()
                            .await
                            .translate_tool_call_update(&update),
                    ),
                    SessionUpdate::Plan(plan) => {
                        Some(translator_notif.lock().await.translate_plan(&plan))
                    }
                    _ => None,
                };

                if let Some(ev) = event {
                    send_body(&outbound_tx_acp, agent_message::Body::TranscriptEvent(ev)).await;
                }
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |req: RequestPermissionRequest,
                        responder: Responder<RequestPermissionResponse>,
                        _cx: ConnectionTo<agent_client_protocol::role::acp::Agent>| {
                let response = approve_permission(&req);
                responder.respond(response)
            },
            agent_client_protocol::on_receive_request!(),
        );

    let repo_dir = params.repo_dir.clone();
    let home_dir = params.home_dir.clone();
    let assign = params.assign.clone();
    let mcp_command = params.mcp_command.clone();
    let runtime_sock_path = params.runtime_sock_path.clone();
    let outbound_tx_turn = params.outbound_tx.clone();
    let git_for_turn = git.clone();
    let workspace_for_turn = workspace.clone();

    let outbox_turn = Arc::clone(&params.outbox);
    let prepare_diverged = prepare_res.diverged;

    // Commits up to this SHA are known to be on the remote. After a failed prepare push or on a
    // diverged branch HEAD isn't, so nothing is assumed to be pushed.
    let initial_last_pushed_sha = if prepare_res.push_failed || prepare_res.diverged {
        None
    } else {
        let ob = params.outbox.lock().await;
        ob.run_state()
            .last_pushed_sha
            .clone()
            .or_else(|| Some(prepare_res.head_sha.clone()))
    };
    let _ = params
        .outbox
        .lock()
        .await
        .set_last_pushed_sha(initial_last_pushed_sha.clone());
    let loop_initial_last_pushed_sha = initial_last_pushed_sha;

    let mut cancel_rx = Some(cancel_broadcast_rx);
    // Shared with the turn loop so reports of completed turns survive a driver error
    let shared_push_reports: Arc<Mutex<Vec<PushReport>>> = Arc::new(Mutex::new(Vec::new()));
    let loop_push_reports = Arc::clone(&shared_push_reports);

    struct TurnLoopOutcome {
        session_id: String,
        last_pushed_sha: Option<String>,
        outcome: RunOutcome,
        detail: String,
        retry_after: Option<Duration>,
        notices: Vec<Notice>,
    }

    let run_driver_result: Result<TurnLoopOutcome, _> = client
        .connect_with(transport, async move |cx| {
            // Setup ACP session
            let setup_res = setup_session(
                &cx,
                &repo_dir,
                assign.continuation.as_ref(),
                &mcp_command,
                &runtime_sock_path,
            )
            .await?;

            let final_session_id = setup_res.session_id.clone();
            let loop_notices = setup_res.notices.clone();
            for n in &setup_res.notices {
                emit_notice(&outbound_tx_turn, n.clone()).await;
            }

            // Persist harness session id in outbox (Finding 22)
            let _ = outbox_turn
                .lock()
                .await
                .set_harness_session_id(Some(final_session_id.clone()));

            let mut budget_tracker = BudgetTracker::new(assign.budgets.as_ref());
            let mut session_log_tracker =
                SessionLogTracker::new(&home_dir, &repo_dir, &final_session_id);
            let mut turn_index = 0u32;
            let mut last_pushed_sha = loop_initial_last_pushed_sha;

            let final_outcome;
            let final_detail;
            let final_retry_after;

            loop {
                // Pre-turn budget checks
                if budget_tracker.is_wall_clock_exceeded() {
                    final_outcome = RunOutcome::LimitReached;
                    final_detail = "Wall clock budget exceeded".to_string();
                    final_retry_after = None;
                    break;
                }

                if budget_tracker.is_turn_limit_reached() {
                    final_outcome = RunOutcome::LimitReached;
                    final_detail = "Turn budget reached".to_string();
                    final_retry_after = None;
                    break;
                }

                if cancelled.load(Ordering::SeqCst) {
                    final_outcome = RunOutcome::Cancelled;
                    final_detail = "Run cancelled by control plane".to_string();
                    final_retry_after = None;
                    break;
                }

                if prompt_queue.lock().await.is_empty() {
                    final_outcome = RunOutcome::EndTurn;
                    final_detail = "Turn ended normally with no queued prompts".to_string();
                    final_retry_after = None;
                    break;
                }

                turn_index += 1;
                let turn_id = format!("turn_{}", turn_index);
                translator_mutex.lock().await.set_turn_id(&turn_id);

                let (prompts_for_turn, start_events) =
                    prompt_queue.lock().await.start_turn(&turn_id);

                for ev in start_events {
                    send_body(&outbound_tx_turn, agent_message::Body::TranscriptEvent(ev)).await;
                }

                let prepend_summary = if turn_index == 1 && !setup_res.is_resumed {
                    assign.continuation.as_ref().map(|c| c.summary.as_str())
                } else {
                    None
                };

                let prompt_blocks = build_acp_prompt_blocks(&prompts_for_turn, prepend_summary);

                session_log_tracker.mark_turn_start();

                // Send PromptRequest to ACP adapter
                let prompt_req =
                    PromptRequest::new(SessionId::new(&*final_session_id), prompt_blocks);
                let prompt_fut = cx.send_request(prompt_req).block_task();
                tokio::pin!(prompt_fut);

                let remaining_wall_clock = budget_tracker
                    .remaining_wall_clock()
                    .unwrap_or(Duration::from_secs(86400 * 365));
                let deadline = tokio::time::sleep(remaining_wall_clock);
                tokio::pin!(deadline);

                // Wait for the prompt response, a cancel, or the wall-clock deadline. A closed
                // cancel channel only stops the cancel branch; the deadline stays active.
                let mut wall_clock_exceeded = false;
                let mut needs_cancel = false;
                let mut prompt_res = None;
                while prompt_res.is_none() && !needs_cancel {
                    tokio::select! {
                        res = &mut prompt_fut => {
                            prompt_res = Some(res);
                        }
                        recv_res = recv_cancel(&mut cancel_rx) => match recv_res {
                            Ok(_reason) => {
                                cancelled.store(true, Ordering::SeqCst);
                                needs_cancel = true;
                            }
                            Err(broadcast::error::RecvError::Closed) => {
                                cancel_rx = None;
                            }
                            Err(broadcast::error::RecvError::Lagged(_)) => {
                                needs_cancel = cancelled.load(Ordering::SeqCst);
                            }
                        },
                        _ = &mut deadline => {
                            wall_clock_exceeded = true;
                            needs_cancel = true;
                        }
                    }
                }

                let prompt_res = match prompt_res {
                    Some(res) => res,
                    None => {
                        let _ = cx.send_notification(CancelNotification::new(SessionId::new(
                            &*final_session_id,
                        )));
                        match tokio::time::timeout(CANCEL_RESPONSE_TIMEOUT, &mut prompt_fut).await {
                            Ok(res) => res,
                            Err(_) => {
                                let mut err = agent_client_protocol::Error::internal_error();
                                err.message =
                                    "timed out waiting for prompt response after cancel".into();
                                Err(err)
                            }
                        }
                    }
                };

                let mut rate_limited = None;
                let mut harness_crashed = false;
                let mut stop_reason = None;
                let mut turn_output_tokens = 0u64;

                match prompt_res {
                    Ok(resp) => {
                        let proto_stop_reason = acp_to_proto_stop_reason(resp.stop_reason);
                        stop_reason = Some(proto_stop_reason);

                        // Ordering: the ACP dispatch loop runs each notification handler to completion before it
                        // routes the next message, so every chunk of this turn was already queued to
                        // `outbound_tx` before `prompt_res` resolved. TurnEnded cannot overtake them.
                        // Capture usage
                        let (usage, usage_source) = session_log_tracker.parse_turn_usage();

                        for u in &usage {
                            turn_output_tokens += u.output_tokens;
                        }
                        budget_tracker.record_turn(turn_output_tokens);

                        let turn_ended_event = translator_mutex
                            .lock()
                            .await
                            .translate_turn_ended_with_stop_reason(
                                proto_stop_reason,
                                usage,
                                usage_source,
                                "",
                            );

                        send_body(
                            &outbound_tx_turn,
                            agent_message::Body::TranscriptEvent(turn_ended_event),
                        )
                        .await;
                    }
                    Err(e) => {
                        if let Some(rl) = parse_rate_limit(&e) {
                            rate_limited = Some(rl.clone());
                            let turn_ended_event = translator_mutex
                                .lock()
                                .await
                                .translate_turn_ended_with_stop_reason(
                                    StopReason::RateLimited,
                                    Vec::new(),
                                    UsageSource::Unspecified,
                                    &rl.detail,
                                );
                            send_body(
                                &outbound_tx_turn,
                                agent_message::Body::TranscriptEvent(turn_ended_event),
                            )
                            .await;
                        } else {
                            harness_crashed = true;
                            let turn_ended_event = translator_mutex
                                .lock()
                                .await
                                .translate_turn_ended_with_stop_reason(
                                    StopReason::Error,
                                    Vec::new(),
                                    UsageSource::Unspecified,
                                    &e.message,
                                );
                            send_body(
                                &outbound_tx_turn,
                                agent_message::Body::TranscriptEvent(turn_ended_event),
                            )
                            .await;
                        }
                    }
                }

                // Push task branch after turn if commits were created
                if let Ok(Some(pr)) =
                    push_after_turn(&git_for_turn, &workspace_for_turn, &mut last_pushed_sha).await
                {
                    let _ = outbox_turn
                        .lock()
                        .await
                        .set_last_pushed_sha(last_pushed_sha.clone());
                    loop_push_reports.lock().await.push(pr.clone());
                    send_body(&outbound_tx_turn, agent_message::Body::PushReport(pr)).await;
                }

                // Evaluate outcome after turn
                let outcome_input = TurnOutcomeInput {
                    cancelled: cancelled.load(Ordering::SeqCst),
                    end_run_after_turn: end_run_requested.load(Ordering::SeqCst),
                    rate_limited,
                    wall_clock_exceeded,
                    output_tokens_exceeded: budget_tracker.is_output_tokens_exceeded(),
                    stop_reason,
                    harness_crashed,
                    has_queued_prompts: prompt_queue.lock().await.has_queued(),
                    turn_limit_reached: budget_tracker.is_turn_limit_reached(),
                };

                match determine_run_outcome(&outcome_input) {
                    RunOutcomeDecision::ContinueNextTurn => {
                        continue;
                    }
                    RunOutcomeDecision::Finalize {
                        outcome,
                        detail,
                        retry_after,
                    } => {
                        final_outcome = outcome;
                        final_detail = detail;
                        final_retry_after = retry_after;
                        break;
                    }
                }
            }

            Ok(TurnLoopOutcome {
                session_id: final_session_id,
                last_pushed_sha,
                outcome: final_outcome,
                detail: final_detail,
                retry_after: final_retry_after,
                notices: loop_notices,
            })
        })
        .await;

    // Stop the harness subprocess: the ACP connection (and with it the harness' stdin) is closed
    // at this point, so give it time to flush its session state before killing it.
    if tokio::time::timeout(HARNESS_EXIT_GRACE, child.wait())
        .await
        .is_err()
    {
        let _ = child.kill().await;
    }

    // `push_after_turn` keeps the outbox state current, so it is right even after a driver error
    let outbox_last_pushed_sha = params
        .outbox
        .lock()
        .await
        .run_state()
        .last_pushed_sha
        .clone();
    let mut loop_outcome = match run_driver_result {
        Ok(out) => out,
        Err(e) => TurnLoopOutcome {
            session_id: String::new(),
            last_pushed_sha: outbox_last_pushed_sha,
            outcome: RunOutcome::Failed,
            detail: format!("Run driver error: {e}"),
            retry_after: None,
            notices: Vec::new(),
        },
    };

    collected_push_reports.extend(shared_push_reports.lock().await.drain(..));
    collected_notices.extend(loop_outcome.notices);

    // 6. Finalize workspace
    let finalize_params = FinalizeParams {
        repo_dir: &params.repo_dir,
        runtime_dir: &params.runtime_dir,
        workspace: &workspace,
        run_id: &params.assign.run_id,
        outcome: loop_outcome.outcome,
        detail: &loop_outcome.detail,
        harness_session_id: &loop_outcome.session_id,
        hooks_path: &params.hooks_path,
        credential_helper: &credential_helper_cmd,
        wip_size_cap_bytes: 10 * 1024 * 1024,
        diverged: prepare_diverged,
    };

    let finalize_res =
        match finalize_workspace(&git, &finalize_params, &mut loop_outcome.last_pushed_sha).await {
            Ok(res) => res,
            Err(e) => {
                return Ok(fail_run(
                    &params.outbound_tx,
                    format!("Workspace finalization failed: {e}"),
                    loop_outcome.session_id.clone(),
                    collected_push_reports,
                    collected_notices,
                )
                .await);
            }
        };

    let _ = params
        .outbox
        .lock()
        .await
        .set_last_pushed_sha(loop_outcome.last_pushed_sha.clone());

    let mut run_report = finalize_res.run_report;
    run_report.outcome = loop_outcome.outcome as i32;
    run_report.detail = loop_outcome.detail.clone();
    run_report.harness_session_id = loop_outcome.session_id.clone();

    if let Some(retry_dur) = loop_outcome.retry_after {
        let retry_timestamp = chrono::Utc::now()
            + chrono::Duration::from_std(retry_dur).unwrap_or(chrono::Duration::hours(1));
        run_report.retry_after = Some(df_proto::pbjson_types::Timestamp {
            seconds: retry_timestamp.timestamp(),
            nanos: 0,
        });
    }

    collected_push_reports.extend(finalize_res.push_reports.clone());
    for pr in &finalize_res.push_reports {
        send_body(
            &params.outbound_tx,
            agent_message::Body::PushReport(pr.clone()),
        )
        .await;
    }

    collected_notices.extend(finalize_res.notices.clone());
    for n in &finalize_res.notices {
        emit_notice(&params.outbound_tx, n.clone()).await;
    }

    send_body(
        &params.outbound_tx,
        agent_message::Body::RunReport(run_report.clone()),
    )
    .await;

    Ok(RunExecutionResult {
        outcome: loop_outcome.outcome,
        run_report,
        push_reports: collected_push_reports,
        notices: collected_notices,
    })
}
