//! Harness sessions: resume, permissions, prompt queue and hidden blocks.

#![allow(
    clippy::too_many_lines,
    reason = "scenario tests set up and check a whole run"
)]

use df_proto::agent::v1::control_message;

#[tokio::test]
async fn test_scenario_harness_resume_success() {
    use crate::harness::{build_acp_prompt_blocks, setup_session};
    use crate::test_harness::MockAcpAgent;
    use agent_client_protocol::schema::v1::McpServer;
    use agent_client_protocol::{Channel, Client};
    use df_proto::agent::v1::Continuation;

    let mock_agent = MockAcpAgent::new()
        .with_load_session(true)
        .with_existing_session("sess-existing-123");

    let agent_builder = mock_agent.build();
    let (transport, peer) = Channel::duplex();

    let agent_task = tokio::spawn(async move {
        agent_builder.connect_to(peer).await.unwrap();
    });

    let client = Client.builder();
    let client_task = tokio::spawn(async move {
        client
            .connect_with(transport, async move |cx| {
                let temp = tempfile::tempdir().unwrap();
                let repo_dir = temp.path().join("repo");
                let mcp_cmd = std::path::PathBuf::from("/opt/factory/factory-agent");
                let mcp_sock = std::path::PathBuf::from("/run/factory/runtime.sock");

                let continuation = Continuation {
                    harness_session_id: "sess-existing-123".to_string(),
                    summary: "Context from previous run".to_string(),
                };

                let res = setup_session(&cx, &repo_dir, Some(&continuation), &mcp_cmd, &mcp_sock)
                    .await
                    .expect("setup session should succeed");

                assert!(res.is_resumed, "Session should be resumed");
                assert_eq!(res.session_id, "sess-existing-123");
                assert!(res.notices.is_empty(), "No notices on clean resume");

                // When resumed, summary is NOT prepended to prompts
                let prepend_summary = if !res.is_resumed {
                    Some(continuation.summary.as_str())
                } else {
                    None
                };
                let prompt_blocks = build_acp_prompt_blocks(
                    &[df_proto::agent::v1::Prompt {
                        prompt_id: "p1".to_string(),
                        author: None,
                        queued_at: None,
                        content: vec![df_proto::transcript::v1::ContentBlock {
                            kind: Some(df_proto::transcript::v1::content_block::Kind::Text(
                                df_proto::transcript::v1::TextContent {
                                    text: "Follow up prompt".to_string(),
                                },
                            )),
                            hidden: false,
                        }],
                    }],
                    prepend_summary,
                );
                assert_eq!(prompt_blocks.len(), 1);
                match &prompt_blocks[0] {
                    agent_client_protocol::schema::v1::ContentBlock::Text(t) => {
                        assert_eq!(t.text, "Follow up prompt");
                    }
                    _ => panic!("Expected text block"),
                }

                Ok(())
            })
            .await
    });

    client_task.await.unwrap().unwrap();
    agent_task.abort();

    // Verify mock agent received the MCP server
    let servers = mock_agent.received_mcp_servers.lock().unwrap();
    assert_eq!(servers.len(), 1);
    match &servers[0] {
        McpServer::Stdio(s) => {
            assert_eq!(s.name, "factory");
            assert_eq!(s.args, vec!["mcp", "--socket", "/run/factory/runtime.sock"]);
        }
        _ => panic!("Expected Stdio MCP server"),
    }
}

