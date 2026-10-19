//! Integration and unit tests for `fake-acp-agent`.

use std::sync::Arc;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    ContentBlock, ContentChunk, InitializeRequest, NewSessionRequest, PlanEntryStatus,
    PromptRequest, SessionNotification, SessionUpdate, StopReason, TextContent,
};
use agent_client_protocol::{Channel, Client, ConnectionTo};
use tokio::sync::Mutex;

use crate::agent::{AgentState, build_agent};
use crate::script::Script;

#[test]
fn test_script_yaml_parsing() {
    let yaml = r#"
sessions:
  new:
    - - say: "Hello from fake agent"
      - think: "Planning next steps"
      - tool_call:
          id: "call_42"
          title: "Inspect workspace"
          output: "ok"
      - plan:
          entries:
            - content: "Step 1"
              status: completed
      - usage:
          used: 120
          size: 100000
      - end: end_turn
  loaded:
    - - say: "Resumed session turn"
      - end: end_turn
"#;

    let script = Script::from_yaml_str(yaml).expect("should parse valid YAML script");
    assert_eq!(script.sessions.new.len(), 1);
    assert_eq!(script.sessions.new[0].steps.len(), 6);
    assert_eq!(script.sessions.loaded.len(), 1);
    assert_eq!(script.sessions.loaded[0].steps.len(), 2);
}

