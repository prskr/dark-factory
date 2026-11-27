//! `tools/list`: the tools of every server enabled for the run, prefixed with the server name
//! (iter4-mcp-gateway design D5).

use std::collections::HashSet;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use df_core::mcp::McpCallOutcome;
use df_core::mcp::{is_agent_usable_tool, prefixed_tool_name};
use df_proto::gateway::v1::McpServer;
use futures::StreamExt;
use futures::stream::FuturesUnordered;
use hyper::StatusCode;
use serde_json::{Map, Value, json};
use tokio::time::Instant;

use super::client::{ClientError, SessionKey};
use super::jsonrpc::{json_response, rpc_result};
use super::record::Pending;
use super::target::Target;
use super::{CachedList, Mcp, Response};
use crate::policy::{Policy, Sandbox};
use df_core::mcp::LIST_TOOL;

/// Time one `tools/list` answer may take, so the harness's own list timeout is not hit first.
const LIST_DEADLINE: Duration = Duration::from_secs(20);
/// How long a run's aggregated list is reused on a gateway pod.
const LIST_CACHE_TTL: Duration = Duration::from_secs(60);
/// The fields of an upstream tool the endpoint serves.
const SERVED_FIELDS: [&str; 5] = [
    "title",
    "description",
    "inputSchema",
    "outputSchema",
    "annotations",
];

impl Mcp {
    pub(super) async fn tools_list(
        &self,
        policy: &Policy,
        source: IpAddr,
        sandbox: &Sandbox,
        id: &Value,
    ) -> Response {
        let signature = policy.mcp_signature(source);
        let tools = if let Some(tools) = self.cached_list(&sandbox.run_id, &signature) {
            tools
        } else {
            let tools = Arc::new(self.collect_tools(policy, source, &sandbox.run_id).await);
            if let Ok(mut lists) = self.lists.lock() {
                lists.insert(
                    sandbox.run_id.clone(),
                    CachedList {
                        signature,
                        at: Instant::now(),
                        tools: tools.clone(),
                    },
                );
            }
            tools
        };
        json_response(
            StatusCode::OK,
            &[],
            &rpc_result(id, json!({ "tools": *tools })),
        )
    }

    /// The run's list if it was built within [`LIST_CACHE_TTL`] from the same servers and
    /// configuration versions.
    fn cached_list(&self, run_id: &str, signature: &[(String, u64)]) -> Option<Arc<Vec<Value>>> {
        let lists = self.lists.lock().ok()?;
        let cached = lists.get(run_id)?;
        (cached.signature == signature && cached.at.elapsed() < LIST_CACHE_TTL)
            .then(|| cached.tools.clone())
    }

    /// Asks every server in parallel; one that fails, exceeds a limit or is not done by the
    /// deadline is left out and recorded.
    async fn collect_tools(&self, policy: &Policy, source: IpAddr, run_id: &str) -> Vec<Value> {
        let servers = policy.mcp_servers(source);
        let pendings: Vec<Pending> = servers
            .iter()
            .map(|server| Pending::start(&self.sink, run_id, &server.name, LIST_TOOL, 0))
            .collect();
        let mut results: Vec<Option<Result<Vec<Value>, McpCallOutcome>>> =
            servers.iter().map(|_| None).collect();
        let mut running: FuturesUnordered<_> = servers
            .iter()
            .enumerate()
            .map(|(index, server)| async move {
                (index, self.list_server(policy, run_id, server).await)
            })
            .collect();
        let deadline = Instant::now() + LIST_DEADLINE;
        while let Ok(Some((index, result))) =
            tokio::time::timeout_at(deadline, running.next()).await
        {
            if let Some(slot) = results.get_mut(index) {
                *slot = Some(result);
            }
        }
        drop(running);
        let mut tools = Vec::new();
        for (pending, result) in pendings.into_iter().zip(results) {
            match result {
                Some(Ok(listed)) => {
                    pending.discard();
                    tools.extend(listed);
                }
                Some(Err(outcome)) => pending.finish(outcome, 0),
                None => pending.finish(McpCallOutcome::Timeout, 0),
            }
        }
        tools
    }

    /// The shaped tools of one server; a failure is retried once unless it cannot go away by
    /// itself.
    async fn list_server(
        &self,
        policy: &Policy,
        run_id: &str,
        server: &McpServer,
    ) -> Result<Vec<Value>, McpCallOutcome> {
        let credential = policy.provider_credential(&server.credential_ref);
        let target = Target::from_proto(server, credential, self.client.allows_plain_http())
            .map_err(|_error| McpCallOutcome::UpstreamError)?;
        let key = SessionKey {
            run_id: run_id.to_owned(),
            server_id: server.id.clone(),
        };
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self.client.list_tools(&key, &target).await {
                Ok(list) => return Ok(shape_tools(&target, list.tools)),
                Err(ClientError::TimedOut) => return Err(McpCallOutcome::Timeout),
                Err(ClientError::TooLarge) => return Err(McpCallOutcome::TooLarge),
                Err(ClientError::Blocked | ClientError::Target(_)) => {
                    return Err(McpCallOutcome::UpstreamError);
                }
                Err(_) if attempt == 1 => {}
                Err(_) => return Err(McpCallOutcome::UpstreamError),
            }
        }
    }
}

/// Keeps the served fields, prefixes the names and leaves out tools an agent cannot use and
/// later duplicates.
fn shape_tools(target: &Target, tools: Vec<Value>) -> Vec<Value> {
    let mut seen = HashSet::new();
    let mut shaped = Vec::new();
    for tool in tools {
        let Some(name) = tool.get("name").and_then(Value::as_str) else {
            continue;
        };
        if !is_agent_usable_tool(&target.name, name) {
            tracing::warn!(server = %target.name, tool = name, "tool left out: its name is not usable by agents");
            continue;
        }
        if !seen.insert(name.to_owned()) {
            continue;
        }
        let mut entry = Map::new();
        entry.insert(
            "name".into(),
            Value::String(prefixed_tool_name(&target.name, name)),
        );
        for field in SERVED_FIELDS {
            if let Some(value) = tool.get(field) {
                entry.insert(field.into(), value.clone());
            }
        }
        shaped.push(Value::Object(entry));
    }
    shaped
}
