//! `tools/call`: checks the run's enabled servers, forwards the call with the server's
//! credential and records the outcome (iter4-mcp-gateway design D5, D6).

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use df_core::mcp::{McpCallOutcome, split_tool_name};
use hyper::StatusCode;
use serde_json::{Value, json};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use super::client::{ClientError, Reply, SessionKey};
use super::jsonrpc::{json_response, rpc_result, tool_error};
use super::record::Pending;
use super::target::Target;
use super::{Mcp, Response, empty};
use crate::policy::{Policy, Sandbox};

/// Time one upstream call may take.
const CALL_TIMEOUT: Duration = Duration::from_secs(120);
/// Calls in progress per run and gateway pod.
pub(super) const MAX_CONCURRENT_CALLS: usize = 8;

/// A registered cancellation entry, removed when the call ends.
struct Registered<'a> {
    mcp: &'a Mcp,
    key: (String, String),
}

impl Drop for Registered<'_> {
    fn drop(&mut self) {
        if let Ok(mut cancels) = self.mcp.cancels.lock() {
            cancels.remove(&self.key);
        }
    }
}

impl Mcp {
    pub(super) async fn tools_call(
        &self,
        policy: &Policy,
        source: IpAddr,
        sandbox: &Sandbox,
        id: &Value,
        params: &Value,
        request_bytes: u64,
    ) -> Response {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let (server_name, tool) = split_tool_name(name).unwrap_or((name, ""));
        let pending = Pending::start(
            &self.sink,
            &sandbox.run_id,
            server_name,
            if tool.is_empty() { name } else { tool },
            request_bytes,
        );
        let Some(server) = policy
            .mcp_server(source, server_name)
            .filter(|_| !tool.is_empty())
        else {
            return answer(
                id,
                pending,
                McpCallOutcome::Rejected,
                tool_error(&format!("tool {name} is not available in this run")),
            );
        };
        let slot = self.slot(&sandbox.run_id);
        let Ok(_permit) = slot.try_acquire_owned() else {
            return answer(
                id,
                pending,
                McpCallOutcome::Rejected,
                tool_error("too many concurrent calls"),
            );
        };
        let credential = policy.provider_credential(&server.credential_ref);
        let target = match Target::from_proto(server, credential, self.client.allows_plain_http()) {
            Ok(target) => target,
            Err(error) => {
                tracing::warn!(run_id = %sandbox.run_id, server = server_name, ?error, "MCP server unusable");
                return answer(
                    id,
                    pending,
                    McpCallOutcome::UpstreamError,
                    tool_error(&ClientError::Target(error).message()),
                );
            }
        };
        let cancel = CancellationToken::new();
        let _registered = self.register(&sandbox.run_id, id, &cancel);
        let key = SessionKey {
            run_id: sandbox.run_id.clone(),
            server_id: server.id.clone(),
        };
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let call = self
            .client
            .call_tool(&key, &target, tool, arguments, &cancel);
        match tokio::time::timeout(CALL_TIMEOUT, call).await {
            Err(_elapsed) => answer(
                id,
                pending,
                McpCallOutcome::Timeout,
                tool_error(&ClientError::TimedOut.message()),
            ),
            Ok(Ok(Reply::Result(result))) => {
                let failed = result.get("isError").and_then(Value::as_bool) == Some(true);
                let outcome = if failed {
                    McpCallOutcome::ToolError
                } else {
                    McpCallOutcome::Ok
                };
                answer(id, pending, outcome, result)
            }
            Ok(Ok(Reply::Error(error))) => {
                let message = error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("upstream error");
                answer(id, pending, McpCallOutcome::ToolError, tool_error(message))
            }
            Ok(Err(ClientError::Cancelled)) => {
                pending.finish(McpCallOutcome::Cancelled, 0);
                empty(StatusCode::ACCEPTED)
            }
            Ok(Err(error)) => {
                let outcome = match error {
                    ClientError::TimedOut => McpCallOutcome::Timeout,
                    ClientError::TooLarge => McpCallOutcome::TooLarge,
                    _ => McpCallOutcome::UpstreamError,
                };
                answer(id, pending, outcome, tool_error(&error.message()))
            }
        }
    }

    pub(super) fn slot(&self, run_id: &str) -> Arc<Semaphore> {
        let Ok(mut slots) = self.slots.lock() else {
            return Arc::new(Semaphore::new(MAX_CONCURRENT_CALLS));
        };
        slots
            .entry(run_id.to_owned())
            .or_insert_with(|| Arc::new(Semaphore::new(MAX_CONCURRENT_CALLS)))
            .clone()
    }

    fn register(&self, run_id: &str, id: &Value, token: &CancellationToken) -> Registered<'_> {
        let key = (run_id.to_owned(), id.to_string());
        if let Ok(mut cancels) = self.cancels.lock() {
            cancels.insert(key.clone(), token.clone());
        }
        Registered { mcp: self, key }
    }
}

/// The JSON-RPC answer with `result`, reported with `outcome`.
fn answer(id: &Value, pending: Pending, outcome: McpCallOutcome, result: Value) -> Response {
    let message = rpc_result(id, result);
    pending.finish(outcome, message.to_string().len() as u64);
    json_response(StatusCode::OK, &[], &message)
}
