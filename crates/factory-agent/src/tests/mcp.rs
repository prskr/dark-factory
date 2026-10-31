//! The MCP server the runtime exposes to the harness.

#![expect(
    clippy::too_many_lines,
    reason = "scenario tests set up and check a whole run"
)]

#[tokio::test]
async fn test_scenario_mcp_server_initialize_tools_list_and_call() {
    use crate::runtime_socket::{RuntimeHandler, RuntimeSocketServer};
    use df_proto::agent::v1::{ToolAnnotations, ToolCallResult, ToolDescriptor};
    use df_proto::transcript::v1::{
        ContentBlock, TextContent, content_block::Kind as ContentBlockKind,
    };
    use rmcp::ServiceExt;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct MockMcpRuntimeHandler {
        call_count: AtomicUsize,
        last_call_id: tokio::sync::Mutex<Option<String>>,
    }

    #[tonic::async_trait]
    impl RuntimeHandler for MockMcpRuntimeHandler {
        async fn handle_mcp_list_tools(&self) -> Vec<ToolDescriptor> {
            vec![
                ToolDescriptor {
                    name: "update_pull_request".to_string(),
                    title: "Update Pull Request".to_string(),
                    description: "Updates PR title and body".to_string(),
                    input_schema_json: r#"{"type":"object"}"#.to_string(),
                    output_schema_json: None,
                    annotations: Some(ToolAnnotations {
                        read_only: false,
                        destructive: false,
                        idempotent: true,
                        open_world: false,
                    }),
                },
                ToolDescriptor {
                    name: "comment_on_issue".to_string(),
                    title: "Comment on Issue".to_string(),
                    description: "Adds a comment to the task issue".to_string(),
                    input_schema_json: r#"{"type":"object"}"#.to_string(),
                    output_schema_json: None,
                    annotations: Some(ToolAnnotations {
                        read_only: false,
                        destructive: false,
                        idempotent: false,
                        open_world: false,
                    }),
                },
                ToolDescriptor {
                    name: "report_progress".to_string(),
                    title: "Report Progress".to_string(),
                    description: "Reports intermediate progress".to_string(),
                    input_schema_json: r#"{"type":"object"}"#.to_string(),
                    output_schema_json: None,
                    annotations: Some(ToolAnnotations {
                        read_only: false,
                        destructive: false,
                        idempotent: true,
                        open_world: false,
                    }),
                },
                ToolDescriptor {
                    name: "complete_task".to_string(),
                    title: "Complete Task".to_string(),
                    description: "Marks the task as completed".to_string(),
                    input_schema_json: r#"{"type":"object"}"#.to_string(),
                    output_schema_json: None,
                    annotations: Some(ToolAnnotations {
                        read_only: false,
                        destructive: false,
                        idempotent: true,
                        open_world: false,
                    }),
                },
            ]
        }

        async fn handle_mcp_call_tool(
            &self,
            name: &str,
            _arguments_json: &str,
        ) -> Result<ToolCallResult, String> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            // Runtime assigns a UUIDv7 call_id
            let call_id = format!("{}", uuid::Uuid::now_v7());
            *self.last_call_id.lock().await = Some(call_id.clone());

            match name {
                "comment_on_issue" => {
                    // Scenario: Error result (task has no linked issue)
                    Ok(ToolCallResult {
                        call_id,
                        is_error: true,
                        content: vec![ContentBlock {
                            kind: Some(ContentBlockKind::Text(TextContent {
                                text: "this task has no linked issue".to_string(),
                            })),
                            hidden: false,
                        }],
                        structured_content_json: String::new(),
                        end_run_after_turn: false,
                        await_input_after_turn: false,
                    })
                }
                "complete_task" => {
                    // Scenario: complete_task sets end_run_after_turn
                    Ok(ToolCallResult {
                        call_id,
                        is_error: false,
                        content: vec![ContentBlock {
                            kind: Some(ContentBlockKind::Text(TextContent {
                                text: "task completed successfully".to_string(),
                            })),
                            hidden: false,
                        }],
                        structured_content_json: r#"{"status":"completed"}"#.to_string(),
                        end_run_after_turn: true,
                        await_input_after_turn: false,
                    })
                }
                _ => Err(format!("unknown tool: {name}")),
            }
        }
    }

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let socket_path = temp_dir.path().join("runtime.sock");

    let handler = Arc::new(MockMcpRuntimeHandler {
        call_count: AtomicUsize::new(0),
        last_call_id: tokio::sync::Mutex::new(None),
    });

    let server = RuntimeSocketServer::bind(
        &socket_path,
        Arc::clone(&handler) as Arc<dyn RuntimeHandler>,
    )
    .expect("bind server");
    let _server_handle = server.spawn();

    // Spawn McpBridgeServer over a duplex stream
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let (server_read, server_write) = tokio::io::split(server_io);
    let socket_path_clone = socket_path.clone();
    let _mcp_handle = tokio::spawn(async move {
        crate::mcp::run_mcp_stream(server_read, server_write, &socket_path_clone)
            .await
            .ok();
    });

    let (client_read, client_write) = tokio::io::split(client_io);
    let client = ().serve((client_read, client_write)).await.expect("client connect");

    // Scenario: Four tools from Assign.tools
    let tools = client.list_all_tools().await.expect("list_all_tools");
    assert_eq!(tools.len(), 4);
    let tool_names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();
    assert_eq!(
        tool_names,
        vec![
            "update_pull_request",
            "comment_on_issue",
            "report_progress",
            "complete_task"
        ]
    );

    // Scenario: Error result (with is_error: true and text "this task has no linked issue")
    let call_params = rmcp::model::CallToolRequestParams::new("comment_on_issue");
    let res = client.call_tool(call_params).await.expect("call_tool");
    assert_eq!(res.is_error, Some(true));
    assert!(!res.content.is_empty());
    let first_block = &res.content[0];
    let json_str = serde_json::to_string(first_block).unwrap_or_default();
    assert!(
        json_str.contains("this task has no linked issue"),
        "expected error message in content, got: {json_str}"
    );

    // Verify call_id generated was valid UUIDv7
    let last_id = handler
        .last_call_id
        .lock()
        .await
        .clone()
        .expect("call_id was set");
    let parsed_uuid = uuid::Uuid::parse_str(&last_id).expect("valid uuid");
    assert_eq!(parsed_uuid.get_version_num(), 7, "call_id must be UUIDv7");

    // Scenario: complete_task success with structured content
    let call_params_complete = rmcp::model::CallToolRequestParams::new("complete_task");
    let res_complete = client
        .call_tool(call_params_complete)
        .await
        .expect("call_tool");
    assert_ne!(res_complete.is_error, Some(true));
    assert_eq!(
        res_complete.structured_content,
        Some(serde_json::json!({"status": "completed"}))
    );
}