#[tokio::test]
async fn test_scenario_harness_resume_fails_falls_back_to_new() {
    use crate::harness::{build_acp_prompt_blocks, setup_session};
    use crate::test_harness::MockAcpAgent;
    use agent_client_protocol::{Channel, Client};
    use df_proto::agent::v1::Continuation;

    // Agent does NOT have this session
    let mock_agent = MockAcpAgent::new().with_load_session(true);

    let agent_builder = mock_agent.build();
    let (transport, peer) = Channel::duplex();

    let agent_task = tokio::spawn(async move {
        agent_builder.connect_to(peer).await.unwrap();
    });

    let client = Client.builder();
    let client_task = tokio::spawn(async move {
        client
            .connect_with(transport, async move |cx| {
                let temp = tempfile::tempdir().unwrap();
                let repo_dir = temp.path().join("repo");
                let mcp_cmd = std::path::PathBuf::from("/opt/factory/factory-agent");
                let mcp_sock = std::path::PathBuf::from("/run/factory/runtime.sock");

                let continuation = Continuation {
                    harness_session_id: "nonexistent-sess".to_string(),
                    summary: "Summary of earlier progress".to_string(),
                };

                let res = setup_session(&cx, &repo_dir, Some(&continuation), &mcp_cmd, &mcp_sock)
                    .await
                    .expect("fallback to new session should succeed");

                assert!(!res.is_resumed, "Session should NOT be resumed");
                assert_ne!(res.session_id, "nonexistent-sess");
                assert_eq!(res.notices.len(), 1);
                assert_eq!(res.notices[0].code, "harness.session_resume_failed");

                // When resume fails, summary IS prepended to first turn's prompts
                let prepend_summary = if !res.is_resumed {
                    Some(continuation.summary.as_str())
                } else {
                    None
                };
                let prompt_blocks = build_acp_prompt_blocks(
                    &[df_proto::agent::v1::Prompt {
                        prompt_id: "p1".to_string(),
                        author: None,
                        queued_at: None,
                        content: vec![df_proto::transcript::v1::ContentBlock {
                            kind: Some(df_proto::transcript::v1::content_block::Kind::Text(
                                df_proto::transcript::v1::TextContent {
                                    text: "First prompt of run 2".to_string(),
                                },
                            )),
                            hidden: false,
                        }],
                    }],
                    prepend_summary,
                );
                assert_eq!(prompt_blocks.len(), 2);
                match &prompt_blocks[0] {
                    agent_client_protocol::schema::v1::ContentBlock::Text(t) => {
                        assert_eq!(
                            t.text,
                            "Context from previous run:\nSummary of earlier progress"
                        );
                    }
                    _ => panic!("Expected text block for summary"),
                }
                match &prompt_blocks[1] {
                    agent_client_protocol::schema::v1::ContentBlock::Text(t) => {
                        assert_eq!(t.text, "First prompt of run 2");
                    }
                    _ => panic!("Expected text block for user prompt"),
                }

                Ok(())
            })
            .await
    });

    client_task.await.unwrap().unwrap();
    agent_task.abort();
}

#[test]
fn test_scenario_harness_permission_auto_approved() {
    use crate::harness::approve_permission;
    use agent_client_protocol::schema::v1::{
        PermissionOption, PermissionOptionId, PermissionOptionKind, RequestPermissionOutcome,
        RequestPermissionRequest, SessionId, ToolCallUpdate, ToolCallUpdateFields,
    };

    let opt_allow = PermissionOption::new(
        PermissionOptionId::new("opt-allow-1"),
        "Allow command execution",
        PermissionOptionKind::AllowOnce,
    );
    let opt_reject = PermissionOption::new(
        PermissionOptionId::new("opt-reject-1"),
        "Deny command execution",
        PermissionOptionKind::RejectOnce,
    );

    let req = RequestPermissionRequest::new(
        SessionId::new("sess-1"),
        ToolCallUpdate::new("call-1", ToolCallUpdateFields::default()),
        vec![opt_reject, opt_allow],
    );

    let resp = approve_permission(&req);
    match resp.outcome {
        RequestPermissionOutcome::Selected(selected) => {
            assert_eq!(selected.option_id.0.as_ref(), "opt-allow-1");
        }
        _ => panic!("Expected Selected permission outcome"),
    }
}

