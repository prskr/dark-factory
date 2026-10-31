//! Runtime handler backing `/run/factory/runtime.sock` for the factory tools (MCP).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use df_proto::agent::v1::{
    AgentMessage, ToolCallRequest, ToolCallResult, ToolDescriptor, agent_message,
};
use tokio::sync::{Mutex, mpsc, oneshot};
use uuid::Uuid;

use crate::runtime_socket::RuntimeHandler;

/// Upper bound for a factory tool call to be answered by the control plane.
const TOOL_CALL_TIMEOUT: Duration = Duration::from_mins(30);

/// Runtime handler backing `/run/factory/runtime.sock` for the factory tools (MCP).
pub struct DriverRuntimeHandler {
    tools: Vec<ToolDescriptor>,
    outbound_tx: mpsc::Sender<AgentMessage>,
    pending_tool_calls: Arc<Mutex<HashMap<String, oneshot::Sender<ToolCallResult>>>>,
}

impl DriverRuntimeHandler {
    pub fn new(
        tools: Vec<ToolDescriptor>,
        outbound_tx: mpsc::Sender<AgentMessage>,
        pending_tool_calls: Arc<Mutex<HashMap<String, oneshot::Sender<ToolCallResult>>>>,
    ) -> Self {
        Self {
            tools,
            outbound_tx,
            pending_tool_calls,
        }
    }
}

#[tonic::async_trait]
impl RuntimeHandler for DriverRuntimeHandler {
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
