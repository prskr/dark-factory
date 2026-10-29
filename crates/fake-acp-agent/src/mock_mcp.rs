//! Mock MCP server for testing stdio MCP client forwarding in tests.

use rmcp::{
    ServerHandler, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ServerCapabilities,
        ServerConfig,
    },
    service::RequestContext,
};

#[derive(Debug, Clone, Default)]
pub struct MockMcpServer;

impl ServerHandler for MockMcpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }

    #[expect(
        clippy::unused_async_trait_impl,
        reason = "the trait method is async; the mock answers immediately"
    )]
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResponse, rmcp::ErrorData> {
        let args_json = serde_json::to_string(&request.arguments).unwrap_or_default();
        let result = CallToolResult::success(vec![ContentBlock::text(format!(
            "tool '{}' called with args: {}",
            request.name, args_json
        ))]);
        Ok(CallToolResponse::Complete(result))
    }
}

/// Serve the mock MCP server on standard input/output.
pub async fn run_stdio() -> anyhow::Result<()> {
    let transport = (tokio::io::stdin(), tokio::io::stdout());
    let running = MockMcpServer.serve(transport).await?;
    running.waiting().await?;
    Ok(())
}