#[tokio::test]
async fn test_scenario_fake_acp_agent_subprocess_session_new_and_resume() {
    use crate::harness::{TokioAsyncReadCompat, TokioAsyncWriteCompat, setup_session};
    use agent_client_protocol::{ByteStreams, Client};
    use df_proto::agent::v1::Continuation;

    let fake_bin = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/debug/fake-acp-agent");

    if !fake_bin.exists() {
        eprintln!(
            "Skipping subprocess test because {} not found",
            fake_bin.display()
        );
        return;
    }

    let temp = tempfile::tempdir().unwrap();
    let home_dir = temp.path().join("home");
    let repo_dir = temp.path().join("repo");
    std::fs::create_dir_all(&home_dir).unwrap();
    std::fs::create_dir_all(&repo_dir).unwrap();

    let script_yaml = r#"
sessions:
  new:
    - - say: "Hello from turn 1"
      - end: end_turn
  loaded:
    - - say: "Hello from resumed turn"
      - end: end_turn
"#;
    let script_file = temp.path().join("script.yaml");
    std::fs::write(&script_file, script_yaml).unwrap();

    // 1. First run: session/new
    let mut cmd = tokio::process::Command::new(&fake_bin);
    cmd.arg("--script")
        .arg(&script_file)
        .env("HOME", &home_dir)
        .current_dir(&repo_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit());

    let mut child = cmd.spawn().expect("spawn fake-acp-agent");
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();

    let transport = ByteStreams::new(TokioAsyncWriteCompat(stdin), TokioAsyncReadCompat(stdout));

    let mcp_cmd = std::path::PathBuf::from("/opt/factory/factory-agent");
    let mcp_sock = std::path::PathBuf::from("/run/factory/runtime.sock");

    let repo_dir_1 = repo_dir.clone();
    let mcp_cmd_1 = mcp_cmd.clone();
    let mcp_sock_1 = mcp_sock.clone();

    let client = Client.builder();
    let setup_result = client
        .connect_with(transport, async move |cx| {
            let res = setup_session(&cx, &repo_dir_1, None, &mcp_cmd_1, &mcp_sock_1)
                .await
                .expect("setup session new");
            Ok(res)
        })
        .await
        .expect("client connect_with");

    assert!(!setup_result.is_resumed);
    let session_id = setup_result.session_id;
    assert!(!session_id.is_empty());
    let _ = child.kill().await;

    // 2. Second run: session/load with session_id
    let mut cmd2 = tokio::process::Command::new(&fake_bin);
    cmd2.arg("--script")
        .arg(&script_file)
        .env("HOME", &home_dir)
        .current_dir(&repo_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit());

    let mut child2 = cmd2.spawn().expect("spawn fake-acp-agent 2");
    let stdin2 = child2.stdin.take().unwrap();
    let stdout2 = child2.stdout.take().unwrap();

    let transport2 = ByteStreams::new(TokioAsyncWriteCompat(stdin2), TokioAsyncReadCompat(stdout2));

    let continuation = Continuation {
        harness_session_id: session_id.clone(),
        summary: "Previous summary".to_string(),
    };

    let repo_dir_2 = repo_dir.clone();
    let mcp_cmd_2 = mcp_cmd.clone();
    let mcp_sock_2 = mcp_sock.clone();

    let client2 = Client.builder();
    let setup_result2 = client2
        .connect_with(transport2, async move |cx| {
            let res = setup_session(
                &cx,
                &repo_dir_2,
                Some(&continuation),
                &mcp_cmd_2,
                &mcp_sock_2,
            )
            .await
            .expect("setup session load");
            Ok(res)
        })
        .await
        .expect("client2 connect_with");

    assert!(setup_result2.is_resumed);
    assert_eq!(setup_result2.session_id, session_id);
    let _ = child2.kill().await;
}

#[test]
fn test_scenario_two_queued_messages_arrive_during_turn() {
    use crate::harness::{PromptQueue, build_acp_prompt_blocks};
    use df_proto::agent::v1::Prompt;
    use df_proto::transcript::v1::{
        ContentBlock, Principal, TextContent, content_block::Kind as ContentKind,
        event::Kind as EventKind,
    };

    let mut queue = PromptQueue::new();

    let p1 = Prompt {
        prompt_id: "prompt-1".to_string(),
        author: Some(Principal {
            kind: Some(df_proto::transcript::v1::principal::Kind::Human(
                df_proto::transcript::v1::HumanPrincipal {
                    user_id: "user-123".to_string(),
                    display_name: "Alice".to_string(),
                },
            )),
        }),
        content: vec![ContentBlock {
            kind: Some(ContentKind::Text(TextContent {
                text: "Turn 1 prompt".to_string(),
            })),
            hidden: false,
        }],
        queued_at: None,
    };

    queue.push(p1);

    // Turn 1 starts with p1
    let (turn_1_prompts, turn_1_events) = queue.start_turn("turn-1");
    assert_eq!(turn_1_prompts.len(), 1);
    assert_eq!(turn_1_events.len(), 2); // TurnStarted + UserMessage

    let turn_started_1 = match &turn_1_events[0].kind {
        Some(EventKind::TurnStarted(ts)) => ts,
        _ => panic!("Expected TurnStarted event"),
    };
    assert_eq!(turn_started_1.prompt_ids, vec!["prompt-1"]);

    let user_msg_1 = match &turn_1_events[1].kind {
        Some(EventKind::UserMessage(um)) => um,
        _ => panic!("Expected UserMessage event"),
    };
    assert_eq!(user_msg_1.prompt_id, "prompt-1");

    // While Turn 1 is active, two new prompts arrive
    let p2 = Prompt {
        prompt_id: "prompt-2".to_string(),
        author: None,
        content: vec![ContentBlock {
            kind: Some(ContentKind::Text(TextContent {
                text: "Second prompt (queued during turn 1)".to_string(),
            })),
            hidden: false,
        }],
        queued_at: None,
    };

    let p3 = Prompt {
        prompt_id: "prompt-3".to_string(),
        author: None,
        content: vec![ContentBlock {
            kind: Some(ContentKind::Text(TextContent {
                text: "Third prompt (queued during turn 1)".to_string(),
            })),
            hidden: false,
        }],
        queued_at: None,
    };

    queue.push(p2);
    queue.push(p3);
    assert_eq!(queue.len(), 2);
    assert!(queue.has_queued());

    // When turn 1 finishes, turn 2 starts with BOTH queued prompts
    let (turn_2_prompts, turn_2_events) = queue.start_turn("turn-2");
    assert_eq!(turn_2_prompts.len(), 2);
    assert_eq!(turn_2_prompts[0].prompt_id, "prompt-2");
    assert_eq!(turn_2_prompts[1].prompt_id, "prompt-3");

    // TurnStarted must list both prompt ids
    assert_eq!(turn_2_events.len(), 3); // 1 TurnStarted + 2 UserMessage
    let turn_started_2 = match &turn_2_events[0].kind {
        Some(EventKind::TurnStarted(ts)) => ts,
        _ => panic!("Expected TurnStarted event"),
    };
    assert_eq!(turn_started_2.prompt_ids, vec!["prompt-2", "prompt-3"]);

    let user_msg_2 = match &turn_2_events[1].kind {
        Some(EventKind::UserMessage(um)) => um,
        _ => panic!("Expected UserMessage 2"),
    };
    assert_eq!(user_msg_2.prompt_id, "prompt-2");

    let user_msg_3 = match &turn_2_events[2].kind {
        Some(EventKind::UserMessage(um)) => um,
        _ => panic!("Expected UserMessage 3"),
    };
    assert_eq!(user_msg_3.prompt_id, "prompt-3");

    // ACP prompt blocks contain both prompt messages
    let acp_blocks = build_acp_prompt_blocks(&turn_2_prompts, None);
    assert_eq!(acp_blocks.len(), 2);

    // Queue is now empty
    assert!(!queue.has_queued());
    assert_eq!(queue.len(), 0);
}