#[tokio::test]
async fn test_scripted_turn_produces_expected_updates_and_stop_reason() {
    let yaml = r#"
sessions:
  new:
    - - say: "Hello from fake agent"
      - think: "Thinking deeply"
      - tool_call:
          id: "call_1"
          title: "Run diagnostic"
          output: { "status": "all green" }
      - plan:
          entries:
            - content: "Initial task"
              status: completed
      - usage:
          used: 350
          size: 200000
      - end: end_turn
"#;

    let script = Script::from_yaml_str(yaml).expect("script should parse");
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let state = Arc::new(Mutex::new(AgentState::new(
        script,
        temp_dir.path().to_path_buf(),
    )));

    let agent = build_agent(state);
    let (transport, peer) = Channel::duplex();

    let updates = Arc::new(Mutex::new(Vec::new()));
    let updates_cb = Arc::clone(&updates);

    let client = Client.builder().on_receive_notification(
        async move |notification: SessionNotification,
                    _cx: ConnectionTo<agent_client_protocol::role::acp::Agent>| {
            updates_cb.lock().await.push(notification.update);
            Ok(())
        },
        agent_client_protocol::on_receive_notification!(),
    );

    // Run client and agent connected over duplex in-memory channel
    let client_task = tokio::spawn(async move {
        client
            .connect_with(transport, async move |cx| {
                // 1. Initialize
                let init_res = cx
                    .send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task()
                    .await
                    .expect("initialize failed");
                assert_eq!(init_res.protocol_version, ProtocolVersion::V1);

                // 2. New session
                let new_session_res = cx
                    .send_request(NewSessionRequest::new(temp_dir.path().to_path_buf()))
                    .block_task()
                    .await
                    .expect("new session failed");
                let session_id = new_session_res.session_id;

                // 3. Prompt turn
                let prompt_res = cx
                    .send_request(PromptRequest::new(
                        session_id.clone(),
                        vec![ContentBlock::Text(TextContent::new("Hello agent"))],
                    ))
                    .block_task()
                    .await
                    .expect("prompt turn failed");

                assert_eq!(prompt_res.stop_reason, StopReason::EndTurn);

                Ok(())
            })
            .await
    });

    let agent_task = tokio::spawn(async move {
        let _ = agent.connect_to(peer).await;
    });

    let (client_res, _) = tokio::join!(client_task, agent_task);
    client_res
        .expect("client task panicked")
        .expect("client run failed");

    let received = updates.lock().await.clone();
    assert_eq!(received.len(), 6, "Expected 6 updates emitted during turn");

    // 1. Agent text chunk
    match &received[0] {
        SessionUpdate::AgentMessageChunk(ContentChunk { content, .. }) => match content {
            ContentBlock::Text(TextContent { text, .. }) => {
                assert_eq!(text, "Hello from fake agent");
            }
            other => panic!("Expected text content block, got: {other:?}"),
        },
        other => panic!("Expected AgentMessageChunk, got: {other:?}"),
    }

    // 2. Agent thought chunk
    match &received[1] {
        SessionUpdate::AgentThoughtChunk(ContentChunk { content, .. }) => match content {
            ContentBlock::Text(TextContent { text, .. }) => {
                assert_eq!(text, "Thinking deeply");
            }
            other => panic!("Expected thought content block, got: {other:?}"),
        },
        other => panic!("Expected AgentThoughtChunk, got: {other:?}"),
    }

    // 3. Tool call
    match &received[2] {
        SessionUpdate::ToolCall(call) => {
            assert_eq!(call.tool_call_id.0.as_ref(), "call_1");
            assert_eq!(call.title, "Run diagnostic");
        }
        other => panic!("Expected ToolCall, got: {other:?}"),
    }

    // 4. Tool call update
    match &received[3] {
        SessionUpdate::ToolCallUpdate(update) => {
            assert_eq!(update.tool_call_id.0.as_ref(), "call_1");
            assert_eq!(
                update.fields.status,
                Some(agent_client_protocol::schema::v1::ToolCallStatus::Completed)
            );
        }
        other => panic!("Expected ToolCallUpdate, got: {other:?}"),
    }

    // 5. Plan
    match &received[4] {
        SessionUpdate::Plan(plan) => {
            assert_eq!(plan.entries.len(), 1);
            assert_eq!(plan.entries[0].content, "Initial task");
            assert_eq!(plan.entries[0].status, PlanEntryStatus::Completed);
        }
        other => panic!("Expected Plan, got: {other:?}"),
    }

    // 6. Usage update
    match &received[5] {
        SessionUpdate::UsageUpdate(usage) => {
            assert_eq!(usage.used, 350);
            assert_eq!(usage.size, 200000);
        }
        other => panic!("Expected UsageUpdate, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_scenario_commit_in_a_turn() {
    let temp_repo = tempfile::tempdir().expect("tempdir");
    let repo_path = temp_repo.path();

    // Set up a git repository in repo_path
    let run_cmd = |cmd: &str| {
        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(repo_path)
            .status()
            .expect("cmd failed");
        assert!(status.success(), "Command failed: {cmd}");
    };

    run_cmd("git init -b main");
    run_cmd("git config user.name 'Test Runner'");
    run_cmd("git config user.email 'test@example.org'");
    run_cmd("git config commit.gpgsign false");

    let yaml = r#"
sessions:
  new:
    - - shell: "echo x > a.txt && git add a.txt && git commit --no-gpg-sign -m add"
      - end: end_turn
"#;

    let script = Script::from_yaml_str(yaml).expect("script should parse");
    let temp_session = tempfile::tempdir().expect("tempdir");
    let state = Arc::new(Mutex::new(AgentState::new(
        script,
        temp_session.path().to_path_buf(),
    )));

    let agent = build_agent(state);
    let (transport, peer) = Channel::duplex();

    let repo_path_buf = repo_path.to_path_buf();
    let client_task = tokio::spawn(async move {
        Client
            .builder()
            .connect_with(transport, async move |cx| {
                cx.send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task()
                    .await
                    .expect("init");

                let new_session = cx
                    .send_request(NewSessionRequest::new(repo_path_buf))
                    .block_task()
                    .await
                    .expect("new session");

                let res = cx
                    .send_request(PromptRequest::new(
                        new_session.session_id,
                        vec![ContentBlock::Text(TextContent::new("make a commit"))],
                    ))
                    .block_task()
                    .await
                    .expect("prompt");

                assert_eq!(res.stop_reason, StopReason::EndTurn);
                Ok(())
            })
            .await
    });

    let agent_task = tokio::spawn(async move {
        let _ = agent.connect_to(peer).await;
    });

    let (client_res, _) = tokio::join!(client_task, agent_task);
    client_res.expect("client panic").expect("client error");

    // Verify commit exists in repository
    let git_log = std::process::Command::new("git")
        .args(["log", "-1", "--pretty=%B"])
        .current_dir(repo_path)
        .output()
        .expect("git log");
    assert!(git_log.status.success());
    let log_msg = String::from_utf8_lossy(&git_log.stdout);
    assert!(log_msg.contains("add"));
}

#[tokio::test]
async fn test_scenario_factory_tool_call() {
    let yaml = r#"
sessions:
  new:
    - - mcp:
          server: factory
          tool: report_progress
          args:
            summary: "tests pass"
      - end: end_turn
"#;

    let script = Script::from_yaml_str(yaml).expect("script should parse");
    let temp_session = tempfile::tempdir().expect("tempdir");
    let state = Arc::new(Mutex::new(AgentState::new(
        script,
        temp_session.path().to_path_buf(),
    )));

    let agent = build_agent(state);
    let (transport, peer) = Channel::duplex();

    let updates = Arc::new(Mutex::new(Vec::new()));
    let updates_cb = Arc::clone(&updates);

    let client = Client.builder().on_receive_notification(
        async move |notification: SessionNotification,
                    _cx: ConnectionTo<agent_client_protocol::role::acp::Agent>| {
            updates_cb.lock().await.push(notification.update);
            Ok(())
        },
        agent_client_protocol::on_receive_notification!(),
    );

    let mock_script = temp_session.path().join("mock_mcp.sh");
    let script_body = r#"#!/bin/sh
while IFS= read -r line; do
  id=$(echo "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *initialize*)
      echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{\"tools\":{}},\"serverInfo\":{\"name\":\"factory\",\"version\":\"1.0\"}}}"
      ;;
    *notifications/initialized*)
      ;;
    *tools/call*)
      echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"tests pass\"}],\"isError\":false}}"
      ;;
  esac
done
"#;
    std::fs::write(&mock_script, script_body).expect("write mock script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&mock_script, std::fs::Permissions::from_mode(0o755))
            .expect("chmod");
    }

    let client_task = tokio::spawn(async move {
        client
            .connect_with(transport, async move |cx| {
                cx.send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task()
                    .await
                    .expect("init");

                let stdio_server =
                    agent_client_protocol::schema::v1::McpServerStdio::new("factory", mock_script);

                let mut new_session_req = NewSessionRequest::new(temp_session.path().to_path_buf());
                new_session_req.mcp_servers =
                    vec![agent_client_protocol::schema::v1::McpServer::Stdio(
                        stdio_server,
                    )];

                let new_session = cx
                    .send_request(new_session_req)
                    .block_task()
                    .await
                    .expect("new session");

                let res = cx
                    .send_request(PromptRequest::new(
                        new_session.session_id,
                        vec![ContentBlock::Text(TextContent::new("call tool"))],
                    ))
                    .block_task()
                    .await
                    .expect("prompt");

                assert_eq!(res.stop_reason, StopReason::EndTurn);
                Ok(())
            })
            .await
    });

    let agent_task = tokio::spawn(async move {
        let _ = agent.connect_to(peer).await;
    });

    let (client_res, _) = tokio::join!(client_task, agent_task);
    client_res.expect("client panic").expect("client error");

    let received = updates.lock().await.clone();
    assert_eq!(received.len(), 2, "Expected ToolCall and ToolCallUpdate");

    match &received[0] {
        SessionUpdate::ToolCall(call) => {
            assert_eq!(call.title, "mcp__factory__report_progress");
            assert_eq!(call.name.as_deref(), Some("mcp__factory__report_progress"));
        }
        other => panic!("Expected ToolCall, got: {other:?}"),
    }

    match &received[1] {
        SessionUpdate::ToolCallUpdate(update) => {
            assert_eq!(
                update.fields.status,
                Some(agent_client_protocol::schema::v1::ToolCallStatus::Completed)
            );
            let content = update.fields.content.as_ref().expect("content present");
            assert!(!content.is_empty(), "Expected content in ToolCallUpdate");
            // Verify result text mentions "tests pass"
            match &content[0] {
                agent_client_protocol::schema::v1::ToolCallContent::Content(c) => {
                    match &c.content {
                        ContentBlock::Text(t) => {
                            assert!(
                                t.text.contains("tests pass"),
                                "Expected result to mention 'tests pass', got: {}",
                                t.text
                            );
                        }
                        other => panic!("Expected Text block, got: {other:?}"),
                    }
                }
                other => panic!("Expected Content variant, got: {other:?}"),
            }
        }
        other => panic!("Expected ToolCallUpdate, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_scenario_rate_limit() {
    let yaml = r#"
sessions:
  new:
    - - end:
          rate_limited:
            retry_after_seconds: 3600
"#;

    let script = Script::from_yaml_str(yaml).expect("script should parse");
    let temp_session = tempfile::tempdir().expect("tempdir");
    let state = Arc::new(Mutex::new(AgentState::new(
        script,
        temp_session.path().to_path_buf(),
    )));

    let agent = build_agent(state);
    let (transport, peer) = Channel::duplex();

    let client_task = tokio::spawn(async move {
        Client
            .builder()
            .connect_with(transport, async move |cx| {
                cx.send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task()
                    .await
                    .expect("init");

                let new_session = cx
                    .send_request(NewSessionRequest::new(temp_session.path().to_path_buf()))
                    .block_task()
                    .await
                    .expect("new session");

                let res = cx
                    .send_request(PromptRequest::new(
                        new_session.session_id,
                        vec![ContentBlock::Text(TextContent::new("hello"))],
                    ))
                    .block_task()
                    .await;

                let err = res.expect_err("expected rate limit error");
                let err_data = serde_json::to_string(&err).unwrap();
                assert!(
                    err_data.contains("3600") || err_data.contains("rate_limited"),
                    "Expected error to contain rate limit info, got: {err_data}"
                );
                Ok(())
            })
            .await
    });

    let agent_task = tokio::spawn(async move {
        let _ = agent.connect_to(peer).await;
    });

    let (client_res, _) = tokio::join!(client_task, agent_task);
    client_res.expect("client panic").expect("client error");
}

#[tokio::test]
async fn test_scenario_session_load_and_resumption() {
    let yaml = r#"
sessions:
  new:
    - - say: "Turn 1 in first run"
      - end: end_turn
  loaded:
    - - say: "Turn 1 in second run resumed"
      - end: end_turn
"#;

    let script = Script::from_yaml_str(yaml).expect("script should parse");
    let temp_session = tempfile::tempdir().expect("tempdir");
    let state = Arc::new(Mutex::new(AgentState::new(
        script,
        temp_session.path().to_path_buf(),
    )));

    let agent = build_agent(state);
    let (transport, peer) = Channel::duplex();

    let updates = Arc::new(Mutex::new(Vec::new()));
    let updates_cb = Arc::clone(&updates);

    let client = Client.builder().on_receive_notification(
        async move |notification: SessionNotification,
                    _cx: ConnectionTo<agent_client_protocol::role::acp::Agent>| {
            updates_cb.lock().await.push(notification.update);
            Ok(())
        },
        agent_client_protocol::on_receive_notification!(),
    );

    let session_dir = temp_session.path().to_path_buf();
    let client_task = tokio::spawn(async move {
        client
            .connect_with(transport, async move |cx| {
                cx.send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task()
                    .await
                    .expect("init");

                // Run 1: new session
                let new_session = cx
                    .send_request(NewSessionRequest::new(session_dir.clone()))
                    .block_task()
                    .await
                    .expect("new session");
                let session_id = new_session.session_id;

                let res1 = cx
                    .send_request(PromptRequest::new(
                        session_id.clone(),
                        vec![ContentBlock::Text(TextContent::new("run 1 prompt"))],
                    ))
                    .block_task()
                    .await
                    .expect("prompt 1");
                assert_eq!(res1.stop_reason, StopReason::EndTurn);

                // Run 2: load session with same ID
                let load_res = cx
                    .send_request(agent_client_protocol::schema::v1::LoadSessionRequest::new(
                        session_id.clone(),
                        session_dir.clone(),
                    ))
                    .block_task()
                    .await;
                assert!(load_res.is_ok(), "session/load should succeed for run 1 id");

                let res2 = cx
                    .send_request(PromptRequest::new(
                        session_id.clone(),
                        vec![ContentBlock::Text(TextContent::new("run 2 prompt"))],
                    ))
                    .block_task()
                    .await
                    .expect("prompt 2");
                assert_eq!(res2.stop_reason, StopReason::EndTurn);

                // Run 3: load non-existent session
                let bad_id = agent_client_protocol::schema::v1::SessionId::new("non-existent-id");
                let bad_load = cx
                    .send_request(agent_client_protocol::schema::v1::LoadSessionRequest::new(
                        bad_id,
                        session_dir,
                    ))
                    .block_task()
                    .await;
                assert!(bad_load.is_err(), "session/load should fail for unknown id");

                Ok(())
            })
            .await
    });

    let agent_task = tokio::spawn(async move {
        let _ = agent.connect_to(peer).await;
    });

    let (client_res, _) = tokio::join!(client_task, agent_task);
    client_res.expect("client panic").expect("client error");

    let received = updates.lock().await.clone();
    assert_eq!(received.len(), 2);

    match &received[0] {
        SessionUpdate::AgentMessageChunk(c) => match &c.content {
            ContentBlock::Text(t) => assert_eq!(t.text, "Turn 1 in first run"),
            _ => panic!("Expected text"),
        },
        _ => panic!("Expected AgentMessageChunk"),
    }

    match &received[1] {
        SessionUpdate::AgentMessageChunk(c) => match &c.content {
            ContentBlock::Text(t) => assert_eq!(t.text, "Turn 1 in second run resumed"),
            _ => panic!("Expected text"),
        },
        _ => panic!("Expected AgentMessageChunk"),
    }
}

#[tokio::test]
async fn test_scenario_hang_until_cancelled() {
    let yaml = r#"
sessions:
  new:
    - - say: "Starting long task..."
      - end: hang
"#;

    let script = Script::from_yaml_str(yaml).expect("script should parse");
    let temp_session = tempfile::tempdir().expect("tempdir");
    let state = Arc::new(Mutex::new(AgentState::new(
        script,
        temp_session.path().to_path_buf(),
    )));

    let agent = build_agent(state);
    let (transport, peer) = Channel::duplex();

    let client_task = tokio::spawn(async move {
        Client
            .builder()
            .connect_with(transport, async move |cx| {
                cx.send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task()
                    .await
                    .expect("init");

                let new_session = cx
                    .send_request(NewSessionRequest::new(temp_session.path().to_path_buf()))
                    .block_task()
                    .await
                    .expect("new session");
                let session_id = new_session.session_id;

                let sid = session_id.clone();
                let prompt_fut = cx.send_request(PromptRequest::new(
                    sid.clone(),
                    vec![ContentBlock::Text(TextContent::new("hang prompt"))],
                ));

                // Send cancel after 100ms
                let sid_cancel = sid.clone();
                let cancel_sender = cx.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    let _ = cancel_sender.send_notification(
                        agent_client_protocol::schema::v1::CancelNotification::new(sid_cancel),
                    );
                });

                let res = prompt_fut.block_task().await.expect("prompt response");
                assert_eq!(res.stop_reason, StopReason::Cancelled);

                Ok(())
            })
            .await
    });

    let agent_task = tokio::spawn(async move {
        let _ = agent.connect_to(peer).await;
    });

    let (client_res, _) = tokio::join!(client_task, agent_task);
    client_res.expect("client panic").expect("client error");
}
