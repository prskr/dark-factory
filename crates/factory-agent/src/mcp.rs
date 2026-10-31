//! MCP stdio server bridging factory tools to the runtime Unix socket.
//!
//! Follows `specs/factory-mcp-server/spec.md`, `design/agent-run.md` §7, and ADR-0027.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rmcp::{
    ServerHandler, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock,
        ErrorData as McpError, JsonObject, ListToolsResult, PaginatedRequestParams,
        ServerCapabilities, ServerConfig, Tool, ToolAnnotations,
    },
    service::RequestContext,
};
use tokio::net::UnixStream;

use crate::runtime_socket::{
    McpToolDescriptorWire, RuntimeRequest, RuntimeResponse, read_frame, write_frame,
};

/// MCP server handler that bridges `tools/list` and `tools/call` over the runtime Unix domain socket.
#[derive(Debug, Clone)]
pub struct McpBridgeServer {
    socket_path: PathBuf,
}

impl McpBridgeServer {
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
        }
    }
}

impl ServerHandler for McpBridgeServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<rmcp::RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let mut stream = UnixStream::connect(&self.socket_path).await.map_err(|e| {
            McpError::internal_error(format!("failed to connect to runtime socket: {e}"), None)
        })?;

        write_frame(&mut stream, &RuntimeRequest::McpListTools)
            .await
            .map_err(|e| {
                McpError::internal_error(format!("failed to send McpListTools: {e}"), None)
            })?;

        let resp: RuntimeResponse = read_frame(&mut stream).await.map_err(|e| {
            McpError::internal_error(format!("failed to read McpListTools response: {e}"), None)
        })?;

        match resp {
            RuntimeResponse::McpListTools { tools } => {
                let mcp_tools = tools.into_iter().map(wire_to_mcp_tool).collect();
                Ok(ListToolsResult::with_all_items(mcp_tools))
            }
            other @ RuntimeResponse::McpCallTool { .. } => Err(McpError::internal_error(
                format!("unexpected runtime socket response: {other:?}"),
                None,
            )),
        }
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let mut stream = UnixStream::connect(&self.socket_path).await.map_err(|e| {
            McpError::internal_error(format!("failed to connect to runtime socket: {e}"), None)
        })?;

        let args_json = match &request.arguments {
            Some(args) => serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string()),
            None => "{}".to_string(),
        };
        let req = RuntimeRequest::McpCallTool {
            name: request.name.to_string(),
            arguments_json: args_json,
        };

        write_frame(&mut stream, &req).await.map_err(|e| {
            McpError::internal_error(format!("failed to send McpCallTool: {e}"), None)
        })?;

        let resp: RuntimeResponse = read_frame(&mut stream).await.map_err(|e| {
            McpError::internal_error(format!("failed to read McpCallTool response: {e}"), None)
        })?;

        match resp {
            RuntimeResponse::McpCallTool {
                is_error,
                content,
                structured_content_json,
                ..
            } => {
                let content_blocks: Vec<ContentBlock> =
                    content.into_iter().map(ContentBlock::text).collect();
                let mut res = if is_error {
                    CallToolResult::error(content_blocks)
                } else {
                    CallToolResult::success(content_blocks)
                };

                if let Some(json_str) = structured_content_json
                    && let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_str)
                {
                    res.structured_content = Some(val);
                }

                Ok(CallToolResponse::Complete(res))
            }
            other @ RuntimeResponse::McpListTools { .. } => Err(McpError::internal_error(
                format!("unexpected runtime socket response: {other:?}"),
                None,
            )),
        }
    }
}

fn wire_to_mcp_tool(wire: McpToolDescriptorWire) -> Tool {
    let mut input_schema: JsonObject =
        serde_json::from_str(&wire.input_schema_json).unwrap_or_default();
    if input_schema.is_empty() || !input_schema.contains_key("type") {
        input_schema.insert("type".to_string(), serde_json::json!("object"));
    }
    let desc = if wire.description.is_empty() {
        None
    } else {
        Some(Cow::Owned(wire.description))
    };
    let mut tool = Tool::new_with_raw(wire.name, desc, Arc::new(input_schema));
    if !wire.title.is_empty() {
        tool.title = Some(wire.title);
    }
    if let Some(out_json) = wire.output_schema_json
        && !out_json.is_empty()
        && let Ok(out_obj) = serde_json::from_str::<JsonObject>(&out_json)
    {
        tool = tool.with_raw_output_schema(Arc::new(out_obj));
    }
    let annotations = ToolAnnotations::default()
        .read_only(wire.read_only)
        .destructive(wire.destructive)
        .idempotent(wire.idempotent)
        .open_world(wire.open_world);
    tool.with_annotations(annotations)
}

/// Run the MCP server over an async bidirectional stream (used for tests or stdio).
pub async fn run_mcp_stream<R, W>(reader: R, writer: W, socket_path: &Path) -> anyhow::Result<()>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
    W: tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let server = McpBridgeServer::new(socket_path.to_path_buf());
    let running = server.serve((reader, writer)).await?;
    running.waiting().await?;
    Ok(())
}

/// Run the MCP server on stdio.
pub async fn run_mcp_stdio_server(socket_path: &Path) -> anyhow::Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    run_mcp_stream(stdin, stdout, socket_path).await
}