#[test]
fn test_hidden_block_is_delivered_and_keeps_its_flag_in_the_event() {
    use crate::harness::{PromptQueue, build_acp_prompt_blocks};
    use df_proto::agent::v1::Prompt;
    use df_proto::transcript::v1::{
        ContentBlock, TextContent, content_block::Kind as ContentKind, event::Kind as EventKind,
    };

    let block = |text: &str, hidden| ContentBlock {
        kind: Some(ContentKind::Text(TextContent { text: text.into() })),
        hidden,
    };
    let mut queue = PromptQueue::new();
    queue.push(Prompt {
        prompt_id: "p".into(),
        author: None,
        content: vec![block("visible", false), block("secret", true)],
        queued_at: None,
    });

    let (prompts, events) = queue.start_turn("t");

    // A missing UserMessage yields no flags and fails the assertion.
    let flags: Vec<bool> = match &events[1].kind {
        Some(EventKind::UserMessage(um)) => um.content.iter().map(|b| b.hidden).collect(),
        _ => Vec::new(),
    };
    assert_eq!(flags, [false, true]);
    assert_eq!(build_acp_prompt_blocks(&prompts, None).len(), 2);
}

#[test]
fn test_scenario_prompt_queue_deduplicates_duplicate_prompts() {
    use crate::connection::CommandFilter;
    use df_proto::agent::v1::{ControlMessage, Prompt, control_message};

    // Duplicates are dropped at the connection boundary by `CommandFilter`
    let msg = ControlMessage {
        ack: 0,
        body: Some(control_message::Body::Prompt(Prompt {
            prompt_id: "prompt-dup".to_string(),
            author: None,
            content: vec![],
            queued_at: None,
        })),
    };
    let mut filter = CommandFilter::new();
    assert!(filter.filter_control_message(&msg));
    assert!(!filter.filter_control_message(&msg));
}

#[test]
fn test_empty_prompt_ids_are_not_duplicates() {
    use crate::connection::CommandFilter;
    use crate::harness::PromptQueue;
    use df_proto::agent::v1::{ControlMessage, Prompt};

    let mut queue = PromptQueue::new();
    queue.push(Prompt::default());
    queue.push(Prompt::default());
    assert_eq!(queue.len(), 2);

    let msg = ControlMessage {
        ack: 0,
        body: Some(control_message::Body::Prompt(Prompt::default())),
    };
    let mut filter = CommandFilter::new();
    assert!(filter.filter_control_message(&msg));
    assert!(filter.filter_control_message(&msg));
}
