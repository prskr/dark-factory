//! Bridge from the MCP servers an ACP client passes in `session/new` to `rpi-agent` tools.
//!
//! Each stdio server is spawned as a child process and every tool it lists becomes one
//! [`AgentTool`] named `mcp__<server>__<tool>` (the naming Claude Code uses, so prompts stay
//! portable between harnesses).

use std::sync::Arc;
use std::time::Duration;

use agent_client_protocol::schema::v1::McpServer;
use async_trait::async_trait;
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, JsonObject};
use rmcp::service::{RoleClient, RunningService};
use rpi_agent::{AgentError, AgentTool, AgentToolResult, TextContentOrImage, ToolResultPartial};
use rpi_ai::types::{Schema, TextContent, Tool};
use tokio::process::{Child, Command};
use tokio_util::sync::CancellationToken;

/// Bound for starting a server and listing its tools, so a hung server cannot block `session/new`.
pub const SETUP_TIMEOUT: Duration = Duration::from_secs(30);
/// Bound for one tool call (still cancellable through the signal).
const CALL_TIMEOUT: Duration = Duration::from_secs(600);

/// A running MCP server: the client side of the connection plus the child process behind it.
struct Server {
    client: RunningService<RoleClient, ()>,
    _child: Child,
}

struct McpTool {
    server: Arc<Server>,
    mcp_name: String,
    schema: Tool,
}

/// Spawn the stdio servers and list their tools. Non-stdio servers are skipped.
pub async fn connect(
    servers: &[McpServer],
    setup_timeout: Duration,
) -> anyhow::Result<Vec<Arc<dyn AgentTool>>> {
    let mut tools: Vec<Arc<dyn AgentTool>> = Vec::new();
    for server in servers {
        let McpServer::Stdio(stdio) = server else {
            // ponytail: only stdio servers exist today (the factory MCP bridge).
            continue;
        };
        let mut cmd = Command::new(&stdio.command);
        cmd.args(&stdio.args)
            .envs(stdio.env.iter().map(|e| (&e.name, &e.value)))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd.spawn()?;
        let (Some(stdout), Some(stdin)) = (child.stdout.take(), child.stdin.take()) else {
            anyhow::bail!("mcp server {}: stdio not captured", stdio.name);
        };
        let (client, listed) = tokio::time::timeout(setup_timeout, async {
            let client = ().serve((stdout, stdin)).await?;
            let listed = client.list_all_tools().await?;
            anyhow::Ok((client, listed))
        })
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "mcp server {}: no answer within {setup_timeout:?}",
                stdio.name
            )
        })??;
        let server = Arc::new(Server {
            client,
            _child: child,
        });
        for t in listed {
            tools.push(Arc::new(McpTool {
                server: server.clone(),
                mcp_name: t.name.to_string(),
                schema: Tool {
                    name: format!("mcp__{}__{}", stdio.name, t.name),
                    description: t.description.as_deref().unwrap_or_default().to_string(),
                    parameters: Schema::new(serde_json::Value::Object((*t.input_schema).clone())),
                    constrained_sampling: None,
                },
            }));
        }
    }
    Ok(tools)
}

#[async_trait]
impl AgentTool for McpTool {
    fn schema(&self) -> &Tool {
        &self.schema
    }

    fn label(&self) -> &str {
        &self.schema.name
    }

    async fn execute(
        &self,
        _tool_call_id: &str,
        params: serde_json::Value,
        signal: CancellationToken,
        _on_update: Arc<dyn Fn(ToolResultPartial) + Send + Sync>,
    ) -> Result<AgentToolResult, AgentError> {
        let mut req = CallToolRequestParams::new(self.mcp_name.clone());
        if let serde_json::Value::Object(args) = params {
            req = req.with_arguments(args as JsonObject);
        }
        let res = tokio::select! {
            _ = signal.cancelled() => return Err(AgentError::Abort),
            res = tokio::time::timeout(CALL_TIMEOUT, self.server.client.call_tool(req)) => res
                .map_err(|_| AgentError::tool(format!("mcp tool {} timed out", self.mcp_name)))?
                .map_err(|e| AgentError::tool(e.to_string()))?,
        };
        let text = res
            .content
            .iter()
            .filter_map(|c| serde_json::to_value(c).ok())
            .map(|v| {
                v["text"]
                    .as_str()
                    .map_or_else(|| v.to_string(), str::to_owned)
            })
            .collect::<Vec<_>>()
            .join("\n");
        if res.is_error == Some(true) {
            return Err(AgentError::tool(text));
        }
        Ok(AgentToolResult {
            content: vec![TextContentOrImage::Text(TextContent {
                kind: Default::default(),
                text,
                text_signature: None,
            })],
            details: res.structured_content.unwrap_or_default(),
            usage: None,
            added_tool_names: Vec::new(),
            terminate: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::McpServerStdio;

    #[tokio::test]
    async fn hung_mcp_server_fails_setup_instead_of_blocking() {
        // `sleep` never speaks MCP, so the handshake never completes.
        let hung = McpServer::Stdio(McpServerStdio::new("hung", "sleep").args(vec!["600".into()]));
        let err = connect(&[hung], Duration::from_millis(200))
            .await
            .err()
            .expect("setup must time out");
        assert!(err.to_string().contains("no answer"), "{err:#}");
    }
}
