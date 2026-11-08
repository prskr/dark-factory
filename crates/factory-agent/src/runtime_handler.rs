//! Runtime handler backing `/run/factory/runtime.sock` for the factory tools (MCP).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use df_proto::agent::v1::{
    AgentMessage, ToolCallRequest, ToolCallResult, ToolDescriptor, agent_message,
};
use tokio::sync::{Mutex, mpsc, oneshot};
use uuid::Uuid;

use crate::artifact_path::prepare_write_artifact;
use crate::runtime_socket::RuntimeHandler;

/// Upper bound for a factory tool call to be answered by the control plane.
const TOOL_CALL_TIMEOUT: Duration = Duration::from_mins(30);

/// Runtime handler backing `/run/factory/runtime.sock` for the factory tools (MCP).
pub struct DriverRuntimeHandler {
    tools: Vec<ToolDescriptor>,
    outbound_tx: mpsc::Sender<AgentMessage>,
    pending_tool_calls: Arc<Mutex<HashMap<String, oneshot::Sender<ToolCallResult>>>>,
    /// Directories `write_artifact` may read files from: the workspace and `/tmp`.
    artifact_roots: Vec<PathBuf>,
}

impl DriverRuntimeHandler {
    pub fn new(
        tools: Vec<ToolDescriptor>,
        outbound_tx: mpsc::Sender<AgentMessage>,
        pending_tool_calls: Arc<Mutex<HashMap<String, oneshot::Sender<ToolCallResult>>>>,
        artifact_roots: Vec<PathBuf>,
    ) -> Self {
        Self {
            tools,
            outbound_tx,
            pending_tool_calls,
            artifact_roots,
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
        let (arguments_json, attachment) = if name == "write_artifact" {
            let prepared = prepare_write_artifact(&self.artifact_roots, arguments_json).await?;
            (prepared.arguments_json, prepared.attachment)
        } else {
            (arguments_json.to_string(), None)
        };
        let call_id = Uuid::now_v7().to_string();
        let (tx, rx) = oneshot::channel();

        self.pending_tool_calls
            .lock()
            .await
            .insert(call_id.clone(), tx);

        let req = ToolCallRequest {
            call_id: call_id.clone(),
            tool_name: name.to_string(),
            arguments_json,
            attachment: attachment.unwrap_or_default().into(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn write_artifact_from_a_file_reaches_the_control_plane_as_an_attachment() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("login.png");
        std::fs::write(&file, b"\x89PNG\r\n\x1a\n").unwrap();
        let (tx, mut rx) = mpsc::channel(4);
        let handler = Arc::new(DriverRuntimeHandler::new(
            Vec::new(),
            tx,
            Arc::new(Mutex::new(HashMap::new())),
            vec![tmp.path().to_path_buf()],
        ));
        let args = serde_json::json!({ "name": "login.png", "path": file }).to_string();

        // The call waits for an answer that never comes: only the sent request matters.
        let call = tokio::spawn({
            let handler = Arc::clone(&handler);
            async move { handler.handle_mcp_call_tool("write_artifact", &args).await }
        });
        let sent = rx.recv().await;
        call.abort();

        let Some(AgentMessage {
            body: Some(agent_message::Body::ToolCallRequest(request)),
            ..
        }) = sent
        else {
            panic!("no tool call was sent");
        };
        assert_eq!(request.attachment.as_ref(), b"\x89PNG\r\n\x1a\n");
        assert!(!request.arguments_json.contains("path"));
    }

    #[tokio::test]
    async fn write_artifact_outside_the_roots_fails_without_sending() {
        let (tx, mut rx) = mpsc::channel(4);
        let handler = DriverRuntimeHandler::new(
            Vec::new(),
            tx,
            Arc::new(Mutex::new(HashMap::new())),
            vec![PathBuf::from("/data/repo")],
        );

        let err = handler
            .handle_mcp_call_tool("write_artifact", r#"{"name":"x","path":"/etc/passwd"}"#)
            .await
            .unwrap_err();

        assert!(err.contains("/data/repo"), "{err}");
        assert!(rx.try_recv().is_err());
    }
}
