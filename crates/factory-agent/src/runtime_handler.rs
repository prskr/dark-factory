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
use crate::transcript::text_block;

/// Upper bound for a factory tool call to be answered by the control plane.
const TOOL_CALL_TIMEOUT: Duration = Duration::from_mins(30);

/// The Dark Factory tool calls of the harness that wait for the control plane's result, and
/// whether the turn was interrupted: one lock covers both, so a call registered while the turn
/// is interrupted is cancelled, never left waiting.
#[derive(Debug, Default)]
pub struct OpenToolCalls {
    calls: HashMap<String, oneshot::Sender<ToolCallResult>>,
    /// Who interrupted the running turn, until the turn ends.
    interrupted_by: Option<String>,
}

/// Shared handle on [`OpenToolCalls`].
pub type SharedToolCalls = Arc<Mutex<OpenToolCalls>>;

impl OpenToolCalls {
    /// Takes the call `call_id` if it still waits; a late result finds none.
    pub fn take(&mut self, call_id: &str) -> Option<oneshot::Sender<ToolCallResult>> {
        self.calls.remove(call_id)
    }

    /// Interrupts the turn: completes every waiting call as cancelled and cancels the calls
    /// registered until [`Self::end_interrupt`].
    pub fn interrupt(&mut self, by: &str) {
        for (call_id, tx) in std::mem::take(&mut self.calls) {
            tx.send(cancelled_result(call_id, by)).ok(); // the harness may have given up
        }
        self.interrupted_by = Some(by.to_string());
    }

    /// The turn is over: new calls wait for the control plane again.
    pub fn end_interrupt(&mut self) {
        self.interrupted_by = None;
    }
}

/// The result of a call that an interrupt cancelled.
pub fn cancelled_result(call_id: String, by: &str) -> ToolCallResult {
    ToolCallResult {
        call_id,
        is_error: true,
        content: vec![text_block(format!("Interrupted by {by}"))],
        ..Default::default()
    }
}

/// Runtime handler backing `/run/factory/runtime.sock` for the factory tools (MCP).
pub struct DriverRuntimeHandler {
    tools: Vec<ToolDescriptor>,
    outbound_tx: mpsc::Sender<AgentMessage>,
    pending_tool_calls: SharedToolCalls,
    /// Directories `write_artifact` may read files from: the workspace and `/tmp`.
    artifact_roots: Vec<PathBuf>,
}

impl DriverRuntimeHandler {
    pub fn new(
        tools: Vec<ToolDescriptor>,
        outbound_tx: mpsc::Sender<AgentMessage>,
        pending_tool_calls: SharedToolCalls,
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

        {
            let mut open = self.pending_tool_calls.lock().await;
            if let Some(by) = &open.interrupted_by {
                // The turn is being cancelled: nothing goes to the control plane.
                return Ok(cancelled_result(call_id, by));
            }
            open.calls.insert(call_id.clone(), tx);
        }

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
            self.pending_tool_calls.lock().await.take(&call_id);
            return Err("outbound channel closed".to_string());
        }

        // `end_run_after_turn` is recorded by the inbound loop before the result is delivered here
        if let Ok(result) = tokio::time::timeout(TOOL_CALL_TIMEOUT, rx).await {
            result.map_err(|_closed| "tool call was dropped or aborted".to_string())
        } else {
            self.pending_tool_calls.lock().await.take(&call_id);
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
            Arc::new(Mutex::new(OpenToolCalls::default())),
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
            Arc::new(Mutex::new(OpenToolCalls::default())),
            vec![PathBuf::from("/data/repo")],
        );

        let err = handler
            .handle_mcp_call_tool("write_artifact", r#"{"name":"x","path":"/etc/passwd"}"#)
            .await
            .unwrap_err();

        assert!(err.contains("/data/repo"), "{err}");
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn a_call_registered_while_the_turn_is_interrupted_is_cancelled_and_not_sent() {
        let (tx, mut rx) = mpsc::channel(4);
        let open: SharedToolCalls = Arc::new(Mutex::new(OpenToolCalls::default()));
        let handler = DriverRuntimeHandler::new(Vec::new(), tx, Arc::clone(&open), Vec::new());

        open.lock().await.interrupt("Peter Kurfer");
        let result = handler
            .handle_mcp_call_tool("ask_question", "{}")
            .await
            .unwrap();

        assert!(result.is_error);
        assert!(format!("{result:?}").contains("Interrupted by Peter Kurfer"));
        assert!(rx.try_recv().is_err(), "nothing goes to the control plane");

        // Once the turn is over, calls wait for the control plane again.
        open.lock().await.end_interrupt();
        let call =
            tokio::spawn(async move { handler.handle_mcp_call_tool("ask_question", "{}").await });
        assert!(rx.recv().await.is_some());
        call.abort();
    }
}
