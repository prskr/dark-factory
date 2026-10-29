//! Runtime handler backing `/run/factory/runtime.sock` for MCP and Git credentials.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use df_proto::agent::v1::{
    AgentMessage, CredentialGrant, CredentialRequest, GitCredentialRequest, ToolCallRequest,
    ToolCallResult, ToolDescriptor, agent_message, credential_grant, credential_request,
};
use tokio::sync::{Mutex, mpsc, oneshot};
use uuid::Uuid;

use crate::connection::CREDENTIAL_REQUEST_TIMEOUT;
use crate::runtime_socket::RuntimeHandler;

/// Cached credentials are refreshed this long before their stated expiry.
const CREDENTIAL_EXPIRY_MARGIN_SECS: i64 = 30;
/// Upper bound for a factory tool call to be answered by the control plane.
const TOOL_CALL_TIMEOUT: Duration = Duration::from_mins(30);

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
        if let Ok(result) = tokio::time::timeout(TOOL_CALL_TIMEOUT, rx).await {
            result.map_err(|_closed| "tool call was dropped or aborted".to_string())
        } else {
            self.pending_tool_calls.lock().await.remove(&call_id);
            Err(format!("tool call timed out after {TOOL_CALL_TIMEOUT:?}"))
        }
    }
}
